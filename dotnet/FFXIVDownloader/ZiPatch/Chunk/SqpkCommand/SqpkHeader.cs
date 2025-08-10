/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;
using DotNext;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkHeader : SqpkChunk, ISqpkChunk<SqpkHeader>
{
    public static char Command => 'H';

    public enum TargetFileKind : byte
    {
        Dat = (byte)'D',
        Index = (byte)'I'
    }
    public enum TargetHeaderKind : byte
    {
        Version = (byte)'V',
        Index = (byte)'I',
        Data = (byte)'D'
    }

    public const int HEADER_SIZE = 1024;

    public TargetFileKind FileKind { get; }
    public TargetHeaderKind HeaderKind { get; }
    public SqexFile TargetFile { get; }

    public long HeaderDataPatchOffset { get; }
    public byte[] HeaderData { get; }

    public SqpkHeader(BinaryReader reader)
    {
        FileKind = (TargetFileKind)reader.ReadByte();
        HeaderKind = (TargetHeaderKind)reader.ReadByte();
        reader.ReadByte(); // Alignment

        if (FileKind == TargetFileKind.Dat)
            TargetFile = new SqpackDatFile(reader);
        else
            TargetFile = new SqpackIndexFile(reader);

        HeaderDataPatchOffset = reader.GetUserData().Get(ZiPatchChunk.BaseStreamOffset) + reader.BaseStream.Position;
        HeaderData = reader.ReadBytes(HEADER_SIZE);
    }

    public SqpkHeader(BinaryReader reader, ReadOnlySpan<string> names)
    {
        TargetFile = new PlaceholderedSqexFile(names[0]);
        HeaderKind = (TargetHeaderKind)reader.ReadByte();
        HeaderDataPatchOffset = reader.ReadInt64();
        HeaderData = null!;
    }

    static SqpkHeader ISqpkChunk<SqpkHeader>.Read(BinaryReader reader) =>
        new(reader);

    public override async Task ApplyAsync(ZiPatchConfig config)
    {
        var file = await config.OpenFile(TargetFile.GetPath(config.Platform)).ConfigureAwait(false);

        await file.WriteAsync(HeaderData, HeaderKind == TargetHeaderKind.Version ? 0 : HEADER_SIZE).ConfigureAwait(false);
    }

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.SqpkHeader;
        names.Add(TargetFile.GetPath(ZiPatchConfig.PlatformId.Placeholder));
        writer.Write((byte)HeaderKind);
        writer.Write(HeaderDataPatchOffset);
    }

    public override string ToString() =>
        $"{FourCC}:{Command}:{FileKind}:{HeaderKind}:{TargetFile}";
}
