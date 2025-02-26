/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;
using DotNext;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkAddData : SqpkChunk, ISqpkChunk<SqpkAddData>
{
    public static char Command => 'A';

    public SqexFile TargetFile { get; }
    public long BlockOffset { get; }
    public long BlockNumber { get; }
    public long BlockDeleteNumber { get; }

    public long BlockDataPatchOffset { get; }
    public byte[] BlockData { get; }

    public SqpkAddData(BinaryReader reader)
    {
        reader.ReadBytes(3); // Alignment

        TargetFile = new SqpackDatFile(reader);

        BlockOffset = (long)reader.ReadUInt32BE() << 7;
        BlockNumber = (long)reader.ReadUInt32BE() << 7;
        BlockDeleteNumber = (long)reader.ReadUInt32BE() << 7;

        BlockDataPatchOffset = reader.GetUserData().Get(ZiPatchChunk.BaseStreamOffset) + reader.BaseStream.Position;
        BlockData = reader.ReadBytes(checked((int)BlockNumber));
    }

    public SqpkAddData(BinaryReader reader, ReadOnlySpan<string> names)
    {
        TargetFile = new PlaceholderedSqexFile(names[0]);
        BlockOffset = reader.ReadInt64();
        BlockNumber = reader.ReadInt64();
        BlockDeleteNumber = reader.ReadInt64();
        BlockDataPatchOffset = reader.ReadInt64();
        BlockData = null!;
    }

    static SqpkAddData ISqpkChunk<SqpkAddData>.Read(BinaryReader reader) =>
        new(reader);

    public override async Task ApplyAsync(ZiPatchConfig config)
    {
        var file = await config.OpenFile(TargetFile.GetPath(config.Platform)).ConfigureAwait(false);

        await file.WriteAsync(BlockData, BlockOffset).ConfigureAwait(false);
        await file.WipeAsync(BlockDeleteNumber, BlockOffset + BlockData.Length).ConfigureAwait(false);
    }

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.SqpkAddData;
        names.Add(TargetFile.GetPath(ZiPatchConfig.PlatformId.Placeholder));
        writer.Write(BlockOffset);
        writer.Write(BlockNumber);
        writer.Write(BlockDeleteNumber);
        writer.Write(BlockDataPatchOffset);
    }

    public override string ToString() =>
        $"{FourCC}:{Command}:{TargetFile}:{BlockOffset}:{BlockNumber}:{BlockDeleteNumber}";
}
