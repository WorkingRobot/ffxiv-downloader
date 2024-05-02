/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Patching.Util;
using FFXIVDownloader.Patching.ZiPatch.Util;

namespace FFXIVDownloader.Patching.ZiPatch.Chunk.SqpkCommand
{
    class SqpkExpandData : SqpkChunk
    {
        public new static string Command = "E";


        public SqpackDatFile TargetFile { get; protected set; }
        public long BlockOffset { get; protected set; }
        public long BlockNumber { get; protected set; }


        public SqpkExpandData(ChecksumBinaryReader reader, long size) : base(reader, size) {}

        protected override void ReadChunk()
        {
            using var advanceAfter = new AdvanceOnDispose(Reader, Size);
            Reader.ReadBytes(3);

            TargetFile = new SqpackDatFile(Reader);

            BlockOffset = (long)Reader.ReadUInt32BE() << 7;
            BlockNumber = (long)Reader.ReadUInt32BE();

            Reader.ReadUInt32(); // Reserved
        }

        public override void ApplyChunk(ZiPatchConfig config, IProgress<float> progress)
        {
            TargetFile.ResolvePath(config.Platform);

            var file = config.OpenStream(TargetFile);

            SqpackDatFile.WriteEmptyFileBlockAt(file, BlockOffset, BlockNumber);
        }

        public override string ToString()
        {
            return $"{Type}:{Command}:{BlockOffset}:{BlockNumber}";
        }
    }
}