using FFXIVDownloader.Thaliak;
using System.Diagnostics.CodeAnalysis;
using static FFXIVDownloader.ZiPatch.Config.ZiPatchConfig;

namespace FFXIVDownloader.Lut;

public sealed class ClutHeader
{
    public const ushort MAGIC = 0xDF23;

    public ClutVersion FileVersion { get; }
    public CompressType Compression { get; set; }
    public PlatformId Platform { get; init; }

    public required string Repository { get; set; }
    public required ParsedVersionString Version { get; set; }
    public required string? BasePatchUrl { get; set; }

    public int DecompressedSize { get; set; }
    public int CompressedSize { get; set; }

    [SetsRequiredMembers]
    public ClutHeader()
    {
        FileVersion = ClutVersion.Initial;
        Platform = PlatformId.Win32;
        Repository = "UNKNOWN";
        Version = ParsedVersionString.Epoch;
        BasePatchUrl = null;
    }

    [SetsRequiredMembers]
    public ClutHeader(BinaryReader reader)
    {
        var magic = reader.ReadUInt16();
        if (magic != MAGIC)
            throw new LutException($"Invalid magic: {magic:X4}");

        FileVersion = (ClutVersion)reader.ReadUInt16();
        if (FileVersion != ClutVersion.Initial)
            throw new LutException($"Unsupported version: {FileVersion}");

        Compression = (CompressType)reader.ReadByte();
        Platform = (PlatformId)reader.ReadByte();

        Repository = reader.ReadString();
        Version = new ParsedVersionString(reader.ReadString());
        BasePatchUrl = reader.ReadString();
        if (string.IsNullOrWhiteSpace(BasePatchUrl))
            BasePatchUrl = null;

        DecompressedSize = reader.ReadInt32();
        if (Compression != CompressType.None)
            CompressedSize = reader.ReadInt32();
    }

    public void Write(BinaryWriter writer)
    {
        writer.Write(MAGIC);
        writer.Write((ushort)FileVersion);
        writer.Write((byte)Compression);
        writer.Write((byte)Platform);
        writer.Write(Repository);
        writer.Write(Version.ToString("P"));
        writer.Write(BasePatchUrl ?? string.Empty);
        writer.Write(DecompressedSize);
        if (Compression != CompressType.None)
            writer.Write(CompressedSize);
    }
}
