/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk;

public interface ISqpkChunk<T> : IZiPatchChunk<SqpkChunk> where T : SqpkChunk, ISqpkChunk<T>
{
    abstract static char Command { get; }

    new abstract static T Read(BinaryReader reader);
}

public abstract class SqpkChunk : IZiPatchChunk<SqpkChunk>
{
    public static string FourCC => "SQPK";

    public static SqpkChunk Read(BinaryReader reader)
    {
        try
        {
            // Have not seen this differ from size
            var innerSize = reader.ReadInt32BE();
            if (reader.BaseStream.Length != innerSize)
                throw new ZiPatchException("Sqpk size mismatch");

            var command = reader.ReadFixedLengthString(1)[0];

            return
                TryRead<SqpkAddData>(command, reader) ??
                TryRead<SqpkDeleteData>(command, reader) ??
                TryRead<SqpkHeader>(command, reader) ??
                TryRead<SqpkTargetInfo>(command, reader) ??
                TryRead<SqpkExpandData>(command, reader) ??
                TryRead<SqpkIndex>(command, reader) ??
                TryRead<SqpkFile>(command, reader) ??
                TryRead<SqpkPatchInfo>(command, reader) ??
                throw new ZiPatchException($"Invalid sqpk command {command}");
        }
        catch (EndOfStreamException e)
        {
            throw new ZiPatchException("Could not get command", e);
        }
    }

    public abstract Task ApplyAsync(ZiPatchConfig config);

    public abstract void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type);

    public override string ToString() =>
        FourCC;

    private static SqpkChunk? TryRead<T>(char command, BinaryReader reader) where T : SqpkChunk, ISqpkChunk<T> =>
        T.Command == command ? (SqpkChunk?)T.Read(reader) : default;
}
