/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkPatchInfo : SqpkChunk, ISqpkChunk<SqpkPatchInfo>
{
    // This is a NOP on recent patcher versions
    public static char Command => 'X';

    // Don't know what this stuff is for
    public byte Status { get; protected set; }
    public byte Version { get; protected set; }
    public ulong InstallSize { get; protected set; }

    public SqpkPatchInfo(BinaryReader Reader)
    {
        Status = Reader.ReadByte();
        Version = Reader.ReadByte();
        Reader.ReadByte(); // Alignment

        InstallSize = Reader.ReadUInt64BE();
    }

    static SqpkPatchInfo ISqpkChunk<SqpkPatchInfo>.Read(BinaryReader reader) =>
        new(reader);

    public override Task ApplyAsync(ZiPatchConfig config) =>
        Task.CompletedTask;

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.SqpkPatchInfo;
        writer.Write(Status);
        writer.Write(Version);
        writer.Write(InstallSize);
    }

    public override string ToString() =>
        $"{FourCC}:{Command}:{Status}:{Version}:{InstallSize}";
}
