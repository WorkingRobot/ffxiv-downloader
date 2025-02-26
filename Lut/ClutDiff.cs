using FFXIVDownloader.Thaliak;

namespace FFXIVDownloader.Lut;

public sealed class ClutDiff
{
    public string Repository { get; }
    public string? BasePatchUrl { get; }
    public ParsedVersionString VersionFrom { get; }
    public ParsedVersionString VersionTo { get; }

    public HashSet<string> RemovedFolders { get; }
    public HashSet<string> RemovedFiles { get; }

    public HashSet<string> AddedFolders { get; }
    public Dictionary<string, List<ClutDataRef>> AddedFiles { get; }

    public ClutDiff(ClutFile toFile) : this(new ClutFile
    {
        Header = new ClutHeader
        {
            Repository = toFile.Header.Repository,
            Version = ParsedVersionString.Epoch
        }
    }, toFile)
    {

    }

    public ClutDiff(ClutFile fromFile, ClutFile toFile)
    {
        VersionFrom = fromFile.Header.Version;
        VersionTo = toFile.Header.Version;

        if (fromFile.Header.Repository != toFile.Header.Repository)
            throw new ArgumentException("Repositories must match");
        if (VersionFrom >= VersionTo)
            throw new ArgumentException("VersionTo must be greater than VersionFrom");

        Repository = fromFile.Header.Repository;
        BasePatchUrl = toFile.Header.BasePatchUrl ?? fromFile.Header.BasePatchUrl;

        RemovedFolders = [.. fromFile.Folders];
        RemovedFolders.ExceptWith(toFile.Folders);

        RemovedFiles = [.. fromFile.Files.Keys];
        RemovedFiles.ExceptWith(toFile.Files.Keys);

        AddedFolders = [.. toFile.Folders];
        AddedFolders.ExceptWith(fromFile.Folders);

        AddedFiles = [];
        foreach (var (path, toData) in toFile.Files)
        {
            // If a brand new file is added, add it whole.
            if (!fromFile.Files.ContainsKey(path))
            {
                AddedFiles.Add(path, toData.Data);
                continue;
            }

            var newData = toData.Data.Where(d => d.AppliedVersion > VersionFrom).ToList();
            if (newData.Count > 0)
                AddedFiles.Add(path, newData);
        }
    }
}
