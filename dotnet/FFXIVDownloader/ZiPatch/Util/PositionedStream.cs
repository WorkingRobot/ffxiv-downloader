namespace FFXIVDownloader.ZiPatch.Util;

public sealed class PositionedStream(Stream stream) : Stream
{
    public Stream BaseStream { get; } = stream;
    private long TrackedPosition { get; set; }

    public override bool CanRead => BaseStream.CanRead;
    public override bool CanSeek => BaseStream.CanSeek;
    public override bool CanWrite => BaseStream.CanWrite;
    public override long Length => BaseStream.Length;
    public override long Position
    {
        get => TrackedPosition;
        set => TrackedPosition = BaseStream.Position = value;
    }

    public override void Flush() =>
        BaseStream.Flush();

    public override int Read(byte[] buffer, int offset, int count)
    {
        var ret = BaseStream.Read(buffer, offset, count);
        TrackedPosition += ret;
        return ret;
    }

    public override long Seek(long offset, SeekOrigin origin)
    {
        var ret = BaseStream.Seek(offset, origin);
        TrackedPosition = ret;
        return ret;
    }

    public override void SetLength(long value) =>
        BaseStream.SetLength(value);

    public override void Write(byte[] buffer, int offset, int count)
    {
        BaseStream.Write(buffer, offset, count);
        TrackedPosition += count;
    }

    protected override void Dispose(bool disposing)
    {
        if (disposing)
            BaseStream.Dispose();
        base.Dispose(disposing);
    }
}