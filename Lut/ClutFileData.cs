using FFXIVDownloader.Thaliak;
using System.Collections.Frozen;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;

namespace FFXIVDownloader.Lut;

public sealed class ClutFileData
{
    public List<ClutDataRef> Data { get; }

    public ClutFileData()
    {
        Data = [];
    }

    public ClutFileData(BinaryReader reader, ReadOnlySpan<ParsedVersionString> patchMap)
    {
        var dataSize = reader.ReadInt32();
        Data = new(dataSize);
        long lastOffset = 0;
        for (var i = 0; i < dataSize; ++i)
            Data.Add(new(reader, patchMap, ref lastOffset));
        lastOffset = 0;

        var data = CollectionsMarshal.AsSpan(Data);
        foreach (ref var item in data)
            item.ReadOffset(reader, ref lastOffset);
        foreach (ref var item in data)
            item.ReadLength(reader);
    }

    [MethodImpl(MethodImplOptions.AggressiveInlining)]
    public void Write(BinaryWriter writer, FrozenDictionary<ParsedVersionString, int> patchMap)
    {
        writer.Write(Data.Count);
        long lastOffset = 0;
        foreach (var data in Data)
            data.Write(writer, patchMap, ref lastOffset);
        lastOffset = 0;
        foreach (var data in Data)
            data.WriteOffset(writer, ref lastOffset);
        foreach (var data in Data)
            data.WriteLength(writer);
    }
}
