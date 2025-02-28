namespace FFXIVDownloader.ZiPatch.Config;

public sealed class BlackHoleTargetFile : ITargetFile
{
    public ValueTask WriteAsync(ReadOnlyMemory<byte> data, long offset, CancellationToken token = default) =>
        ValueTask.CompletedTask;

    public ValueTask TruncateAsync(CancellationToken token) =>
        ValueTask.CompletedTask;

    public void Dispose() { }
}
