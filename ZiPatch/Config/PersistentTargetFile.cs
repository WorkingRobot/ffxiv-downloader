namespace FFXIVDownloader.ZiPatch.Config;

public sealed class PersistentTargetFile(string filePath) : ITargetFile
{
    private FileStream Stream { get; } =
        new FileStream(filePath, FileMode.OpenOrCreate, FileAccess.ReadWrite, FileShare.Read, 1 << 16, true);

    public ValueTask WriteAsync(ReadOnlyMemory<byte> data, long offset, CancellationToken token = default) =>
        RandomAccess.WriteAsync(Stream.SafeFileHandle, data, offset, token);

    public ValueTask TruncateAsync(CancellationToken token = default)
    {
        RandomAccess.SetLength(Stream.SafeFileHandle, 0);
        return ValueTask.CompletedTask;
    }

    public void Dispose() =>
        Stream.Dispose();
}
