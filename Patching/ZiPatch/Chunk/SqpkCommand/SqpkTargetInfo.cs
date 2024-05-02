/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using FFXIVDownloader.Patching.Util;
using FFXIVDownloader.Patching.ZiPatch.Util;

namespace FFXIVDownloader.Patching.ZiPatch.Chunk.SqpkCommand
{
    internal class SqpkTargetInfo : SqpkChunk
    {
        // Only Platform is used on recent patcher versions
        public new static string Command = "T";

        // US/EU/JP are Global
        // ZH seems to also be Global
        // KR is unknown
        public enum RegionId : short
        {
            Global = -1
        }

        public ZiPatchConfig.PlatformId Platform { get; protected set; }
        public RegionId Region { get; protected set; }
        public bool IsDebug { get; protected set; }
        public ushort Version { get; protected set; }
        public ulong DeletedDataSize { get; protected set; }
        public ulong SeekCount { get; protected set; }

        public SqpkTargetInfo(ChecksumBinaryReader reader, long size) : base(reader, size) {}

        protected override void ReadChunk()
        {
            using var advanceAfter = new AdvanceOnDispose(Reader, Size);
            // Reserved
            Reader.ReadBytes(3);

            Platform = (ZiPatchConfig.PlatformId)Reader.ReadUInt16BE();
            Region = (RegionId)Reader.ReadInt16BE();
            IsDebug = Reader.ReadInt16BE() != 0;
            Version = Reader.ReadUInt16BE();
            DeletedDataSize = Reader.ReadUInt64();
            SeekCount = Reader.ReadUInt64();

            // Empty 32 + 64 bytes
        }

        public override void ApplyChunk(ZiPatchConfig config, IProgress<float> progress)
        {
            config.Platform = Platform;
        }

        public override string ToString()
        {
            return $"{Type}:{Command}:{Platform}:{Region}:{IsDebug}:{Version}:{DeletedDataSize}:{SeekCount}";
        }
    }
}