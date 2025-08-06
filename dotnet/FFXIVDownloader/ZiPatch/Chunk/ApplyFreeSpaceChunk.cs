/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk;

public class ApplyFreeSpaceChunk : IZiPatchChunk<ApplyFreeSpaceChunk>
{
    // This is a NOP on recent patcher versions, so I don't think we'll be seeing it.
    public static string FourCC => "APFS";

    // TODO: No samples of this were found, so these fields are theoretical
    public long UnknownFieldA { get; protected set; }
    public long UnknownFieldB { get; protected set; }

    public ApplyFreeSpaceChunk(BinaryReader Reader)
    {
        UnknownFieldA = Reader.ReadInt64BE();
        UnknownFieldB = Reader.ReadInt64BE();
    }

    public static ApplyFreeSpaceChunk Read(BinaryReader reader) =>
        new(reader);

    public Task ApplyAsync(ZiPatchConfig config) =>
        Task.CompletedTask;

    public void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type) =>
        type = ChunkType.ApplyFreeSpace;

    public override string ToString() =>
        $"{FourCC}:{UnknownFieldA}:{UnknownFieldB}";

}
