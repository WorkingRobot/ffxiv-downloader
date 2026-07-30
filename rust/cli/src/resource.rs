use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures::TryStreamExt;
use reqwest::Client;
use tokio::io::AsyncRead;
use tokio_util::io::StreamReader;

/// Fetches patches, LUTs and CLUTs, from a local path or over HTTP.
pub struct Fetcher {
    client: Client,
    /// Checked first for `<version>.<extension>`, so a locally held copy is used
    /// instead of downloading.
    override_path: Option<PathBuf>,
}

impl Fetcher {
    pub fn new(override_path: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            client: Client::builder()
                .user_agent("FFXIV PATCH CLIENT")
                .build()
                .context("Failed to create HTTP client")?,
            override_path,
        })
    }

    pub async fn bytes(&self, location: &str, version: &str, extension: &str) -> Result<Vec<u8>> {
        if let Some(path) = self.local(location, version, extension) {
            return tokio::fs::read(&path)
                .await
                .with_context(|| format!("reading {}", path.display()));
        }
        Ok(self
            .client
            .get(location)
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("fetching {location}"))?
            .bytes()
            .await?
            .to_vec())
    }

    /// A patch is up to a gigabyte and a half, so it is read as it arrives.
    pub async fn stream(
        &self,
        location: &str,
        version: &str,
        extension: &str,
    ) -> Result<Box<dyn AsyncRead + Send + Unpin>> {
        if let Some(path) = self.local(location, version, extension) {
            let file = tokio::fs::File::open(&path)
                .await
                .with_context(|| format!("opening {}", path.display()))?;
            return Ok(Box::new(file));
        }

        let response = self
            .client
            .get(location)
            .send()
            .await?
            .error_for_status()
            .with_context(|| format!("fetching {location}"))?;
        Ok(Box::new(StreamReader::new(
            response.bytes_stream().map_err(std::io::Error::other),
        )))
    }

    fn local(&self, location: &str, version: &str, extension: &str) -> Option<PathBuf> {
        if let Some(base) = &self.override_path {
            let path = base.join(format!("{version}.{extension}"));
            if path.exists() {
                log::info!("Using override for {version}");
                return Some(path);
            }
        }
        let path = Path::new(location);
        path.exists().then(|| path.to_path_buf())
    }
}

/// Join a base with a name, whether the base is a directory or a URL prefix.
pub fn join(base: Option<&str>, name: &str) -> String {
    match base {
        Some(base) if !base.is_empty() => format!("{}/{name}", base.trim_end_matches('/')),
        _ => name.to_string(),
    }
}
