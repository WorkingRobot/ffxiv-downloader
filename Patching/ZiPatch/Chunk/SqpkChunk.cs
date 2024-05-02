/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Patching.Util;
using FFXIVDownloader.Patching.ZiPatch.Chunk.SqpkCommand;
using FFXIVDownloader.Patching.ZiPatch.Util;

namespace FFXIVDownloader.Patching.ZiPatch.Chunk
{
    public abstract class SqpkChunk : ZiPatchChunk
    {
        public new static string Type = "SQPK";
        public static string Command { get; protected set; }


        private static readonly Dictionary<string, Func<ChecksumBinaryReader, long, SqpkChunk>> CommandTypes =
            new()
            {
                { SqpkAddData.Command, (reader, size) => new SqpkAddData(reader, size) },
                { SqpkDeleteData.Command, (reader, size) => new SqpkDeleteData(reader, size) },
                { SqpkHeader.Command, (reader, size) => new SqpkHeader(reader, size) },
                { SqpkTargetInfo.Command, (reader, size) => new SqpkTargetInfo(reader, size) },
                { SqpkExpandData.Command, (reader, size) => new SqpkExpandData(reader, size) },
                { SqpkIndex.Command, (reader, size) => new SqpkIndex(reader, size) },
                { SqpkFile.Command, (reader, size) => new SqpkFile(reader, size) },
                { SqpkPatchInfo.Command, (reader, size) => new SqpkPatchInfo(reader, size) }
            };

        public static ZiPatchChunk GetCommand(ChecksumBinaryReader reader, long size)
        {
            try
            {
                // Have not seen this differ from size
                var innerSize = reader.ReadInt32BE();
                if (size != innerSize)
                    throw new ZiPatchException();

                var command = reader.ReadFixedLengthString(1u);
                if (!CommandTypes.TryGetValue(command, out var constructor))
                    throw new ZiPatchException();

                var chunk = constructor(reader, innerSize - 5);

                return chunk;
            }
            catch (EndOfStreamException e)
            {
                throw new ZiPatchException("Could not get command", e);
            }
        }


        protected override void ReadChunk()
        {
            using var advanceAfter = new AdvanceOnDispose(Reader, Size);
        }

        protected SqpkChunk(ChecksumBinaryReader reader, long size) : base(reader, size)
        { }

        public override string ToString()
        {
            return Type;
        }
    }
}