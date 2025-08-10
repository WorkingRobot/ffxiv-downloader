using FFXIVDownloader.Thaliak;
using System.Diagnostics.CodeAnalysis;

namespace FFXIVDownloader.Lut;

public sealed class LutHeader
{
    public const ushort MAGIC = 0xDE22;

    public LutVersion FileVersion { get; }
    public CompressType Compression { get; init; }

    public required string Repository { get; set; }
    public required PatchVersion Version { get; set; }

    [SetsRequiredMembers]
    public LutHeader()
    {
        FileVersion = LutVersion.SeparateVersioning;
        Repository = "UNKNOWN";
        Version = PatchVersion.Epoch;
    }

    [SetsRequiredMembers]
    public LutHeader(BinaryReader reader)
    {
        var magic = reader.ReadUInt16();
        if (magic != MAGIC)
            throw new LutException($"Invalid magic: {magic:X4}");

        FileVersion = (LutVersion)reader.ReadUInt16();
        if (FileVersion != LutVersion.SeparateVersioning)
            throw new LutException($"Unsupported version: {FileVersion}");

        Compression = (CompressType)reader.ReadByte();

        Repository = reader.ReadString();
        Version = new PatchVersion(reader.ReadString());
    }

    public void Write(BinaryWriter writer)
    {
        writer.Write(MAGIC);
        writer.Write((ushort)FileVersion);
        writer.Write((byte)Compression);
        writer.Write(Repository);
        writer.Write(Version.ToString("P"));
    }
}
