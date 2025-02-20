/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using DotMake.CommandLine;
using FFXIVDownloader.ZiPatch.Util;

namespace FFXIVDownloader.ZiPatch.Config;

public sealed class PersistentZiPatchConfig(string gamePath) : ZiPatchConfig
{
    private Dictionary<string, FileStream> Streams { get; } = [];

    public string GamePath { get; } = Path.TrimEndingDirectorySeparator(Path.GetFullPath(gamePath));
    public IReadOnlyCollection<string> OpenedStreams => Streams.Keys;

    private const int STREAM_OPEN_WAIT_MS = 1000;
    private const int STREAM_OPEN_TRIES = 5;

    private string GetPath(string filePath) => Path.Join(GamePath, filePath);

    public override async Task<Stream> OpenStream(string path)
    {
        path = GetPath(path);
        if (Streams.TryGetValue(path, out var stream))
            return stream;

        if (Path.GetDirectoryName(path) is { } dirName)
            Directory.CreateDirectory(dirName);

        var tries = STREAM_OPEN_TRIES;
        do
        {
            try
            {
                stream = new FileStream(path, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.Read, 1 << 16);
                Streams.Add(path, stream);
                return stream;
            }
            catch (IOException)
            {
                if (tries == 0)
                    throw;

                await Task.Delay(STREAM_OPEN_WAIT_MS).ConfigureAwait(false);
            }
        } while (0 < --tries);

        throw new FileNotFoundException($"Could not find file {path}");
    }

    public override Task CreateDirectory(string path)
    {
        Directory.CreateDirectory(GetPath(path));
        return Task.CompletedTask;
    }

    public override async Task DeleteFile(string path)
    {
        path = GetPath(path);
        if (Streams.Remove(path, out var stream))
            await stream.DisposeAsync().ConfigureAwait(false);
        File.Delete(path);
    }

    public override Task DeleteDirectory(string path)
    {
        Directory.Delete(GetPath(path));
        return Task.CompletedTask;
    }

    public override async Task DeleteExpansion(ushort expansionId, Predicate<string>? shouldKeep = null)
    {
        shouldKeep ??= ShouldKeep;

        var expansionFolder = SqexExtensions.GetExpansionFolder(expansionId);

        foreach (var dirName in ExpacFolders)
        {
            var dir = GetPath(Path.Combine(dirName, expansionFolder));
            if (Directory.Exists(dir))
            {
                foreach (var file in Directory.GetFiles(dir))
                {
                    if (!shouldKeep(file))
                    {
                        if (Streams.Remove(file, out var stream))
                            await stream.DisposeAsync().ConfigureAwait(false);
                        File.Delete(file);
                    }
                }
            }
        }
    }

    public override async ValueTask DisposeAsync()
    {
        foreach (var stream in Streams.Values)
            await stream.DisposeAsync().ConfigureAwait(false);
        Streams.Clear();
    }
}
