using DotMake.CommandLine;
using FFXIVDownloader.Lut;
using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader;

[CliCommand(Name = "clut", Description = "Create a chain LUT file from a patch url.", Parent = typeof(MainCommand))]
public class ChainLutCommand
{
    public required MainCommand Parent { get; set; }

    [CliOption(Required = false, Arity = CliArgumentArity.OneOrMore, Description = "The url (or file paths) of the LUT files.")]
    public required string[] Urls { get; set; }

    [CliOption(Required = false, Description = "The url (or file path) of the base CLUT file. If omitted, it will be assumed that you are starting from scratch.")]
    public required string? BaseClut { get; set; }

    [CliOption(Required = false, Description = "The output directory to write the CLUTs to. If omitted, the current directory will be used.")]
    public string OutputPath { get; set; } = Directory.GetCurrentDirectory();

    [CliOption(Required = false, Description = "The compression method to use for the CLUT files.")]
    public CompressType Compression { get; set; } = CompressType.Brotli;

    public async Task RunAsync()
    {
        var token = Parent.Init();

        OutputPath = Directory.CreateDirectory(OutputPath).FullName;
        Log.Info($"Output Path: {OutputPath}");

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

        foreach (var url in Urls)
        {
            Log.Info($"Downloading {url}");

            var version = new ParsedVersionString(Path.GetFileNameWithoutExtension(url));

            using var httpStream = await patchClient.GetFileAsync(url, token).ConfigureAwait(false);
            using var bufferedStream = new BufferedStream(httpStream, 1 << 20);
            using var reader = new BinaryReader(bufferedStream);

            var lutFile = new LutFile(reader);
            foreach (var chunk in lutFile.Chunks)
                clut.ApplyLut(version.ToString("P"), chunk);

            var fileName = Path.GetFileName(Path.ChangeExtension(url, "clut"));
            Log.Debug($"Writing to {fileName}");

            long fileSize;
            using (var clutStream = new FileStream(Path.Join(OutputPath, fileName), FileMode.Create, FileAccess.Write, FileShare.Read))
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
