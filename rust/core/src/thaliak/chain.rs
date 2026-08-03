use std::collections::HashMap;
use std::sync::LazyLock;

use anyhow::{Context, Result, ensure};
use reqwest::Client;

use crate::file::version::{GameVersion, PatchVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Patch {
    pub url: String,
    pub size: i64,
}

impl Patch {
    pub fn version(&self) -> Result<PatchVersion> {
        let name = self
            .url
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        ensure!(!stem.is_empty(), "no version in patch url {}", self.url);
        PatchVersion::new(stem)
    }
}

#[derive(Debug, Clone)]
pub struct Node {
    pub version: GameVersion,
    pub is_active: bool,
    pub prerequisites: Vec<GameVersion>,
    pub patches: Vec<Patch>,
}

/// `[(slug, [(version, replacement or end-of-chain)])]`.
type OverrideTable = &'static [(
    &'static str,
    &'static [(&'static str, Option<&'static str>)],
)];

/// `[(slug, [(version, patch url, patch size)])]`.
type ExtraTable = &'static [(&'static str, &'static [(&'static str, &'static str, i64)])];

/// Thaliak's prerequisite edges are wrong or absent for these versions. The value
/// replaces whichever predecessor the graph would otherwise pick, and `None` ends the
/// chain there.
const OVERRIDES: OverrideTable = &[
    // Global
    (
        "4e9a232b",
        &[
            // Thaliak incorrectly orders these hist patches.
            // aa comes after z. It's not lexicographically sorted.
            ("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000ag")),
            ("H2024.05.31.0000.0000b", Some("H2024.05.31.0000.0000a")),
            ("H2024.05.31.0000.0000aa", Some("H2024.05.31.0000.0000z")),
            // Spooky unseen patches o.o
            ("2024.04.23.0000.0000", Some("2024.04.22.0000.0001")),
            ("2024.04.22.0000.0001", Some("2024.03.27.0000.0000")),
            ("2023.06.14.0000.0000", Some("2023.06.13.0000.0001")),
            ("2023.06.13.0000.0001", Some("2023.05.11.0000.0001")),
            ("2017.06.06.0000.0001", Some("H2017.06.06.0000.0001m")),
            ("H2017.06.06.0000.0001a", None),
        ],
    ),
    (
        "6b936f08",
        &[("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000d"))],
    ),
    (
        "f29a3eb2",
        &[("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000e"))],
    ),
    (
        "859d0e24",
        &[("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000g"))],
    ),
    (
        "1bf99b87",
        &[("2024.05.31.0000.0000", Some("H2024.05.31.0000.0000i"))],
    ),
    // Korea
    (
        "de199059",
        &[
            ("2024.11.02.0000.0000", Some("H2024.11.02.0000.0000ad")),
            ("H2024.11.02.0000.0000b", Some("H2024.11.02.0000.0000a")),
            ("H2024.11.02.0000.0000aa", Some("H2024.11.02.0000.0000z")),
        ],
    ),
    (
        "573d8c07",
        &[("2024.10.22.0002.0000", Some("H2024.10.22.0002.0000c"))],
    ),
    (
        "ce34ddbd",
        &[("2024.10.22.0003.0000", Some("H2024.10.22.0003.0000e"))],
    ),
    (
        "b933ed2b",
        &[("2024.11.02.0000.0000", Some("H2024.11.02.0000.0000f"))],
    ),
    (
        "27577888",
        &[("2024.11.02.0000.0000", Some("H2024.11.02.0000.0000g"))],
    ),
    // China
    (
        "c38effbc",
        &[
            ("2024.09.09.0000.0000", Some("H2024.09.09.0000.0000ad")),
            ("H2024.09.09.0000.0000b", Some("H2024.09.09.0000.0000a")),
            ("H2024.09.09.0000.0000aa", Some("H2024.09.09.0000.0000z")),
        ],
    ),
    (
        "77420d17",
        &[("2024.08.27.0002.0000", Some("H2024.08.27.0002.0000c"))],
    ),
    (
        "ee4b5cad",
        &[("2024.08.27.0003.0000", Some("H2024.08.27.0003.0000e"))],
    ),
    (
        "994c6c3b",
        &[("2024.09.09.0000.0000", Some("H2024.09.09.0000.0000f"))],
    ),
    (
        "0728f998",
        &[("2024.09.09.0000.0000", Some("H2024.09.09.0000.0000g"))],
    ),
];

/// Versions Thaliak never listed, but that an overridden edge points at.
const EXTRA_VERSIONS: ExtraTable = &[(
    "4e9a232b",
    &[
        (
            "2023.06.13.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2023.06.13.0000.0001.patch",
            89_863_002,
        ),
        (
            "2024.04.22.0000.0001",
            "http://patch-dl.ffxiv.com/game/4e9a232b/D2024.04.22.0000.0001.patch",
            16_909_460,
        ),
    ],
)];

type Overrides = HashMap<GameVersion, Option<GameVersion>>;

static PARSED_OVERRIDES: LazyLock<HashMap<&'static str, Overrides>> = LazyLock::new(|| {
    OVERRIDES
        .iter()
        .map(|(slug, entries)| {
            let entries = entries
                .iter()
                .map(|(from, to)| {
                    (
                        GameVersion::new(from).expect("override version"),
                        to.map(|to| GameVersion::new(to).expect("override version")),
                    )
                })
                .collect();
            (*slug, entries)
        })
        .collect()
});

static PARSED_EXTRA: LazyLock<HashMap<&'static str, Vec<Node>>> = LazyLock::new(|| {
    EXTRA_VERSIONS
        .iter()
        .map(|(slug, entries)| {
            let nodes = entries
                .iter()
                .map(|(version, url, size)| Node {
                    version: GameVersion::new(version).expect("extra version"),
                    is_active: false,
                    prerequisites: Vec::new(),
                    patches: vec![Patch {
                        url: (*url).to_string(),
                        size: *size,
                    }],
                })
                .collect();
            (*slug, nodes)
        })
        .collect()
});

pub fn overrides_for(slug: &str) -> Option<&'static Overrides> {
    PARSED_OVERRIDES.get(slug)
}

/// A version's place in the graph: the patch that reaches it, and the version that
/// patch is applied on top of.
#[derive(Debug, Clone)]
pub struct Step {
    pub version: GameVersion,
    pub patch: Patch,
    pub parent: Option<GameVersion>,
}

pub async fn get_versions(client: &Client, slug: &str) -> Result<Vec<Node>> {
    let mut nodes = super::query_versions(client, slug).await?;

    if let Some(extra) = PARSED_EXTRA.get(slug) {
        for node in extra {
            log::debug!("Injecting {} into patch chain", node.version);
            nodes.push(node.clone());
        }
    }

    Ok(nodes)
}

/// The ordered list of patches to apply to reach `version`.
pub async fn get_patch_chain(
    client: &Client,
    slug: &str,
    version: &GameVersion,
) -> Result<Vec<(GameVersion, Patch)>> {
    let nodes = get_versions(client, slug).await?;
    build_chain(slug, &nodes, version)
}

/// Every version the repository offers, not only those on the chain to one of them.
pub async fn get_patch_forest(client: &Client, slug: &str) -> Result<Vec<Step>> {
    let nodes = get_versions(client, slug).await?;
    build_forest(slug, &nodes)
}

type VersionIndex<'a> = HashMap<&'a GameVersion, &'a Node>;

fn index_versions<'a>(slug: &str, nodes: &'a [Node]) -> VersionIndex<'a> {
    let mut by_version = HashMap::with_capacity(nodes.len());
    for node in nodes {
        if by_version.insert(&node.version, node).is_some() {
            // Thaliak has caught up with an injected version, so EXTRA_VERSIONS has an
            // entry it no longer needs.
            log::warn!("{slug} lists {} more than once", node.version);
        }
    }
    by_version
}

fn sole_patch<'a>(slug: &str, node: &'a Node) -> Result<&'a Patch> {
    ensure!(
        node.patches.len() == 1,
        "{slug} version {} has {} patches, expected 1",
        node.version,
        node.patches.len()
    );
    Ok(&node.patches[0])
}

/// The version a patch is applied on top of, or `None` where a lineage begins.
///
/// Thaliak lists a lineage's every later version as a prerequisite of the full-install
/// patch that starts it, so those edges point forward. A prerequisite is older than what
/// it precedes, which makes the newer ones no such thing; discarding them also leaves a
/// walk whose version strictly descends, and which therefore ends.
fn predecessor<'a>(
    slug: &str,
    by_version: &VersionIndex<'a>,
    node: &Node,
) -> Result<Option<&'a Node>> {
    if let Some(replacement) = overrides_for(slug).and_then(|o| o.get(&node.version)) {
        let Some(replacement) = replacement else {
            return Ok(None);
        };
        log::debug!("Overriding {} with {replacement}", node.version);
        return by_version
            .get(replacement)
            .copied()
            .with_context(|| format!("{slug} has no version {replacement}"))
            .map(Some);
    }

    // Among the prerequisites, take the newest. An inactive version may still lead back
    // through inactive ones; an active one may not.
    Ok(node
        .prerequisites
        .iter()
        .filter(|prereq| **prereq < node.version)
        .map(|prereq| {
            by_version
                .get(prereq)
                .copied()
                .with_context(|| format!("{slug} has no version {prereq}"))
        })
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .filter(|prereq| !node.is_active || prereq.is_active)
        .max_by(|a, b| a.version.cmp(&b.version)))
}

fn build_chain(
    slug: &str,
    nodes: &[Node],
    version: &GameVersion,
) -> Result<Vec<(GameVersion, Patch)>> {
    let by_version = index_versions(slug, nodes);

    let mut chain: Vec<(GameVersion, Patch)> = Vec::new();
    let mut current = by_version.get(version).copied();
    while let Some(node) = current {
        chain.push((node.version.clone(), sole_patch(slug, node)?.clone()));
        current = predecessor(slug, &by_version, node)?;
    }

    chain.reverse();
    Ok(chain)
}

/// Every version in the repository, each paired with the one it follows. Ordered so
/// that a version comes after its parent, which holds because a parent is older.
pub fn build_forest(slug: &str, nodes: &[Node]) -> Result<Vec<Step>> {
    let by_version = index_versions(slug, nodes);

    let mut steps = by_version
        .values()
        .map(|node| {
            Ok(Step {
                version: node.version.clone(),
                patch: sole_patch(slug, node)?.clone(),
                parent: predecessor(slug, &by_version, node)?.map(|parent| parent.version.clone()),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    steps.sort_by(|a, b| a.version.cmp(&b.version));
    Ok(steps)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(version: &str, prerequisites: &[&str]) -> Node {
        Node {
            version: GameVersion::new(version).unwrap(),
            is_active: true,
            prerequisites: prerequisites
                .iter()
                .map(|version| GameVersion::new(version).unwrap())
                .collect(),
            patches: vec![Patch {
                url: format!("http://patches/{version}.patch"),
                size: 1,
            }],
        }
    }

    /// The lineage: a full install, one more historic patch, then two ordinary ones. The
    /// full install lists everything that follows it as a prerequisite.
    fn lineage() -> [Node; 4] {
        [
            node(
                "H2024.01.01.0000.0000a",
                &["2024.02.02.0000.0000", "2024.03.03.0000.0000"],
            ),
            node("H2024.01.01.0000.0000b", &["H2024.01.01.0000.0000a"]),
            node("2024.02.02.0000.0000", &["H2024.01.01.0000.0000b"]),
            node("2024.03.03.0000.0000", &["2024.02.02.0000.0000"]),
        ]
    }

    fn walked(nodes: &[Node], version: &str) -> Vec<String> {
        build_chain("slug", nodes, &GameVersion::new(version).unwrap())
            .unwrap()
            .into_iter()
            .map(|(version, _)| version.to_string())
            .collect()
    }

    /// Taking a full install's forward-pointing prerequisites at face value sends a walk
    /// up the lineage and back down it, so a version in the middle would be reached by
    /// applying its own successors first.
    #[test]
    fn a_chain_stops_where_its_lineage_begins() {
        let nodes = lineage();
        assert_eq!(
            walked(&nodes, "H2024.01.01.0000.0000b"),
            ["H2024.01.01.0000.0000a", "H2024.01.01.0000.0000b"]
        );
        assert_eq!(
            walked(&nodes, "2024.03.03.0000.0000"),
            [
                "H2024.01.01.0000.0000a",
                "H2024.01.01.0000.0000b",
                "2024.02.02.0000.0000",
                "2024.03.03.0000.0000",
            ]
        );
    }

    /// The walk to a version is the walk to its parent with that version on the end, so
    /// folding the forest reaches every version having applied each patch once.
    #[test]
    fn a_forest_holds_every_version_after_the_one_it_follows() {
        let nodes = lineage();
        let forest = build_forest("slug", &nodes).unwrap();
        assert_eq!(forest.len(), nodes.len());
        assert_eq!(
            forest.iter().filter(|step| step.parent.is_none()).count(),
            1
        );

        let mut placed: Vec<GameVersion> = Vec::new();
        for step in &forest {
            if let Some(parent) = &step.parent {
                assert!(
                    placed.contains(parent),
                    "{} is placed before {parent}, which it follows",
                    step.version
                );
                assert!(
                    *parent < step.version,
                    "{parent} is not older than {}",
                    step.version
                );
            }
            let chain = walked(&nodes, &step.version.to_string());
            assert_eq!(chain.last().unwrap(), &step.version.to_string());
            placed.push(step.version.clone());
        }
    }
}
