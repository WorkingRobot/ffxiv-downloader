using FFXIVDownloader.ZiPatch.Chunk;

namespace FFXIVDownloader.Lut;

public sealed class ClutFileData
{
    public List<ClutDataRef> Data { get; }

    public ClutFileData()
    {
        Data = [];
    }

    public ClutFileData(BinaryReader reader, ReadOnlySpan<ClutPatchRef> patchMap)
    {
        var dataSize = reader.ReadInt32();
        Data = new(dataSize);
        for (var i = 0; i < dataSize; ++i)
            Data.Add(new(reader, patchMap));
    }

    public void Write(BinaryWriter writer, ReadOnlySpan<ClutPatchRef> patchMap)
    {
        writer.Write(Data.Count);
        foreach (var data in Data)
            data.Write(writer, patchMap);
    }
}