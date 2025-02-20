namespace FFXIVDownloader.Lut;

public sealed class ClutDataRef
{
    public enum RefType : byte
    {
        Patch = 0,
        Zero = 1,
        EmptyBlock = 2,
    }

    public RefType Type { get; init; }
    public long Offset { get; init; }
    public long Length { get; init; }
    public long? BlockCount { get; init; }
    public ClutPatchRef? Patch { get; init; }

    public ClutDataRef()
    {
        Type = RefType.Zero;
        Offset = 0;
        Length = 0;
    }

    public ClutDataRef(BinaryReader reader, ReadOnlySpan<ClutPatchRef> patchMap)
    {
        Type = (RefType)reader.ReadByte();
        Offset = reader.ReadInt64();
        Length = reader.ReadInt64();
        if (Type == RefType.EmptyBlock)
            BlockCount = reader.ReadInt64();
        if (Type == RefType.Patch)
            Patch = patchMap[reader.ReadInt32()];
    }

    public void Write(BinaryWriter writer, ReadOnlySpan<ClutPatchRef> patchMap)
    {
        writer.Write((byte)Type);
        writer.Write(Offset);
        writer.Write(Length);
        if (Type == RefType.EmptyBlock)
            writer.Write(BlockCount!.Value);
        if (Type == RefType.Patch)
            writer.Write(patchMap.IndexOf(Patch!.Value));
    }

    public static ClutDataRef FromRawPatchData(string patch, long patchOffset, long fileOffset, long length)
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
                DecompressedSize = null
            }
        };
    }

    public static ClutDataRef FromZeros(long fileOffset, long length)
    {
        return new ClutDataRef
        {
            Type = RefType.Zero,
            Offset = fileOffset,
            Length = length,
            Patch = null
        };
    }

    public static (ClutDataRef, ClutDataRef) FromEmpty(long fileOffset, long length)
    {
        var empty = new ClutDataRef
        {
            Type = RefType.EmptyBlock,
            Offset = fileOffset,
            Length = 24,
            BlockCount = length >> 7,
            Patch = null
        };
        var zero = new ClutDataRef
        {
            Type = RefType.Zero,
            Offset = fileOffset + 24,
            Length = length - 24,
            Patch = null
        };
        return (empty, zero);
    }

    public static ClutDataRef FromCompressedPatchData(string patch, long patchOffset, long fileOffset, long compressedLength, long length)
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
                DecompressedSize = length
            }
        };
    }
}