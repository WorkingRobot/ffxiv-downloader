/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkTargetInfo : SqpkChunk, ISqpkChunk<SqpkTargetInfo>
{
    // Only Platform is used on recent patcher versions
    public static char Command => 'T';

    // US/EU/JP are Global
    // ZH seems to also be Global
    // KR is unknown
    public enum RegionId : short
    {
        Global = -1
    }

    public ZiPatchConfig.PlatformId Platform { get; }
    public RegionId Region { get; }
    public bool IsDebug { get; }
    public ushort Version { get; }
    public ulong DeletedDataSize { get; }
    public ulong SeekCount { get; }

    public SqpkTargetInfo(BinaryReader reader)
    {
        // Reserved
        reader.ReadBytes(3);

        Platform = (ZiPatchConfig.PlatformId)reader.ReadUInt16BE();
        Region = (RegionId)reader.ReadInt16BE();
        IsDebug = reader.ReadInt16BE() != 0;
        Version = reader.ReadUInt16BE();
        DeletedDataSize = reader.ReadUInt64();
        SeekCount = reader.ReadUInt64();

        // Empty 32 + 64 bytes
    }

    static SqpkTargetInfo ISqpkChunk<SqpkTargetInfo>.Read(BinaryReader reader) =>
        new(reader);

    public override Task ApplyAsync(ZiPatchConfig config)
    {
        config.Platform = Platform;
        return Task.CompletedTask;
    }

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.SqpkTargetInfo;
        writer.Write((ushort)Platform);
        writer.Write((short)Region);
        writer.Write(IsDebug);
        writer.Write(Version);
        writer.Write(DeletedDataSize);
        writer.Write(SeekCount);
    }

    public override string ToString() =>
        $"{FourCC}:{Command}:{Platform}:{Region}:{IsDebug}:{Version}:{DeletedDataSize}:{SeekCount}";
}
