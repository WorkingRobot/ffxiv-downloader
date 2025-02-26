using DotNext.IO;
using FFXIVDownloader.Thaliak;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.ZiPatch.Util;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Compression;
using System.Net;
using System.Net.Http.Headers;
using System.Runtime.CompilerServices;
using System.Text;

namespace FFXIVDownloader.Lut;

public sealed class ClutPatcher : IDisposable
{
    private static readonly int[] BackoffDelays = [500, 1000, 2000, 3000, 5000, 10000, 15000, 20000, 25000, 30000, 45000, 60000];
    private const int MAX_RANGE_HEADER_SIZE = 1 << 12;
    private const int MIN_RANGE_DISTANCE = 1 << 9;

    public ClutDiff Diff { get; }
    public int Concurrency { get; }
    public Uri BasePatchUrl { get; }

    private HttpClient Client { get; }
    private SemaphoreSlim ClientSemaphore { get; } = new(10);
    private ConcurrentQueue<(PatchSection, ReadOnlyMemory<byte>)> ApplyQueue { get; } = [];

    private readonly record struct PatchSection(ParsedVersionString Version, ClutPatchRef Location);
    private readonly record struct DataRefOperation(string File, ClutDataRef DataRef, ReadOnlyMemory<byte>? PatchData);
    private class MergedRange(ClutPatchRef part)
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


    public ClutPatcher(ClutDiff diff, int concurrency, string? basePatchUrl = null)
    {
        Diff = diff;
        Concurrency = concurrency;
        BasePatchUrl = new(
            Diff.BasePatchUrl ??
            basePatchUrl ??
            throw new ArgumentNullException(nameof(basePatchUrl), "No base url exists"),
        UriKind.Absolute);
        Client = new(new SocketsHttpHandler
        {
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = false,
            ResponseDrainTimeout = Timeout.InfiniteTimeSpan,
        })
        {
            BaseAddress = BasePatchUrl,
            DefaultRequestHeaders =
            {
                { "User-Agent", "FFXIV PATCH CLIENT" }
            }
        };
    }

    public Task ApplyAsync(ZiPatchConfig config, CancellationToken token = default) =>
        Parallel.ForEachAsync(
            GetDataRefsAsync(token),
            new ParallelOptions { CancellationToken = token, MaxDegreeOfParallelism = Concurrency },
            (operation, token) => ApplyDataRefAsync(config, operation.File, operation.DataRef, operation.PatchData, token)
        );

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

        var refsNondownload = 0;
        var refsDownload = 0;
        while (true)
        {
            token.ThrowIfCancellationRequested();
            if (ApplyQueue.TryDequeue(out var item))
            {
                var (section, data) = item;
                foreach (var p in downloadRefs[section])
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
            else if (!downloadTask.IsCompleted)
            {
                Log.Debug("Waiting");
                await Task.Delay(100, token).ConfigureAwait(false);
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
                (patch, dat) => ApplyQueue.Enqueue((new(group.Key, patch), dat)),
                token);
        });
    }

    private ValueTask GetPatchesFromVersionAsync(ParsedVersionString version, IEnumerable<ClutPatchRef> parts, Action<ClutPatchRef, ReadOnlyMemory<byte>> onRangeRecieved, CancellationToken token = default)
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
            (range, token) =>
                GetRangesFromVersionAsync(version, range.Header, async (offset, data, token) => {
                    Dictionary<(long, int), ReadOnlyMemory<byte>> uncompressedData = [];

                    async ValueTask<ReadOnlyMemory<byte>> GetAsUncompressedAsync(long sliceOffset, int sliceSize)
                    {
                        if (uncompressedData.TryGetValue((sliceOffset, sliceSize), out var sliceData))
                            return sliceData;

                        using var ms = new MemoryStream(sliceSize * 2);
                        using var s = data.Slice(checked((int)(sliceOffset - offset)), sliceSize).AsStream();
                        using var d = new DeflateStream(s, CompressionMode.Decompress);
                        await d.CopyToAsync(ms, token).ConfigureAwait(false);
                        ReadOnlyMemory<byte> ret = ms.GetBuffer().AsMemory()[..(int)ms.Length];
                        uncompressedData[(sliceOffset, sliceSize)] = ret;
                        return ret;
                    }

                    ReadOnlyMemory<byte> GetAsRaw(long sliceOffset, int sliceSize) =>
                        data.Slice(checked((int)(sliceOffset - offset)), sliceSize);

                    ValueTask<ReadOnlyMemory<byte>> GetAs(long sliceOffset, int sliceSize, bool isCompressed) =>
                        isCompressed ?
                            GetAsUncompressedAsync(sliceOffset, sliceSize) :
                            ValueTask.FromResult(GetAsRaw(sliceOffset, sliceSize));

                    foreach (var dat in range.MergedRanges.First(r => r.Offset == offset).Parts)
                        onRangeRecieved(dat, await GetAs(dat.Offset, dat.Size, dat.IsCompressed).ConfigureAwait(false));
                }, token: token)
        );
        return new(ret);
    }

    private async ValueTask GetRangesFromVersionAsync(ParsedVersionString version, RangeHeaderValue rangeHeader, Func<long, ReadOnlyMemory<byte>, CancellationToken, Task> onRangeRecieved, int backoffIdx = 0, CancellationToken token = default)
    {
        if (backoffIdx >= BackoffDelays.Length)
            throw new InvalidOperationException("Failed to download range");

        HttpResponseMessage? rsp = null;

        using var request = new HttpRequestMessage(HttpMethod.Get, $"{version:P}.patch");
        request.Headers.Range = rangeHeader;

        using var sema = await SemaphoreLock.CreateAsync(ClientSemaphore, token).ConfigureAwait(false);
        Log.Info($"Downloading {version}; {rangeHeader.Ranges.Count}");
        try
        {
            rsp = await Client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead, token).ConfigureAwait(false);
            rsp.EnsureSuccessStatusCode();
        }
        catch (TaskCanceledException)
        {
            throw;
        }
        catch (Exception e)
        {
            Log.Error(e);
            rsp?.Dispose();

            if (rangeHeader.Ranges.Count == 1)
                throw new InvalidOperationException("Failed to download range");

            Log.Warn($"Retrying {version}; {rangeHeader.Ranges.Count}");
            var halfCount = rangeHeader.Ranges.Count / 2;
            var r1 = rangeHeader.Ranges.Take(halfCount);
            var r2 = rangeHeader.Ranges.Skip(halfCount);

            var r1Header = new RangeHeaderValue();
            foreach (var r in r1)
                r1Header.Ranges.Add(r);

            var r2Header = new RangeHeaderValue();
            foreach (var r in r2)
                r2Header.Ranges.Add(r);

            sema.Dispose();
            await Task.Delay(BackoffDelays[backoffIdx], token).ConfigureAwait(false);

            await GetRangesFromVersionAsync(version, r1Header, onRangeRecieved, backoffIdx + 1, token).ConfigureAwait(false);
            await GetRangesFromVersionAsync(version, r2Header, onRangeRecieved, backoffIdx + 1, token).ConfigureAwait(false);
            return;
        }
        using var resp = rsp;

        await foreach (var ((start, _), data) in IterateMultipartRanges(resp, token).WithCancellation(token).ConfigureAwait(false))
            await onRangeRecieved(start, data, token).ConfigureAwait(false);
    }

    private static async IAsyncEnumerable<((long From, long To), ReadOnlyMemory<byte>)> IterateMultipartRanges(HttpResponseMessage message, [EnumeratorCancellation] CancellationToken token = default)
    {
        if (message.StatusCode is not (HttpStatusCode.PartialContent or HttpStatusCode.OK))
            throw new HttpRequestException($"Invalid status code for ranges ({message.StatusCode}; {message.Content.Headers.ContentType}; {message.Content.Headers.ContentLength})", null, message.StatusCode);

        if (message.Content.Headers.ContentType?.MediaType != "multipart/byteranges")
        {
            var range = message.Content.Headers.ContentRange ?? throw new InvalidOperationException("Missing Content-Range header");
            var arr = await message.Content.ReadAsByteArrayAsync(token).ConfigureAwait(false);
            yield return ((range.From!.Value, range.To!.Value + 1), arr);
            yield break;
        }

        if (message.StatusCode == HttpStatusCode.OK)
            throw new InvalidOperationException("Recieved OK for byte range");

        var boundary = message.Content.Headers.ContentType?.Parameters.FirstOrDefault(p => p.Name == "boundary")?.Value ??
            throw new InvalidOperationException($"Missing boundary ({message.Content.Headers.ContentType})");
        if (message.Content.Headers.ContentLength == 0)
            throw new InvalidOperationException("Empty multipart response");

        using var httpStream = await message.Content.ReadAsStreamAsync(token).ConfigureAwait(false);
        using var stream = new BufferedStream(httpStream, 1 << 20);
        using var reader = new BinaryReader(stream, Encoding.UTF8);
        string ReadLine()
        {
            var b = new StringBuilder();
            char character;
            while ((character = reader.ReadChar()) is not ('\n' or '\r'))
                b.Append(character);
            return b.ToString();
        }

        while (true)
        {
            string currentLine;
            do
            {
                currentLine = ReadLine();
            } while (currentLine == string.Empty);
            if (currentLine == $"--{boundary}--")
                yield break;
            if (currentLine != $"--{boundary}")
                throw new InvalidOperationException("Invalid boundary");

            ContentRangeHeaderValue? range = null;
            while (true)
            {
                currentLine = ReadLine();
                if (currentLine == string.Empty)
                    break;

                var kv = currentLine.Split(':', 2);
                if (kv.Length != 2)
                    throw new InvalidOperationException("Invalid header");
                if (range != null)
                    continue;
                if (!kv[0].Equals("Content-Range", StringComparison.OrdinalIgnoreCase))
                    continue;
                if (!ContentRangeHeaderValue.TryParse(kv[1], out range))
                    throw new InvalidOperationException("Invalid Content-Range header");
            }

            if (range == null)
                throw new InvalidOperationException("Missing Content-Range header");

            var buffer = new byte[range.To!.Value + 1 - range.From!.Value];
            await stream.ReadExactlyAsync(buffer, token).ConfigureAwait(false);
            yield return ((range.From.Value, range.To.Value + 1), buffer);

            currentLine = ReadLine();
            if (currentLine != string.Empty)
                throw new InvalidOperationException("Invalid boundary");
        }
    }

    public void Dispose()
    {
        Client.Dispose();
        ClientSemaphore.Dispose();
    }
}
