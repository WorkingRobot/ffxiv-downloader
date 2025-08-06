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
    public required GameVersion Version { get; set; }
    public required PatchVersion PatchVersion { get; set; }
    public required string? BasePatchUrl { get; set; }

    public int DecompressedSize { get; set; }
    public int CompressedSize { get; set; }

    [SetsRequiredMembers]
    public ClutHeader()
    {
        FileVersion = ClutVersion.SeparateVersioning;
        Platform = PlatformId.Win32;
        Repository = "UNKNOWN";
        Version = GameVersion.Epoch;
        PatchVersion = PatchVersion.Epoch;
        BasePatchUrl = null;
    }

    [SetsRequiredMembers]
    public ClutHeader(BinaryReader reader)
    {
        var magic = reader.ReadUInt16();
        if (magic != MAGIC)
            throw new LutException($"Invalid magic: {magic:X4}");

        FileVersion = (ClutVersion)reader.ReadUInt16();
        if (FileVersion != ClutVersion.SeparateVersioning)
            throw new LutException($"Unsupported version: {FileVersion}");

        Compression = (CompressType)reader.ReadByte();
        Platform = (PlatformId)reader.ReadByte();

        Repository = reader.ReadString();
        Version = new GameVersion(reader.ReadString());
        PatchVersion = new PatchVersion(reader.ReadString());
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
        writer.Write(Version.ToString());
        writer.Write(PatchVersion.ToString());
        writer.Write(BasePatchUrl ?? string.Empty);
        writer.Write(DecompressedSize);
        if (Compression != CompressType.None)
            writer.Write(CompressedSize);
    }
}
