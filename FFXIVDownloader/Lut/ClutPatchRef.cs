using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader.Lut;

public readonly record struct ClutPatchRef
{
    public long Offset { get; init; }
    public int Size { get; init; }
    public bool IsCompressed { get; init; }

    public long End => Offset + Size;

    public ClutPatchRef(BinaryReader reader, ref long patchOffset)
    {
        Offset = reader.Read7BitEncodedInt64() + patchOffset;
        Size = reader.Read7BitEncodedInt();
        IsCompressed = reader.ReadBoolean();
        patchOffset = Offset;
    }

    public void Write(BinaryWriter writer, ref long patchOffset)
    {
        writer.Write7BitEncodedInt64(Offset - patchOffset);
        writer.Write7BitEncodedInt(Size);
        writer.Write(IsCompressed);
        patchOffset = Offset;
    }
}
