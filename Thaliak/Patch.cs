namespace FFXIVDownloader.Thaliak;

public sealed record Patch
{
    public required string Url { get; init; }
    public required long Size { get; init; }
}