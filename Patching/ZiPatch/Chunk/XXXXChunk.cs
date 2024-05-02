/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Patching.Util;
using FFXIVDownloader.Patching.ZiPatch.Util;

namespace FFXIVDownloader.Patching.ZiPatch.Chunk
{
    // ReSharper disable once InconsistentNaming
    public class XXXXChunk : ZiPatchChunk
    {
        // TODO: This... Never happens.
        public new static string Type = "XXXX";

        protected override void ReadChunk()
        {
            using var advanceAfter = new AdvanceOnDispose(Reader, Size);
        }

        public XXXXChunk(ChecksumBinaryReader reader, long size) : base(reader, size) { }

        public override string ToString()
        {
            return Type;
        }
    }
}