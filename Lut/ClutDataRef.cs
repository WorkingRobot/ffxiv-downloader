using FFXIVDownloader.Thaliak;
using System.Collections.Frozen;
using System.Runtime.CompilerServices;

namespace FFXIVDownloader.Lut;

public struct ClutDataRef
{
    public enum RefType : byte
    {
        Patch = 0,
        Zero = 1,
        EmptyBlock = 2,
    }

    public RefType Type { get; init; }
    public long Offset { get; set; }
    public int Length { get; set; }
    public int? BlockCount { get; init; }
    public ClutPatchRef? Patch { get; init; }

    public ClutDataRef()
    {
        Type = RefType.Zero;
        Offset = 0;
        Length = 0;
    }

    public ClutDataRef(BinaryReader reader, ReadOnlySpan<ParsedVersionString> patchMap, ref long patchOffset)
    {
        Type = (RefType)reader.ReadByte();
        if (Type == RefType.EmptyBlock)
            BlockCount = reader.ReadInt32();
        if (Type == RefType.Patch)
            Patch = new(reader, patchMap, ref patchOffset);
    }

    public void ReadOffset(BinaryReader reader, ref long lastOffset)
    {
        Offset = reader.Read7BitEncodedInt64() + lastOffset;
        lastOffset = Offset;
    }

    public void ReadLength(BinaryReader reader)
    {
        Length = reader.Read7BitEncodedInt();
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public readonly void Write(BinaryWriter writer, FrozenDictionary<ParsedVersionString, int> patchMap, ref long patchOffset)
    {
        writer.Write((byte)Type);
        if (Type == RefType.EmptyBlock)
            writer.Write(BlockCount!.Value);
        if (Type == RefType.Patch)
            Patch!.Value.Write(writer, patchMap, ref patchOffset);
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public readonly void WriteOffset(BinaryWriter writer, ref long lastOffset)
    {
        writer.Write7BitEncodedInt64(Offset - lastOffset);
        lastOffset = Offset;
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public readonly void WriteLength(BinaryWriter writer)
    {
        writer.Write7BitEncodedInt(Length);
    }

    public static ClutDataRef FromRawPatchData(ParsedVersionString patch, long patchOffset, long fileOffset, int length)
    {
        return new ClutDataRef
        {
            Type = RefType.Patch,
            Offset = fileOffset,
            Length = length,
            Patch = new ClutPatchRef
            {
                Patch = patch,
                Offset = patchOffset,
                Size = length,
                IsCompressed = false
            }
        };
    }

    public static ClutDataRef FromZeros(long fileOffset, int length)
    {
        return new ClutDataRef
        {
            Type = RefType.Zero,
            Offset = fileOffset,
            Length = length,
            Patch = null
        };
    }

    public static (ClutDataRef, ClutDataRef?) FromEmpty(long fileOffset, int length)
    {
        if ((length & 0x7F) != 0)
            throw new ArgumentException("Length must be a multiple of 128", nameof(length));
        var empty = new ClutDataRef
        {
            Type = RefType.EmptyBlock,
            Offset = fileOffset,
            Length = 24,
            BlockCount = length >> 7,
            Patch = null
        };
        ClutDataRef? zero = null;
        if (length != 0)
        {
            zero = new ClutDataRef
            {
                Type = RefType.Zero,
                Offset = fileOffset + 24,
                Length = length - 24,
                Patch = null
            };
        }
        return (empty, zero);
    }

    public static ClutDataRef FromCompressedPatchData(ParsedVersionString patch, long patchOffset, long fileOffset, int compressedLength, int length)
    {
        return new ClutDataRef
        {
            Type = RefType.Patch,
            Offset = fileOffset,
            Length = length,
            Patch = new ClutPatchRef
            {
                Patch = patch,
                Offset = patchOffset,
                Size = compressedLength,
                IsCompressed = true
            }
        };
    }
}
