/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkDeleteData : SqpkChunk, ISqpkChunk<SqpkDeleteData>
{
    public static char Command => 'D';

    public SqexFile TargetFile { get; protected set; }
    public long BlockOffset { get; protected set; }
    public long BlockNumber { get; protected set; }

    public SqpkDeleteData(BinaryReader reader)
    {
        reader.ReadBytes(3); // Alignment

        TargetFile = new SqpackDatFile(reader);

        BlockOffset = (long)reader.ReadUInt32BE() << 7;
        BlockNumber = (long)reader.ReadUInt32BE() << 7;

        reader.ReadUInt32(); // Reserved
    }

    public SqpkDeleteData(BinaryReader reader, ReadOnlySpan<string> names)
    {
        TargetFile = new PlaceholderedSqexFile(names[0]);
        BlockOffset = reader.ReadInt64();
        BlockNumber = reader.ReadInt64();
    }

    static SqpkDeleteData ISqpkChunk<SqpkDeleteData>.Read(BinaryReader reader) =>
        new(reader);

    public override async Task ApplyAsync(ZiPatchConfig config)
    {
        var file = await config.OpenStream(TargetFile.GetPath(config.Platform)).ConfigureAwait(false);

        await SqexExtensions.WriteEmptyFileBlockAt(file, BlockOffset, BlockNumber).ConfigureAwait(false);
    }

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.SqpkDeleteData;
        names.Add(TargetFile.GetPath(ZiPatchConfig.PlatformId.Placeholder));
        writer.Write(BlockOffset);
        writer.Write(BlockNumber);
    }

    public override string ToString() =>
        $"{FourCC}:{Command}:{TargetFile}:{BlockOffset}:{BlockNumber}";
}
