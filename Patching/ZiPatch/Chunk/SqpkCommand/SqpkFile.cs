/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Patching.Util;
using FFXIVDownloader.Patching.ZiPatch.Util;

namespace FFXIVDownloader.Patching.ZiPatch.Chunk.SqpkCommand
{
    internal class SqpkFile : SqpkChunk
    {
        public new static string Command = "F";

        public enum OperationKind : byte
        {
            AddFile = (byte)'A',
            RemoveAll = (byte)'R',

            // I've seen no cases in the wild of these two
            DeleteFile = (byte)'D',
            MakeDirTree = (byte)'M'
        }

        public OperationKind Operation { get; protected set; }
        public long FileOffset { get; protected set; }
        public long FileSize { get; protected set; }
        public ushort ExpansionId { get; protected set; }
        public SqexFile TargetFile { get; protected set; }

        public List<long> CompressedDataSourceOffsets { get; protected set; }
        public List<SqpkCompressedBlock> CompressedData { get; protected set; }

        public SqpkFile(ChecksumBinaryReader reader, long size) : base(reader, size) {}

        protected override void ReadChunk()
        {
            using var advanceAfter = new AdvanceOnDispose(Reader, Size);
            Operation = (OperationKind)Reader.ReadByte();
            Reader.ReadBytes(2); // Alignment

            FileOffset = Reader.ReadInt64BE();
            FileSize = Reader.ReadInt64BE();

            var pathLen = Reader.ReadUInt32BE();

            ExpansionId = Reader.ReadUInt16BE();
            Reader.ReadBytes(2);

            TargetFile = new SqexFile(Reader.ReadFixedLengthString(pathLen));

            if (Operation == OperationKind.AddFile)
            {
                CompressedData = new List<SqpkCompressedBlock>();

                while (advanceAfter.NumBytesRemaining > 0)
                    CompressedData.Add(new SqpkCompressedBlock(Reader));
            }
        }

        public override void ApplyChunk(ZiPatchConfig config, IProgress<float> progress)
        {
            switch (Operation)
            {
                // Default behaviour falls through to AddFile, though this shouldn't happen
                case OperationKind.AddFile:
                default:
                    var fileStream = config.OpenStream(TargetFile);

                    if (FileOffset == 0)
                        fileStream.SetLength(0);

                    fileStream.Position = FileOffset;
                    foreach (var block in CompressedData)
                        block.DecompressInto(fileStream, progress);

                    break;

                case OperationKind.RemoveAll:
                    config.DeleteExpansion(ExpansionId);
                    break;

                case OperationKind.DeleteFile:
                    config.DeleteFile(TargetFile.RelativePath);
                    break;

                case OperationKind.MakeDirTree:
                    config.CreateDirectory(TargetFile.RelativePath);
                    break;
            }
        }

        public override string ToString()
        {
            return $"{Type}:{Command}:{Operation}:{FileOffset}:{FileSize}:{ExpansionId}:{TargetFile}";
        }
    }
}