/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Util;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.Lut;

namespace FFXIVDownloader.ZiPatch.Chunk;

public class ApplyOptionChunk : IZiPatchChunk<ApplyOptionChunk>
{
    public static string FourCC => "APLY";

    public enum ApplyOptionKind : uint
    {
        IgnoreMissing = 1,
        IgnoreOldMismatch = 2
    }

    // These are both false on all files seen
    public ApplyOptionKind OptionKind { get; }

    public bool OptionValue { get; }

    public ApplyOptionChunk(BinaryReader reader)
    {
        OptionKind = (ApplyOptionKind)reader.ReadUInt32BE();

        // Discarded padding, always 0x0000_0004 as far as observed
        reader.ReadBytes(4);

        var value = reader.ReadUInt32BE() != 0;

        if (OptionKind == ApplyOptionKind.IgnoreMissing ||
            OptionKind == ApplyOptionKind.IgnoreOldMismatch)
            OptionValue = value;
        else
            OptionValue = false; // defaults to false if OptionKind isn't valid
    }

    public static ApplyOptionChunk Read(BinaryReader reader) =>
        new(reader);

    public Task ApplyAsync(ZiPatchConfig config)
    {
        switch (OptionKind)
        {
            case ApplyOptionKind.IgnoreMissing:
                config.IgnoreMissing = OptionValue;
                break;

            case ApplyOptionKind.IgnoreOldMismatch:
                config.IgnoreOldMismatch = OptionValue;
                break;
        }
        return Task.CompletedTask;
    }

    public void WriteLUT(BinaryWriter writer, List<string> names, out ChunkType type)
    {
        type = ChunkType.ApplyOption;
        writer.Write((byte)OptionKind);
        writer.Write(OptionValue);
    }

    public override string ToString() =>
        $"{FourCC}:{OptionKind}:{OptionValue}";

}
