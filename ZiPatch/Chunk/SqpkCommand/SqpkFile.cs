/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;
using static FFXIVDownloader.ZiPatch.Config.ZiPatchConfig;

namespace FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;

public class SqpkFile : SqpkChunk, ISqpkChunk<SqpkFile>
{
    public static char Command => 'F';

    public enum OperationKind : byte
    {
        AddFile = (byte)'A',
        RemoveAll = (byte)'R',

        // I've seen no cases in the wild of these two
        DeleteFile = (byte)'D',
        MakeDirTree = (byte)'M'
    }

    public OperationKind Operation { get; }
    public long FileOffset { get; }
    public long FileSize { get; }
    public ushort ExpansionId { get; }
    public SqexFile TargetFile { get; }

    public List<SqpkCompressedBlock>? CompressedData { get; }

    public SqpkFile(BinaryReader reader)
    {
        Operation = (OperationKind)reader.ReadByte();
        reader.ReadBytes(2); // Alignment

        FileOffset = reader.ReadInt64BE();
        FileSize = reader.ReadInt64BE();

        var pathLen = reader.ReadInt32BE();

        ExpansionId = reader.ReadUInt16BE();
        reader.ReadBytes(2);

        TargetFile = new PlainSqexFile(reader.ReadFixedLengthString(pathLen));

        if (Operation == OperationKind.AddFile)
        {
            CompressedData = [];

            while (reader.BaseStream.Length > reader.BaseStream.Position)
                CompressedData.Add(new SqpkCompressedBlock(reader));
        }
    }

    public SqpkFile(BinaryReader reader, OperationKind op, ReadOnlySpan<string> names)
    {
        TargetFile = null!;
        Operation = op;
        switch (Operation)
        {
            case OperationKind.AddFile:
                TargetFile = new PlaceholderedSqexFile(names[0]);
                FileOffset = reader.ReadInt64();
                var compDataCount = reader.ReadInt32();
                CompressedData = new(compDataCount);
                for (var i = 0; i < compDataCount; ++i)
                    CompressedData.Add(new(reader, names));
                break;

            case OperationKind.RemoveAll:
                ExpansionId = reader.ReadUInt16();
                break;

            case OperationKind.DeleteFile:
                TargetFile = new PlaceholderedSqexFile(names[0]);
                break;

            case OperationKind.MakeDirTree:
                TargetFile = new PlaceholderedSqexFile(names[0]);
                break;

            default:
                throw new ZiPatchException($"Operation {Operation} is not supported.");
        }
    }

    static SqpkFile ISqpkChunk<SqpkFile>.Read(BinaryReader reader) =>
        new(reader);

    public override async Task ApplyAsync(ZiPatchConfig config)
    {
        switch (Operation)
        {
            case OperationKind.AddFile:
                var fileStream = await config.OpenStream(TargetFile.GetPath(config.Platform)).ConfigureAwait(false);

                if (FileOffset == 0)
                    fileStream.SetLength(0);

                fileStream.Position = FileOffset;
                foreach (var block in CompressedData!)
                    await block.DecompressIntoAsync(fileStream).ConfigureAwait(false);

                break;

            case OperationKind.RemoveAll:
                await config.DeleteExpansion(ExpansionId).ConfigureAwait(false);
                break;

            case OperationKind.DeleteFile:
                await config.DeleteFile(TargetFile.GetPath(config.Platform)).ConfigureAwait(false);
                break;

            case OperationKind.MakeDirTree:
                await config.CreateDirectory(TargetFile.GetPath(config.Platform)).ConfigureAwait(false);
                break;

            default:
                throw new ZiPatchException($"Operation {Operation} is not supported.");
        }
    }

    public override void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        switch (Operation)
        {
            case OperationKind.AddFile:
                type = ChunkType.SqpkFileAdd;
                names.Add(TargetFile.GetPath(PlatformId.Placeholder));
                writer.Write(FileOffset);
                writer.Write(CompressedData!.Count);
                foreach (var block in CompressedData!)
                    block.WriteLUT(writer);
                break;

            case OperationKind.RemoveAll:
                type = ChunkType.SqpkFileDelExpac;
                writer.Write(ExpansionId);
                break;

            case OperationKind.DeleteFile:
                type = ChunkType.SqpkFileDelete;
                names.Add(TargetFile.GetPath(PlatformId.Placeholder));
                break;

            case OperationKind.MakeDirTree:
                type = ChunkType.SqpkFileMkdir;
                names.Add(TargetFile.GetPath(PlatformId.Placeholder));
                break;

            default:
                throw new ZiPatchException($"Operation {Operation} is not supported.");
        }
    }

    public override string ToString() =>
        $"{FourCC}:{Command}:{Operation}:{FileOffset}:{FileSize}:{ExpansionId}:{TargetFile}";
}
