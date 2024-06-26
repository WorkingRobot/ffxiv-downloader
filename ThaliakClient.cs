using GraphQL;
using GraphQL.Client.Http;
using GraphQL.Client.Serializer.SystemTextJson;

namespace FFXIVDownloader;

public sealed class ThaliakClient : IDisposable
{
    private GraphQLHttpClient Client { get; }

    public ThaliakClient()
    {
        Client = new("https://thaliak.xiv.dev/graphql/2022-08-14", new SystemTextJsonSerializer());
    }

    public async Task<ThaliakRepository> GetVersionAsync(string slug)
    {
        return (await Client.SendQueryAsync<ThaliakVersionQueryResponse>(new GraphQLRequest
        {
            Query = @"
query($repoId:String) {
  repository(slug: $repoId) {
    description
    slug
    name
    latestVersion {
      versionString
    }
  }
}",
            Variables = new
            {
                repoId = slug,
            },
        }).ConfigureAwait(false)).Data.Repository;
    }

    public async Task<ThaliakVersion[]> GetPatchListAsync(string slug)
    {
        return (await Client.SendQueryAsync<ThaliakPatchListQueryResponse>(new GraphQLRequest
        {
            Query = @"
query($repoId:String) {
  repository(slug: $repoId) {
    versions{
      versionString
      isActive
      prerequisiteVersions{
        versionString
      }
      patches{
        url
      }
    }
  }
}",
            Variables = new
            {
                repoId = slug
            }
        }).ConfigureAwait(false)).Data.Repository.Versions!;
    }

    public async Task<List<ThaliakVersion>> GetPatchChainAsync(string slug)
    {
        var data = (await Client.SendQueryAsync<ThaliakPatchListQueryResponse>(new GraphQLRequest
        {
            Query = @"
query($repoId:String) {
    repository(slug: $repoId) {
        versions {
            versionString
            isActive
            patches {
                url
            }
        }
        latestVersion {
            versionString
            isActive
            patches {
                url
            }
        }
    }
}",
            Variables = new
            {
                repoId = slug
            }
        }).ConfigureAwait(false)).Data.Repository;

        var versions = data.Versions!.Where(v => v.IsActive == true).OrderBy(v => new ThaliakParsedVersionString(v.VersionString)).ToList();
        if (data.LatestVersion!.VersionString != versions[^1].VersionString)
            throw new InvalidOperationException("Latest version is not the latest version");

        return versions.ToList();
    }

    public void Dispose()
    {
        Client.Dispose();
    }
}

public sealed record ThaliakVersionQueryResponse(ThaliakRepository Repository);

public sealed record ThaliakPatchListQueryResponse(ThaliakRepository Repository);

public sealed record ThaliakRepository(string? Description, string? Slug, string? Name, ThaliakVersion? LatestVersion, ThaliakVersion[]? Versions);

public sealed record ThaliakVersion(string VersionString, ThaliakVersion[]? PrerequisiteVersions, ThaliakPatches[]? Patches, bool? IsActive);

public sealed record ThaliakPatches(string Url);

public sealed record ThaliakParsedVersionString : IComparable<ThaliakParsedVersionString>, IEquatable<ThaliakParsedVersionString>
{
    public int Year { get; }
    public int Month { get; }
    public int Day { get; }
    public int Part { get; }
    public int Revision { get; }
    public bool IsHistoric { get; }
    public string? Section { get; }

    public ThaliakParsedVersionString(string versionString)
    {
        if (versionString.StartsWith('H'))
        {
            IsHistoric = true;
            versionString = versionString[1..];
        }
        else if (versionString.StartsWith('D'))
        {
            IsHistoric = false;
            versionString = versionString[1..];
        }

        while (char.IsAsciiLetterLower(versionString[^1]))
        {
            Section ??= string.Empty;
            Section = versionString[^1] + Section;
            versionString = versionString[..^1];
        }

        var parts = versionString.Split('.');
        if (parts.Length != 5)
            throw new ArgumentException("Invalid version string", nameof(versionString));

        Year = int.Parse(parts[0]);
        Month = int.Parse(parts[1]);
        Day = int.Parse(parts[2]);
        Part = int.Parse(parts[3]);
        Revision = int.Parse(parts[4]);
    }

    public int CompareTo(ThaliakParsedVersionString? other)
    {
        if (other is null)
            return 1;

        if (Year != other.Year)
            return Year.CompareTo(other.Year);

        if (Month != other.Month)
            return Month.CompareTo(other.Month);

        if (Day != other.Day)
            return Day.CompareTo(other.Day);

        if (Part != other.Part)
            return Part.CompareTo(other.Part);

        if (Revision != other.Revision)
            return Revision.CompareTo(other.Revision);

        if (IsHistoric != other.IsHistoric)
            return other.IsHistoric.CompareTo(IsHistoric);

        if (Section != other.Section)
            return (Section ?? string.Empty).CompareTo(other.Section ?? string.Empty);

        return 0;
    }

    public bool Equals(ThaliakParsedVersionString? other)
    {
        return CompareTo(other) == 0;
    }

    public override int GetHashCode()
    {
        return HashCode.Combine(Year, Month, Day, Part, Revision, IsHistoric, Section);
    }

    public static bool operator <(ThaliakParsedVersionString left, ThaliakParsedVersionString right)
    {
        return left is null ? right is not null : left.CompareTo(right) < 0;
    }

    public static bool operator <=(ThaliakParsedVersionString left, ThaliakParsedVersionString right)
    {
        return left is null || left.CompareTo(right) <= 0;
    }

    public static bool operator >(ThaliakParsedVersionString left, ThaliakParsedVersionString right)
    {
        return left is not null && left.CompareTo(right) > 0;
    }

    public static bool operator >=(ThaliakParsedVersionString left, ThaliakParsedVersionString right)
    {
        return left is null ? right is null : left.CompareTo(right) >= 0;
    }
}
