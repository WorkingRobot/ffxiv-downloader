/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.Text;
using FFXIVDownloader.ZiPatch.Config;

namespace FFXIVDownloader.ZiPatch.Util;

public sealed class SqpackDatFile(BinaryReader reader) : SqpackFile(reader)
{
    public override string GetPath(ZiPatchConfig.PlatformId platform) =>
        $"{GetBasePath(platform)}.dat{FileId}";
}
