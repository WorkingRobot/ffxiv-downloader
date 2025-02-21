using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader.Lut;

public readonly record struct ClutPatchRef : IComparable<ClutPatchRef>
{
    public ParsedVersionString Patch { get; init; }
    public long Offset { get; init; }
    public int Size { get; init; }
    public bool IsCompressed { get; init; }

    public ClutPatchRef(BinaryReader reader, ReadOnlySpan<ParsedVersionString> patchMap, ref long patchOffset)
    {
        Patch = patchMap[reader.Read7BitEncodedInt()];
        Offset = reader.Read7BitEncodedInt64() + patchOffset;
        Size = reader.Read7BitEncodedInt();
        IsCompressed = reader.ReadBoolean();
        patchOffset = Offset;
    }

    public void Write(BinaryWriter writer, IReadOnlyDictionary<ParsedVersionString, int> patchMap, ref long patchOffset)
    {
        writer.Write7BitEncodedInt(patchMap[Patch]);
        writer.Write7BitEncodedInt64(Offset - patchOffset);
        writer.Write7BitEncodedInt(Size);
        writer.Write(IsCompressed);
        patchOffset = Offset;
    }

    public int CompareTo(ClutPatchRef other)
    {
        if (Patch != other.Patch)
            return Patch.CompareTo(other.Patch);
        if (Offset != other.Offset)
            return Offset.CompareTo(other.Offset);
        if (Size != other.Size)
            return Size.CompareTo(other.Size);
        if (IsCompressed != other.IsCompressed)
            return IsCompressed.CompareTo(other.IsCompressed);
        return 0;
    }
}
