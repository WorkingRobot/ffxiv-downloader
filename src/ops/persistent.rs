use std::{
    collections::{HashMap, hash_map::Entry},
    fs::OpenOptions,
    io::{ErrorKind, Result},
    path::{Path, PathBuf},
    sync::Arc,
};

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::ops::{
    EXPAC_FOLDERS,
    file_ops::{FileOperations, TargetFile},
    get_expansion_folder,
    io::{OpenOptionsExt, PositionedFile},
};

pub struct PersistentFileOperations {
    game_path: PathBuf,
    open_files: Mutex<HashMap<PathBuf, PersistentFile>>,
}

#[async_trait]
impl FileOperations for PersistentFileOperations {
    type File = PersistentFile;

    async fn open_file(&self, path: &str) -> Result<Self::File> {
        let mut files = self.open_files.lock().await;

        let path = self.get_full_path(path);
        let entry = files.entry(path.clone());
        match entry {
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(entry) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                // If the file is not already open, we create a new PersistentFile
                let file = PersistentFile::new(self.get_full_path(path))?;
                Ok(entry.insert(file).clone())
            }
        }
    }

    async fn create_directory(&self, path: &str) -> Result<()> {
        let path = self.get_full_path(path);
        std::fs::create_dir_all(&path)
    }

    async fn delete_file(&self, path: &str) -> Result<()> {
        let mut files = self.open_files.lock().await;
        let path = self.get_full_path(path);
        files.remove(&path);
        match std::fs::remove_file(path) {
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    async fn delete_directory(&self, path: &str) -> Result<()> {
        let full_path = self.get_full_path(path);
        std::fs::remove_dir(full_path)
    }

    async fn delete_expansion(
        &self,
        expansion_id: u16,
        mut should_keep: impl (FnMut(String) -> bool) + Send,
    ) -> Result<()> {
        let mut files = self.open_files.lock().await;

        let expansion_folder = get_expansion_folder(expansion_id);

        for dir_name in EXPAC_FOLDERS {
            let dir = self.get_full_path(format!("{dir_name}/{expansion_folder}"));
            if dir.exists() {
                for file in dir.read_dir()? {
                    let file = file?;
                    if file.file_type()?.is_file() {
                        let path = file.path();
                        if !(should_keep)(path.to_string_lossy().to_string()) {
                            files.remove(&path);
                            std::fs::remove_file(path)?;
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

impl PersistentFileOperations {
    pub fn new(game_path: impl AsRef<Path>) -> Self {
        let game_path = game_path.as_ref().to_path_buf();
        Self {
            game_path,
            open_files: Mutex::new(HashMap::new()),
        }
    }

    #[inline]
    fn get_full_path(&self, path: impl AsRef<Path>) -> PathBuf {
        self.game_path.join(path)
    }
}

#[derive(Clone)]
#[repr(transparent)]
pub struct PersistentFile(Arc<PositionedFile>);

#[async_trait]
impl TargetFile for PersistentFile {
    async fn write_at(&self, data: &[u8], offset: u64) -> Result<()> {
        self.0.pwrite_all(offset as usize, data).await?;
        Ok(())
    }

    async fn truncate(&self) -> Result<()> {
        self.0.truncate(0).await?;
        Ok(())
    }
}

impl PersistentFile {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .open_positioned(path)?;
        Ok(Self(Arc::new(file)))
    }
}
