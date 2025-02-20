using System.Diagnostics.CodeAnalysis;
using System.IO.Compression;
using System.Runtime.InteropServices;
using DotNext.Collections.Generic;
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
        var patchLen = reader.ReadInt32();
        var patches = new string[patchLen];
        for (var i = 0; i < patchLen; i++)
            patches[i] = reader.ReadString();

        var patchRefLen = reader.ReadInt32();
        var patchRefs = new ClutPatchRef[patchRefLen];
        for (var i = 0; i < patchRefLen; i++)
            patchRefs[i] = new(reader, patches);

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
            fileData[i] = new(reader, patchRefs);

        Folders = [.. folders];
        Files = fileNames.Zip(fileData).ToDictionary();
    }

    private void WriteDecompressedData(BinaryWriter writer)
    {
        var patchRefs = Files.Values.SelectMany(f => f.Data).Where(d => d.Patch.HasValue).Select(d => d.Patch!.Value).Distinct().ToArray();
        var patches = patchRefs.Select(p => p.Patch).Distinct().Order().ToArray();

        writer.Write(patches.Length);
        foreach (var patch in patches)
            writer.Write(patch);

        writer.Write(patchRefs.Length);
        foreach (var patchRef in patchRefs)
            patchRef.Write(writer, patches);

        writer.Write(Folders.Count);
        foreach (var folder in Folders)
            writer.Write(folder);

        writer.Write(Files.Count);
        foreach (var path in Files.Keys)
            writer.Write(path);
        foreach (var file in Files.Values)
            file.Write(writer, patchRefs);
    }

    public void ApplyLut(string patch, LutChunk chunk)
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
                ApplyLut(new SqpkDeleteData(reader, names));
                break;
            case ChunkType.SqpkExpandData:
                ApplyLut(new SqpkExpandData(reader, names));
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

    private void ApplyLut(string patch, SqpkHeader chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        file.Data.Add(ClutDataRef.FromRawPatchData(patch, chunk.HeaderDataPatchOffset, chunk.HeaderKind == SqpkHeader.TargetHeaderKind.Version ? 0 : SqpkHeader.HEADER_SIZE, SqpkHeader.HEADER_SIZE));
    }

    private void ApplyLut(string patch, SqpkAddData chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        file.Data.Add(ClutDataRef.FromRawPatchData(patch, chunk.BlockDataPatchOffset, chunk.BlockOffset, chunk.BlockNumber));
        file.Data.Add(ClutDataRef.FromZeros(chunk.BlockOffset + chunk.BlockNumber, chunk.BlockDeleteNumber));
    }

    private void ApplyLut(SqpkDeleteData chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        var (empty, zero) = ClutDataRef.FromEmpty(chunk.BlockOffset, chunk.BlockOffset);
        file.Data.Add(empty);
        file.Data.Add(zero);
    }

    private void ApplyLut(SqpkExpandData chunk)
    {
        var file = Files.GetOrAdd(GetPath(chunk.TargetFile));
        var (empty, zero) = ClutDataRef.FromEmpty(chunk.BlockOffset, chunk.BlockOffset);
        file.Data.Add(empty);
        file.Data.Add(zero);
    }

    private void ApplyLut(string patch, SqpkFile chunk)
    {
        switch (chunk.Operation)
        {
            case OperationKind.AddFile:
                var file = Files.GetOrAdd(GetPath(chunk.TargetFile));

                if (chunk.FileOffset == 0)
                    file.Data.Clear();

                var off = chunk.FileOffset;
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