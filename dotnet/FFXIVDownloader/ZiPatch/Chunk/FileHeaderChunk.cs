/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk;

public class FileHeaderChunk : IZiPatchChunk<FileHeaderChunk>
{
    public static string FourCC => "FHDR";

    // V1?/2
    public byte Version { get; protected set; }
    public string PatchType { get; protected set; }
    public uint EntryFiles { get; protected set; }

    // V3
    public uint AddDirectories { get; protected set; }
    public uint DeleteDirectories { get; protected set; }
    public long DeleteDataSize { get; protected set; } // Split in 2 DWORD; Low, High
    public uint MinorVersion { get; protected set; }
    public uint RepositoryName { get; protected set; }
    public uint Commands { get; protected set; }
    public uint SqpkAddCommands { get; protected set; }
    public uint SqpkDeleteCommands { get; protected set; }
    public uint SqpkExpandCommands { get; protected set; }
    public uint SqpkHeaderCommands { get; protected set; }
    public uint SqpkFileCommands { get; protected set; }

    public FileHeaderChunk(BinaryReader reader)
    {
        Version = (byte)(reader.ReadUInt32() >> 16);
        PatchType = reader.ReadFixedLengthString(4);
        EntryFiles = reader.ReadUInt32BE();

        if (Version == 3)
        {
            AddDirectories = reader.ReadUInt32BE();
            DeleteDirectories = reader.ReadUInt32BE();
            DeleteDataSize = reader.ReadUInt32BE() | ((long)reader.ReadUInt32BE() << 32);
            MinorVersion = reader.ReadUInt32BE();
            RepositoryName = reader.ReadUInt32BE();
            Commands = reader.ReadUInt32BE();
            SqpkAddCommands = reader.ReadUInt32BE();
            SqpkDeleteCommands = reader.ReadUInt32BE();
            SqpkExpandCommands = reader.ReadUInt32BE();
            SqpkHeaderCommands = reader.ReadUInt32BE();
            SqpkFileCommands = reader.ReadUInt32BE();
        }

        // 0xB8 of unknown data for V3, 0x08 of 0x00 for V2
        // ... Probably irrelevant.
    }

    public static FileHeaderChunk Read(BinaryReader reader) =>
        new(reader);

    public Task ApplyAsync(ZiPatchConfig config) =>
        Task.CompletedTask;

    public void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.FileHeader;

        writer.Write(Version);
        writer.Write(PatchType);
        writer.Write(EntryFiles);

        writer.Write(AddDirectories);
        writer.Write(DeleteDirectories);
        writer.Write(DeleteDataSize);
        writer.Write(MinorVersion);
        writer.Write(RepositoryName);
        writer.Write(Commands);
        writer.Write(SqpkAddCommands);
        writer.Write(SqpkDeleteCommands);
        writer.Write(SqpkExpandCommands);
        writer.Write(SqpkHeaderCommands);
        writer.Write(SqpkFileCommands);
    }

    public override string ToString() =>
        $"{FourCC}:V{Version}:{RepositoryName:X8}";
}
