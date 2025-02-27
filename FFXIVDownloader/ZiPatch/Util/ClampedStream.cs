
namespace FFXIVDownloader.ZiPatch.Util;

public sealed class ClampedStream(Stream stream, long length) : Stream
{
    public Stream BaseStream { get; } = stream;
    private long TrackedPosition { get; set; }

    public override bool CanRead => BaseStream.CanRead;
    public override bool CanSeek => false;
    public override bool CanWrite => BaseStream.CanWrite;
    public override long Length { get; } = length;
    public override long Position
    {
        get => TrackedPosition;
        set => throw new NotSupportedException();
    }

    public override void Flush() =>
        BaseStream.Flush();

    public override int Read(Span<byte> buffer)
    {
        buffer = buffer[..checked((int)Math.Min(buffer.Length, Length - TrackedPosition))];
        var ret = BaseStream.Read(buffer);
        TrackedPosition += ret;
        if (TrackedPosition > Length)
            throw new InvalidOperationException("Overread stream");
        return ret;
    }

    public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
    {
        if (buffer.Length > Length - TrackedPosition)
            buffer = buffer[..checked((int)(Length - TrackedPosition))];
        var ret = await BaseStream.ReadAsync(buffer, cancellationToken).ConfigureAwait(false);
        if (TrackedPosition + ret > Length)
            throw new InvalidOperationException("Overread stream");
        TrackedPosition += ret;
        return ret;
    }

    public override int Read(byte[] buffer, int offset, int count)
    {
        count = checked((int)Math.Min(buffer.Length, Length - TrackedPosition));
        var ret = BaseStream.Read(buffer, offset, count);
        TrackedPosition += ret;
        if (TrackedPosition > Length)
            throw new InvalidOperationException("Overread stream");
        return ret;
    }

    public override long Seek(long offset, SeekOrigin origin) =>
        throw new NotSupportedException();

    public override void SetLength(long value) =>
        throw new NotSupportedException();

    public override void Write(ReadOnlySpan<byte> buffer)
    {
        buffer = buffer[..checked((int)Math.Min(buffer.Length, Length - TrackedPosition))];
        TrackedPosition += buffer.Length;
        if (TrackedPosition > Length)
            throw new InvalidOperationException("Overread stream");
        BaseStream.Write(buffer);
    }

    public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default)
    {
        buffer = buffer[..checked((int)Math.Min(buffer.Length, Length - TrackedPosition))];
        TrackedPosition += buffer.Length;
        if (TrackedPosition > Length)
            throw new InvalidOperationException("Overread stream");
        return BaseStream.WriteAsync(buffer, cancellationToken);
    }

    public override void Write(byte[] buffer, int offset, int count)
    {
        count = checked((int)Math.Min(buffer.Length, Length - TrackedPosition));
        TrackedPosition += count;
        if (TrackedPosition > Length)
            throw new InvalidOperationException("Overread stream");
        BaseStream.Write(buffer, offset, count);
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
        {
            if (Length != TrackedPosition)
                throw new InvalidOperationException("Stream was not fully read");
        }
        base.Dispose(disposing);
    }
}
