using FFXIVDownloader.Thaliak;
using FFXIVDownloader.ZiPatch.Util;
using System.Net;
using System.Net.Http.Headers;
using System.Runtime.CompilerServices;
using System.Text;

namespace FFXIVDownloader;

public sealed class PatchClient : IDisposable
{
    private static readonly int[] BackoffDelays = [500, 1000, 2000, 3000, 5000, 10000, 15000, 20000, 25000, 30000, 45000, 60000];

    public static string? OverridePath { get; set; }

    public HttpClient Client { get; }
    private SemaphoreSlim ConnectionSemaphore { get; }

    public PatchClient(int maxConnections)
    {
        Client = new(new SocketsHttpHandler
        {
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = false,
            ResponseDrainTimeout = Timeout.InfiniteTimeSpan,
        })
        {
            DefaultRequestHeaders =
            {
                { "Connection", "Keep-Alive" },
                { "User-Agent", "FFXIV PATCH CLIENT" }
            }
        };
        ConnectionSemaphore = new(maxConnections);
    }

    public Task<Stream> GetClutAsync(string url, ParsedVersionString version, CancellationToken token = default) =>
        GetResourceAsync(url, version, "clut", token);

    public Task<Stream> GetLutAsync(string url, ParsedVersionString version, CancellationToken token = default) =>
        GetResourceAsync(url, version, "lut", token);

    public Task<Stream> GetPatchAsync(string url, ParsedVersionString version, CancellationToken token = default) =>
        GetResourceAsync(url, version, "patch", token);

    public IAsyncEnumerable<(ContentRangeHeaderValue Range, Stream Stream)> GetPatchRangedAsync(string url, ParsedVersionString version, RangeHeaderValue ranges, CancellationToken token = default) =>
        GetResourceRangedAsync(url, version, "patch", ranges, token);

    public async Task<Stream> GetResourceAsync(string url, ParsedVersionString version, string extension, CancellationToken token = default)
    {
        if (OverridePath != null)
        {
            var filePath = Path.Join(OverridePath, $"{version:P}.{extension}");
            if (File.Exists(filePath))
            {
                Log.Info($"Using override for {version}");
                return File.OpenRead(filePath);
            }
        }
        if (File.Exists(url))
            return File.OpenRead(url);
        else
        {
            using var sema = await SemaphoreLock.CreateAsync(ConnectionSemaphore, token).ConfigureAwait(false);
            var response = await Client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, token).ConfigureAwait(false);
            response.EnsureSuccessStatusCode();
            return await response.Content.ReadAsStreamAsync(token).ConfigureAwait(false);
        }
    }


    private async IAsyncEnumerable<(ContentRangeHeaderValue Range, Stream Stream)> GetResourceRangedAsync(string url, ParsedVersionString version, string extension, RangeHeaderValue ranges, [EnumeratorCancellation] CancellationToken token = default)
    {
        if (OverridePath != null)
        {
            var filePath = Path.Join(OverridePath, $"{version:P}.{extension}");
            if (File.Exists(filePath))
            {
                Log.Info($"Using override for {version}");
                foreach (var data in GetRangedFile(filePath, ranges))
                    yield return data;
                yield break;
            }
        }

        if (File.Exists(url))
        {
            foreach (var data in GetRangedFile(url, ranges))
                yield return data;
        }
        else
        {
            Log.Info($"Downloading {version}; {ranges.Ranges.Sum(r => r.To - r.From + 1) / (double)(1 << 20):0.00} MiB; {ranges.Ranges.Count} ranges");
            await foreach (var data in GetRangedHttpAsync(url, version, ranges, 0, token).WithCancellation(token).ConfigureAwait(false))
                yield return data;
        }
    }

    private static IEnumerable<(ContentRangeHeaderValue Range, Stream Stream)> GetRangedFile(string url, RangeHeaderValue ranges)
    {
        using var fileStream = File.OpenRead(url);
        foreach (var range in ranges.Ranges)
        {
            var ret = new ContentRangeHeaderValue(range.From!.Value, range.To!.Value);

            fileStream.Position = ret.From!.Value;
            var length = ret.To!.Value - ret.From.Value + 1;

            var stream = new ClampedStream(fileStream, length);
            yield return (ret, stream);
        }
    }

    private async IAsyncEnumerable<(ContentRangeHeaderValue Range, Stream Stream)> GetRangedHttpAsync(string url, ParsedVersionString version, RangeHeaderValue ranges, int backoffIdx, [EnumeratorCancellation] CancellationToken token)
    {
        if (backoffIdx >= BackoffDelays.Length)
            throw new InvalidOperationException("Failed to download range");

        HttpResponseMessage? rsp = null;

        using var request = new HttpRequestMessage(HttpMethod.Get, url);
        request.Headers.Range = ranges;

        using var sema = await SemaphoreLock.CreateAsync(ConnectionSemaphore, token).ConfigureAwait(false);
        RangeHeaderValue? r1 = null, r2 = null;
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

            if (ranges.Ranges.Count == 1)
                throw new InvalidOperationException("Failed to download range");

            Log.Warn($"Retrying {version}; {ranges.Ranges.Sum(r => r.To - r.From + 1) / (double)(1 << 20):0.00} MiB; {ranges.Ranges.Count} ranges");
            var rangeList = ranges.Ranges.ToArray().AsSpan();
            var halfCount = rangeList.Length / 2;

            r1 = new();
            foreach (var r in rangeList[..halfCount])
                r1.Ranges.Add(r);

            r2 = new();
            foreach (var r in rangeList[halfCount..])
                r2.Ranges.Add(r);

            sema.Dispose();
            await Task.Delay(BackoffDelays[backoffIdx], token).ConfigureAwait(false);
        }

        if (r1 != null && r2 != null)
        {
            await foreach (var data in GetRangedHttpAsync(url, version, r1, backoffIdx + 1, token).WithCancellation(token).ConfigureAwait(false))
                yield return data;
            await foreach (var data in GetRangedHttpAsync(url, version, r2, backoffIdx + 1, token).WithCancellation(token).ConfigureAwait(false))
                yield return data;
            yield break;
        }

        using var resp = rsp;
        await foreach (var data in IterateMultipartRanges(resp!, token).WithCancellation(token).ConfigureAwait(false))
            yield return data;
    }

    private static async IAsyncEnumerable<(ContentRangeHeaderValue Range, Stream Stream)> IterateMultipartRanges(HttpResponseMessage message, [EnumeratorCancellation] CancellationToken token = default)
    {
        if (message.StatusCode is not (HttpStatusCode.PartialContent or HttpStatusCode.OK))
            throw new HttpRequestException($"Invalid status code for ranges ({message.StatusCode}; {message.Content.Headers.ContentType}; {message.Content.Headers.ContentLength})", null, message.StatusCode);

        if (message.Content.Headers.ContentType?.MediaType != "multipart/byteranges")
        {
            var range = message.Content.Headers.ContentRange ?? throw new InvalidOperationException("Missing Content-Range header");
            var dataStream = await message.Content.ReadAsStreamAsync(token).ConfigureAwait(false);
            yield return (range, dataStream);
            yield break;
        }

        if (message.StatusCode == HttpStatusCode.OK)
            throw new InvalidOperationException("Recieved OK for byte range");

        var boundary = message.Content.Headers.ContentType?.Parameters.FirstOrDefault(p => p.Name == "boundary")?.Value ??
            throw new InvalidOperationException($"Missing boundary ({message.Content.Headers.ContentType})");
        if (message.Content.Headers.ContentLength == 0)
            throw new InvalidOperationException("Empty multipart response");

        using var httpStream = await message.Content.ReadAsStreamAsync(token).ConfigureAwait(false);
        using var reader = new BinaryReader(httpStream, Encoding.UTF8);
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

            using (var rangeStream = new ClampedStream(httpStream, range.To!.Value + 1 - range.From!.Value))
                yield return (range, rangeStream);

            currentLine = ReadLine();
            if (currentLine != string.Empty)
                throw new InvalidOperationException("Invalid boundary");
        }
    }

    public void Dispose()
    {
        Client.Dispose();
    }
}
