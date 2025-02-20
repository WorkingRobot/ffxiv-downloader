/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;

namespace FFXIVDownloader.ZiPatch.Config;

public sealed class FilteredZiPatchConfig<T>(T @base, Predicate<string> filter, IEnumerable<string>? alreadyFilteredFiles = null) : ZiPatchConfig where T : ZiPatchConfig
{
    public T Base { get; } = @base;
    private Predicate<string> Filter { get; } = filter;

    private readonly HashSet<string> filteredFiles = [.. alreadyFilteredFiles ?? []];

    public IReadOnlySet<string> FilteredFiles => filteredFiles;

    public override Task<Stream> OpenStream(string path)
    {
        if (!Filter(path))
        {
            filteredFiles.Add(path);
            return Task.FromResult<Stream>(new BlackHoleStream());
        }

        return Base.OpenStream(path);
    }

    public override Task CreateDirectory(string path) =>
        Base.CreateDirectory(path);

    public override Task DeleteFile(string path)
    {
        if (!Filter(path))
        {
            filteredFiles.Remove(path);
            return Task.CompletedTask;
        }

        return Base.DeleteFile(path);
    }

    public override Task DeleteDirectory(string path) =>
        Base.DeleteDirectory(path);

    public override Task DeleteExpansion(ushort expansionId, Predicate<string>? shouldKeep = null)
    {
        shouldKeep ??= ShouldKeep;
        bool NewShouldKeep(string path)
        {
            var filtered = Filter(path);
            if (!filtered)
                filteredFiles.Remove(path);
            return !(filtered && !shouldKeep(path));
        }
        return Base.DeleteExpansion(expansionId, NewShouldKeep);
    }

    public override ValueTask DisposeAsync() =>
        Base.DisposeAsync();
}
