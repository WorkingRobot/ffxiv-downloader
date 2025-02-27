/* Copyright (c) FFXIVQuickLauncher https://github.com/goatcorp/FFXIVQuickLauncher/blob/master/LICENSE
 *
 * Modified to fit the needs of the project.
 */

using System.Buffers.Binary;
using System.Text;

namespace FFXIVDownloader.ZiPatch.Util;

internal static class BinaryReaderHelpers
{
    public static string ReadFixedLengthString(this BinaryReader reader, int length)
    {
        var data = reader.ReadBytes(length);
        ArgumentOutOfRangeException.ThrowIfNotEqual(data.Length, length);
        return Encoding.ASCII.GetString(data).TrimEnd((char)0);
    }

    public static ushort ReadUInt16BE(this BinaryReader me) =>
        BinaryPrimitives.ReverseEndianness(me.ReadUInt16());

    public static short ReadInt16BE(this BinaryReader me) =>
        BinaryPrimitives.ReverseEndianness(me.ReadInt16());

    public static uint ReadUInt32BE(this BinaryReader me) =>
        BinaryPrimitives.ReverseEndianness(me.ReadUInt32());

    public static int ReadInt32BE(this BinaryReader me) =>
        BinaryPrimitives.ReverseEndianness(me.ReadInt32());

    public static ulong ReadUInt64BE(this BinaryReader me) =>
        BinaryPrimitives.ReverseEndianness(me.ReadUInt64());

    public static long ReadInt64BE(this BinaryReader me) =>
        BinaryPrimitives.ReverseEndianness(me.ReadInt64());
}
