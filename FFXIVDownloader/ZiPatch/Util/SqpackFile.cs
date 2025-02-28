/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using static FFXIVDownloader.ZiPatch.Config.ZiPatchConfig;

namespace FFXIVDownloader.ZiPatch.Util;

public abstract class SqpackFile : SqexFile
{
    protected ushort MainId { get; }
    protected ushort SubId { get; }
    protected uint FileId { get; }

    protected byte ExpansionId => (byte)(SubId >> 8);

    protected SqpackFile(BinaryReader reader)
    {
        MainId = reader.ReadUInt16BE();
        SubId = reader.ReadUInt16BE();
        FileId = reader.ReadUInt32BE();
    }

    public static string GetPlatformName(PlatformId platform) =>
        platform switch
        {
            PlatformId.Win32 => "win32",
            PlatformId.Ps3 => "ps3",
            PlatformId.Ps4 => "ps4",
            PlatformId.Ps5 => "ps5",
            PlatformId.Lys => "lys",
            PlatformId.Placeholder => "%PLACEHOLDER%",
            _ => "unknown"
        };

    protected string GetBasePath(PlatformId platform) =>
        $@"/sqpack/{SqexExtensions.GetExpansionFolder(ExpansionId)}/{MainId:x2}{SubId:x4}.{GetPlatformName(platform)}";
}
