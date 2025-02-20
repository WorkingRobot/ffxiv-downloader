using FFXIVDownloader.ZiPatch.Chunk;

namespace FFXIVDownloader.Lut;

public sealed class LutChunk
{
    public ChunkType Type { get; }
    public List<string> Names { get; }
    public byte[] Data { get; }

    public LutChunk(BinaryReader reader, ReadOnlySpan<string> nameMap)
    {
        Type = (ChunkType)reader.ReadByte();
        var nameIdxSize = reader.ReadInt32();
        Names = new(nameIdxSize);
        for (var i = 0; i < nameIdxSize; ++i)
            Names.Add(nameMap[reader.ReadInt32()]);
        var dataSize = reader.ReadInt32();
        Data = reader.ReadBytes(dataSize);
    }

    public LutChunk(IZiPatchChunk chunk)
    {
        Names = [];

        var chunkStream = new MemoryStream(2048);
        using var chunkDataWriter = new BinaryWriter(chunkStream, System.Text.Encoding.UTF8);
        chunk.WriteLUT(chunkDataWriter, Names, out var type);

        Type = type;
        Data = chunkStream.GetBuffer()[..(int)chunkStream.Length];
    }

    public void Write(BinaryWriter writer, ReadOnlySpan<string> nameMap)
    {
        writer.Write((byte)Type);
        writer.Write(Names.Count);
        foreach (var name in Names)
            writer.Write(nameMap.IndexOf(name));
        writer.Write(Data.Length);
        writer.Write(Data);
    }
}