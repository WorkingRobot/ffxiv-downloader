/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Patching.ZiPatch.Chunk;

namespace FFXIVDownloader.Patching.ZiPatch
{
    public class ZiPatchFile : IDisposable
    {
        private static readonly uint[] ZipatchMagic =
        {
            0x50495A91, 0x48435441, 0x0A1A0A0D
        };

        public Stream Stream { get; }


        /// <summary>
        /// Instantiates a ZiPatchFile from a Stream
        /// </summary>
        /// <param name="stream">Stream to a ZiPatch</param>
        public ZiPatchFile(Stream stream)
        {
            Stream = stream;

            var reader = new BinaryReader(stream);
            if (ZipatchMagic.Any(magic => magic != reader.ReadUInt32()))
                throw new ZiPatchException();
        }


        public async IAsyncEnumerable<ZiPatchChunk> GetChunks()
        {
            ZiPatchChunk chunk;
            do
            {
                chunk = await ZiPatchChunk.GetChunk(Stream).ConfigureAwait(false);

                yield return chunk;
            } while (chunk.ChunkType != EndOfFileChunk.Type);
        }

        public void Dispose()
        {
            Stream?.Dispose();
        }
    }
}