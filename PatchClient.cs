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
            AllowAutoRedirect = false
        });
    }

    public async Task<Stream> GetPatchFileAsync(string url)
    {
        var response = await Client.GetAsync(url, HttpCompletionOption.ResponseHeadersRead).ConfigureAwait(false);
        response.EnsureSuccessStatusCode();
        return await response.Content.ReadAsStreamAsync().ConfigureAwait(false);
    }

    public void Dispose()
    {
        Client.Dispose();
    }
}
