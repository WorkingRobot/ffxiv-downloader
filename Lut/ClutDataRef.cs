using FFXIVDownloader.Thaliak;
using System.Collections.Frozen;
using System.Runtime.CompilerServices;

namespace FFXIVDownloader.Lut;

public struct ClutDataRef
public struct ClutDataRef : IEquatable<ClutDataRef>
{
    public enum RefType : byte
    {
        Patch = 0,
        Zero = 1,
        EmptyBlock = 2,
        SplitPatch = 3,
    }

    public RefType Type { get; init; }
    public long Offset { get; set; }
    public int Length { get; set; }
    public int? BlockCount { get; init; }
    public int? PatchOffset { get; init; }
    public ClutPatchRef? Patch { get; init; }

    public readonly long End => Offset + Length;

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
        if (Type is RefType.Patch or RefType.SplitPatch)
            Patch = new(reader, patchMap, ref patchOffset);
        if (Type == RefType.SplitPatch)
            PatchOffset = reader.Read7BitEncodedInt();
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
        if (Type is RefType.Patch or RefType.SplitPatch)   
            Patch!.Value.Write(writer, patchMap, ref patchOffset);
        if (Type == RefType.SplitPatch)
            writer.Write7BitEncodedInt(PatchOffset ?? 0);
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

    public readonly bool Equals(ClutDataRef other)
    {
        if (Type != other.Type)
            return false;
        if (Offset != other.Offset)
            return false;
        if (Length != other.Length)
            return false;
        if (BlockCount != other.BlockCount)
            return false;
        if (PatchOffset != other.PatchOffset)
            return false;
        if (Patch != other.Patch)
            return false;
        return true;
    }

    public override readonly bool Equals(object? obj) => obj is ClutDataRef other && Equals(other);

    public override readonly int GetHashCode() => HashCode.Combine(Type, Offset, Length, BlockCount, PatchOffset, Patch);

    public static bool operator ==(ClutDataRef left, ClutDataRef right) => left.Equals(right);

    public static bool operator !=(ClutDataRef left, ClutDataRef right) => !left.Equals(right);

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

    // fileOffset is the offset to write to the file
    // patchFileOffset is the offset of the uncompressed data in the patch
    public static ClutDataRef FromSplitPatchData(ClutPatchRef patch, long fileOffset, int patchOffset, int length)
    {
        return new ClutDataRef
        {
            Type = RefType.SplitPatch,
            Offset = fileOffset,
            Length = length,
            PatchOffset = patchOffset,
            Patch = patch
        };
    }

    public static ClutDataRef FromSliceInterval(ref readonly ClutDataRef source, long fileOffsetStart, long fileOffsetEnd) =>
        FromSlice(in source, fileOffsetStart, checked((int)(fileOffsetEnd - fileOffsetStart)));

    public static ClutDataRef FromSlice(ref readonly ClutDataRef source, long fileOffset, int length)
    {
        return source.Type switch
        {
            RefType.Zero => FromZeros(fileOffset, length),
            RefType.EmptyBlock => throw new InvalidOperationException("Cannot slice an EmptyBlock"),
            RefType.Patch => FromSplitPatchData(source.Patch!.Value, fileOffset, checked((int)(fileOffset - source.Offset)), length),
            RefType.SplitPatch => FromSplitPatchData(source.Patch!.Value, fileOffset, checked((int)(fileOffset - source.Offset + source.PatchOffset!.Value)), length),
            _ => throw new InvalidOperationException("Unknown RefType"),
        };
    }
}
