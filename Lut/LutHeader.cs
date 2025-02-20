namespace FFXIVDownloader.Lut;

public sealed class LutHeader
{
    public const ushort MAGIC = 0xDE22;

    public LutVersion Version { get; }
    public CompressType Compression { get; init; }

    public LutHeader()
    {
        Version = LutVersion.Initial;
    }

    public LutHeader(BinaryReader reader)
    {
        var magic = reader.ReadUInt16();
        if (magic != MAGIC)
            throw new LutException($"Invalid magic: {magic:X4}");

        Version = (LutVersion)reader.ReadUInt16();
        if (Version != LutVersion.Initial)
            throw new LutException($"Unsupported version: {Version}");

        Compression = (CompressType)reader.ReadByte();
    }

    public void Write(BinaryWriter writer)
    {
        writer.Write(MAGIC);
        writer.Write((ushort)Version);
        writer.Write((byte)Compression);
    }
}