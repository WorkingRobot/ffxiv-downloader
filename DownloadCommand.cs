using System.Text.RegularExpressions;
using DotMake.CommandLine;
using FFXIVDownloader.ZiPatch;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader;

[CliCommand(Description = "Download a list of files from a slug and version.", Parent = typeof(MainCommand))]
public class DownloadCommand
{
    [CliOption(Required = true, Description = "The slug of the repository.")]
    public required string Slug { get; set; }

    [CliOption(Required = false, Description = "The version to download. If blank, the latest version will be downloaded.")]
    public string? Version { get; set; }

    [CliOption(Required = false, Arity = CliArgumentArity.ZeroOrMore, Description = "The file regexes to download. If omitted, all files will be downloaded.")]
    public string[] Files { get; set; } = [];

    [CliOption(Required = false, Description = "The output directory to download the files to. If omitted, the current directory will be used.")]
    public string OutputPath { get; set; } = Directory.GetCurrentDirectory();

    public async Task RunAsync()
    {
        var cts = new CancellationTokenSource();
        Console.CancelKeyPress += (sender, eventArgs) =>
        {
            cts.Cancel();
            eventArgs.Cancel = true;
        };

        OutputPath = Directory.CreateDirectory(OutputPath).FullName;
        Log.Info($"Output Path: {OutputPath}");

        if (Files.Length == 0)
            Files = [".*"];

        var regexes = Files.Select(f => new Regex(f, RegexOptions.Compiled | RegexOptions.IgnoreCase)).ToArray();
        bool RegexMatches(string path) =>
            regexes.Any(
                r =>
                    r.Match(path) is { Success: true, Value.Length: var len } &&
                    len == path.Length
            );

        using var thaliak = new ThaliakClient();

        var meta = await thaliak.GetRepositoryMetadataAsync(Slug, cts.Token).ConfigureAwait(false);
        Log.Verbose($"Repository:");
        Log.Verbose($"  Slug: {Slug}");
        Log.Verbose($"  Name: {meta.Name}");
        Log.Verbose($"  Description: {meta.Description}");
        Log.Verbose($"  Latest Version: {meta.LatestVersion?.VersionString}");

        var version = Version != null ? new ParsedVersionString(Version) : meta.LatestVersion!.VersionString;
        Log.Info($"Downloading version {version}");

        Log.Verbose($"Downloading patch chain");
        var chain = await thaliak.GetPatchChainAsync(Slug, version, cts.Token).ConfigureAwait(false);

        var cache = await CacheMetadata.GetAsync(OutputPath).ConfigureAwait(false);
        if (cache.FilteredFiles.Any(RegexMatches))
            throw new InvalidOperationException("Some files were filtered out from previous patches. Please delete the output directory and try again.");

        while (chain.Count > 0)
        {
            if (cache.InstalledVersions.Contains(chain[0].Version.ToString("P")))
            {
                Log.Info($"Skipping patch {chain[0].Version}");
                chain.RemoveAt(0);
            }
            else
                break;
        }

        Log.Info($"Total Size: {chain.Sum(p => p.Patch.Size) / (double)(1 << 30):0.00} GiB");

        using var patchClient = new PatchClient();
        foreach (var (ver, patch) in chain)
        {
            Log.Info($"Downloading patch {ver}");
            Log.Verbose($"  URL: {patch.Url}");
            Log.Verbose($"  Size: {patch.Size / (double)(1 << 20):0.00} MiB");

            using var httpStream = await patchClient.GetFileAsync(new(patch.Url), cts.Token).ConfigureAwait(false);
            using var patchStream = new BufferedStream(httpStream, 1 << 20);

            var config = new FilteredZiPatchConfig<PersistentZiPatchConfig>(
                new(OutputPath),
                RegexMatches,
                cache.FilteredFiles
            );

            using (var file = new ZiPatchFile(httpStream))
            {
                await foreach (var chunk in file.GetChunksAsync().WithCancellation(cts.Token).ConfigureAwait(false))
                {
                    Log.Debug($"Applying chunk {chunk}");
                    await chunk.ApplyAsync(config).ConfigureAwait(false);
                }
            }

            cache.InstalledVersions.Add(ver.ToString("P"));
            cache.FilteredFiles.Clear();
            cache.FilteredFiles.AddRange(config.FilteredFiles);
            Log.Debug($"Writing out cache");
            await cache.WriteAsync(OutputPath).ConfigureAwait(false);
            Log.Verbose($"Installed version {ver}");
        }
    }
}