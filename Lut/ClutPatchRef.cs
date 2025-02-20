namespace FFXIVDownloader.Lut;

public readonly record struct ClutPatchRef
{
    public string Patch { get; init; }
    public long Offset { get; init; }
    public long Size { get; init; }
    public long? DecompressedSize { get; init; }

    public bool IsCompressed => DecompressedSize.HasValue;

    public ClutPatchRef(BinaryReader reader, ReadOnlySpan<string> nameMap)
    {
        Patch = nameMap[reader.ReadInt32()];
        Offset = reader.ReadInt64();
        Size = reader.ReadInt64();
        DecompressedSize = reader.ReadInt64();
        if (DecompressedSize == long.MaxValue)
            DecompressedSize = null;
    }

    public void Write(BinaryWriter writer, ReadOnlySpan<string> nameMap)
    {
        writer.Write(nameMap.IndexOf(Patch));
        writer.Write(Offset);
        writer.Write(Size);
        writer.Write(DecompressedSize ?? long.MaxValue);
    }
}