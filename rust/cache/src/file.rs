use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use bytes::Bytes;
use either::Either;
use futures::{
    StreamExt, TryStreamExt,
    stream::{self, PollNext},
};
use itertools::Itertools;
use xiv_core::{
    create_empty_file_block,
    file::{data_ref::DataRef, slug::Slug, version::GameVersion},
};

use crate::{server::Server, stream::CacheFileStream, weakling::Weakling};

#[derive(Debug, PartialEq, Eq, Hash)]
struct OffsetBuffer<'a> {
    file_offset: u64,
    buffer: &'a mut [u8],
}

impl<'a> OffsetBuffer<'a> {
    fn new(offset: u64, buffer: &'a mut [u8]) -> Self {
        Self {
            file_offset: offset,
            buffer,
        }
    }

    fn offset(&self) -> u64 {
        self.file_offset
    }

    fn len(&self) -> usize {
        self.buffer.len()
    }

    fn write(&mut self, offset: u64, data: &[u8]) {
        let file_start = offset.max(self.file_offset);
        let file_end = (offset + data.len() as u64).min(self.file_offset + self.len() as u64);

        let src_start = (file_start - offset) as usize;
        let src_end = (file_end - offset) as usize;

        let dst_start = (file_start - self.file_offset) as usize;
        let dst_end = (file_end - self.file_offset) as usize;

        if src_start < data.len()
            && src_end <= data.len()
            && dst_start < self.len()
            && dst_end <= self.len()
        {
            self.buffer[dst_start..dst_end].copy_from_slice(&data[src_start..src_end]);
        } else {
            panic!(
                "Buffer overflow: offset {} with length {} exceeds buffer size {}",
                file_start,
                data.len(),
                self.len()
            )
        }
    }

    fn wipe(&mut self, offset: u64, len: u32) {
        let file_start = offset.max(self.file_offset);
        let file_end = (offset + len as u64).min(self.file_offset + self.len() as u64);

        let dst_start = (file_start - self.file_offset) as usize;
        let dst_end = (file_end - self.file_offset) as usize;

        if dst_start < self.len() && dst_end <= self.len() {
            self.buffer[dst_start..dst_end].fill(0);
        } else {
            panic!(
                "Buffer overflow: offset {} with length {} exceeds buffer size {}",
                file_start,
                len,
                self.len()
            );
        }
    }

    fn write_empty_file_block(&mut self, offset: u64, block_count: i32) {
        let empty_block = create_empty_file_block(block_count.into());
        self.write(offset, &empty_block)
    }
}

#[derive(Debug, Clone)]
pub struct CacheFile {
    server: Server,
    slug: Slug,
    version: GameVersion,
    file_name: String,
    length: u64,
    file_data: Weakling<Vec<DataRef>>,
}

impl CacheFile {
    pub async fn new(
        server: Server,
        slug: Slug,
        version: GameVersion,
        file_name: String,
    ) -> std::io::Result<Self> {
        let file_data = Self::fetch_file_data(&server, slug, version.clone(), &file_name).await?;

        if !file_data.is_sorted_by_key(|f| f.offset()) {
            return Err(std::io::Error::other("File data is not sorted by offset"));
        }

        let length = file_data.last().map_or(0, |f| f.offset() + f.len() as u64);

        Ok(Self {
            server,
            slug,
            version,
            file_name,
            length,
            file_data: Arc::downgrade(&file_data).into(),
        })
    }

    async fn fetch_file_data(
        server: &Server,
        slug: Slug,
        version: GameVersion,
        file_path: &String,
    ) -> std::io::Result<Arc<Vec<DataRef>>> {
        let clut = server
            .get_clut(slug, version)
            .await
            .map_err(std::io::Error::other)?;
        let file_data =
            clut.files.get(file_path).cloned().ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "file not found")
            })?;
        Ok(file_data)
    }

    pub async fn exists(
        server: &Server,
        slug: Slug,
        version: GameVersion,
        file_path: String,
    ) -> Result<bool> {
        let clut = server.get_clut(slug, version).await?;
        Ok(clut.files.contains_key(&file_path))
    }

    pub async fn file_data(&self) -> Arc<Vec<DataRef>> {
        self.file_data
            .fetch(async || {
                Self::fetch_file_data(
                    &self.server,
                    self.slug,
                    self.version.clone(),
                    &self.file_name,
                )
                .await
                .expect("Failed to fetch file data")
            })
            .await
    }

    pub fn len(&self) -> u64 {
        self.length
    }

    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    pub fn into_reader(self) -> CacheFileStream {
        CacheFileStream::new(self)
    }

    async fn find_data_ref_idx(&self, offset: u64) -> Option<usize> {
        let file_data = self.file_data().await;
        let result = file_data.binary_search_by_key(&offset, |r| r.offset());
        match result {
            Ok(idx) => Some(idx),
            Err(idx)
                if idx > 0
                    && file_data
                        .get(idx - 1)
                        .map(|i| (i.offset() + i.len() as u64) > offset)
                        .unwrap_or_default() =>
            {
                Some(idx - 1)
            }
            _ => None,
        }
    }

    async fn find_data_ref_range(&self, offset: u64, len: u64) -> Option<(usize, usize)> {
        let start_idx = self.find_data_ref_idx(offset).await?;
        let end_idx = self.find_data_ref_idx(offset + len - 1).await?;
        if start_idx <= end_idx {
            Some((start_idx, end_idx + 1)) // end_idx is inclusive
        } else {
            None
        }
    }

    pub async fn pread(&self, offset: u64, buffer: &mut [u8]) -> std::io::Result<()> {
        let mut buffer = OffsetBuffer::new(offset, buffer);

        let (ref_start, ref_end) = self
            .find_data_ref_range(buffer.offset(), buffer.len() as u64)
            .await
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid offset or length")
            })?;

        let refs = &self.file_data().await[ref_start..ref_end];
        let mut patch_refs = HashMap::new();
        for data_ref in refs {
            if let Some(patch_ref) = data_ref.patch() {
                patch_refs
                    .entry((data_ref.applied_version(), patch_ref))
                    .or_insert_with(Vec::new)
                    .push(data_ref);
            }
        }

        let now = std::time::Instant::now();
        let download_stream = self
            .server
            .get_patch_data(
                self.slug,
                patch_refs.keys().copied().collect_vec().into_iter(),
            )
            .await
            .map_err(std::io::Error::other)?
            .map(|r| r.map(Either::Right));

        let plain_stream = stream::iter(
            refs.iter()
                .filter(|r| !r.is_patch())
                .map(|r| Ok(Either::Left(r))),
        );
        let full_stream =
            stream::select_with_strategy(plain_stream, download_stream, |()| PollNext::Right);

        let mut overhead = Duration::ZERO;
        let mut calls = 0;
        full_stream
            .try_fold(
                (&mut buffer, &mut patch_refs, &mut overhead, &mut calls),
                |(buffer_ref, patch_refs, d, c), operation| async {
                    let n = Instant::now();
                    match operation {
                        Either::Left(data_ref) => {
                            Self::process_op_plain(data_ref, buffer_ref);
                        }
                        Either::Right(((version, patch_ref), bytes)) => {
                            let refs =
                                patch_refs.remove(&(version, patch_ref)).ok_or_else(|| {
                                    anyhow::anyhow!(
                                        "No patch references found for version {:?} and patch {:?}",
                                        version,
                                        patch_ref
                                    )
                                })?;
                            assert!(!refs.is_empty(), "Patch references should not be empty");
                            for patch_ref in refs {
                                Self::process_op_patch(patch_ref, &bytes, buffer_ref);
                            }
                        }
                    }
                    *d += n.elapsed();
                    *c += 1;
                    Ok((buffer_ref, patch_refs, d, c))
                },
            )
            .await
            .map_err(std::io::Error::other)?;

        let elapsed = now.elapsed();
        log::trace!(
            "pread completed in {:.2}ms ({:.2}ms; {calls} calls): offset {}, len {} ({})",
            elapsed.as_secs_f32() * 1000.0,
            overhead.as_secs_f32() * 1000.0,
            buffer.offset(),
            buffer.len(),
            self.file_name
        );

        Ok(())
    }

    fn process_op_plain(op: &DataRef, buffer: &mut OffsetBuffer<'_>) {
        assert!(!op.is_patch(), "Expected plain DataRef, got patch");
        if op.is_zero() {
            buffer.wipe(op.offset(), op.len());
        } else if op.is_empty_block() {
            buffer.write_empty_file_block(op.offset(), op.block_count().unwrap());
        } else {
            unreachable!()
        }
    }

    fn process_op_patch(op: &DataRef, bytes: &Bytes, buffer: &mut OffsetBuffer<'_>) {
        assert!(op.is_patch(), "Expected patch DataRef, got plain");
        let patch_offset = op.patch_offset().unwrap() as usize;
        let data = bytes.slice(patch_offset..patch_offset + op.len() as usize);
        buffer.write(op.offset(), &data)
    }
}
