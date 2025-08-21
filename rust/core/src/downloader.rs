use crate::file::patch_ref::PatchRef;
use crate::file::version::PatchVersion;
use anyhow::{Context, Result, bail};
use bytes::Bytes;
use flate2::read::DeflateDecoder;
use futures::{
    FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt, future,
    stream::{self, try_unfold},
};
use http_content_range::{ContentRange, ContentRangeBytes};
use multer::{Multipart, parse_boundary};
use reqwest::{
    Client,
    header::{self, HeaderMap},
};
use reqwest_leaky_bucket::leaky_bucket::RateLimiter;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{Jitter, RetryTransientMiddleware, policies::ExponentialBackoff};
use std::{collections::HashMap, sync::Arc, time::Duration};
use std::{fmt, io::Read};
use tokio::sync::Semaphore;
use tokio_util::io::StreamReader;

// Akamai restricts the range header size to at most 1034 bytes from my testing,
// but it doesn't work sometimes, so use a smaller number
const MAX_RANGE_HEADER_SIZE: usize = 1 << 12;
const MIN_RANGE_DISTANCE: u64 = 1 << 9;

/// Reference to a `DataRef` by its indices
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DataReference {
    pub file_name_index: usize,
    pub data_ref_index: usize,
}

/// Reference to a `PatchRef` by its indices
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PatchReference(DataReference);

/// Represents merged/consolidated HTTP ranges for efficient downloading
/// Matches the C# `MergedRange` class functionality
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
    /// Implements the C# `MergedRange.Add()` logic
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

#[derive(Debug)]
pub struct Downloader {
    client: ClientWithMiddleware,
    semaphore: Semaphore,
}

impl Downloader {
    pub fn new(max_concurrent_downloads: usize) -> Result<Self> {
        let client = Client::builder()
            .pool_max_idle_per_host(max_concurrent_downloads)
            .read_timeout(std::time::Duration::from_secs(5))
            .danger_accept_invalid_hostnames(true)
            .user_agent("FFXIV PATCH CLIENT")
            .build()
            .context("Failed to create HTTP client")?;

        let limiter = RateLimiter::builder()
            .max(32)
            .initial(64)
            .refill(16)
            .interval(Duration::from_millis(500))
            .build();

        let retry_policy = ExponentialBackoff::builder()
            .retry_bounds(Duration::from_secs(1), Duration::from_secs(15))
            .jitter(Jitter::Bounded)
            .base(2)
            .build_with_total_retry_duration(Duration::from_secs(30));

        let client = ClientBuilder::new(client)
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .with(reqwest_leaky_bucket::rate_limit_all(limiter))
            .build();

        Ok(Self {
            client,
            semaphore: Semaphore::new(max_concurrent_downloads),
        })
    }

    pub fn get_patch_data<'a>(
        &self,
        base_patch_url: &str,
        refs: impl Iterator<Item = (&'a PatchVersion, &'a PatchRef)>,
    ) -> impl Stream<Item = Result<((Arc<PatchVersion>, PatchRef), Bytes)>> + Send {
        let refs_by_version = refs.into_iter().fold(
            HashMap::new(),
            |mut acc: HashMap<&_, Vec<&_>>, (version, patch_ref)| {
                acc.entry(version).or_default().push(patch_ref);
                acc
            },
        );

        let base_patch_url = base_patch_url.trim_end_matches('/');

        let stream_by_version = refs_by_version.into_iter().map(|(version, patches)| {
            self.get_patch_data_for_version(base_patch_url, version, patches)
        });

        stream::select_all(stream_by_version)
    }

    fn get_patch_data_for_version(
        &self,
        base_patch_url: &str,
        version: &PatchVersion,
        mut patches: Vec<&PatchRef>,
    ) -> impl Stream<Item = Result<((Arc<PatchVersion>, PatchRef), Bytes)>> + Send {
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
                let url = format!("{base_patch_url}/{version}.patch");
                async move {
                    let permit = self.semaphore.acquire().await?;
                    log::debug!(
                        "Downloading {} ({:.2} MiB; {} ranges) {}",
                        url,
                        batch.0.iter().map(|r| r.size).sum::<u64>() as f64 / (1 << 20) as f64,
                        batch.0.len(),
                        self.semaphore.available_permits()
                    );
                    let t = std::time::Instant::now();
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
                                let Some(field) = multipart.next_field().await? else {
                                    return Ok(None);
                                };
                                let content_range = Self::get_content_range_bytes(field.headers())?;
                                let bytes = field.bytes().await?;
                                Ok(Some(((content_range, bytes), multipart)))
                            });
                            drop(permit);
                            stream.boxed()
                        }
                        Err(multer::Error::NoMultipart) => stream::once(async move {
                            let content_range = Self::get_content_range_bytes(response.headers())?;
                            let rcv = t.elapsed();
                            let bytes = response.bytes().await?;
                            let e = t.elapsed();
                            log::debug!(
                                "Downloaded {:.2} MiB in {:.2}ms (bytes in {:.2}ms)",
                                bytes.len() as f64 / (1 << 20) as f64,
                                e.as_secs_f32() * 1000.0,
                                (e - rcv).as_secs_f32() * 1000.0
                            );
                            drop(permit);
                            Ok((content_range, bytes))
                        })
                        .boxed(),
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
            .map(|fut| fut.try_flatten_stream().boxed());

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
                        .boxed()
                    } else {
                        future::ok(((version, patch_ref), bytes)).boxed()
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
