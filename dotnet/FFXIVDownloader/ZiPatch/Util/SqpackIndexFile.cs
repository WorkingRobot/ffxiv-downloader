/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Config;

namespace FFXIVDownloader.ZiPatch.Util;

public class SqpackIndexFile(BinaryReader reader) : SqpackFile(reader)
{
    public override string GetPath(ZiPatchConfig.PlatformId platform) =>
        $"{GetBasePath(platform)}.index{(FileId == 0 ? string.Empty : FileId.ToString())}";
}
