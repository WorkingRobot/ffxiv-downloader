namespace FFXIVDownloader.Lut;

public enum ChunkType : byte
{
    AddDirectory = 0,
    ApplyFreeSpace = 1,
    ApplyOption = 2,
    DeleteDirectory = 3,
    EndOfFile = 4,
    FileHeader = 5,
    XXXX = 6,

    SqpkAddData = 32,
    SqpkDeleteData = 33,
    SqpkExpandData = 34,
    SqpkHeader = 35,
    SqpkIndex = 36,
    SqpkPatchInfo = 37,
    SqpkTargetInfo = 38,

    SqpkFileAdd = 64,
    SqpkFileDelExpac = 65,
    SqpkFileDelete = 66,
    SqpkFileMkdir = 67,
}