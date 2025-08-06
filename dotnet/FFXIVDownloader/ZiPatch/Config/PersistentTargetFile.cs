using Microsoft.Win32.SafeHandles;

namespace FFXIVDownloader.ZiPatch.Config;

public sealed class PersistentTargetFile(string filePath) : ITargetFile
{
    private SafeFileHandle Handle { get; } =
       File.OpenHandle(filePath, FileMode.OpenOrCreate, FileAccess.Write, FileShare.Read, FileOptions.RandomAccess | FileOptions.Asynchronous);

    public ValueTask WriteAsync(ReadOnlyMemory<byte> data, long offset, CancellationToken token = default) =>
        RandomAccess.WriteAsync(Handle, data, offset, token);

    public ValueTask TruncateAsync(CancellationToken token)
    {
        RandomAccess.SetLength(Handle, 0);
        return ValueTask.CompletedTask;
    }

    public void Dispose() =>
        Handle.Dispose();
}
