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

fn build_chain(
    slug: &str,
    nodes: &[Node],
    version: &GameVersion,
) -> Result<Vec<(GameVersion, Patch)>> {
    let mut by_version: HashMap<&GameVersion, &Node> = HashMap::with_capacity(nodes.len());
    for node in nodes {
        if by_version.insert(&node.version, node).is_some() {
            // Thaliak has caught up with an injected version, so EXTRA_VERSIONS has an
            // entry it no longer needs.
            log::warn!("{slug} lists {} more than once", node.version);
        }
    }

    let overrides = overrides_for(slug);
    let mut chain: Vec<(GameVersion, Patch)> = Vec::new();
    let mut current = by_version.get(version).copied();

    while let Some(node) = current {
        ensure!(
            node.patches.len() == 1,
            "{slug} version {} has {} patches, expected 1",
            node.version,
            node.patches.len()
        );
        chain.push((node.version.clone(), node.patches[0].clone()));

        if let Some(replacement) = overrides.and_then(|o| o.get(&node.version)) {
            let Some(replacement) = replacement else {
                break;
            };
            log::debug!("Overriding {} with {replacement}", node.version);
            current = Some(
                by_version
                    .get(replacement)
                    .copied()
                    .with_context(|| format!("{slug} has no version {replacement}"))?,
            );
            continue;
        }

        // Among the prerequisites not already walked, take the newest. An inactive
        // version may still lead back through inactive ones; an active one may not.
        current = node
            .prerequisites
            .iter()
            .filter(|prereq| !chain.iter().any(|(walked, _)| walked == *prereq))
            .map(|prereq| {
                by_version
                    .get(prereq)
                    .copied()
                    .with_context(|| format!("{slug} has no version {prereq}"))
            })
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .filter(|prereq| !node.is_active || prereq.is_active)
            .max_by(|a, b| a.version.cmp(&b.version));
    }

    chain.reverse();
    Ok(chain)
}
