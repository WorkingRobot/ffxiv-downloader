/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.IO.Compression;
using DotNext;
using DotNext.IO;
using FFXIVDownloader.ZiPatch.Chunk;
using FFXIVDownloader.ZiPatch.Config;

namespace FFXIVDownloader.ZiPatch.Util;

public class SqpkCompressedBlock
{
    public int HeaderSize { get; }
    public int CompressedSize { get; }
    public int DataSize { get; }

    public bool IsCompressed => CompressedSize != 32000;

    public long DataBlockPatchOffset { get; }
    public byte[] DataBlock { get; }

    public SqpkCompressedBlock(BinaryReader reader)
    {
        var pos = reader.BaseStream.Position;

        HeaderSize = reader.ReadInt32();
        reader.ReadUInt32(); // Pad
        CompressedSize = reader.ReadInt32();
        DataSize = reader.ReadInt32();
        reader.BaseStream.Position = pos + HeaderSize;

        DataBlockPatchOffset = reader.GetUserData().Get(ZiPatchChunk.BaseStreamOffset) + reader.BaseStream.Position;
        if (IsCompressed)
            DataBlock = reader.ReadBytes(CompressedSize);
        else
            DataBlock = reader.ReadBytes(DataSize);

        // https://xiv.dev/data-files/zipatch/sqpk#type-f-file-operation
        // https://github.com/goatcorp/FFXIVQuickLauncher/blob/77115d3f7d920179ce83f0708ec97ba2450ce795/src/XIVLauncher.Common/Patching/ZiPatch/Util/SqpkCompressedBlock.cs#L13
        // The block is actually aligned to 128 bytes. It's just aligned to the 
        // beginning of the block, not to the beginning of the data or the patch file.
        var readSize = reader.BaseStream.Position - pos;
        var alignedSize = Align(readSize, 128);
        reader.BaseStream.Position = pos + alignedSize;
    }

    public SqpkCompressedBlock(BinaryReader reader, ReadOnlySpan<string> nameMap)
    {
        CompressedSize = reader.ReadInt32();
        DataSize = reader.ReadInt32();
        DataBlockPatchOffset = reader.ReadInt64();
        DataBlock = null!;
    }

    public async ValueTask DecompressIntoAsync(ITargetFile file, long offset)
    {
        ReadOnlyMemory<byte> block = DataBlock.AsMemory();
        if (IsCompressed)
        {
            var uncompData = new byte[DataSize];
            using (var ms = new MemoryStream(uncompData))
            using (var stream = new DeflateStream(block.AsStream(), CompressionMode.Decompress))
            {
                await stream.CopyToAsync(ms).ConfigureAwait(false);
            }
            await file.WriteAsync(uncompData, offset).ConfigureAwait(false);
        }
        else
            await file.WriteAsync(block, offset).ConfigureAwait(false);
    }

    public void WriteLUT(BinaryWriter writer)
    {
        writer.Write(CompressedSize);
        writer.Write(DataSize);
        writer.Write(DataBlockPatchOffset);
    }

    private static long Align(long value, long alignment)
    {
        return (value + alignment - 1) & ~(alignment - 1);
    }
}
