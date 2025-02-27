namespace FFXIVDownloader.ZiPatch.Util;

public sealed class SemaphoreLock : IDisposable
{
    private readonly SemaphoreSlim semaphore;
    private bool disposed;

    private SemaphoreLock(SemaphoreSlim semaphore)
    {
        this.semaphore = semaphore;
    }

    public static async Task<SemaphoreLock> CreateAsync(SemaphoreSlim semaphore, CancellationToken token = default)
    {
        await semaphore.WaitAsync(token).ConfigureAwait(false);
        return new(semaphore);
    }

    public void Dispose()
    {
        if (disposed)
            return;
        disposed = true;
        semaphore.Release();
    }
}
