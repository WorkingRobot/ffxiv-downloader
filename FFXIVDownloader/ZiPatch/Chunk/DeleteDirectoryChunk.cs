﻿/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk;

public class DeleteDirectoryChunk : IZiPatchChunk<DeleteDirectoryChunk>
{
    public static string FourCC => "DELD";

    public string DirName { get; }

    public DeleteDirectoryChunk(BinaryReader reader)
    {
        var dirNameLen = reader.ReadInt32BE();
        DirName = reader.ReadFixedLengthString(dirNameLen);
    }

    public DeleteDirectoryChunk(BinaryReader reader, ReadOnlySpan<string> names)
    {
        DirName = names[0];
    }

    public static DeleteDirectoryChunk Read(BinaryReader reader) =>
        new(reader);

    public Task ApplyAsync(ZiPatchConfig config) =>
        config.DeleteDirectory(DirName);

    public void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.DeleteDirectory;
        names.Add(DirName);
    }

    public override string ToString() =>
        $"{FourCC}:{DirName}";
}
