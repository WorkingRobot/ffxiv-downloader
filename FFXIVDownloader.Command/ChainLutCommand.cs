using DotMake.CommandLine;
using FFXIVDownloader.Lut;
using FFXIVDownloader.Thaliak;
using System.Diagnostics;

namespace FFXIVDownloader.Command;

[CliCommand(Name = "clut", Description = "Create a chain LUT file from a patch url. If a slug is provided, the base path will be the folder to search the .lut files for. Otherwise, the base path will be used in conjuction with the provided urls.", Parent = typeof(MainCommand))]
public class ChainLutCommand
{
    public required MainCommand Parent { get; set; }

    [CliOption(Required = true, Description = "The slug of the repository.")]
    public required string Slug { get; set; }

    [CliOption(Required = false, Description = "The version to use from the slug. If blank, the latest version will be used.")]
    public string? Version { get; set; }

    [CliOption(Required = false, Description = "The base path of the LUT files. Can be a file path or a url fragment.")]
    public string? BasePath { get; set; }

    [CliOption(Required = false, Description = "The base URL to provide for future consumers of the .clut to use to resolve .patch URLs.")]
    public string? BasePatchUrl { get; set; }

    [CliOption(Required = false, Arity = CliArgumentArity.OneOrMore, Description = "The url (or file paths) of the LUT files.")]
    public string[]? Urls { get; set; }

    [CliOption(Required = false, Description = "The url (or file path) of the base CLUT file. If omitted, it will be assumed that you are starting from scratch.")]
    public string? BaseClut { get; set; }

    [CliOption(Required = false, Description = "The output directory to write the CLUTs to. If omitted, the current directory will be used.")]
    public string OutputPath { get; set; } = Directory.GetCurrentDirectory();

    [CliOption(Required = false, Description = "The compression method to use for the CLUT files.")]
    public CompressType Compression { get; set; } = CompressType.Brotli;

    public async Task RunAsync()
    {
        var token = Parent.Init();

        OutputPath = Directory.CreateDirectory(OutputPath).FullName;
        Log.Info($"Output Path: {OutputPath}");

        var chain = await LutCommand.GetChainAsync(Urls, Slug, Version, token).ConfigureAwait(false);

        using var patchClient = new PatchClient(10);

        ClutFile clut;
        if (BaseClut != null)
        {
            using var httpStream = await patchClient.GetClutAsync(BaseClut, ParsedVersionString.Epoch, token).ConfigureAwait(false);
            using var bufferedStream = new BufferedStream(httpStream, 1 << 20);

            using var reader = new BinaryReader(bufferedStream);
            clut = new(reader);
            clut.Header.Compression = Compression;
            clut.Header.Repository = Slug;
            if (!string.IsNullOrWhiteSpace(BasePatchUrl))
                clut.Header.BasePatchUrl = BasePatchUrl;
        }
        else
            clut = new()
            {
                Header = new ClutHeader
                {
                    Compression = Compression,
                    Repository = Slug,
                    Version = ParsedVersionString.Epoch,
                    BasePatchUrl = BasePatchUrl
                }
            };

        foreach (var (ver, patch) in chain)
        {
            var url = patch.Url;
            // .patch is usually because we got the patch url from Thaliak
            if (Path.GetExtension(url) == ".patch")
                url = Path.Join(BasePath, $"{ver:P}.lut");
            else if (!Path.IsPathRooted(url))
                url = Path.Join(BasePath, url);

            Log.Info($"Processing {ver}");
            Log.Verbose($"  URL: {url}");

            using var httpStream = await patchClient.GetLutAsync(url, ver, token).ConfigureAwait(false);
            using var bufferedStream = new BufferedStream(httpStream, 1 << 20);
            using var reader = new BinaryReader(bufferedStream);

            clut.Header.Repository = Slug;
            clut.Header.Version = ver;
            var lutFile = new LutFile(reader);
            foreach (var chunk in lutFile.Chunks)
                clut.ApplyLut(ver, chunk);

            Log.Verbose("Optimizing");
            var n = Stopwatch.StartNew();
            clut.RemoveOverlaps();
            n.Stop();
            Log.Verbose($"Optimized in {n.Elapsed.TotalSeconds:0.00}s");

            var fileName = $"{ver:P}.clut";
            var outPath = Path.Join(OutputPath, fileName);
            Log.Verbose($"Writing to {fileName}");

            long fileSize;
            using (var clutStream = new FileStream(outPath, FileMode.Create, FileAccess.Write, FileShare.Read))
            {
                using var writer = new BinaryWriter(clutStream);
                clut.Write(writer);
                fileSize = clutStream.Length;
            }

            Log.Info($"Finished {fileName} ({fileSize / (double)(1 << 10):0.00} KiB)");

            //Log.DebugObject(clut);
        }
    }
}
