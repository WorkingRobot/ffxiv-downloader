using System.IO.Compression;

namespace FFXIVDownloader.Lut;

public sealed class LutFile
{
    public LutHeader Header { get; init; }
    public List<LutChunk> Chunks { get; }

    public LutFile()
    {
        Header = new();
        Chunks = [];
    }

    public LutFile(BinaryReader reader)
    {
        Header = new(reader);
        Chunks = [];

        using Stream? decompStream = Header.Compression switch
        {
            CompressType.None => null,
            CompressType.Zlib => new ZLibStream(reader.BaseStream, CompressionMode.Decompress, true),
            CompressType.Brotli => new BrotliStream(reader.BaseStream, CompressionMode.Decompress, true),
            _ => throw new LutException($"Unsupported compression: {Header.Compression}")
        };
        var decompReader = new BinaryReader(decompStream ?? reader.BaseStream);
        ReadDecompressedData(decompReader);
    }

    public void Write(BinaryWriter writer)
    {
        Header.Write(writer);

        using Stream? compStream = Header.Compression switch
        {
            CompressType.None => null,
            CompressType.Zlib => new ZLibStream(writer.BaseStream, CompressionLevel.Optimal, true),
            CompressType.Brotli => new BrotliStream(writer.BaseStream, CompressionLevel.Optimal, true),
            _ => throw new LutException($"Unsupported compression: {Header.Compression}")
        };
        var compWriter = new BinaryWriter(compStream ?? writer.BaseStream);
        WriteDecompressedData(compWriter);
    }

    private void ReadDecompressedData(BinaryReader reader)
    {
        var nameMapLen = reader.ReadInt32();
        var nameMap = new string[nameMapLen];
        for (var i = 0; i < nameMapLen; i++)
            nameMap[i] = reader.ReadString();

        var chunkLen = reader.ReadInt32();
        Chunks.Capacity = chunkLen;
        for (var i = 0; i < chunkLen; i++)
            Chunks.Add(new(reader, nameMap));
    }

    private void WriteDecompressedData(BinaryWriter writer)
    {
        var nameMap = Chunks.SelectMany(c => c.Names).Distinct().Order().ToArray();
        writer.Write(nameMap.Length);
        foreach (var name in nameMap)
            writer.Write(name);

        writer.Write(Chunks.Count);
        foreach (var chunk in Chunks)
            chunk.Write(writer, nameMap);
    }
}