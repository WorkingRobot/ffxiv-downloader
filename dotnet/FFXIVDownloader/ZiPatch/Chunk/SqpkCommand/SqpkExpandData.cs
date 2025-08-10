/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkExpandData : SqpkChunk, ISqpkChunk<SqpkExpandData>
{
    public static char Command => 'E';

    public SqexFile TargetFile { get; protected set; }
    public long BlockOffset { get; protected set; }
    public long BlockNumber { get; protected set; }

    public SqpkExpandData(BinaryReader reader)
    {
        reader.ReadBytes(3);

        TargetFile = new SqpackDatFile(reader);

        BlockOffset = (long)reader.ReadUInt32BE() << 7;
        BlockNumber = (long)reader.ReadUInt32BE() << 7;

        reader.ReadUInt32(); // Reserved
    }

    public SqpkExpandData(BinaryReader reader, ReadOnlySpan<string> names)
    {
        TargetFile = new PlaceholderedSqexFile(names[0]);
        BlockOffset = reader.ReadInt64();
        BlockNumber = reader.ReadInt64();
    }

    static SqpkExpandData ISqpkChunk<SqpkExpandData>.Read(BinaryReader reader) =>
        new(reader);

    public override async Task ApplyAsync(ZiPatchConfig config)
    {
        var file = await config.OpenFile(TargetFile.GetPath(config.Platform)).ConfigureAwait(false);

        await file.WipeAsync(BlockOffset, BlockNumber).ConfigureAwait(false);
        await file.WriteEmptyFileBlockAt(BlockNumber >> 7, BlockOffset).ConfigureAwait(false);
    }

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.SqpkExpandData;
        names.Add(TargetFile.GetPath(ZiPatchConfig.PlatformId.Placeholder));
        writer.Write(BlockOffset);
        writer.Write(BlockNumber);
    }

    public override string ToString() =>
        $"{FourCC}:{Command}:{BlockOffset}:{BlockNumber}";
}
