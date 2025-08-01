use crate::{
    diff::ClutDiff,
    file::{data_ref::DataRef, version::PatchVersion},
    ops::{TargetFile, TargetFileExt},
};
use crate::{file::patch_ref::PatchRef, ops::FileOperations};
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use either::Either;
use flate2::read::DeflateDecoder;
use futures::{
    FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt, future,
    stream::{self, PollNext, try_unfold},
};
use http_content_range::{ContentRange, ContentRangeBytes};
use multer::{Multipart, parse_boundary};
use reqwest::{
    Client,
    header::{self, HeaderMap},
};
use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use std::{fmt, io::Read};
use tokio::sync::Semaphore;
use tokio_util::io::StreamReader;

// Akamai restricts the range header size to at most 1034 bytes from my testing,
// but it doesn't work sometimes, so use a smaller number
const MAX_RANGE_HEADER_SIZE: usize = 1 << 12;
const MIN_RANGE_DISTANCE: u64 = 1 << 9;

/// Reference to a DataRef by its indices
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataReference {
    pub file_name_index: usize,
    pub data_ref_index: usize,
}

/// Reference to a PatchRef by its indices
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PatchReference(DataReference);

/// Represents merged/consolidated HTTP ranges for efficient downloading
/// Matches the C# MergedRange class functionality
#[derive(Debug, Clone)]
pub struct MergedRange {
    pub offset: u64,
    pub size: u64,
    pub parts: Vec<PatchRef>,
}

impl MergedRange {
    /// Create a new merged range from a single patch reference
    pub fn new(patch_ref: PatchRef) -> Self {
        Self {
            offset: patch_ref.offset,
            size: patch_ref.size as u64,
            parts: vec![patch_ref],
        }
    }

    /// Try to add another patch reference to this range
    /// Returns true if merged successfully, false if ranges are too far apart
    /// Implements the C# MergedRange.Add() logic
    pub fn try_add(&mut self, patch_ref: &PatchRef) -> bool {
        if (patch_ref.offset + patch_ref.size as u64 + MIN_RANGE_DISTANCE) < self.offset {
            false
        } else {
            self.size = self
                .size
                .max(patch_ref.offset + patch_ref.size as u64 - self.offset);
            self.parts.push(patch_ref.clone());
            true
        }
    }

    pub fn end(&self) -> u64 {
        self.offset + self.size - 1
    }

    /// Get the range header string for this range
    pub fn to_range_header_value(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for MergedRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.offset, self.end())
    }
}

/// A batch of HTTP ranges to download together
#[derive(Debug, Default)]
pub struct RangeBatch(Vec<MergedRange>);

impl RangeBatch {
    /// Try to add a range to this batch
    /// Returns true if added successfully, false if it would exceed header size limit
    pub fn try_add(&mut self, range: MergedRange) -> bool {
        self.0.push(range);
        if self.to_range_header().len() > MAX_RANGE_HEADER_SIZE {
            self.0.pop(); // Remove last added range if it exceeds limit
            false
        } else {
            true
        }
    }

    pub fn to_range_header(&self) -> String {
        self.to_string()
    }
}

impl fmt::Display for RangeBatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bytes=")?;
        for (i, range) in self.0.iter().enumerate() {
            if i > 0 {
                write!(f, ",")?;
            }
            write!(f, "{range}")?;
        }
        Ok(())
    }
}

/// The main CLUT patcher that handles downloading and applying patches
pub struct ClutPatcher<F: FileOperations> {
    client: Client,
    diff: ClutDiff,
    file_names: Vec<String>,
    ops: Arc<F>,
    // token: CancellationToken,
    semaphore: Semaphore,
    max_concurrent_writes: usize,
}

impl<F: FileOperations> ClutPatcher<F> {
    /// Create a new CLUT patcher instance
    pub fn new(
        diff: ClutDiff,
        ops: F,
        max_concurrent_downloads: usize,
        max_concurrent_writes: usize,
    ) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .danger_accept_invalid_hostnames(true)
                .user_agent("FFXIV PATCH CLIENT")
                .build()
                .context("Failed to create HTTP client")?,
            file_names: diff.added_files.keys().cloned().collect(),
            diff,
            ops: Arc::new(ops),
            semaphore: Semaphore::new(max_concurrent_downloads),
            max_concurrent_writes,
        })
    }

    pub fn operations(&self) -> Arc<F> {
        self.ops.clone()
    }

    pub async fn apply_diff(&self) -> Result<()> {
        for removed_file in &self.diff.removed_files {
            self.ops.delete_file(removed_file).await?;
        }

        for removed_dir in &self.diff.removed_folders {
            self.ops.delete_directory(removed_dir).await?;
        }

        for added_dir in &self.diff.added_folders {
            self.ops.create_directory(added_dir).await?;
        }

        self.process_data_refs().await?;

        Ok(())
    }

    async fn process_data_refs(&self) -> Result<()> {
        let all_refs = self.file_names.iter().enumerate().flat_map(|(idx, name)| {
            (0..self.diff.added_files[name].len()).map(move |data_ref_idx| DataReference {
                file_name_index: idx,
                data_ref_index: data_ref_idx,
            })
        });

        let plain_refs: Vec<_> = all_refs
            .clone()
            .filter(|data_ref| !self.get_data_ref(data_ref).is_patch())
            .collect();
        let plain_refs_len = plain_refs.len();

        let mut patch_refs_len = 0usize;
        let mut patch_refs = HashMap::new();
        for data_ref in all_refs
            .clone()
            .filter(|data_ref| self.get_data_ref(data_ref).is_patch())
        {
            let patch_ref = PatchReference(data_ref.clone());
            patch_refs
                .entry(self.get_patch_ref(&patch_ref))
                .or_insert_with(Vec::new)
                .push(patch_ref);
            patch_refs_len += 1;
        }

        let write_size = all_refs.fold(0usize, |acc, data_ref| {
            acc + self.get_data_ref(&data_ref).len() as usize
        });
        let download_size = patch_refs
            .keys()
            .fold(0usize, |acc, (_, patch_ref)| acc + patch_ref.size as usize);

        log::info!(
            "Total write size: {:.2} MiB",
            write_size as f64 / (1 << 20) as f64
        );
        log::info!(
            "Approx. download size: {:.2} MiB",
            download_size as f64 / (1 << 20) as f64
        );

        let plain_stream = stream::iter(plain_refs.into_iter().map(|r| Ok(Either::Left(r))));
        let download_stream = self
            .get_patch_data(patch_refs.keys().copied().collect())
            .map(|r| r.map(Either::Right));
        let full_stream =
            stream::select_with_strategy(plain_stream, download_stream, |_: &mut ()| {
                PollNext::Right
            });

        let plain_refs_seen = AtomicUsize::new(0);
        let patch_refs_seen = AtomicUsize::new(0);

        full_stream
            .try_for_each_concurrent(Some(self.max_concurrent_writes), |operation| async {
                match operation {
                    Either::Left(data_ref) => {
                        plain_refs_seen.fetch_add(1, Ordering::Relaxed);
                        self.process_op_plain(&data_ref).await?;
                    }
                    Either::Right(((version, patch_ref), bytes)) => {
                        let refs =
                            patch_refs
                                .get(&(version.as_ref(), &patch_ref))
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "No patch references found for version {:?} and patch {:?}",
                                        version,
                                        patch_ref
                                    )
                                })?;
                        assert!(!refs.is_empty(), "Patch references should not be empty");
                        for patch_ref in refs {
                            patch_refs_seen.fetch_add(1, Ordering::Relaxed);
                            self.process_op_patch(patch_ref, &bytes).await?;
                        }
                    }
                };
                Ok(())
            })
            .await?;

        let plain_refs_seen = plain_refs_seen.load(Ordering::SeqCst);
        let patch_refs_seen = patch_refs_seen.load(Ordering::SeqCst);
        assert_eq!(
            plain_refs_seen, plain_refs_len,
            "Expected to process {plain_refs_len} plain references, but got {plain_refs_seen}",
        );
        assert_eq!(
            patch_refs_seen, patch_refs_len,
            "Expected to process {patch_refs_len} patch references, but got {patch_refs_seen}",
        );

        Ok(())
    }

    async fn process_op_plain(&self, data_ref: &DataReference) -> Result<()> {
        let file = self.ops.open_file(self.get_data_ref_name(data_ref)).await?;
        let data_ref = self.get_data_ref(data_ref);
        assert!(!data_ref.is_patch(), "Expected plain DataRef, got patch");
        if data_ref.is_zero() {
            file.wipe(data_ref.offset(), data_ref.len()).await?;
        } else if data_ref.is_empty_block() {
            file.write_empty_file_block(data_ref.block_count().unwrap(), data_ref.offset())
                .await?;
        } else {
            unreachable!()
        }
        Ok(())
    }

    async fn process_op_patch(&self, patch_ref: &PatchReference, bytes: &Bytes) -> Result<()> {
        let file = self
            .ops
            .open_file(self.get_data_ref_name(&patch_ref.0))
            .await?;
        let data_ref = self.get_data_ref(&patch_ref.0);
        assert!(data_ref.is_patch(), "Expected patch DataRef, got plain");
        let patch_offset = data_ref.patch_offset().unwrap() as usize;
        let data = bytes.slice(patch_offset..patch_offset + data_ref.len() as usize);
        file.write_at(&data, data_ref.offset()).await?;
        Ok(())
    }

    fn get_patch_data(
        &self,
        refs: Vec<(&PatchVersion, &PatchRef)>,
    ) -> impl Stream<Item = Result<((Arc<PatchVersion>, PatchRef), Bytes)>> {
        let refs_by_version = refs.into_iter().fold(
            HashMap::new(),
            |mut acc: HashMap<&_, Vec<&_>>, (version, patch_ref)| {
                acc.entry(version).or_default().push(patch_ref);
                acc
            },
        );

        let stream_by_version = refs_by_version
            .into_iter()
            .map(|(version, patches)| self.get_patch_data_for_version(version, patches));

        stream::select_all(stream_by_version)
    }

    fn get_patch_data_for_version(
        &self,
        version: &PatchVersion,
        mut patches: Vec<&PatchRef>,
    ) -> impl Stream<Item = Result<((Arc<PatchVersion>, PatchRef), Bytes)>> {
        log::info!("Partially downloading {version}");

        patches.sort_by_key(|patch| patch.offset);
        let mut merged_ranges: Vec<MergedRange> = vec![];
        for patch_ref in patches {
            if let Some(last) = merged_ranges.last_mut() {
                if last.try_add(patch_ref) {
                    continue;
                }
            }
            merged_ranges.push(MergedRange::new(patch_ref.clone()));
        }

        let mut range_batches: Vec<RangeBatch> = vec![];
        for range in merged_ranges {
            if let Some(last) = range_batches.last_mut() {
                if last.try_add(range.clone()) {
                    continue;
                }
            }
            range_batches.push(RangeBatch(vec![range]));
        }

        let version = Arc::new(version.clone());
        let batch_streams = range_batches
            .into_iter()
            .map(|batch| {
                let version = version.clone();
                async move {
                    let url = format!(
                        "{}/{}.patch",
                        self.diff.base_patch_url.trim_end_matches('/'),
                        version
                    );
                    let _permit = self.semaphore.acquire().await?;
                    log::info!(
                        "Downloading {} ({:.2} MiB; {} ranges)",
                        url,
                        batch.0.iter().map(|r| r.size).sum::<u64>() as f64 / (1 << 20) as f64,
                        batch.0.len()
                    );
                    let response = self
                        .client
                        .get(url)
                        .header(header::RANGE, batch.to_range_header())
                        .send()
                        .await?
                        .error_for_status()?;
                    let header = response
                        .headers()
                        .get(header::CONTENT_TYPE)
                        .ok_or_else(|| {
                            anyhow::anyhow!("Missing Content-Type header in response")
                        })?;
                    let range_stream = match parse_boundary(header.to_str()?) {
                        Ok(boundary) => {
                            let reader = StreamReader::new(
                                response.bytes_stream().map_err(std::io::Error::other),
                            );
                            let multipart = Multipart::with_reader(reader, boundary);
                            let stream = try_unfold(multipart, |mut multipart| async move {
                                let field = match multipart.next_field().await? {
                                    Some(field) => field,
                                    None => return Ok(None),
                                };
                                let content_range = Self::get_content_range_bytes(field.headers())?;
                                let bytes = field.bytes().await?;
                                Ok(Some(((content_range, bytes), multipart)))
                            });
                            stream.boxed_local()
                        }
                        Err(multer::Error::NoMultipart) => stream::once(async move {
                            let content_range = Self::get_content_range_bytes(response.headers())?;
                            let bytes = response.bytes().await?;
                            Ok((content_range, bytes))
                        })
                        .boxed_local(),
                        Err(e) => {
                            bail!(e)
                        }
                    };
                    let batch_ref = Arc::new(batch);
                    let batch_stream = range_stream.and_then(move |(range, bytes)| {
                        let batch = batch_ref.clone();
                        let version = version.clone();
                        async move {
                            let range = batch
                                .0
                                .iter()
                                .find(|r| {
                                    r.offset == range.first_byte && r.end() == range.last_byte
                                })
                                .cloned()
                                .ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "No matching range found for {}-{} in batch",
                                        range.first_byte,
                                        range.last_byte
                                    )
                                })?;
                            Ok(((version, range), bytes))
                        }
                    });
                    Ok(batch_stream)
                }
            })
            .map(|fut| fut.try_flatten_stream().boxed_local());

        let range_streams =
            stream::select_all(batch_streams).map_ok(|((version, range), bytes)| {
                stream::iter(range.parts.into_iter().map(move |patch_ref| {
                    let relative_start = patch_ref
                        .offset
                        .checked_sub(range.offset)
                        .expect("PatchRef offset should always be within range");
                    let relative_end = relative_start + patch_ref.size as u64;
                    Ok((
                        (version.clone(), patch_ref.clone()),
                        bytes.slice(relative_start as usize..relative_end as usize),
                    ))
                }))
                .and_then(|((version, patch_ref), bytes)| {
                    if patch_ref.is_compressed {
                        async move {
                            let decompressed = Self::decompress_patch_data(&bytes)?;
                            Ok(((version, patch_ref), Bytes::from(decompressed)))
                        }
                        .boxed_local()
                    } else {
                        future::ok(((version, patch_ref), bytes)).boxed_local()
                    }
                })
            });

        range_streams.try_flatten_unordered(None)
    }

    fn get_content_range_bytes(headers: &HeaderMap) -> Result<ContentRangeBytes> {
        let content_range = headers
            .get(header::CONTENT_RANGE)
            .ok_or_else(|| anyhow::anyhow!("Missing Content-Range header in response"))
            .and_then(|v| {
                ContentRange::parse_bytes(v.as_bytes()).ok_or_else(|| {
                    anyhow::anyhow!("Missing/Invalid Content-Range header in multipart field")
                })
            })?;

        let content_range = match content_range {
            ContentRange::Bytes(range) => range,
            range => {
                return Err(anyhow::anyhow!(
                    "Expected byte range in multipart field, got {range:?}",
                ));
            }
        };
        Ok(content_range)
    }

    fn get_data_ref_name(&self, data_ref: &DataReference) -> &str {
        &self.file_names[data_ref.file_name_index]
    }

    /// Get data reference by indices
    fn get_data_ref(&self, data_ref: &DataReference) -> &DataRef {
        &self.diff.added_files[self.get_data_ref_name(data_ref)][data_ref.data_ref_index]
    }

    /// Get patch reference by indices
    fn get_patch_ref(&self, patch_ref: &PatchReference) -> (&PatchVersion, &PatchRef) {
        let data_ref = self.get_data_ref(&patch_ref.0);
        (data_ref.applied_version(), data_ref.patch().unwrap())
    }

    /// Decompress patch data using raw DEFLATE
    fn decompress_patch_data(compressed_data: &[u8]) -> Result<Vec<u8>> {
        let mut decoder = DeflateDecoder::new_with_buf(compressed_data, vec![0; 1 << 14]);
        let mut decompressed = Vec::new();

        decoder
            .read_to_end(&mut decompressed)
            .context("Failed to decompress patch data")?;

        Ok(decompressed)
    }
}
