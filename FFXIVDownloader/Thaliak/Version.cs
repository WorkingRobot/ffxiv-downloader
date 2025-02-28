namespace FFXIVDownloader.Thaliak;

public record Version
{
    public required ParsedVersionString VersionString { get; init; }
}

public sealed record AnnotatedVersion : Version
{
    public required bool IsActive { get; init; }
    public required List<Version> PrerequisiteVersions { get; init; }
    public required List<Patch> Patches { get; init; }
}