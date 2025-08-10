namespace FFXIVDownloader.Thaliak;

public sealed record Repository
{
    public string? Name { get; init; }
    public string? Description { get; init; }

    public Version? LatestVersion { get; init; }
    public List<AnnotatedVersion>? Versions { get; init; }
}
