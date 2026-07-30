mod cache;
mod clut;
mod diff;
mod download;
mod lut;
mod ops;
mod patcher;
mod resource;

use clap::{Parser, Subcommand, ValueEnum};
use shadow_rs::shadow;
use std::sync::Arc;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};
use std::{io::Cursor, path::PathBuf};
use xiv_core::file::clut_lazy::LazyClut;
use xiv_core::file::types::CompressType;

use crate::download::{DownloadCommand, DownloadConfigArgs};
use crate::resource::Fetcher;

shadow!(build);

#[derive(Parser)]
#[command(name = build::PROJECT_NAME)]
#[command(version = build::CLAP_LONG_VERSION)]
#[command(about = build::PKG_DESCRIPTION)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Is this a CI run?
    #[arg(long, hide = true, default_value_t = false)]
    gha: bool,

    /// Enable verbose logging.
    #[arg(long, global = true)]
    verbose: bool,

    /// Enable debug logging. Implies verbose logging.
    #[arg(long, global = true)]
    debug: bool,

    /// Directory to read patch, LUT and CLUT files from instead of downloading them.
    #[arg(long, global = true, value_name = "DIR")]
    patch_override_path: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Test CLUT file parsing
    TestClut {
        /// Directory to recursively search for CLUT files
        #[arg(short = 'd', long, value_name = "DIR", default_value = ".")]
        directory: String,
    },
    /// Download FFXIV files using CLUT patches
    Download {
        #[command(flatten)]
        config_args: DownloadConfigArgs,
    },
    /// Build a LUT for each patch in a repository's chain
    Lut(lut::LutArgs),
    /// Fold a chain of LUTs into a CLUT per game version
    Clut(clut::ClutArgs),
    /// Print a repository's version graph in the DOT language
    Graphviz {
        /// Repository slug to graph
        #[arg(short, long, value_name = "SLUG")]
        slug: String,
        /// Check that every patch is actually downloadable
        #[arg(long)]
        verify_existence: bool,
        /// Only show versions that are still offered
        #[arg(long, value_name = "BOOL", default_value_t = true, num_args = 0..=1)]
        active: bool,
    },
}

/// Compression for a LUT or CLUT payload. Named as the C# implementation named them,
/// since existing invocations pass `-c Brotli`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "PascalCase")]
enum Compression {
    None,
    Zlib,
    Brotli,
    /// Smaller and much faster to decode, but only defined for CLUT version 3.
    Zstd,
}

impl From<Compression> for CompressType {
    fn from(value: Compression) -> Self {
        match value {
            Compression::None => CompressType::None,
            Compression::Zlib => CompressType::Zlib,
            Compression::Brotli => CompressType::Brotli,
            Compression::Zstd => CompressType::Zstd,
        }
    }
}

impl std::fmt::Display for Compression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

fn find_clut_files<P: AsRef<Path>>(dir: P) -> std::io::Result<Vec<PathBuf>> {
    fn visit_dir(dir: &Path, clut_files: &mut Vec<PathBuf>) -> std::io::Result<()> {
        if dir.is_dir() {
            for entry in fs::read_dir(dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.is_dir() {
                    visit_dir(&path, clut_files)?;
                } else if path.extension().and_then(|s| s.to_str()) == Some("clut") {
                    clut_files.push(path);
                }
            }
        }
        Ok(())
    }

    let mut clut_files = Vec::new();
    visit_dir(dir.as_ref(), &mut clut_files)?;
    clut_files.sort();
    Ok(clut_files)
}

fn test_clut_file<P: AsRef<Path>>(file_path: P) -> anyhow::Result<()> {
    let path = file_path.as_ref();
    let data = fs::read(path)?;
    let _clut_file = LazyClut::read(Cursor::new(data))?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    let level = if cli.debug {
        "trace"
    } else if cli.verbose || cfg!(debug_assertions) {
        "debug"
    } else {
        "info"
    };
    env_logger::Builder::from_env(env_logger::Env::new().default_filter_or(level)).init();
    if cli.gha {
        log::info!("Running in CI/CD mode. o/");
    }

    match cli.command {
        Commands::TestClut { directory } => test_clut_files(&directory),
        Commands::Download { config_args } => {
            let mut download_cmd = DownloadCommand::new(config_args.into())?;
            let (version, updated) = download_cmd.run().await?;
            if cli.gha
                && let Some(outputs_path) = std::env::var_os("GITHUB_OUTPUT")
            {
                OpenOptions::new()
                    .write(true)
                    .open(Path::new(&outputs_path))?
                    .write_all(format!("version={version}\nupdated={updated}\n").as_bytes())?;
            }
            Ok(())
        }
        Commands::Lut(args) => {
            let fetcher = Arc::new(Fetcher::new(cli.patch_override_path)?);
            lut::run(args, fetcher, &thaliak_client()?).await
        }
        Commands::Clut(args) => {
            let fetcher = Arc::new(Fetcher::new(cli.patch_override_path)?);
            clut::run(args, fetcher, &thaliak_client()?).await
        }
        Commands::Graphviz {
            slug,
            verify_existence,
            active,
        } => {
            let tree = xiv_core::thaliak::graphviz::get_graphviz_tree(
                &thaliak_client()?,
                &slug,
                verify_existence,
                active,
            )
            .await?;
            print!("{tree}");
            Ok(())
        }
    }
}

fn thaliak_client() -> anyhow::Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!("{}/{}", build::PROJECT_NAME, build::PKG_VERSION))
        .build()?)
}

fn test_clut_files(directory: &str) -> anyhow::Result<()> {
    println!("Searching for CLUT files in: {directory}");

    let clut_files = find_clut_files(directory)?;
    println!("Found {} CLUT files", clut_files.len());

    let mut successful = 0;
    let mut failed = 0;

    for (i, file_path) in clut_files.iter().enumerate() {
        print!(
            "Testing {}/{}: {} ... ",
            i + 1,
            clut_files.len(),
            file_path.display()
        );

        match test_clut_file(file_path) {
            Ok(()) => {
                println!("✓ OK");
                successful += 1;
            }
            Err(e) => {
                println!("✗ FAILED: {e}");
                failed += 1;
            }
        }
    }

    println!("\n=== Test Summary ===");
    println!("Total files: {}", clut_files.len());
    println!("Successful: {successful}");
    println!("Failed: {failed}");
    println!(
        "Success rate: {:.1}%",
        if !clut_files.is_empty() {
            (successful as f64 / clut_files.len() as f64) * 100.0
        } else {
            0.0
        }
    );

    Ok(())
}
