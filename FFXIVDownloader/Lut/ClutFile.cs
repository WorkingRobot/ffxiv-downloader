using System.Collections.Frozen;
using System.IO.Compression;
using System.Runtime.InteropServices;
using DotNext.Collections.Generic;
using FFXIVDownloader.Thaliak;
using FFXIVDownloader.ZiPatch;
using FFXIVDownloader.ZiPatch.Chunk;
using FFXIVDownloader.ZiPatch.Chunk.SqpkCommand;
using FFXIVDownloader.ZiPatch.Config;
using FFXIVDownloader.ZiPatch.Util;
using static FFXIVDownloader.ZiPatch.Chunk.SqpkCommand.SqpkFile;
using static FFXIVDownloader.ZiPatch.Config.ZiPatchConfig;

namespace FFXIVDownloader.Lut;

public sealed class ClutFile
{
    public ClutHeader Header { get; init; }
    public HashSet<string> Folders { get; private set; }
    public Dictionary<string, ClutFileData> Files { get; private set; }

    public ClutFile()
    {
        Header = new();
        Folders = [];
        Files = [];
    }

    public ClutFile(BinaryReader reader)
    {
        Header = new(reader);
        Folders = [];
        Files = [];

        var compressedData = new byte[Header.CompressedSize];
        reader.BaseStream.ReadExactly(compressedData);

        byte[] decompressedData;
        if (Header.Compression == CompressType.None)
            decompressedData = compressedData;
        else
        {
            decompressedData = new byte[Header.DecompressedSize];
            switch (Header.Compression)
            {
                case CompressType.Zlib:
                    {
                        using var decompressor = new ZLibStream(new MemoryStream(compressedData), CompressionMode.Decompress);
                        decompressor.ReadExactly(decompressedData);
                        break;
                    }
                case CompressType.Brotli:
                    {
                        BrotliDecoder.TryDecompress(compressedData, decompressedData, out var written);
                        if (written != decompressedData.Length)
                            throw new LutException($"Failed to decompress data: {written} != {decompressedData.Length}");
                        break;
                    }
                default:
                    throw new LutException($"Unsupported compression: {Header.Compression}");
            }
        }

        using var decompReader = new BinaryReader(new MemoryStream(decompressedData));
        ReadDecompressedData(decompReader);
    }

    public void Write(BinaryWriter writer)
    {
        var decompressedData = WriteDecompressedData();

        ReadOnlyMemory<byte> compressedData;
        if (Header.Compression == CompressType.None)
            compressedData = decompressedData;
        else
        {
            switch (Header.Compression)
            {
                case CompressType.Zlib:
                    {
                        using var s = new MemoryStream();
                        using (var compressor = new ZLibStream(s, CompressionLevel.Optimal, true))
                            compressor.Write(decompressedData.Span);
                        compressedData = s.GetBuffer().AsMemory()[..(int)s.Length];
                        break;
                    }
                case CompressType.Brotli:
                    {
                        var compData = new byte[BrotliEncoder.GetMaxCompressedLength(decompressedData.Length)];
                        if (!BrotliEncoder.TryCompress(decompressedData.Span, compData, out var written))
                            throw new LutException($"Failed to compress data: {written} != {decompressedData.Length}");
                        compressedData = compData.AsMemory()[..written];
                        break;
                    }
                default:
                    throw new LutException($"Unsupported compression: {Header.Compression}");
            }
            Header.CompressedSize = compressedData.Length;
        }

        Header.DecompressedSize = decompressedData.Length;
        Header.Write(writer);

        writer.Write(compressedData.Span);
    }

    private void ReadDecompressedData(BinaryReader reader)
    {
        var patchLen = reader.ReadInt32();
        var patches = new ParsedVersionString[patchLen];
        for (var i = 0; i < patchLen; i++)
            patches[i] = new(reader.ReadString());

        var folderLen = reader.ReadInt32();
        var folders = new string[folderLen];
        for (var i = 0; i < folderLen; i++)
            folders[i] = reader.ReadString();

        var fileLen = reader.ReadInt32();

        var fileNames = new string[fileLen];
        for (var i = 0; i < fileLen; i++)
            fileNames[i] = reader.ReadString();

        var fileData = new ClutFileData[fileLen];
        for (var i = 0; i < fileLen; i++)
            fileData[i] = new(reader, patches);

        Folders = [.. folders];
        Files = fileNames.Zip(fileData).ToDictionary();
    }

    private ReadOnlyMemory<byte> WriteDecompressedData()
    {
        using var stream = new MemoryStream();
        using var writer = new BinaryWriter(stream);

        var patchSet = new HashSet<ParsedVersionString>();
        foreach (var file in Files.Values)
            foreach (var data in file.Data)
                patchSet.Add(data.AppliedVersion);
        var i = 0;
        var patches = patchSet.ToFrozenDictionary(v => v, v => i++);

        writer.Write(patches.Count);
        foreach (var patch in patches.OrderBy(k => k.Value))
            writer.Write(patch.Key.ToString("P"));

        writer.Write(Folders.Count);
        foreach (var folder in Folders)
            writer.Write(folder);

        writer.Write(Files.Count);
        foreach (var path in Files.Keys)
            writer.Write(path);
        foreach (var file in Files.Values)
            file.Write(writer, patches);

        return stream.GetBuffer().AsMemory()[..(int)stream.Length];
    }

    public void RemoveOverlaps()
    {
        Parallel.ForEach(Files, f => f.Value.RemoveOverlaps(f.Key));
    }

    public void ApplyLut(ParsedVersionString patch, LutChunk chunk)
    {
        using var dataStream = new MemoryStream(chunk.Data, false);
        using var reader = new BinaryReader(dataStream);
        ReadOnlySpan<string> names = CollectionsMarshal.AsSpan(chunk.Names);

        switch (chunk.Type)
        {
            case ChunkType.AddDirectory:
                ApplyLut(new AddDirectoryChunk(reader, names));
                break;
            case ChunkType.DeleteDirectory:
                ApplyLut(new DeleteDirectoryChunk(reader, names));
                break;
            case ChunkType.SqpkHeader:
                ApplyLut(patch, new SqpkHeader(reader, names));
                break;
            case ChunkType.SqpkAddData:
                ApplyLut(patch, new SqpkAddData(reader, names));
                break;
            case ChunkType.SqpkDeleteData:
                ApplyLut(patch, new SqpkDeleteData(reader, names));
                break;
            case ChunkType.SqpkExpandData:
                ApplyLut(patch, new SqpkExpandData(reader, names));
                break;
            case ChunkType.SqpkFileAdd:
                ApplyLut(patch, new SqpkFile(reader, OperationKind.AddFile, names));
                break;
            case ChunkType.SqpkFileMkdir:
                ApplyLut(patch, new SqpkFile(reader, OperationKind.MakeDirTree, names));
                break;
            case ChunkType.SqpkFileDelete:
                ApplyLut(patch, new SqpkFile(reader, OperationKind.DeleteFile, names));
                break;
            case ChunkType.SqpkFileDelExpac:
                ApplyLut(patch, new SqpkFile(reader, OperationKind.RemoveAll, names));
                break;
        }
    }

    private static string NormalizePath(string path) => path.Replace('\\', '/').Trim('/');

    private string GetPath(SqexFile file) => NormalizePath(file.GetPath(Header.Platform));

    private void ApplyLut(AddDirectoryChunk chunk) =>
        Folders.Add(NormalizePath(chunk.DirName));

    private void ApplyLut(DeleteDirectoryChunk chunk) =>
        Folders.Remove(NormalizePath(chunk.DirName));

    private void ApplyLut(ParsedVersionString patch, SqpkHeader chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        file.Data.Add(ClutDataRef.FromRawPatchData(patch, chunk.HeaderDataPatchOffset, chunk.HeaderKind == SqpkHeader.TargetHeaderKind.Version ? 0 : SqpkHeader.HEADER_SIZE, SqpkHeader.HEADER_SIZE));
    }

    private void ApplyLut(ParsedVersionString patch, SqpkAddData chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        if (chunk.BlockNumber > 0)
            file.Data.Add(ClutDataRef.FromRawPatchData(patch, chunk.BlockDataPatchOffset, chunk.BlockOffset, checked((int)chunk.BlockNumber)));
        if (chunk.BlockDeleteNumber > 0)
            file.Data.Add(ClutDataRef.FromZeros(patch, chunk.BlockOffset + chunk.BlockNumber, checked((int)chunk.BlockDeleteNumber)));
    }

    private void ApplyLut(ParsedVersionString patch, SqpkDeleteData chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        var (empty, zero) = ClutDataRef.FromEmpty(patch, chunk.BlockOffset, checked((int)chunk.BlockNumber));
        file.Data.Add(empty);
        if (zero is { } zeroBlock)
            file.Data.Add(zeroBlock);
    }

    private void ApplyLut(ParsedVersionString patch, SqpkExpandData chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        var (empty, zero) = ClutDataRef.FromEmpty(patch, chunk.BlockOffset, checked((int)chunk.BlockNumber));
        file.Data.Add(empty);
        if (zero is { } zeroBlock)
            file.Data.Add(zeroBlock);
    }

    private void ApplyLut(ParsedVersionString patch, SqpkFile chunk)
    {
        switch (chunk.Operation)
        {
            case OperationKind.AddFile:
                var file = Files.GetOrAdd(GetPath(chunk.TargetFile));

                var off = chunk.FileOffset;
                if (off == 0)
                {
                    Log.Info($"Clearing file {GetPath(chunk.TargetFile)} ({file.Data.Count} Blocks)");
                    file.Data.Clear();
                }

                foreach (var block in chunk.CompressedData!)
                {
                    if (block.IsCompressed)
                        file.Data.Add(ClutDataRef.FromCompressedPatchData(patch, block.DataBlockPatchOffset, off, block.CompressedSize, block.DataSize));
                    else
                        file.Data.Add(ClutDataRef.FromRawPatchData(patch, block.DataBlockPatchOffset, off, block.DataSize));
                    off += block.DataSize;
                }
                break;

            case OperationKind.RemoveAll:
                var expansionFolder = SqexExtensions.GetExpansionFolder(chunk.ExpansionId);

                foreach (var dirName in ExpacFolders)
                {
                    var dir = $"{dirName}/{expansionFolder}";
                    var removeFiles = Files.Keys.Where(f => f.StartsWith(dir)).Where(f => !ZiPatchConfig.ShouldKeep(f)).ToArray();
                    foreach (var f in removeFiles)
                        Files.Remove(f);
                }
                break;

            case OperationKind.DeleteFile:
                Files.Remove(GetPath(chunk.TargetFile));
                break;

            case OperationKind.MakeDirTree:
                Folders.Add(GetPath(chunk.TargetFile));
                break;

            default:
                throw new ZiPatchException($"Operation {chunk.Operation} is not supported.");
        }
    }
}
