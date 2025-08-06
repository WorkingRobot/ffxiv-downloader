/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Config;

namespace FFXIVDownloader.ZiPatch.Util;

public abstract class SqexFile
{
    public abstract string GetPath(ZiPatchConfig.PlatformId platform);

    public override string ToString() =>
        GetPath(ZiPatchConfig.PlatformId.Win32);
}
