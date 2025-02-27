using System.Text.Json;

namespace FFXIVDownloader.Command;

public sealed record CacheMetadata
{
    public List<string> InstalledVersions { get; set; } = [];
    public List<string> FilteredFiles { get; } = [];

    public static async Task<CacheMetadata> GetAsync(string outputPath)
    {
        var path = Path.Join(outputPath, ".cachemeta.json");
        if (!File.Exists(path))
            return new();

        using var stream = File.OpenRead(path);
        return await JsonSerializer.DeserializeAsync<CacheMetadata>(stream).ConfigureAwait(false) ?? new();
    }

    public Task WriteAsync(string outputPath)
    {
        var path = Path.Join(outputPath, ".cachemeta.json");

        using var stream = File.OpenWrite(path);
        return JsonSerializer.SerializeAsync(stream, this);
    }
}
