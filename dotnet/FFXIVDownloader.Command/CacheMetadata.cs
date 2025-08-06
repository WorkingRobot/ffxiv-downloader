using System.Text.Json;
using System.Text.Json.Serialization;

namespace FFXIVDownloader.Command;

public sealed partial record CacheMetadata
{
    public List<string> InstalledVersions { get; set; } = [];
    public List<string> FilteredFiles { get; } = [];

    public static async Task<CacheMetadata> GetAsync(string outputPath)
    {
        var path = Path.Join(outputPath, ".cachemeta.json");
        if (!File.Exists(path))
            return new();

        using var stream = File.OpenRead(path);
        return await JsonSerializer.DeserializeAsync(stream, JsonContext.Default.CacheMetadata).ConfigureAwait(false) ?? new();
    }

    public async Task WriteAsync(string outputPath)
    {
        var path = Path.Join(outputPath, ".cachemeta.json");

        using var stream = File.OpenWrite(path);
        stream.SetLength(0);
        await JsonSerializer.SerializeAsync(stream, this, JsonContext.Default.CacheMetadata).ConfigureAwait(false);
    }

    [JsonSerializable(typeof(CacheMetadata))]
    public partial class JsonContext : JsonSerializerContext
    {
    }
}

