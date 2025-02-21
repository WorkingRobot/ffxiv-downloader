/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.Text;

namespace FFXIVDownloader.ZiPatch.Util;

public static class SqexExtensions
{
    private static readonly ReadOnlyMemory<byte> WipeBuffer = new byte[1 << 16];

    public static ValueTask WriteFromOffset(this Stream stream, ReadOnlyMemory<byte> data, long offset)
    {
        stream.Position = offset;
        return stream.WriteAsync(data);
    }

    public static async Task Wipe(this Stream stream, long length)
    {
        var numFullChunks = length / WipeBuffer.Length;
        for (var i = 0; i < numFullChunks; i++)
            await stream.WriteAsync(WipeBuffer).ConfigureAwait(false);
        await stream.WriteAsync(WipeBuffer[..checked((int)(length - (numFullChunks * WipeBuffer.Length)))]).ConfigureAwait(false);
    }

    public static Task WipeFromOffset(this Stream stream, long length, long offset)
    {
        stream.Position = offset;
        return stream.Wipe(length);
    }

    public static async Task WriteEmptyFileBlockAt(Stream stream, long offset, long length)
    {
        await stream.WipeFromOffset(length, offset).ConfigureAwait(false);
        stream.Position = offset;

        using var file = new BinaryWriter(stream, Encoding.UTF8, true);

        // FileBlockHeader - the 0 writes are technically unnecessary but are in for illustrative purposes

        // Block size
        file.Write(1 << 7);
        // ????
        file.Write(0);
        // File size
        file.Write(0);
        // Total number of blocks?
        file.Write((length >> 7) - 1);
        // Used number of blocks?
        file.Write(0);
    }

    public static string GetExpansionFolder(ushort expansionId) =>
        expansionId == 0 ? "ffxiv" : $"ex{expansionId}";
}
