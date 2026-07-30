use std::fmt::Write as _;

use anyhow::{Result, bail};
use reqwest::{Client, StatusCode};

use super::chain::{Patch, get_versions, overrides_for};

pub async fn get_graphviz_tree(
    client: &Client,
    slug: &str,
    verify_existence: bool,
    filter_inactive: bool,
) -> Result<String> {
    let mut tree = get_versions(client, slug).await?;
    tree.sort_by(|a, b| b.version.cmp(&a.version));
    if filter_inactive {
        tree.retain(|node| node.is_active);
    }

    let overrides = overrides_for(slug);
    let mut out = String::from("digraph {\n");

    for (idx, node) in tree.iter().enumerate() {
        let exists = !verify_existence || patch_exists(client, &node.patches[0]).await?;
        let (fill, font) = match (exists, node.is_active) {
            (true, true) => ("lightgreen", "black"),
            (true, false) => ("yellow", "black"),
            (false, true) => ("red", "white"),
            (false, false) => ("darkred", "white"),
        };
        writeln!(
            out,
            "  Idx{idx} [ label = \"{}\" style = filled fillcolor = {fill} fontcolor = {font} ]",
            node.version
        )?;

        let mut prereqs: Vec<_> = match overrides.and_then(|o| o.get(&node.version)) {
            Some(replacement) => replacement.clone().into_iter().collect(),
            None => node
                .prerequisites
                .iter()
                .filter(|prereq| **prereq < node.version)
                .cloned()
                .collect(),
        };
        prereqs.sort_by(|a, b| b.cmp(a));

        for (list_idx, prereq) in prereqs.iter().enumerate() {
            let Some(prereq_idx) = tree.iter().position(|v| v.version == *prereq) else {
                continue;
            };
            write!(out, "  Idx{idx} -> Idx{prereq_idx}")?;
            if list_idx != 0 {
                write!(out, " [ color = red ]")?;
            }
            out.push('\n');
        }
    }

    out.push_str("}\n");
    Ok(out)
}

async fn patch_exists(client: &Client, patch: &Patch) -> Result<bool> {
    let status = client.head(&patch.url).send().await?.status();
    match status {
        StatusCode::OK => Ok(true),
        StatusCode::NOT_FOUND => Ok(false),
        other => bail!("Unexpected status code: {other}"),
    }
}
