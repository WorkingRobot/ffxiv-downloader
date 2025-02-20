/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;

namespace FFXIVDownloader.ZiPatch.Config;

public abstract class ZiPatchConfig : IAsyncDisposable
{
    public static string[] ExpacFolders { get; } = ["sqpack", "movie"];
    private static string[] DelExpacFilter { get; } = [".var", "00000.bk2", "00001.bk2", "00002.bk2", "00003.bk2"];

    public enum PlatformId : ushort
    {
        Win32 = 0,
        Ps3 = 1,
        Ps4 = 2,
        Ps5 = 3,
        Lys = 4,

        Placeholder = ushort.MaxValue - 1,
        Unknown = ushort.MaxValue
    }

    public PlatformId Platform { get; set; }
    public bool IgnoreMissing { get; set; }
    public bool IgnoreOldMismatch { get; set; }

    public abstract Task<Stream> OpenStream(string path);

    public abstract Task CreateDirectory(string path);

    public abstract Task DeleteFile(string path);

    public abstract Task DeleteDirectory(string path);

    public abstract Task DeleteExpansion(ushort expansionId, Predicate<string>? shouldKeep = null);

    public abstract ValueTask DisposeAsync();

    public static bool ShouldKeep(string path) =>
        DelExpacFilter.Any(path.EndsWith);
}
