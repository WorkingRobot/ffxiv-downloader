/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.IO.Hashing;
using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;
using DotNext;
using DotNext.IO;

namespace FFXIVDownloader.ZiPatch.Chunk;

public interface IZiPatchChunk
{
    Task ApplyAsync(ZiPatchConfig config);

    void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type);
}

public interface IZiPatchChunk<T> : IZiPatchChunk where T : IZiPatchChunk<T>
{
    abstract static string FourCC { get; }

    abstract static T Read(BinaryReader reader);
}

public static class ZiPatchChunk
{
    internal static UserDataSlot<long> BaseStreamOffset = new();

    // Only FileHeader, ApplyOption, Sqpk, and EOF have been observed in XIVARR+ patches
    // AddDirectory and DeleteDirectory can theoretically happen, so they're implemented
    // ApplyFreeSpace doesn't seem to show up anymore, and EntryFile will just error out
    public static async Task<IZiPatchChunk> ReadAsync(Stream stream, CancellationToken token = default)
    {
        try
        {
            var streamReader = new BinaryReader(stream);
            var size = checked((int)streamReader.ReadUInt32BE());

            // header + size of chunk + checksum
            var readSize = 4 + size + 4;

            var data = new byte[readSize];

            // Read into buffer
            await stream.ReadExactlyAsync(data, token).ConfigureAwait(false);

            ReadOnlyMemory<byte> memory = data.AsMemory();

            string fourcc;
            uint checksum;
            {
                using var reader = new BinaryReader(memory.AsStream());
                fourcc = reader.ReadFixedLengthString(4);
                reader.BaseStream.Position += size;
                checksum = reader.ReadUInt32BE();
            }

            var calculatedChecksum = Crc32.HashToUInt32(memory.Span[..^4]);
            if (checksum != calculatedChecksum)
                throw new ZiPatchException($"Checksum mismatch {fourcc}: File: {checksum:X8} != Calculated: {calculatedChecksum:X8}");

            {
                using var reader = new BinaryReader(memory[4..^4].AsStream());
                reader.GetUserData().Set(BaseStreamOffset, stream.Position - readSize + 4);
                return
                    TryRead<FileHeaderChunk>(fourcc, reader) ??
                    TryRead<ApplyOptionChunk>(fourcc, reader) ??
                    TryRead<ApplyFreeSpaceChunk>(fourcc, reader) ??
                    TryRead<AddDirectoryChunk>(fourcc, reader) ??
                    TryRead<DeleteDirectoryChunk>(fourcc, reader) ??
                    TryRead<SqpkChunk>(fourcc, reader) ??
                    TryRead<EndOfFileChunk>(fourcc, reader) ??
                    TryRead<XXXXChunk>(fourcc, reader) ??
                    throw new ZiPatchException($"Invalid chunk type {fourcc}");
            }
        }
        catch (EndOfStreamException e)
        {
            throw new ZiPatchException("Could not get chunk", e);
        }
    }

    private static IZiPatchChunk? TryRead<T>(string fourcc, BinaryReader reader) where T : IZiPatchChunk<T> =>
        T.FourCC == fourcc ? T.Read(reader) : default;
}
