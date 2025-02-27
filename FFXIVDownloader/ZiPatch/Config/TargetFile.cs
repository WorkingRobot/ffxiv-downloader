namespace FFXIVDownloader.ZiPatch.Config;

public interface ITargetFile : IDisposable
{
    ValueTask WriteAsync(ReadOnlyMemory<byte> data, long offset, CancellationToken token = default);

    ValueTask TruncateAsync(CancellationToken token = default);
}
