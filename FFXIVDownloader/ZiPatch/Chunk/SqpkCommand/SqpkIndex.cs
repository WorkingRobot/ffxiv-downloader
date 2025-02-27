/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkIndex : SqpkChunk, ISqpkChunk<SqpkIndex>
{
    // This is a NOP on recent patcher versions.
    public static char Command => 'I';

    public enum IndexCommandKind : byte
    {
        Add = (byte)'A',
        Delete = (byte)'D'
    }

    public IndexCommandKind IndexCommand { get; protected set; }
    public bool IsSynonym { get; protected set; }
    public SqpackIndexFile TargetFile { get; protected set; }
    public ulong FileHash { get; protected set; }
    public uint BlockOffset { get; protected set; }

    // TODO: Figure out what this is used for
    public uint BlockNumber { get; protected set; }

    public SqpkIndex(BinaryReader Reader)
    {
        IndexCommand = (IndexCommandKind)Reader.ReadByte();
        IsSynonym = Reader.ReadBoolean();
        Reader.ReadByte(); // Alignment

        TargetFile = new SqpackIndexFile(Reader);

        FileHash = Reader.ReadUInt64BE();

        BlockOffset = Reader.ReadUInt32BE();
        BlockNumber = Reader.ReadUInt32BE();
    }

    static SqpkIndex ISqpkChunk<SqpkIndex>.Read(BinaryReader reader) =>
        new(reader);

    public override Task ApplyAsync(ZiPatchConfig config) =>
        Task.CompletedTask;

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type) =>
        type = ChunkType.SqpkIndex;

    public override string ToString() =>
        $"{FourCC}:{Command}:{IndexCommand}:{IsSynonym}:{TargetFile}:{FileHash:X8}:{BlockOffset}:{BlockNumber}";
}
