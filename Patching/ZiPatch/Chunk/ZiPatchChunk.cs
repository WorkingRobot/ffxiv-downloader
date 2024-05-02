/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.Reflection;
using FFXIVDownloader.Patching.Util;
using FFXIVDownloader.Patching.ZiPatch.Util;

namespace FFXIVDownloader.Patching.ZiPatch.Chunk
{
    public abstract class ZiPatchChunk
    {
        public static string Type { get; protected set; }
        // Hack: C# doesn't let you get static fields from instances.
        public virtual string ChunkType => (string) GetType()
            .GetField("Type", BindingFlags.Static | BindingFlags.FlattenHierarchy | BindingFlags.Public)
            !.GetValue(null)!;

        public long Size { get; protected set; }
        public uint Checksum { get; protected set; }
        public uint CalculatedChecksum { get; protected set; }

        protected readonly ChecksumBinaryReader Reader;

        private static readonly AsyncLocal<MemoryStream> LocalMemoryStream = new();


        // Only FileHeader, ApplyOption, Sqpk, and EOF have been observed in XIVARR+ patches
        // AddDirectory and DeleteDirectory can theoretically happen, so they're implemented
        // ApplyFreeSpace doesn't seem to show up anymore, and EntryFile will just error out
        private static readonly Dictionary<string, Func<ChecksumBinaryReader, long, ZiPatchChunk>> ChunkTypes =
            new()
            {
                { FileHeaderChunk.Type, (reader, size) => new FileHeaderChunk(reader, size) },
                { ApplyOptionChunk.Type, (reader, size) => new ApplyOptionChunk(reader, size) },
                { ApplyFreeSpaceChunk.Type, (reader, size) => new ApplyFreeSpaceChunk(reader, size) },
                { AddDirectoryChunk.Type, (reader, size) => new AddDirectoryChunk(reader, size) },
                { DeleteDirectoryChunk.Type, (reader, size) => new DeleteDirectoryChunk(reader, size) },
                { SqpkChunk.Type, SqpkChunk.GetCommand },
                { EndOfFileChunk.Type, (reader, size) => new EndOfFileChunk(reader, size) },
                { XXXXChunk.Type, (reader, size) => new XXXXChunk(reader, size) }
        };


        public static async Task<ZiPatchChunk> GetChunk(Stream stream)
        {
            LocalMemoryStream.Value ??= new MemoryStream();

            var memoryStream = LocalMemoryStream.Value;
            try
            {
                var reader = new BinaryReader(stream);
                var size = checked((int)reader.ReadUInt32BE());

                // size of chunk + header + checksum
                var readSize = size + 4 + 4;

                // Enlarge MemoryStream if necessary, or set length at capacity
                var maxLen = Math.Max(readSize, memoryStream.Capacity);
                if (memoryStream.Length < maxLen)
                    memoryStream.SetLength(maxLen);

                // Read into MemoryStream's inner buffer
                await stream.ReadExactlyAsync(memoryStream.GetBuffer(), 0, readSize).ConfigureAwait(false);

                var binaryReader = new ChecksumBinaryReader(memoryStream);
                binaryReader.InitCrc32();

                var type = binaryReader.ReadFixedLengthString(4u);
                if (!ChunkTypes.TryGetValue(type, out var constructor))
                    throw new ZiPatchException();


                var chunk = constructor(binaryReader, size);

                chunk.ReadChunk();
                chunk.ReadChecksum();
                return chunk;
            }
            catch (EndOfStreamException e)
            {
                throw new ZiPatchException("Could not get chunk", e);
            }
            finally
            {
                memoryStream.Position = 0;
            }
        }

        protected ZiPatchChunk(ChecksumBinaryReader reader, long size)
        {
            Reader = reader;

            Size = size;
        }

        protected virtual void ReadChunk()
        {
            using var advanceAfter = new AdvanceOnDispose(Reader, Size);
        }

        public virtual void ApplyChunk(ZiPatchConfig config, IProgress<float> progress) {}

        protected void ReadChecksum()
        {
            CalculatedChecksum = Reader.GetCrc32();
            Checksum = Reader.ReadUInt32BE();
        }

        public bool IsChecksumValid => CalculatedChecksum == Checksum;
    }
}