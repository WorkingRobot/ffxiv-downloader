using System.Text.Json;
using System.Text.Json.Serialization;
using DotMake.CommandLine;
using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader;

[CliCommand(Description = "Create a version graph from a slug.", Parent = typeof(MainCommand))]
public class GraphvizCommand
{
    [CliOption(Required = true, Description = "The slug of the repository.")]
    public required string Slug { get; set; }

    public async Task RunAsync()
    {
        using var thaliak = new ThaliakClient();

        var meta = await thaliak.GetRepositoryMetadataAsync(Slug).ConfigureAwait(false);
        Log.Debug($"Repository:");
        Log.Debug($"  Slug: {Slug}");
        Log.Debug($"  Name: {meta.Name}");
        Log.Debug($"  Description: {meta.Description}");
        Log.Debug($"  Latest Version: {meta.LatestVersion?.VersionString}");

        Log.Output(await thaliak.GetGraphvizTreeAsync(Slug).ConfigureAwait(false));
    }
}