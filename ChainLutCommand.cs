using DotMake.CommandLine;
using FFXIVDownloader.Lut;
using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader;

[CliCommand(Name = "clut", Description = "Create a chain LUT file from a patch url. If a slug is provided, the base path will be the folder to search the .lut files for. Otherwise, the base path will be used in conjuction with the provided urls.", Parent = typeof(MainCommand))]
public class ChainLutCommand
{
    public required MainCommand Parent { get; set; }

    [CliOption(Required = false, Description = "The slug of the repository.")]
    public string? Slug { get; set; }

    [CliOption(Required = false, Description = "The version to use from the slug. If blank, the latest version will be used.")]
    public string? Version { get; set; }

    [CliOption(Required = false, Description = "The base path of the LUT files. Can be a file path or a url fragment.")]
    public string? BasePath { get; set; }

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

        using var patchClient = new PatchClient();

        ClutFile clut;
        if (BaseClut != null)
        {
            using var httpStream = await patchClient.GetFileAsync(BaseClut, token).ConfigureAwait(false);
            using var bufferedStream = new BufferedStream(httpStream, 1 << 20);

            using var reader = new BinaryReader(bufferedStream);
            clut = new(reader)
            {
                Header = new ClutHeader
                {
                    Compression = Compression
                }
            };
        }
        else
            clut = new()
            {
                Header = new ClutHeader
                {
                    Compression = Compression
                }
            };

        foreach (var (ver, patch) in chain)
        {
            Log.Info($"Processing {ver}");

            var url = patch.Url;
            // .patch is usually because we got the patch url from Thaliak
            if (Path.GetExtension(url) == ".patch")
                url = Path.Join(BasePath, $"{Path.GetFileNameWithoutExtension(url)}.lut");
            else if (!Path.IsPathRooted(url))
                url = Path.Join(BasePath, url);

            using var httpStream = await patchClient.GetFileAsync(url, token).ConfigureAwait(false);
            using var bufferedStream = new BufferedStream(httpStream, 1 << 20);
            using var reader = new BinaryReader(bufferedStream);

            var lutFile = new LutFile(reader);
            foreach (var chunk in lutFile.Chunks)
                clut.ApplyLut(ver, chunk);

            var fileName = $"{ver:P}.clut";
            var outPath = Path.Join(OutputPath, fileName);
            Log.Debug($"Writing to {fileName}");

            long fileSize;
            using (var clutStream = new FileStream(outPath, FileMode.Create, FileAccess.Write, FileShare.Read))
            {
                using var writer = new BinaryWriter(clutStream);
                clut.Write(writer);
                fileSize = clutStream.Length;
            }

            Log.Debug($"Finished {fileName} ({fileSize / (double)(1 << 10):0.00} KiB)");

            //Log.DebugObject(clut);
        }
    }
}
