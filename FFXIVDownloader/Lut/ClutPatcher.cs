using DotNext.IO;
using FFXIVDownloader.Thaliak;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.ZiPatch.Util;
using System.Buffers;
using System.IO.Compression;
using System.Net.Http.Headers;
using System.Runtime.CompilerServices;
using System.Threading.Channels;

namespace FFXIVDownloader.Lut;

public sealed class ClutPatcher : IDisposable
{
    private const int MAX_RANGE_HEADER_SIZE = 1 << 12;
    private const int MIN_RANGE_DISTANCE = 1 << 9;

    public ClutDiff Diff { get; }
    public int Concurrency { get; }
    public Uri BasePatchUrl { get; }

    private PatchClient Client { get; }
    private Channel<(ParsedVersionString, PatchIntervalData)> ApplyQueue { get; } = Channel.CreateBounded<(ParsedVersionString, PatchIntervalData)>(new BoundedChannelOptions(16)
    {
        FullMode = BoundedChannelFullMode.Wait,
        SingleReader = true
    });

    private readonly record struct PatchSection(ParsedVersionString Version, ClutPatchRef Location);

    private readonly record struct DataRefOperation(string File, ClutDataRef DataRef, PatchIntervalData? PatchData);

    private sealed class MergedRange(ClutPatchRef part)
    {
        public long Offset { get; } = part.Offset;
        public int Size { get; private set; } = part.Size;
        public List<ClutPatchRef> Parts { get; } = [part];

        public bool Add(ClutPatchRef part)
        {
            if (part.Offset + part.Size + MIN_RANGE_DISTANCE < Offset)
                return false;

            Size = Math.Max(Size, checked((int)(part.Offset + part.Size - Offset)));
            Parts.Add(part);
            return true;
        }
    }

    private sealed class PatchIntervalData(ClutPatchRef interval, ReadOnlyMemory<byte> fileData)
    {
        public ClutPatchRef Interval { get; } = interval;
        public ReadOnlyMemory<byte> Data { get; } = fileData;

        public static async Task<PatchIntervalData> CreateAsync(ClutPatchRef interval, ReadOnlyMemory<byte> patchData, CancellationToken token = default)
        {
            if (interval.Size != patchData.Length)
                throw new ArgumentException("Invalid patch data size", nameof(patchData));
            if (interval.IsCompressed)
                return new(interval, await DecompressAsync(patchData, token).ConfigureAwait(false));
            return new(interval, patchData);
        }

        private static async ValueTask<ReadOnlyMemory<byte>> DecompressAsync(ReadOnlyMemory<byte> patchData, CancellationToken token = default)
        {
            using var ms = new MemoryStream(patchData.Length * 2);
            using var s = patchData.AsStream();
            using var d = new DeflateStream(s, CompressionMode.Decompress);
            await d.CopyToAsync(ms, token).ConfigureAwait(false);
            return ms.GetBuffer().AsMemory()[..(int)ms.Length];
        }
    }

    public ClutPatcher(ClutDiff diff, int concurrency, string? basePatchUrl = null)
    {
        Diff = diff;
        Concurrency = concurrency;
        BasePatchUrl = new(
            Diff.BasePatchUrl ??
            basePatchUrl ??
            throw new ArgumentNullException(nameof(basePatchUrl), "No base url exists"),
            UriKind.Absolute);

        Client = new(10);
    }

    public Task ApplyAsync(ZiPatchConfig config, CancellationToken token = default)
    {
        return Parallel.ForEachAsync(
            GetDataRefsAsync(token),
            new ParallelOptions { CancellationToken = token, MaxDegreeOfParallelism = Concurrency },
            (operation, token) => ApplyDataRefAsync(config, operation.File, operation.DataRef, operation.PatchData?.Data, token)
        );
    }

    private static async ValueTask ApplyDataRefAsync(ZiPatchConfig config, string path, ClutDataRef dataRef, ReadOnlyMemory<byte>? patchData = null, CancellationToken token = default)
    {
        var file = await config.OpenFile(path).ConfigureAwait(false);
        if (dataRef.Type is ClutDataRef.RefType.Patch or ClutDataRef.RefType.SplitPatch)
        {
            if (!patchData.HasValue)
                throw new ArgumentNullException(nameof(patchData));

            var patchBuffer = patchData.Value;

            var offset = 0;
            if (dataRef.Type == ClutDataRef.RefType.SplitPatch)
                offset = dataRef.PatchOffset!.Value;

            ArgumentOutOfRangeException.ThrowIfLessThan(patchBuffer.Length, offset + dataRef.Length, nameof(patchData));
            patchBuffer = patchBuffer.Slice(offset, dataRef.Length);

            await file.WriteAsync(patchBuffer, dataRef.Offset, token).ConfigureAwait(false);
        }
        else if (dataRef.Type == ClutDataRef.RefType.Zero)
            await file.WipeAsync(dataRef.Offset, dataRef.Length, token).ConfigureAwait(false);
        else if (dataRef.Type == ClutDataRef.RefType.EmptyBlock)
        {
            ArgumentOutOfRangeException.ThrowIfNotEqual(dataRef.Length, 24);
            await file.WriteEmptyFileBlockAt(dataRef.BlockCount!.Value, dataRef.Offset).ConfigureAwait(false);
        }
        else
            throw new InvalidOperationException("Invalid data ref type");
    }

    private async IAsyncEnumerable<DataRefOperation> GetDataRefsAsync([EnumeratorCancellation] CancellationToken token = default)
    {
        var allRefs = Diff.AddedFiles
            .Select(f => (f.Key, f.Value))
            .SelectMany(f => f.Value.Select(r => (File: f.Key, DataRef: r)))
            .ToArray();

        var nonDownloadDataRefsEnum = allRefs
            .Where(f => f.DataRef.Type is not (ClutDataRef.RefType.Patch or ClutDataRef.RefType.SplitPatch));
        var nonDownloadCount = nonDownloadDataRefsEnum.Count();
        var nonDownloadDataRefs = nonDownloadDataRefsEnum.GetEnumerator();
        var downloadRefs = allRefs
            .Where(f => f.DataRef.Type is ClutDataRef.RefType.Patch or ClutDataRef.RefType.SplitPatch)
            .ToLookup(k => new PatchSection(k.DataRef.AppliedVersion, k.DataRef.Patch!.Value));

        var downloadTask = GetPatchDataAsync(downloadRefs.Select(r => r.Key), token);
        _ = downloadTask.ContinueWith(task => ApplyQueue.Writer.Complete(task.Exception), token);

        var refsNondownload = 0;
        var refsDownload = 0;
        while (true)
        {
            token.ThrowIfCancellationRequested();
            if (ApplyQueue.Reader.TryRead(out var item))
            {
                var (version, data) = item;
                foreach (var p in downloadRefs[new(version, data.Interval)])
                {
                    refsDownload++;
                    yield return new(p.File, p.DataRef, data);
                }
            }
            else if (nonDownloadDataRefs.MoveNext())
            {
                refsNondownload++;
                var (path, dataRef) = nonDownloadDataRefs.Current;
                yield return new(path, dataRef, null);
            }
            else if (!ApplyQueue.Reader.Completion.IsCompleted)
            {
                var cts = CancellationTokenSource.CreateLinkedTokenSource(token);
                cts.CancelAfter(5000);
                await ApplyQueue.Reader.WaitToReadAsync(cts.Token).ConfigureAwait(false);
            }
            else
            {
                await downloadTask.ConfigureAwait(false);
                break;
            }
        }
        if (refsNondownload != nonDownloadCount)
            throw new InvalidOperationException("Not all non-downloaded refs were processed");
        if (refsDownload + nonDownloadCount != allRefs.Length)
            throw new InvalidOperationException("Not all downloaded refs were processed");
    }

    private Task GetPatchDataAsync(IEnumerable<PatchSection> sections, CancellationToken token = default)
    {
        var groupedSections = sections
            .GroupBy(p => p.Version);

        var rangeTasks = new List<Task>();

        return Parallel.ForEachAsync(groupedSections, token, (group, token) =>
        {
            var parts = group.Select(p => p.Location).Distinct();
            return GetPatchesFromVersionAsync(
                group.Key, parts,
                (patch, token) => ApplyQueue.Writer.WriteAsync((group.Key, patch), token),
                token);
        });
    }

    private ValueTask GetPatchesFromVersionAsync(ParsedVersionString version, IEnumerable<ClutPatchRef> parts, Func<PatchIntervalData, CancellationToken, ValueTask> onRangeRecieved, CancellationToken token = default)
    {
        Log.Info($"Partially downloading {version}");

        // We can't use RangeHeaderValue because it adds a space after the comma
        // Akamai restricts the range header size to at most 1034 bytes from my testing,
        // but it doesn't work sometimes, so use a smaller number

        List<MergedRange> mergedParts = [];
        foreach (var part in parts.OrderBy(p => p.Offset))
        {
            var last = mergedParts.LastOrDefault();
            if (last is null || !last.Add(part))
                mergedParts.Add(new(part));
        }

        var ranges = new List<(List<MergedRange> MergedRanges, RangeHeaderValue Header)>();
        foreach (var range in mergedParts)
        {
            var value = new RangeItemHeaderValue(range.Offset, range.Offset + range.Size - 1);
            var addNew = false;
            if (ranges.Count == 0)
                addNew = true;
            else
            {
                var last = ranges[^1].Header;
                last.Ranges.Add(value);
                if (last.ToString().Length > MAX_RANGE_HEADER_SIZE)
                {
                    last.Ranges.Remove(value);
                    addNew = true;
                }
            }
            if (addNew)
                ranges.Add(([], new()));

            var (mergedRanges, header) = ranges[^1];
            mergedRanges.Add(range);
            header.Ranges.Add(value);
        }

        var ret = Parallel.ForEachAsync(
            ranges, token,
            async (range, token) =>
            {
                await foreach (var (contentRange, stream) in Client.GetPatchRangedAsync($"{BasePatchUrl}/{version:P}.patch", version, range.Header, token).WithCancellation(token).ConfigureAwait(false))
                {
                    await foreach (var interval in ProcessStreamIntervalsAsync(contentRange.From!.Value, stream, range.MergedRanges.First(r => r.Offset == contentRange.From!.Value).Parts, token).WithCancellation(token))
                        await onRangeRecieved(interval, token).ConfigureAwait(false);
                }
            }
        );
        return new(ret);
    }

    private static async IAsyncEnumerable<PatchIntervalData> ProcessStreamIntervalsAsync(long streamPos, Stream stream, IEnumerable<ClutPatchRef> patches, [EnumeratorCancellation] CancellationToken token = default)
    {
        // Sort intervals by start to process sequentially.
        var sortedIntervals = patches.OrderBy(i => i.Offset).ToList();
        var index = 0;
        var currentPos = streamPos;
        const int SkipBufferSize = 1 << 18;
        var skipBuffer = new byte[SkipBufferSize];

        // Process until all intervals are handled.
        while (index < sortedIntervals.Count)
        {
            // Start a new union segment.
            var unionStart = sortedIntervals[index].Offset;
            var unionEnd = sortedIntervals[index].End;
            List<ClutPatchRef> unionIntervals = [sortedIntervals[index]];
            index++;

            // Merge all intervals that overlap with this union.
            while (index < sortedIntervals.Count &&
                   sortedIntervals[index].Offset <= unionEnd)
            {
                unionIntervals.Add(sortedIntervals[index]);
                unionEnd = Math.Max(unionEnd, sortedIntervals[index].End);
                index++;
            }

            // Skip bytes until unionStart (if needed).
            var bytesToSkip = unionStart - currentPos;
            while (bytesToSkip > 0)
            {
                var toRead = (int)Math.Min(SkipBufferSize, bytesToSkip);
                var read = await stream.ReadAsync(skipBuffer.AsMemory(0, toRead), token).ConfigureAwait(false);
                if (read == 0)
                {
                    // End of stream reached unexpectedly.
                    yield break;
                }
                bytesToSkip -= read;
                currentPos += read;
            }

            // Read the entire union segment.
            var unionLength = (int)(unionEnd - unionStart);
            var unionBuffer = new byte[unionLength];
            var offset = 0;
            while (offset < unionLength)
            {
                var read = await stream.ReadAsync(unionBuffer.AsMemory(offset, unionLength - offset), token).ConfigureAwait(false);
                if (read == 0)
                {
                    // End of stream reached unexpectedly.
                    break;
                }
                offset += read;
                currentPos += read;
            }

            // Yield each interval from the union by slicing the unionBuffer.
            foreach (var patch in unionIntervals)
            {
                var relativeStart = (int)(patch.Offset - unionStart);
                var relativeLength = patch.Size;
                yield return await PatchIntervalData.CreateAsync(patch, unionBuffer.AsMemory(relativeStart, relativeLength), token).ConfigureAwait(false);
            }
        }
    }

    public void Dispose()
    {
        Client.Dispose();
    }
}
