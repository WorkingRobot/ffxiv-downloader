use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use xiv_core::file::lut::Lut;
use xiv_core::file::types::PlatformId;
use xiv_core::file::version::{GameVersion, PatchVersion};
use xiv_core::index::{Repository, VerifyMethod, Verified, now};
use xiv_core::zipatch::chunk::{HEADER_SIZE, resolve_platform};
use xiv_core::zipatch::Chunk;

#[derive(Args, Debug, Clone)]
pub struct VerifyArgs {
    /// Repository slug to verify
    #[arg(short, long, value_name = "SLUG")]
    pub slug: String,
    /// Directory holding repository files
    #[arg(short, long, value_name = "DIR", default_value = ".")]
    pub index_path: PathBuf,
    /// Directory holding the lut files for this repository
    #[arg(short, long, value_name = "DIR")]
    pub luts: PathBuf,
    /// Directory holding sqpack index files downloaded for the version
    #[arg(long, value_name = "DIR")]
    pub indexes: PathBuf,
    /// Version to verify, defaulting to the repository's latest
    #[arg(short, long, value_name = "VERSION")]
    pub version: Option<String>,
}

pub fn run(args: VerifyArgs) -> Result<()> {
    let path = crate::index::repos_dir(&args.index_path)?.join(format!("{}.json", args.slug));
    let text =
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let mut repository: Repository =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;

    let version = match &args.version {
        Some(version) => GameVersion::new(version)?,
        None => repository.latest.clone(),
    };
    let chain = repository.chain(&version)?;

    let mut files: BTreeMap<String, Vec<(u64, u64)>> = BTreeMap::new();
    for (step, patch) in &chain {
        let name = patch.version().unwrap_or_else(|_| {
            PatchVersion::new(&step.to_string()).expect("game version parses as a patch version")
        });
        let lut = args.luts.join(format!("{name}.lut"));
        let lut = Lut::read(BufReader::new(
            File::open(&lut).with_context(|| format!("opening {}", lut.display()))?,
        ))?;
        cover(&mut files, &lut);
    }

    let spans: BTreeMap<String, Vec<(u64, u64)>> = files
        .into_iter()
        .map(|(target, intervals)| (target, merge(intervals)))
        .collect();

    let mut entries = 0u64;
    let mut unsupplied = 0u64;
    for index in sqpack_indexes(&args.indexes)? {
        let (expansion, stem) = describe(&index)?;
        for (dat, offset) in index_entries(&index)? {
            entries += 1;
            let target = format!("sqpack/{expansion}/{stem}.dat{dat}");
            if !covered(spans.get(&target).map(Vec::as_slice).unwrap_or(&[]), offset) {
                unsupplied += 1;
            }
        }
    }

    if entries == 0 {
        bail!("{} holds no sqpack index files", args.indexes.display());
    }

    log::info!(
        "{} {version}: {} patches, {entries} indexed entries, {unsupplied} unsupplied",
        args.slug,
        chain.len()
    );

    let record = Verified {
        at: now(),
        method: VerifyMethod::ChainCoverage,
        unsupplied,
    };
    for (step, _) in &chain {
        if let Some(entry) = repository.patches.get_mut(step) {
            entry.verified = Some(record.clone());
        }
    }
    let mut text = serde_json::to_string_pretty(&repository)?;
    text.push('\n');
    std::fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

fn cover(files: &mut BTreeMap<String, Vec<(u64, u64)>>, lut: &Lut) {
    for chunk in &lut.chunks {
        match chunk {
            Chunk::SqpkAddData {
                target,
                block_offset,
                block_number,
                block_delete_number,
                ..
            } => push(
                files,
                target,
                *block_offset as u64,
                (block_offset + block_number + block_delete_number) as u64,
            ),
            Chunk::SqpkDeleteData {
                target,
                block_offset,
                block_number,
            }
            | Chunk::SqpkExpandData {
                target,
                block_offset,
                block_number,
            } => push(
                files,
                target,
                *block_offset as u64,
                (block_offset + block_number) as u64,
            ),
            Chunk::SqpkHeader {
                target,
                header_kind,
                ..
            } => {
                let start = if Chunk::is_version_header(*header_kind) {
                    0
                } else {
                    HEADER_SIZE as u64
                };
                push(files, target, start, start + HEADER_SIZE as u64);
            }
            Chunk::SqpkFileAdd {
                target,
                file_offset,
                blocks,
            } => {
                let key = key(target);
                if *file_offset == 0 {
                    files.remove(&key);
                }
                let size: i64 = blocks.iter().map(|b| i64::from(b.data_size)).sum();
                push(files, target, *file_offset as u64, (file_offset + size) as u64);
            }
            Chunk::SqpkFileDelete { target } => {
                files.remove(&key(target));
            }
            _ => {}
        }
    }
}

fn key(target: &str) -> String {
    resolve_platform(target, PlatformId::Win32)
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

fn push(files: &mut BTreeMap<String, Vec<(u64, u64)>>, target: &str, start: u64, end: u64) {
    files.entry(key(target)).or_default().push((start, end));
}

fn merge(mut intervals: Vec<(u64, u64)>) -> Vec<(u64, u64)> {
    intervals.sort_unstable();
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(intervals.len());
    for (start, end) in intervals {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

fn covered(spans: &[(u64, u64)], offset: u64) -> bool {
    spans
        .binary_search_by(|(start, end)| {
            if *end <= offset {
                std::cmp::Ordering::Less
            } else if *start > offset {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

fn sqpack_indexes(root: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(path) = stack.pop() {
        for entry in std::fs::read_dir(&path)
            .with_context(|| format!("reading {}", path.display()))?
        {
            let path = entry?.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "index") {
                found.push(path);
            }
        }
    }
    found.sort();
    Ok(found)
}

fn describe(index: &Path) -> Result<(String, String)> {
    let stem = index
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("no name in {}", index.display()))?;
    let expansion = index
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .with_context(|| format!("no expansion directory for {}", index.display()))?;
    Ok((expansion.to_string(), stem.to_string()))
}

fn index_entries(path: &Path) -> Result<Vec<(u32, u64)>> {
    let body = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    let at = |offset: usize| -> Result<u32> {
        body.get(offset..offset + 4)
            .map(|slice| u32::from_le_bytes(slice.try_into().expect("four bytes")))
            .with_context(|| format!("{} is truncated at {offset}", path.display()))
    };
    let header = at(0x0C)? as usize;
    let start = at(header + 8)? as usize;
    let length = at(header + 12)? as usize;
    let mut entries = Vec::with_capacity(length / 16);
    for offset in (start..start + length).step_by(16) {
        let data = at(offset + 8)?;
        entries.push(((data & 0x0F) >> 1, u64::from(data & !0x0F) * 0x08));
    }
    Ok(entries)
}
