/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.ZiPatch.Config;
using System.Text;

namespace FFXIVDownloader.ZiPatch.Util;

public static class SqexExtensions
{
    private static readonly ReadOnlyMemory<byte> WipeBuffer = new byte[1 << 16];

    public static async Task WipeAsync(this ITargetFile file, long offset, long length, CancellationToken token = default)
    {
        var numFullChunks = length / WipeBuffer.Length;
        for (var i = 0; i < numFullChunks; i++)
        {
            await file.WriteAsync(WipeBuffer, offset, token).ConfigureAwait(false);
            offset += WipeBuffer.Length;
        }
        await file.WriteAsync(WipeBuffer[..checked((int)(length - (numFullChunks * WipeBuffer.Length)))], offset, token).ConfigureAwait(false);
    }

    private static byte[] CreateEmptyFileBlockHeader(long blockCount)
    {
        var ret = new byte[24];

        using (var stream = new MemoryStream(ret))
        using (var file = new BinaryWriter(stream, Encoding.UTF8, true))
        {
            // FileBlockHeader - the 0 writes are technically unnecessary but are in for illustrative purposes
            // Block size
            file.Write(1 << 7);
            // ????
            file.Write(0);
            // File size
            file.Write(0);
            // Total number of blocks?
            file.Write(blockCount - 1);
            // Used number of blocks?
            file.Write(0);
        }

        return ret;
    }

    public static ValueTask WriteEmptyFileBlockAt(this ITargetFile stream, long blockCount, long offset) =>
        stream.WriteAsync(CreateEmptyFileBlockHeader(blockCount), offset);

    public static string GetExpansionFolder(ushort expansionId) =>
        expansionId == 0 ? "ffxiv" : $"ex{expansionId}";
}
