/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk;

public class AddDirectoryChunk : IZiPatchChunk<AddDirectoryChunk>
{
    public static string FourCC => "ADIR";

    public string DirName { get; }

    public AddDirectoryChunk(BinaryReader reader)
    {
        var dirNameLen = reader.ReadInt32BE();
        DirName = reader.ReadFixedLengthString(dirNameLen);
    }

    public AddDirectoryChunk(BinaryReader reader, ReadOnlySpan<string> names)
    {
        DirName = names[0];
    }

    public static AddDirectoryChunk Read(BinaryReader reader) =>
        new(reader);

    public Task ApplyAsync(ZiPatchConfig config) =>
        config.CreateDirectory(DirName);

    public void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.AddDirectory;
        names.Add(DirName);
    }

    public override string ToString() =>
        $"{FourCC}:{DirName}";
}
