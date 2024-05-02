/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.Text;

namespace FFXIVDownloader.Patching.ZiPatch.Util
{
    class SqpackDatFile : SqpackFile
    {
        public SqpackDatFile(BinaryReader reader) : base(reader) {}


        protected override string GetFileName(ZiPatchConfig.PlatformId platform) =>
            $"{base.GetFileName(platform)}.dat{FileId}";


        public static void WriteEmptyFileBlockAt(Stream stream, long offset, long blockNumber)
        {
            stream.WipeFromOffset(blockNumber << 7, offset);
            stream.Position = offset;

            using (var file = new BinaryWriter(stream, Encoding.Default, true))
            {
                // FileBlockHeader - the 0 writes are technically unnecessary but are in for illustrative purposes

                // Block size
                file.Write(1 << 7);
                // ????
                file.Write(0);
                // File size
                file.Write(0);
                // Total number of blocks?
                file.Write(blockNumber - 1);
                // Used number of blocks?
                file.Write(0);
            }
        }
    }
}