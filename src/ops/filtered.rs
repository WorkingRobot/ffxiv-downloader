use std::collections::HashSet;
use std::io::Result;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::ops::file_ops::{FileOperations, TargetFile};

pub struct FilteredFileOperations<T: FileOperations> {
    inner: T,
    filter: Box<dyn (Fn(&str) -> bool) + Send + Sync>,
    filtered_files: Mutex<HashSet<String>>,
}

#[async_trait]
impl<T: FileOperations> FileOperations for FilteredFileOperations<T> {
    type File = FilteredFile<T>;

    async fn open_file(&self, path: &str) -> Result<Self::File> {
        let mut filtered_files = self.filtered_files.lock().await;
        if !(self.filter)(path) {
            filtered_files.insert(path.to_string());
            return Ok(FilteredFile::black_hole());
        }

        return Ok(FilteredFile::new(self.inner.open_file(path).await?));
    }

    async fn create_directory(&self, path: &str) -> Result<()> {
        self.inner.create_directory(path).await
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        let mut filtered_files = self.filtered_files.lock().await;
        if !(self.filter)(path) {
            filtered_files.remove(path);
            return Ok(());
        }

        self.inner.delete_file(path).await
    }

    async fn delete_directory(&self, path: &str) -> Result<()> {
        self.inner.delete_directory(path).await
    }
}

impl<T: FileOperations> FilteredFileOperations<T> {
    pub fn new(inner: T, filter: impl Fn(&str) -> bool + Send + Sync + 'static) -> Self {
        Self {
            inner,
            filter: Box::new(filter),
            filtered_files: Mutex::new(HashSet::new()),
        }
    }

    pub async fn filtered_files(&self) -> HashSet<String> {
        self.filtered_files.lock().await.clone()
    }
}

#[derive(Clone)]
#[repr(transparent)]
pub struct FilteredFile<T: FileOperations>(Option<T::File>);

#[async_trait]
impl<T: FileOperations> TargetFile for FilteredFile<T> {
    async fn write_at(&self, data: &[u8], offset: u64) -> Result<()> {
        match &self.0 {
            Some(file) => file.write_at(data, offset).await,
            None => Ok(()),
        }
    }

    async fn truncate(&self) -> Result<()> {
        match &self.0 {
            Some(file) => file.truncate().await,
            None => Ok(()),
        }
    }
}

impl<T: FileOperations> FilteredFile<T> {
    fn new(file: T::File) -> Self {
        Self(Some(file))
    }

    fn black_hole() -> Self {
        Self(None)
    }
}
