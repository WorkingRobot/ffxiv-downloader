using static FFXIVDownloader.ZiPatch.Config.ZiPatchConfig;

namespace FFXIVDownloader.Lut;

public sealed class ClutHeader
{
    public const ushort MAGIC = 0xDF23;

    public ClutVersion Version { get; }
    public CompressType Compression { get; init; }
    public PlatformId Platform { get; init; }
    public int DecompressedSize { get; set; }
    public int CompressedSize { get; set; }

    public ClutHeader()
    {
        Version = ClutVersion.Initial;
        Platform = PlatformId.Win32;
    }

    public ClutHeader(BinaryReader reader)
    {
        var magic = reader.ReadUInt16();
        if (magic != MAGIC)
            throw new LutException($"Invalid magic: {magic:X4}");

        Version = (ClutVersion)reader.ReadUInt16();
        if (Version != ClutVersion.Initial)
            throw new LutException($"Unsupported version: {Version}");

        Compression = (CompressType)reader.ReadByte();
        Platform = (PlatformId)reader.ReadByte();

        DecompressedSize = reader.ReadInt32();
        if (Compression != CompressType.None)
            CompressedSize = reader.ReadInt32();
    }

    public void Write(BinaryWriter writer)
    {
        writer.Write(MAGIC);
        writer.Write((ushort)Version);
        writer.Write((byte)Compression);
        writer.Write((byte)Platform);
        writer.Write(DecompressedSize);
        if (Compression != CompressType.None)
            writer.Write(CompressedSize);
    }
}
