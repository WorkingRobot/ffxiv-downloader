using GraphQL;
using GraphQL.Client.Http;
using GraphQL.Client.Serializer.SystemTextJson;
using System.Collections.Frozen;
using System.Collections.Immutable;
using System.Net;
using System.Text;

namespace FFXIVDownloader.Thaliak;

public sealed class ThaliakClient : IDisposable
{
    private GraphQLHttpClient Client { get; }

    private static FrozenDictionary<string, FrozenDictionary<ParsedVersionString, ParsedVersionString?>> Overrides { get; } =
        new Dictionary<string, FrozenDictionary<ParsedVersionString, ParsedVersionString?>>
        {
            // Global
            {
                "4e9a232b", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    // Thaliak incorrectly orders these hist patches.
                    // aa comes after z. It's not lexicographically sorted.
                    { new("2024.05.31.0000.0000"), new("H2024.05.31.0000.0000ag") },
                    { new("H2024.05.31.0000.0000b"), new("H2024.05.31.0000.0000a") },
                    { new("H2024.05.31.0000.0000aa"), new("H2024.05.31.0000.0000z") },

                    // Spooky unseen patches o.o
                    { new("2024.04.23.0000.0000"), new("2024.04.22.0000.0001") },
                    { new("2024.04.22.0000.0001"), new("2024.03.27.0000.0000") },
                    { new("2023.06.14.0000.0000"), new("2023.06.13.0000.0001") },
                    { new("2023.06.13.0000.0001"), new("2023.05.11.0000.0001") },

                    { new("2017.06.06.0000.0001"), new("H2017.06.06.0000.0001m") },
                    { new("H2017.06.06.0000.0001a"), null },
                }.ToFrozenDictionary()
            },
            {
                "6b936f08", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.05.31.0000.0000"), new("H2024.05.31.0000.0000d") }
                }.ToFrozenDictionary()
            },
            {
                "f29a3eb2", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.05.31.0000.0000"), new("H2024.05.31.0000.0000e") }
                }.ToFrozenDictionary()
            },
            {
                "859d0e24", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.05.31.0000.0000"), new("H2024.05.31.0000.0000g") }
                }.ToFrozenDictionary()
            },
            {
                "1bf99b87", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.05.31.0000.0000"), new("H2024.05.31.0000.0000i") }
                }.ToFrozenDictionary()
            },

            // Korea
            {
                "de199059", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.11.02.0000.0000"), new("H2024.11.02.0000.0000ad") },
                    { new("H2024.11.02.0000.0000b"), new("H2024.11.02.0000.0000a") },
                    { new("H2024.11.02.0000.0000aa"), new("H2024.11.02.0000.0000z") },
                }.ToFrozenDictionary()
            },
            {
                "573d8c07", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.10.22.0002.0000"), new("H2024.10.22.0002.0000c") },
                }.ToFrozenDictionary()
            },
            {
                "ce34ddbd", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.10.22.0003.0000"), new("H2024.10.22.0003.0000e") },
                }.ToFrozenDictionary()
            },
            {
                "b933ed2b", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.11.02.0000.0000"), new("H2024.11.02.0000.0000f") },
                }.ToFrozenDictionary()
            },
            {
                "27577888", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.11.02.0000.0000"), new("H2024.11.02.0000.0000g") },
                }.ToFrozenDictionary()
            },

            // China
            {
                "c38effbc", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.09.09.0000.0000"), new("H2024.09.09.0000.0000ad") },
                    { new("H2024.09.09.0000.0000b"), new("H2024.09.09.0000.0000a") },
                    { new("H2024.09.09.0000.0000aa"), new("H2024.09.09.0000.0000z") },
                }.ToFrozenDictionary()
            },
            {
                "77420d17", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.08.27.0002.0000"), new("H2024.08.27.0002.0000c") },
                }.ToFrozenDictionary()
            },
            {
                "ee4b5cad", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.08.27.0003.0000"), new("H2024.08.27.0003.0000e") },
                }.ToFrozenDictionary()
            },
            {
                "994c6c3b", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.09.09.0000.0000"), new("H2024.09.09.0000.0000f") },
                }.ToFrozenDictionary()
            },
            {
                "0728f998", new Dictionary<ParsedVersionString, ParsedVersionString?>{
                    { new("2024.09.09.0000.0000"), new("H2024.09.09.0000.0000g") },
                }.ToFrozenDictionary()
            },
        }.ToFrozenDictionary();

    private static FrozenDictionary<string, ImmutableArray<AnnotatedVersion>> AdditionalVersions { get; } =
        new Dictionary<string, ImmutableArray<AnnotatedVersion>>
        {
            { "4e9a232b", [
                new AnnotatedVersion
                {
                    VersionString = new("2023.06.13.0000.0001"),
                    IsActive = false,
                    PrerequisiteVersions = [],
                    Patches = [
                        new Patch {
                            Url = "http://patch-dl.ffxiv.com/game/4e9a232b/D2023.06.13.0000.0001.patch",
                            Size = 89863002
                        }
                    ]
                },
                new AnnotatedVersion
                {
                    VersionString = new("2024.04.22.0000.0001"),
                    IsActive = false,
                    PrerequisiteVersions = [],
                    Patches = [
                        new Patch {
                            Url = "http://patch-dl.ffxiv.com/game/4e9a232b/D2024.04.22.0000.0001.patch",
                            Size = 16909460
                        }
                    ]
                }
            ]}
        }.ToFrozenDictionary();

    public ThaliakClient()
    {
        var serializer = new SystemTextJsonSerializer();
        serializer.Options.Converters.Add(new ParsedVersionString.JsonConverter());
        Client = new("https://thaliak.xiv.dev/graphql/2022-08-14", serializer);
    }

    public async Task<Repository> GetRepositoryMetadataAsync(string slug, CancellationToken token = default)
    {
        return (await Client.SendQueryAsync<RepositoryResponse>(new GraphQLRequest
        {
            Query = @"
            query($repoId: String!) {
                repository(slug: $repoId) {
                    name
                    description
                    latestVersion {
                        versionString
                    }
                }
            }",
            Variables = new
            {
                repoId = slug
            }
        }, token).ConfigureAwait(false)).Data.Repository;
    }

    public async Task<List<(ParsedVersionString Version, Patch Patch)>> GetPatchChainAsync(string slug, ParsedVersionString version, CancellationToken token = default)
    {
        var versionList = (await Client.SendQueryAsync<RepositoryResponse>(new GraphQLRequest
        {
            Query = @"
            query($repoId: String!) {
                repository(slug: $repoId) {
                    versions {
                        versionString
                        isActive
                        prerequisiteVersions {
                            versionString
                        }
                        patches {
                            url
                            size
                        }
                    }
                }
            }",
            Variables = new
            {
                repoId = slug
            }
        }, token).ConfigureAwait(false)).Data.Repository.Versions!;

        var additionalVersions = AdditionalVersions.GetValueOrDefault(slug);
        versionList.AddRange(additionalVersions);
        foreach (var addVersion in additionalVersions)
            Log.Verbose($"Injecting {addVersion.VersionString} into patch chain");

        var versions = versionList.ToDictionary(v => v.VersionString);

        var overrides = Overrides.GetValueOrDefault(slug);

        var ret = new List<(ParsedVersionString, Patch)>();

        var nextVer = versions.GetValueOrDefault(version);
        while (nextVer != null)
        {
            ArgumentOutOfRangeException.ThrowIfNotEqual(nextVer.Patches.Count, 1, nameof(nextVer.Patches.Count));
            var patch = nextVer.Patches[0];
            ret.Add((nextVer.VersionString, patch));

            if (overrides?.TryGetValue(nextVer.VersionString, out var @override) ?? false)
            {
                if (@override == null)
                    break;
                Log.Verbose($"Overriding {nextVer.VersionString} with {@override.Value}");
                nextVer = versions.GetValueOrDefault(@override.Value);
                ArgumentNullException.ThrowIfNull(nextVer);
                continue;
            }

            nextVer = nextVer.PrerequisiteVersions
                .Where(prereq => !ret.Any(v => v.Item1 == prereq.VersionString))
                .Select(prereq => versions.GetValueOrDefault(prereq.VersionString) ?? throw new ArgumentNullException(nameof(prereq)))
                .Where(prereq => !nextVer.IsActive || prereq.IsActive)
                .OrderByDescending(prereq => prereq.VersionString)
                .FirstOrDefault();
        }

        ret.Reverse();
        return ret;
    }

    public async Task<string> GetGraphvizTreeAsync(string slug, bool verifyExistence, bool filterInactive)
    {
        var versions = (await Client.SendQueryAsync<RepositoryResponse>(new GraphQLRequest
        {
            Query = @"
            query($repoId: String!) {
                repository(slug: $repoId) {
                    versions {
                        versionString
                        isActive
                        prerequisiteVersions {
                            versionString
                        }
                        patches {
                            url
                            size
                        }
                    }
                }
            }",
            Variables = new
            {
                repoId = slug
            }
        }).ConfigureAwait(false)).Data.Repository.Versions!;

        var additionalVersions = AdditionalVersions.GetValueOrDefault(slug);
        versions.AddRange(additionalVersions);

        var treeList = versions.OrderByDescending(x => x.VersionString).ToList();
        if (filterInactive)
            treeList = [.. treeList.Where(x => x.IsActive)];
        var overrides = Overrides.GetValueOrDefault(slug);
        var b = new StringBuilder();
        b.AppendLine("digraph {");
        foreach (var (idx, ver) in treeList.Index())
        {
            var exists = !verifyExistence || await DoesPatchExist(ver.Patches[0]).ConfigureAwait(false);
            var (fill, font) = (exists, ver.IsActive) switch
            {
                (true, true) => ("lightgreen", "black"),
                (true, false) => ("yellow", "black"),
                (false, true) => ("red", "white"),
                (false, false) => ("darkred", "white")
            };
            b.AppendLine($"  Idx{idx} [ label = \"{ver.VersionString.ToString("P")}\" style = filled fillcolor = {fill} fontcolor = {font} ]");
            var prereqs = ver.PrerequisiteVersions?.Select(v => v.VersionString) ?? [];
            prereqs = prereqs.Where(prereq => prereq < ver.VersionString);
            if (overrides?.TryGetValue(ver.VersionString, out var @override) ?? false)
                prereqs = @override.HasValue ? [@override.Value] : [];
            foreach (var (listIdx, prereq) in prereqs.OrderDescending().Index())
            {
                var prereqIdx = treeList.FindIndex(v => v.VersionString == prereq);
                if (prereqIdx == -1)
                    continue;
                b.Append($"  Idx{idx} -> Idx{prereqIdx}");
                if (listIdx != 0)
                    b.Append(" [ color = red ]");
                b.AppendLine();
            }
        }
        b.AppendLine("}");
        return b.ToString();
    }

    private async Task<bool> DoesPatchExist(Patch patch)
    {
        var resp = await Client.HttpClient.SendAsync(new(HttpMethod.Head, patch.Url), HttpCompletionOption.ResponseHeadersRead).ConfigureAwait(false);
        return resp.StatusCode switch
        {
            HttpStatusCode.OK => true,
            HttpStatusCode.NotFound => false,
            _ => throw new InvalidOperationException($"Unexpected status code: {resp.StatusCode}")
        };
    }

    public void Dispose()
    {
        Client.Dispose();
    }
}

public sealed record RepositoryResponse(Repository Repository);

