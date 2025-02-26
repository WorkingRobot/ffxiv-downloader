using System.Net;

namespace FFXIVDownloader;

public sealed class PatchClient : IDisposable
{
    public HttpClient Client { get; }

    public PatchClient()
    {
        Client = new(new SocketsHttpHandler
        {
            AutomaticDecompression = DecompressionMethods.All,
            AllowAutoRedirect = false,
            EnableMultipleHttp2Connections = true,
        });
        Client.DefaultRequestHeaders.TryAddWithoutValidation("User-Agent", "FFXIV PATCH CLIENT");
    }

    public async Task<Stream> GetFileAsync(string url, CancellationToken token = default)
    {
        if (File.Exists(url))
            return File.OpenRead(url);
        else
        {
            var response = await Client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead, token).ConfigureAwait(false);
            response.EnsureSuccessStatusCode();
            return await response.Content.ReadAsStreamAsync(token).ConfigureAwait(false);
        }
    }

    public void Dispose() =>
        Client.Dispose();
}
