use async_trait::async_trait;
use std::io::Result;

use crate::ops::create_empty_file_block;

/// Represents a target file that can be written to
#[async_trait]
pub trait TargetFile: Send + Sync {
    async fn write_at(&self, data: &[u8], offset: u64) -> Result<()>;
}

/// Main file operations trait - equivalent to C# ZiPatchConfig
#[async_trait]
pub trait FileOperations: Send + Sync {
    type File: TargetFile;

    async fn open_file(&self, path: &str) -> Result<Self::File>;
    async fn create_directory(&self, path: &str) -> Result<()>;
    async fn delete_file(&self, path: &str) -> Result<()>;
    async fn delete_directory(&self, path: &str) -> Result<()>;
}

#[async_trait]
pub trait TargetFileExt: TargetFile {
    async fn wipe(&self, offset: u64, length: u32) -> Result<()>;
    async fn write_empty_file_block(&self, block_count: i32, offset: u64) -> Result<()>;
}

#[async_trait]
impl<T: TargetFile> TargetFileExt for T {
    async fn wipe(&self, offset: u64, length: u32) -> Result<()> {
        let empty_block = vec![0; length as usize];
        self.write_at(&empty_block, offset).await?;
        Ok(())
    }

    async fn write_empty_file_block(&self, block_count: i32, offset: u64) -> Result<()> {
        let empty_block = create_empty_file_block(block_count.into());
        self.write_at(&empty_block, offset).await?;
        Ok(())
    }
}
