use crate::file::patch_ref::PatchRef;
use crate::file::version::PatchVersion;
use anyhow::{Context, Result};
use bytes::Bytes;
use flate2::read::DeflateDecoder;
use futures::{FutureExt, Stream, StreamExt, TryFutureExt, TryStreamExt, future, stream};
use http_content_range::{ContentRange, ContentRangeBytes};
use multer::Multipart;
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

const MAX_RANGE_HEADER_SIZE: usize = 8800;
const MAX_RANGES_PER_REQUEST: usize = 400;
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
    ///
    /// Returns true if merged, false if the reference starts too far past the end to be worth
    /// pulling the bytes in between. Callers add references in ascending offset order, so the
    /// comparison is against this range's end: testing the other direction can never be true for
    /// sorted input, which silently merged every reference in a patch into one range spanning the
    /// whole file.
    pub fn try_add(&mut self, patch_ref: &PatchRef) -> bool {
        if patch_ref.offset > self.offset + self.size + MIN_RANGE_DISTANCE {
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
    /// Returns true if added successfully, false if it would exceed the range or header limits
    pub fn try_add(&mut self, range: MergedRange) -> bool {
        if self.0.len() >= MAX_RANGES_PER_REQUEST {
            return false;
        }
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
            if let Some(last) = merged_ranges.last_mut()
                && last.try_add(patch_ref)
            {
                continue;
            }
            merged_ranges.push(MergedRange::new(patch_ref.clone()));
        }

        let mut range_batches: Vec<RangeBatch> = vec![];
        for range in merged_ranges {
            if let Some(last) = range_batches.last_mut()
                && last.try_add(range.clone())
            {
                continue;
            }
            range_batches.push(RangeBatch(vec![range]));
        }

        let version = Arc::new(version.clone());
        let batch_streams = range_batches.into_iter().map(|batch| {
            let version = version.clone();
            let url = format!("{base_patch_url}/{version}.patch");
            async move {
                let fetched = self.fetch_ranges(&url, &batch.0).await?;
                Ok::<_, anyhow::Error>(stream::iter(
                    fetched
                        .into_iter()
                        .map(move |(range, bytes)| Ok(((version.clone(), range), bytes))),
                ))
            }
            .try_flatten_stream()
            .boxed()
        });

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

    async fn fetch_ranges(
        &self,
        url: &str,
        ranges: &[MergedRange],
    ) -> Result<Vec<(MergedRange, Bytes)>> {
        let parts = self.request_ranges(url, ranges).await?;
        Self::pair_with_ranges(ranges, parts)
    }

    async fn request_ranges(
        &self,
        url: &str,
        ranges: &[MergedRange],
    ) -> Result<Vec<(ContentRangeBytes, Bytes)>> {
        let permit = self.semaphore.acquire().await?;
        let header = RangeBatch(ranges.to_vec()).to_range_header();
        log::debug!(
            "Downloading {} ({:.2} MiB; {} ranges) {}",
            url,
            ranges.iter().map(|r| r.size).sum::<u64>() as f64 / (1 << 20) as f64,
            ranges.len(),
            self.semaphore.available_permits()
        );

        let started = std::time::Instant::now();
        let response = self
            .client
            .get(url)
            .header(header::RANGE, header)
            .send()
            .await?
            .error_for_status()?;

        let boundary = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(Self::byteranges_boundary);

        if let Some(boundary) = boundary {
            let reader = StreamReader::new(response.bytes_stream().map_err(std::io::Error::other));
            let mut multipart = Multipart::with_reader(reader, boundary);
            let mut parts = Vec::with_capacity(ranges.len());
            while let Some(field) = multipart.next_field().await? {
                let content_range = Self::get_content_range_bytes(field.headers())?;
                parts.push((content_range, field.bytes().await?));
            }
            drop(permit);
            return Ok(parts);
        }

        let content_range = Self::get_content_range_bytes(response.headers())?;
        let bytes = response.bytes().await?;
        log::debug!(
            "Downloaded {:.2} MiB in {:.2}ms",
            bytes.len() as f64 / (1 << 20) as f64,
            started.elapsed().as_secs_f32() * 1000.0
        );
        drop(permit);
        Ok(vec![(content_range, bytes)])
    }

    fn byteranges_boundary(content_type: &str) -> Option<String> {
        let (media_type, parameters) = content_type.split_once(';')?;
        if !media_type
            .trim()
            .eq_ignore_ascii_case("multipart/byteranges")
        {
            return None;
        }
        parameters.split(';').find_map(|parameter| {
            let (key, value) = parameter.split_once('=')?;
            key.trim()
                .eq_ignore_ascii_case("boundary")
                .then(|| value.trim().trim_matches('"').to_owned())
        })
    }

    fn pair_with_ranges(
        ranges: &[MergedRange],
        parts: Vec<(ContentRangeBytes, Bytes)>,
    ) -> Result<Vec<(MergedRange, Bytes)>> {
        parts
            .into_iter()
            .map(|(content_range, bytes)| {
                ranges
                    .iter()
                    .find(|r| {
                        r.offset == content_range.first_byte && r.end() == content_range.last_byte
                    })
                    .cloned()
                    .map(|range| (range, bytes))
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "No matching range found for {}-{} in batch",
                            content_range.first_byte,
                            content_range.last_byte
                        )
                    })
            })
            .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    fn patch_ref(offset: u64, size: u32) -> PatchRef {
        PatchRef {
            offset,
            size,
            is_compressed: false,
        }
    }

    #[test]
    fn merges_adjacent_and_nearby_references() {
        let mut range = MergedRange::new(patch_ref(1000, 100));
        assert!(range.try_add(&patch_ref(1100, 100)), "contiguous");
        assert!(
            range.try_add(&patch_ref(1200 + MIN_RANGE_DISTANCE, 50)),
            "within the merge distance"
        );
        assert_eq!(range.offset, 1000);
        assert_eq!(range.size, 250 + MIN_RANGE_DISTANCE);
        assert_eq!(range.parts.len(), 3);
    }

    #[test]
    fn refuses_a_reference_past_the_merge_distance() {
        let mut range = MergedRange::new(patch_ref(0, 1024));
        assert!(!range.try_add(&patch_ref(1024 + MIN_RANGE_DISTANCE + 1, 16)));
        assert_eq!(
            range.size, 1024,
            "a refused reference must not grow the range"
        );
        assert_eq!(range.parts.len(), 1);
    }

    #[test]
    fn reads_the_boundary_out_of_a_byteranges_content_type() {
        assert_eq!(
            Downloader::byteranges_boundary("multipart/byteranges; boundary=04257D9608554D01"),
            Some("04257D9608554D01".to_owned())
        );
        assert_eq!(
            Downloader::byteranges_boundary("multipart/byteranges;boundary=\"quoted\""),
            Some("quoted".to_owned())
        );
        // A single-range reply must not be mistaken for a multipart one.
        assert_eq!(Downloader::byteranges_boundary("text/plain"), None);
        assert_eq!(
            Downloader::byteranges_boundary("multipart/form-data; boundary=x"),
            None
        );
    }

    #[test]
    fn batches_many_ranges_within_the_documented_limits() {
        let mut batch = RangeBatch(vec![]);
        let mut added = 0;
        for i in 0..10_000u64 {
            if !batch.try_add(MergedRange::new(patch_ref(i * 1_000_000, 16))) {
                break;
            }
            added += 1;
        }
        assert!(added > 1, "multi-range requests are the point of batching");
        assert!(added <= MAX_RANGES_PER_REQUEST);
        assert!(batch.to_range_header().len() <= MAX_RANGE_HEADER_SIZE);
        assert!(batch.to_range_header().starts_with("bytes="));
    }
}
