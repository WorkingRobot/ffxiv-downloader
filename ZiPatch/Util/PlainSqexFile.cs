/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Config;

namespace FFXIVDownloader.ZiPatch.Util;

public sealed class PlainSqexFile(string path) : SqexFile
{
    public string Path { get; } = path;

    public override string GetPath(ZiPatchConfig.PlatformId platform) =>
        Path;
}
