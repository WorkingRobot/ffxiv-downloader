/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Lut;
using FFXIVDownloader.ZiPatch.Config;

namespace FFXIVDownloader.ZiPatch.Chunk;

public class EndOfFileChunk : IZiPatchChunk<EndOfFileChunk>
{
    public static string FourCC => "EOF_";

    public static EndOfFileChunk Read(BinaryReader reader) =>
        new();

    public Task ApplyAsync(ZiPatchConfig config) =>
        Task.CompletedTask;

    public void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type) =>
        type = ChunkType.EndOfFile;

    public override string ToString()
    {
        return FourCC;
    }
}
