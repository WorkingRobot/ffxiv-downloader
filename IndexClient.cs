using CommunityToolkit.HighPerformance;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO.Compression;
using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;

namespace FFXIVDownloader;

public sealed class IndexClient : IDisposable
{
    private HttpClient Client { get; }
    private JsonSerializerOptions JsonOptions { get; }

    public IndexClient()
    {
        Client = new()
        {
            BaseAddress = new("https://raw.githubusercontent.com/goatcorp/patchinfo/intl_6_48_hotfix/")
        };

        JsonOptions = new(JsonSerializerDefaults.Web)
        {

        };
    }

    private class LatestData
    {
        public string Boot { get; set; }
        public int BootRevision { get; set; }
        public string Game { get; set; }
        public int GameRevision { get; set; }
        public string Ex1 { get; set; }
        public int Ex1Revision { get; set; }
        public string Ex2 { get; set; }
        public int Ex2Revision { get; set; }
        public string Ex3 { get; set; }
        public int Ex3Revision { get; set; }
        public string Ex4 { get; set; }
        public int Ex4Revision { get; set; }

        public (string Repository, string Version)? GetBySlug(string slug) =>
            slug switch
            {
                "2b5cbc63" => ("boot", Boot),
                "4e9a232b" => ("game", Game),
                "6b936f08" => ("ex1", Ex1),
                "f29a3eb2" => ("ex2", Ex2),
                "859d0e24" => ("ex3", Ex3),
                "1bf99b87" => ("ex4", Ex4),
                _ => null
            };
    }

    public async Task<(string Repository, string Version)?> GetLatestVersionAsync(string slug)
    {
        var data = await Client.GetFromJsonAsync<LatestData>("latest.json", JsonOptions).ConfigureAwait(false) ?? throw new InvalidOperationException("Failed to fetch latest.json");
        return data.GetBySlug(slug);
    }

    public async Task<Stream> GetIndexStreamAsync(string repository, string version)
    {
        var response = await Client.GetAsync($"{repository}/{version}.patch.index", HttpCompletionOption.ResponseHeadersRead).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        return await response.Content.ReadAsStreamAsync().ConfigureAwait(false);
    }

    public async Task<IndexFile> GetIndexFileAsync(string repository, string version)
    {
        using var stream = await GetIndexStreamAsync(repository, version).ConfigureAwait(false);
        using var inflateStream = new DeflateStream(stream, CompressionMode.Decompress);
        using var bufferStream = new BufferedStream(inflateStream, 1 << 20);
        using var reader = new BinaryReader(bufferStream);
        return new IndexFile(reader);
    }

    public void Dispose()
    {
        Client.Dispose();
    }
}

public sealed class IndexFile
{
    public int ExpacVersion { get; }

    public List<IndexSourceFile> SourceFiles { get; }

    public List<IndexTargetFile> TargetFiles { get; }

    public IndexFile(BinaryReader reader)
    {
        ExpacVersion = reader.ReadInt32();

        var sourceFileCount = reader.ReadInt32();
        SourceFiles = new(sourceFileCount);
        for (var i = 0; i < sourceFileCount; ++i)
            SourceFiles.Add(new(reader));
        for (var i = 0; i < sourceFileCount; ++i)
            SourceFiles[i].FinishRead(reader);

        var targetFileCount = reader.ReadInt32();
        TargetFiles = new(targetFileCount);
        for (var i = 0; i < targetFileCount; ++i)
            TargetFiles.Add(new(reader));
    }
}

public sealed class IndexSourceFile
{
    public string Name { get; set; }
    public uint LastPtr { get; set; }

    public IndexSourceFile(BinaryReader reader)
    {
        Name = reader.ReadString();
    }

    public void FinishRead(BinaryReader reader)
    {
        LastPtr = reader.ReadUInt32();
    }
}

public sealed class IndexTargetFile
{
    public string RelativePath { get; }
    public List<IndexTargetFilePart> Parts { get; }

    public IndexTargetFile(BinaryReader reader)
    {
        RelativePath = reader.ReadString();

        var partCount = reader.ReadInt32();
        Parts = new(partCount);
        for (var i = 0; i < partCount; ++i)
            Parts.Add(new(reader));
    }
}

public sealed class IndexTargetFilePart
{
    public uint Offset { get; }
    public uint SourceOffset { get; }
    public uint Size { get; }
    public bool IsDeflated { get; }
    public uint? Crc32 { get; }
    public uint? PlaceholderEntryDataUnits { get; }
    public ushort SplitDecodedSourceFrom { get; }
    public byte TargetIndex { get; }
    public byte SourceIndex { get; }

    public bool IsAllZeros => SourceIndex == 0xFF;
    public bool IsEmpty => SourceIndex == 0xFE;
    public bool IsUnavailable => SourceIndex == 0xFD;
    public bool IsFromSourceFile => !IsAllZeros && !IsEmpty && !IsUnavailable;
    // Why was the index format designed so poorly like this
    public uint EstimatedSourceSize => IsDeflated ? 16384 : Size;

    public IndexTargetFilePart(BinaryReader reader)
    {
        Offset = reader.ReadUInt32();
        SourceOffset = reader.ReadUInt32();

        var sizeAndFlags = reader.ReadUInt32();
        Size = sizeAndFlags & 0x3FFFFFFF;
        IsDeflated = (sizeAndFlags & 0x80000000) != 0;
        var hasCrc32 = (sizeAndFlags & 0x40000000) != 0;
        if (hasCrc32)
            Crc32 = reader.ReadUInt32();
        else
            PlaceholderEntryDataUnits = reader.ReadUInt32();

        SplitDecodedSourceFrom = reader.ReadUInt16();
        TargetIndex = reader.ReadByte();
        SourceIndex = reader.ReadByte();
    }
}

public sealed class IndexFileStream : Stream
{
    public override bool CanRead => true;

    public override bool CanSeek => true;

    public override bool CanWrite => false;

    public override long Length => Target.Parts.Sum(part => part.Size);

    public override long Position { get => CurrentPart.Offset + CurrentPartOffset; set => throw new NotSupportedException(); }

    private IndexTargetFile Target { get; }
    private IReadOnlyList<IndexSourceFile> Sources { get; }
    private HttpClient Client { get; }
    private Dictionary<IndexTargetFilePart, Task<ReadOnlyMemory<byte>>> Parts { get; }
    private int CurrentPartIndex { get; set; }
    private int CurrentPartOffset { get; set; }
    private ReadOnlyMemory<byte>? CurrentPartData { get; set; }

    private IndexTargetFilePart CurrentPart => Target.Parts[CurrentPartIndex];

    public IndexFileStream(IndexTargetFile target, IReadOnlyList<IndexSourceFile> sources, Uri remoteUrl)
    {
        Target = target;
        Sources = sources;
        Client = new(new SocketsHttpHandler()
        {
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = false,
        })
        {
            BaseAddress = remoteUrl
        };
        Parts = [];
    }

    private async Task<ReadOnlyMemory<byte>> GetPartAsync(IndexTargetFilePart part)
    {
        if (Parts.TryGetValue(part, out var task))
            return await task.ConfigureAwait(false);

        return await GetPartAsyncInternal(part).ConfigureAwait(false);
    }

    private async Task<ReadOnlyMemory<byte>> GetPartAsyncInternal(IndexTargetFilePart part)
    {
        if (part.IsAllZeros)
        {
            return new byte[part.Size];
        }
        else if (part.IsUnavailable)
            throw new InvalidOperationException("Part is unavailable");
        else if (part.IsEmpty)
        {
            var ret = new byte[part.Size];
            using var stream = new MemoryStream(ret);
            using var writer = new BinaryWriter(stream);
            writer.Write(1 << 7);
            writer.Seek(10, SeekOrigin.Begin);
            writer.Write(part.PlaceholderEntryDataUnits!.Value);
            return ret;
        }
        else if (part.IsFromSourceFile)
        {
            var source = Sources[part.SourceIndex];

            var parts = Target.Parts.Where(p => p.SourceIndex == part.SourceIndex);

            var rangesTask = GetRangesFromSourceAsync(source, parts);

            foreach (var p in parts)
            {
                Parts.TryAdd(p, rangesTask.ContinueWith(async t =>
                {
                    var r = t.Result[p.SourceOffset];

                    if (r.Length > p.EstimatedSourceSize)
                        r = r[..checked((int)p.EstimatedSourceSize)];

                    if (p.IsDeflated)
                    {
                        using var stream = r.AsStream();
                        using var inflateStream = new DeflateStream(stream, CompressionMode.Decompress);
                        var ret = new byte[p.Size];
                        await inflateStream.ReadExactlyAsync(ret).ConfigureAwait(false);
                        return ret;
                    }

                    return r;
                }).Unwrap());
            }

            if (Parts.TryGetValue(part, out var task))
                return await task.ConfigureAwait(false);
            else
                throw new UnreachableException();
        }
        else
            throw new UnreachableException();
    }

    private async Task<IReadOnlyDictionary<uint, ReadOnlyMemory<byte>>> GetRangesFromSourceAsync(IndexSourceFile source, IEnumerable<IndexTargetFilePart> parts)
    {
        Console.WriteLine($"Partially downloading {new Uri(Client.BaseAddress!, source.Name)}");

        // We can't use RangeHeaderValue because it adds a space after the comma
        // Akamai restricts the range header size to at most 1034 bytes
        var ranges = new List<StringBuilder>();
        foreach (var r in parts
            .Select(p => (p.SourceOffset, p.EstimatedSourceSize))
            .GroupBy(p => p.SourceOffset)
            .Select(g => g.MaxBy(p => p.EstimatedSourceSize)))
        {
            var value = $"{r.SourceOffset}-{Math.Min(source.LastPtr, r.SourceOffset + r.EstimatedSourceSize) - 1},";
            if (ranges.Count == 0 || ranges[^1].Length + value.Length > 1034)
            {
                var b = new StringBuilder("bytes=");
                b.Append(value);
                ranges.Add(b);
            }
            else
                ranges[^1].Append(value);
        }

        var partDictionary = new ConcurrentDictionary<uint, ReadOnlyMemory<byte>>();
        var rangeTasks = new List<Task>();
        using var sem = new SemaphoreSlim(4);
        foreach (var range in ranges)
        {
            await sem.WaitAsync().ConfigureAwait(false);
            rangeTasks.Add(GetRangeFromSourceAsync(source, range.ToString()).ContinueWith(t =>
            {
                sem.Release();
                foreach (var (start, data) in t.Result)
                    partDictionary.TryAdd(start, data);
            }));
        }

        await Task.WhenAll(rangeTasks).ConfigureAwait(false);

        return partDictionary;
    }

    private async Task<IReadOnlyDictionary<uint, ReadOnlyMemory<byte>>> GetRangeFromSourceAsync(IndexSourceFile source, string range)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, source.Name);
        if (!request.Headers.TryAddWithoutValidation("Range", range))
            throw new ArgumentException("Invalid range", nameof(range));

        using var resp = await Client.SendAsync(request, HttpCompletionOption.ResponseHeadersRead).ConfigureAwait(false);
        resp.EnsureSuccessStatusCode();

        var partDictionary = new ConcurrentDictionary<uint, ReadOnlyMemory<byte>>();

        await foreach (var ((start, end), data) in IterateMultipartRanges(resp).ConfigureAwait(false))
        {
            if (!partDictionary.TryAdd(checked((uint)start), data))
                throw new InvalidOperationException("Duplicate part");
        }

        return partDictionary;
    }

    public override int Read(Span<byte> buffer)
    {
        if (buffer.Length == 0)
            return 0;

        if (CurrentPartIndex == Target.Parts.Count)
            return 0;

        var partData = CurrentPartData ??= GetPartAsync(CurrentPart).GetAwaiter().GetResult();
        if (buffer.Length < partData.Length)
        {
            CurrentPartData = partData[buffer.Length..];
            partData.Span[..buffer.Length].CopyTo(buffer);
            CurrentPartOffset += buffer.Length;
            return buffer.Length;
        }
        else
        {
            partData.Span.CopyTo(buffer);
            CurrentPartData = null;
            CurrentPartOffset = 0;
            ++CurrentPartIndex;
            return partData.Length;
        }
    }

    public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
    {
        if (buffer.Length == 0)
            return 0;

        if (CurrentPartIndex == Target.Parts.Count)
            return 0;

        var partData = CurrentPartData ??= await GetPartAsync(CurrentPart).ConfigureAwait(false);
        if (buffer.Length < partData.Length)
        {
            CurrentPartData = partData[buffer.Length..];
            partData[..buffer.Length].CopyTo(buffer);
            CurrentPartOffset += buffer.Length;
            return buffer.Length;
        }
        else
        {
            partData.CopyTo(buffer);
            CurrentPartData = null;
            CurrentPartOffset = 0;
            ++CurrentPartIndex;
            return partData.Length;
        }
    }

    public override int Read(byte[] buffer, int offset, int count) =>
        Read(buffer.AsSpan(offset, count));

    public override Task<int> ReadAsync(byte[] buffer, int offset, int count, CancellationToken cancellationToken)
    {
        return ReadAsync(buffer.AsMemory(offset, count), cancellationToken).AsTask();
    }

    public override long Seek(long offset, SeekOrigin origin) =>
        origin switch
        {
            SeekOrigin.Begin => Position = offset,
            SeekOrigin.Current => Position += offset,
            SeekOrigin.End => Position = Length + offset,
            _ => throw new ArgumentOutOfRangeException(nameof(origin), origin, null)
        };

    public override void SetLength(long value)
    {
        throw new NotSupportedException();
    }

    public override void Write(byte[] buffer, int offset, int count)
    {
        throw new NotSupportedException();
    }

    public override void Flush()
    {
        throw new NotSupportedException();
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            Client.Dispose();
        base.Dispose(disposing);
    }

    private static async IAsyncEnumerable<((long From, long To), ReadOnlyMemory<byte>)> IterateMultipartRanges(HttpResponseMessage message)
    {
        if (message.StatusCode == HttpStatusCode.OK)
        {
            var arr = await message.Content.ReadAsByteArrayAsync().ConfigureAwait(false);
            yield return ((0, arr.Length), arr);
        }
        else if (message.StatusCode == HttpStatusCode.PartialContent)
        {
            var boundary = message.Content.Headers.ContentType!.Parameters.First(p => p.Name == "boundary").Value;
            //var bytes = await message.Content.ReadAsByteArrayAsync().ConfigureAwait(false);
            //var filename = $"test{message.GetHashCode():X8}.bin";
            //Console.WriteLine($"Writing to {filename}");
            //await File.WriteAllBytesAsync(filename, bytes).ConfigureAwait(false);
            //using var stream = new MemoryStream(bytes);
            using var stream = await message.Content.ReadAsStreamAsync().ConfigureAwait(false);
            using var reader = new BinaryReader(stream, Encoding.UTF8);
            string ReadLine()
            {
                var b = new StringBuilder();
                char character;
                while ((character = reader.ReadChar()) != '\n')
                {
                    if (character == '\r')
                        continue;
                    b.Append(character);
                }
                return b.ToString();
            }

            while (true)
            {
                var currentLine = ReadLine();
                while (currentLine == string.Empty)
                    currentLine = ReadLine();
                if (currentLine == "--" + boundary + "--")
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
                await stream.ReadExactlyAsync(buffer).ConfigureAwait(false);
                yield return ((range.From.Value, range.To.Value + 1), buffer);

                currentLine = ReadLine();
                if (currentLine != string.Empty)
                    throw new InvalidOperationException("Invalid boundary");
            }
        }
        else
            throw new HttpRequestException("Invalid status code for ranges", null, message.StatusCode);
    }
}
