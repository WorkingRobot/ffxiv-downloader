using FFXIVDownloader.Patching.ZiPatch;
using FFXIVDownloader.Patching.ZiPatch.Util;
using System.Text.RegularExpressions;

namespace FFXIVDownloader;

public static class Program
{
    public static async Task Main(string[] args)
    {
        if (args.Length != 4)
        {
            Console.WriteLine("Usage: FFXIVDownloader <slug> <current-version> <output-path> <file-regex>");
            return;
        }

        var slug = args[0];
        var currentVersion = args[1];
        var output = args[2];
        var fileRegex = new Regex(args[3], RegexOptions.Compiled | RegexOptions.IgnoreCase);

        if (string.IsNullOrWhiteSpace(currentVersion))
        {
            try
            {
                currentVersion = File.ReadAllText(Path.Combine(output, "cached-ver.txt"));
            }
            catch (FileNotFoundException)
            {
                currentVersion = string.Empty;
            }
        }

        using var thaliak = new ThaliakClient();
        using var patchClient = new PatchClient();
        using var config = new FilteredPersistentZiPatchConfig(output, fileRegex.IsMatch);

        Console.WriteLine($"Downloading patch chain for {slug}");

        var chain = await thaliak.GetPatchChainAsync(slug).ConfigureAwait(false);
        foreach (var version in chain.SkipWhile(v => !string.IsNullOrWhiteSpace(currentVersion) && v.VersionString != currentVersion))
        {
            Console.WriteLine($"Downloading version {version.VersionString}");
            foreach (var patch in version.Patches!)
            {
                Console.WriteLine($"Downloading {patch.Url}");

                using var s = await patchClient.GetPatchFileAsync(patch.Url).ConfigureAwait(false);
                using var s2 = new BufferedStream(s, 1 << 20);
                using var file = new ZiPatchFile(s2);

                await foreach (var chunk in file.GetChunks())
                    chunk.ApplyChunk(config, null!);
            }
        }

        Console.WriteLine("Done");

        File.WriteAllText(Path.Combine(output, "cached-ver.txt"), chain.Last().VersionString);

        if (Environment.GetEnvironmentVariable("GITHUB_OUTPUT") is { } githubOutput)
            Console.SetOut(new StreamWriter(githubOutput));

        SetOutput("updated-files", string.Join(';', config.OpenStreams.Keys));
        SetOutput("latest-version", chain.Last().VersionString);
    }

    private static void WriteToEnv(string variable, string text)
    {
        if (Environment.GetEnvironmentVariable(variable) is { } file)
        {
            using var writer = new StreamWriter(file);
            writer.WriteLine(text);
        }
        else
            Console.WriteLine($"{variable} => {text}");
    }

    private static void SetOutput(string key, string value) =>
        WriteToEnv("GITHUB_OUTPUT", $"{key}={value}");
}

internal sealed class BlackHoleStream : Stream
{
    public override bool CanRead => false;
    public override bool CanSeek => false;
    public override bool CanWrite => true;
    public override long Length => throw new NotSupportedException();
    public override long Position { get => throw new NotSupportedException(); set { } }

    public override void Flush() { }
    public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
    public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
    public override void SetLength(long value) { }
    public override void Write(byte[] buffer, int offset, int count) { }
}

public sealed class FilteredPersistentZiPatchConfig : ZiPatchConfig, IDisposable
{
    private string GamePath { get; }
    private Dictionary<string, FileStream> Streams { get; }
    private Predicate<string> Filter { get; }

    public IReadOnlyDictionary<string, FileStream> OpenStreams => Streams;

    public FilteredPersistentZiPatchConfig(string gamePath, Predicate<string> filter)
    {
        GamePath = Path.GetFullPath(gamePath);
        Streams = [];
        Filter = filter;
    }

    public override Stream OpenStream(SqexFile file)
    {
        if (!Filter(file.RelativePath))
            return new BlackHoleStream();

        if (Streams.TryGetValue(file.RelativePath, out var stream))
            return stream;

        var path = Path.Combine(GamePath, file.RelativePath);
        if (Path.GetDirectoryName(path) is { } dirName)
            Directory.CreateDirectory(dirName);

        int tries = 5;
        int sleeptime = 1;
        do
        {
            try
            {
                stream = new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.Read, 1 << 16);
                if (!Streams.TryAdd(file.RelativePath, stream))
                    Console.WriteLine($"Failed to add stream for {file.RelativePath}");
                return stream;
            }
            catch (IOException)
            {
                if (tries == 0)
                    throw;

                Thread.Sleep(sleeptime * 1000);
            }
        } while (0 < --tries);

        throw new FileNotFoundException($"Could not find file {file.RelativePath}");
    }

    public override void CreateDirectory(string path)
    {
        Directory.CreateDirectory(Path.Combine(GamePath, path));
    }

    public override void DeleteFile(string path)
    {
        if (!Filter(path))
            return;

        if (Streams.Remove(path, out var stream))
            stream.Dispose();

        path = Path.Combine(GamePath, path);
        File.Delete(path);
    }

    public override void DeleteDirectory(string path)
    {
        Directory.Delete(path);
    }

    public override void DeleteExpansion(ushort expansionId)
    {
        var expansionFolder = SqexFile.GetExpansionFolder(expansionId);

        var sqpack = $"{GamePath}/sqpack/{expansionFolder}";
        var movie = $"{GamePath}/movie/{expansionFolder}";

        var shouldKeep = (string f) => new[] { ".var", "00000.bk2", "00001.bk2", "00002.bk2", "00003.bk2" }.Any(f.EndsWith);

        if (Directory.Exists(sqpack))
        {
            foreach (var file in Directory.GetFiles(sqpack))
            {
                if (!Filter(file))
                    continue;
                if (!shouldKeep(file))
                {
                    if (Streams.Remove(file, out var stream))
                        stream.Dispose();
                    File.Delete(file);
                }
            }
        }

        if (Directory.Exists(movie))
        {
            foreach (var file in Directory.GetFiles(movie))
            {
                if (!Filter(file))
                    continue;
                if (!shouldKeep(file))
                {
                    if (Streams.Remove(file, out var stream))
                        stream.Dispose();
                    File.Delete(file);
                }
            }
        }
    }

    public void Dispose()
    {
        foreach (var stream in Streams.Values)
            stream.Dispose();
    }
}

public sealed class FilteredRamConfig : ZiPatchConfig, IDisposable
{
    private Dictionary<string, Stream> Streams { get; }
    private Predicate<string> Filter { get; }

    public FilteredRamConfig(Predicate<string> filter)
    {
        Streams = [];
        Filter = filter;
    }

    public override Stream OpenStream(SqexFile file)
    {
        if (Streams.TryGetValue(file.RelativePath, out var stream))
            return stream;
        Console.WriteLine($"Opening (fake) file: {file.RelativePath}");
        return Streams[file.RelativePath] = Filter(file.RelativePath) ? new MemoryStream() : new BlackHoleStream();
    }

    public override void CreateDirectory(string path)
    {
        Console.WriteLine($"Creating (fake) directory: {path}");
    }

    public override void DeleteFile(string path)
    {
        Console.WriteLine($"Deleting (fake) file: {path}");
        if (Streams.Remove(path, out var stream))
            stream.Dispose();
    }

    public override void DeleteDirectory(string path)
    {
        Console.WriteLine($"Deleting (fake) directory: {path}");
    }

    public override void DeleteExpansion(ushort expansionId)
    {
        Console.WriteLine($"Deleting (fake) expansion: {SqexFile.GetExpansionFolder(expansionId)}");
    }

    public void Dispose()
    {
        foreach (var stream in Streams.Values)
            stream.Dispose();
    }
}
