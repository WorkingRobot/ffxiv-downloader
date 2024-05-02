/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

namespace FFXIVDownloader.Patching.ZiPatch.Util
{
    public static class SqexFileStreamExtensions
    {
        private static readonly byte[] WipeBuffer = new byte[1 << 16];

        public static void WriteFromOffset(this Stream stream, byte[] data, long offset)
        {
            stream.Position = offset;
            stream.Write(data, 0, data.Length);
        }

        public static void Wipe(this Stream stream, long length)
        {
            var numFullChunks = length / WipeBuffer.Length;
            for (var i = 0; i < numFullChunks; i++)
                stream.Write(WipeBuffer, 0, WipeBuffer.Length);
            stream.Write(WipeBuffer, 0, checked((int)(length - numFullChunks * WipeBuffer.Length)));
        }

        public static void WipeFromOffset(this Stream stream, long length, long offset)
        {
            stream.Position = offset;
            stream.Wipe(length);
        }
    }
}