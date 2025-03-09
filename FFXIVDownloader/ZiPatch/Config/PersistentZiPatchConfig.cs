/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;

namespace FFXIVDownloader.ZiPatch.Config;

public sealed class PersistentZiPatchConfig(string gamePath) : ZiPatchConfig
{
    private Dictionary<string, PersistentTargetFile> Files { get; } = [];
    private SemaphoreSlim Lock { get; } = new(1);

    public string GamePath { get; } = Path.TrimEndingDirectorySeparator(Path.GetFullPath(gamePath));
    public IReadOnlyCollection<string> OpenedStreams => Files.Keys;

    private const int STREAM_OPEN_WAIT_MS = 1000;
    private const int STREAM_OPEN_TRIES = 1;

    private string GetPath(string filePath) =>
        Path.Join(GamePath, filePath);

    public override async Task<ITargetFile> OpenFile(string path)
    {
        using var sema = await SemaphoreLock.CreateAsync(Lock).ConfigureAwait(false);

        path = GetPath(path);

        if (Files.TryGetValue(path, out var file))
            return file;

        if (Path.GetDirectoryName(path) is { } dirName)
            Directory.CreateDirectory(dirName);

        var tries = STREAM_OPEN_TRIES;
        do
        {
            try
            {
                file = new(path);
                Files.Add(path, file);
                return file;
            }
            catch (IOException)
            {
                if (--tries == 0)
                    throw;

                await Task.Delay(STREAM_OPEN_WAIT_MS).ConfigureAwait(false);
            }
        } while (true);
    }

    public override Task CreateDirectory(string path)
    {
        Directory.CreateDirectory(GetPath(path));
        return Task.CompletedTask;
    }

    public override async Task DeleteFile(string path)
    {
        using var sema = await SemaphoreLock.CreateAsync(Lock).ConfigureAwait(false);

        path = GetPath(path);
        if (Files.Remove(path, out var stream))
            stream.Dispose();
        File.Delete(path);
    }

    public override Task DeleteDirectory(string path)
    {
        Directory.Delete(GetPath(path));
        return Task.CompletedTask;
    }

    public override async Task DeleteExpansion(ushort expansionId, Predicate<string>? shouldKeep = null)
    {
        using var sema = await SemaphoreLock.CreateAsync(Lock).ConfigureAwait(false);

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
                        if (Files.Remove(file, out var stream))
                            stream.Dispose();
                        File.Delete(file);
                    }
                }
            }
        }
    }

    public override async ValueTask DisposeAsync()
    {
        using var sema = await SemaphoreLock.CreateAsync(Lock).ConfigureAwait(false);

        var files = new List<PersistentTargetFile>(Files.Values);
        Files.Clear();
        foreach (var stream in files)
            stream.Dispose();
        Files.Clear();
    }
}
