using DotMake.CommandLine;
using FFXIVDownloader.Lut;
using FFXIVDownloader.Thaliak;
using FFXIVDownloader.ZiPatch;
using FFXIVDownloader.ZiPatch.Config;
using System.Text.RegularExpressions;

namespace FFXIVDownloader.Command;

[CliCommand(Description = "Download a list of files from a slug and version.", Parent = typeof(MainCommand))]
public class DownloadCommand
{
    public required MainCommand Parent { get; set; }

    [CliOption(Required = true, Description = "The slug of the repository.")]
    public required string Slug { get; set; }

    [CliOption(Required = false, Description = "Whether to use an additional .cachemeta.json file in the directory to manage version state. If used, downloads will always start from scratch.")]
    public bool SkipCache { get; set; }

    [CliOption(Required = false, Description = "The version to download. If blank, the latest version will be downloaded.")]
    public string? Version { get; set; }

    [CliOption(Required = false, Arity = CliArgumentArity.OneOrMore, Description = "The file regexes to download. If omitted, all files will be downloaded.")]
    public string[] Files { get; set; } = [".*"];

    [CliOption(Required = false, Description = "The output directory to download the files to. If omitted, the current directory will be used.")]
    public string OutputPath { get; set; } = Directory.GetCurrentDirectory();

    [CliOption(Required = false, Description = "The CLUT directory to use. If omitted, the patches will be downloaded without one.")]
    public string? ClutPath { get; set; }

    [CliOption(Required = false, Description = "Degree of parallelism to use when applying patches.")]
    public int Parallelism { get; set; } = Environment.ProcessorCount;

    public async Task RunAsync()
    {
        var token = Parent.Init();

        OutputPath = Directory.CreateDirectory(OutputPath).FullName;
        Log.Info($"Output Path: {OutputPath}");

        Regexes = Files.Select(f => new Regex(f, RegexOptions.Compiled | RegexOptions.Singleline | RegexOptions.IgnoreCase)).ToArray();

        using var thaliak = new ThaliakClient();

        var meta = await thaliak.GetRepositoryMetadataAsync(Slug, token).ConfigureAwait(false);
        Log.Verbose($"Repository:");
        Log.Verbose($"  Slug: {Slug}");
        Log.Verbose($"  Name: {meta.Name}");
        Log.Verbose($"  Description: {meta.Description}");
        Log.Verbose($"  Latest Version: {meta.LatestVersion?.VersionString}");

        var version = !string.IsNullOrWhiteSpace(Version) ? new ParsedVersionString(Version) : meta.LatestVersion!.VersionString;
        Log.Info($"Downloading version {version}");

        Log.Verbose($"Downloading patch chain");
        var chain = await thaliak.GetPatchChainAsync(Slug, version, token).ConfigureAwait(false);

        var cache = SkipCache ? new() : await CacheMetadata.GetAsync(OutputPath).ConfigureAwait(false);
        if (cache.FilteredFiles.Any(RegexMatches))
            throw new InvalidOperationException("Some files were filtered out from previous patches. Please delete the output directory and try again.");

        var installedVersion = ParsedVersionString.Epoch;
        while (chain.Count > 0)
        {
            if (cache.InstalledVersions.Contains(chain[0].Version.ToString("P")))
            {
                installedVersion = chain[0].Version;
                Log.Info($"Skipping patch {chain[0].Version}");
                chain.RemoveAt(0);
            }
            else
                break;
        }

        if (chain.Count == 0)
        {
            Log.CIOutput("updated", "false");
            Log.CIOutput("version", installedVersion.ToString());
            return;
        }

        using var patchClient = new PatchClient(10);

        if (!string.IsNullOrWhiteSpace(ClutPath))
            await TryDownloadFromClut(patchClient, installedVersion, chain, cache, token).ConfigureAwait(false);
        else
            await TryDownloadFromRemote(patchClient, chain, cache, token).ConfigureAwait(false);

        Log.CIOutput("updated", "true");
        Log.CIOutput("version", chain[^1].Version.ToString());
    }

    private Regex[]? Regexes { get; set; }
    private bool RegexMatches(string path) =>
        Regexes!.Any(
            r =>
                r.Match(path) is { Success: true, Value.Length: var len } &&
                len == path.Length
        );

    private async Task TryDownloadFromClut(PatchClient patchClient, ParsedVersionString installedVersion, List<(ParsedVersionString Version, Patch Patch)> chain, CacheMetadata cache, CancellationToken token)
    {
        if (chain.Count == 0)
            return;

        var latestVersion = chain[^1].Version;

        if (latestVersion == installedVersion)
            return;

        var baseUrl = chain[^1].Patch.Url;
        baseUrl = baseUrl[..(baseUrl.LastIndexOf('/') + 1)];

        ClutFile? installedClut = null;
        if (installedVersion > ParsedVersionString.Epoch)
        {
            using var stream = await patchClient.GetClutAsync($"{ClutPath}/{installedVersion:P}.clut", installedVersion, token).ConfigureAwait(false);
            using var reader = new BinaryReader(stream);
            installedClut = new(reader);
        }

        ClutFile latestClut;
        {
            using var stream = await patchClient.GetClutAsync($"{ClutPath}/{latestVersion:P}.clut", latestVersion, token).ConfigureAwait(false);
            using var reader = new BinaryReader(stream);
            latestClut = new(reader);
        }

        ClutDiff diff = installedClut != null ? new(installedClut, latestClut) : new(latestClut);

        diff.AddedFiles.Keys.Where(f => !RegexMatches(f)).ToList().ForEach(f => diff.AddedFiles.Remove(f));
        diff.RemovedFiles.Where(f => !RegexMatches(f)).ToList().ForEach(f => diff.RemovedFiles.Remove(f));

        var writeSize = 0L;
        foreach (var d in diff.AddedFiles.Values)
            foreach (var p in d)
                writeSize += p.Length;

        var downloadSize = diff.AddedFiles.Values
            .SelectMany(
                r => r
                    .Where(p => p.Type is ClutDataRef.RefType.Patch or ClutDataRef.RefType.SplitPatch)
                    .Select(p => p.Patch!.Value)
            )
            .Distinct()
            .Sum(d => (long)d.Size);

        Log.Info($"Total Write Size: {writeSize / (double)(1 << 30):0.00} GiB");
        Log.Info($"Approx. Download Size: {downloadSize / (double)(1 << 30):0.00} GiB");

        await using var config = new FilteredZiPatchConfig<PersistentZiPatchConfig>(
            new(OutputPath),
            RegexMatches,
            cache.FilteredFiles
        );
        using (var patcher = new ClutPatcher(diff, Parallelism, baseUrl))
            await patcher.ApplyAsync(config, token).ConfigureAwait(false);

        cache.InstalledVersions.AddRange(chain.Select(v => v.Version.ToString("P")));
        cache.FilteredFiles.Clear();
        cache.FilteredFiles.AddRange(config.FilteredFiles);
        if (!SkipCache)
        {
            Log.Debug($"Writing out cache");
            await cache.WriteAsync(OutputPath).ConfigureAwait(false);
        }
        Log.Verbose($"Installed version {latestVersion}");
    }

    private async Task TryDownloadFromRemote(PatchClient patchClient, List<(ParsedVersionString Version, Patch Patch)> chain, CacheMetadata cache, CancellationToken token)
    {
        Log.Info($"Total Download Size: {chain.Sum(p => p.Patch.Size) / (double)(1 << 30):0.00} GiB");

        foreach (var (ver, patch) in chain)
        {
            Log.Info($"Downloading patch {ver}");
            Log.Verbose($"  URL: {patch.Url}");
            Log.Verbose($"  Size: {patch.Size / (double)(1 << 20):0.00} MiB");

            using var httpStream = await patchClient.GetPatchAsync(patch.Url, ver, token).ConfigureAwait(false);
            using var patchStream = new BufferedStream(httpStream, 1 << 20);

            await using var config = new FilteredZiPatchConfig<PersistentZiPatchConfig>(
                new(OutputPath),
                RegexMatches,
                cache.FilteredFiles
            );

            using (var file = new ZiPatchFile(httpStream))
            {
                await foreach (var chunk in file.GetChunksAsync(token).WithCancellation(token).ConfigureAwait(false))
                {
                    Log.Debug($"Applying chunk {chunk}");
                    await chunk.ApplyAsync(config).ConfigureAwait(false);
                }
            }

            cache.InstalledVersions.Add(ver.ToString("P"));
            cache.FilteredFiles.Clear();
            cache.FilteredFiles.AddRange(config.FilteredFiles);
            if (!SkipCache)
            {
                Log.Debug($"Writing out cache");
                await cache.WriteAsync(OutputPath).ConfigureAwait(false);
            }
            Log.Verbose($"Installed version {ver}");
        }
    }
}
