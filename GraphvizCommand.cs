using DotMake.CommandLine;
using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader;

[CliCommand(Description = "Create a version graph from a slug.", Parent = typeof(MainCommand))]
public class GraphvizCommand
{
    public required MainCommand Parent { get; set; }

    [CliOption(Required = true, Description = "The slug of the repository.")]
    public required string Slug { get; set; }

    public async Task RunAsync()
    {
        var token = Parent.Init();

        using var thaliak = new ThaliakClient();

        var meta = await thaliak.GetRepositoryMetadataAsync(Slug, token).ConfigureAwait(false);
        Log.Debug($"Repository:");
        Log.Debug($"  Slug: {Slug}");
        Log.Debug($"  Name: {meta.Name}");
        Log.Debug($"  Description: {meta.Description}");
        Log.Debug($"  Latest Version: {meta.LatestVersion?.VersionString}");

        Log.Output(await thaliak.GetGraphvizTreeAsync(Slug).ConfigureAwait(false));
    }
}
