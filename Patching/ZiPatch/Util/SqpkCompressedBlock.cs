/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.IO.Compression;

namespace FFXIVDownloader.Patching.ZiPatch.Util
{
    class SqpkCompressedBlock
    {
        public int HeaderSize { get; protected set; }
        public int CompressedSize { get; protected set; }
        public int DecompressedSize { get; protected set; }

        public bool IsCompressed => CompressedSize != 0x7d00;
        public int CompressedBlockLength => (int)(((IsCompressed ? CompressedSize : DecompressedSize) + 143) & 0xFFFF_FF80);

        public byte[] CompressedBlock { get; protected set; }

        public SqpkCompressedBlock(BinaryReader reader)
        {
            HeaderSize = reader.ReadInt32();
            reader.ReadUInt32(); // Pad

            CompressedSize = reader.ReadInt32();
            DecompressedSize = reader.ReadInt32();

            if (IsCompressed)
                CompressedBlock = reader.ReadBytes(CompressedBlockLength - HeaderSize);
            else
            {
                CompressedBlock = reader.ReadBytes(DecompressedSize);

                reader.ReadBytes(CompressedBlockLength - HeaderSize - DecompressedSize);
            }
        }

        public void DecompressInto(Stream outStream, IProgress<float> progress)
        {
            var relativeProgress = new Progress<long>(totalBytes => progress.Report((float)totalBytes / DecompressedSize));
            if (IsCompressed)
                using (var stream = new DeflateStream(new MemoryStream(CompressedBlock), CompressionMode.Decompress))
                    stream.CopyTo(outStream, 81920);
            else
                using (var stream = new MemoryStream(CompressedBlock))
                    stream.CopyTo(outStream, 81920);
        }
    }
}