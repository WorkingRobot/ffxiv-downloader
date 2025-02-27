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
    private SemaphoreSlim Lock { get; } = new(1);

    private readonly HashSet<string> filteredFiles = [.. alreadyFilteredFiles ?? []];

    public IReadOnlySet<string> FilteredFiles => new HashSet<string>(filteredFiles);

    public override async Task<ITargetFile> OpenFile(string path)
    {
        using var sema = await SemaphoreLock.CreateAsync(Lock).ConfigureAwait(false);
        if (!Filter(path))
        {
            filteredFiles.Add(path);
            return new BlackHoleTargetFile();
        }

        return await Base.OpenFile(path).ConfigureAwait(false);
    }

    public override Task CreateDirectory(string path) =>
        Base.CreateDirectory(path);

    public override async Task DeleteFile(string path)
    {
        using var sema = await SemaphoreLock.CreateAsync(Lock).ConfigureAwait(false);
        if (!Filter(path))
        {
            filteredFiles.Remove(path);
            return;
        }

        await Base.DeleteFile(path).ConfigureAwait(false);
    }

    public override Task DeleteDirectory(string path) =>
        Base.DeleteDirectory(path);

    public override async Task DeleteExpansion(ushort expansionId, Predicate<string>? shouldKeep = null)
    {
        using var sema = await SemaphoreLock.CreateAsync(Lock).ConfigureAwait(false);

        shouldKeep ??= ShouldKeep;
        bool NewShouldKeep(string path)
        {
            var filtered = Filter(path);
            if (!filtered)
                filteredFiles.Remove(path);
            return !(filtered && !shouldKeep(path));
        }
        await Base.DeleteExpansion(expansionId, NewShouldKeep).ConfigureAwait(false);
    }

    public override ValueTask DisposeAsync() =>
        Base.DisposeAsync();
}
