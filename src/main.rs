mod cache;
mod diff;
mod download;
mod file;
mod ops;
mod patcher;
mod thaliak;

use clap::{Parser, Subcommand};
use shadow_rs::shadow;
use std::io::Cursor;
use std::path::Path;
use std::{
    fs::{self, OpenOptions},
    io::Write,
};

use crate::download::{DownloadCommand, DownloadConfigArgs};
use crate::file::clut::Clut;

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
}

fn find_clut_files<P: AsRef<Path>>(dir: P) -> std::io::Result<Vec<std::path::PathBuf>> {
    let mut clut_files = Vec::new();

    fn visit_dir(dir: &Path, clut_files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
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

    visit_dir(dir.as_ref(), &mut clut_files)?;
    clut_files.sort();
    Ok(clut_files)
}

fn test_clut_file<P: AsRef<Path>>(file_path: P) -> anyhow::Result<()> {
    let path = file_path.as_ref();
    let data = fs::read(path)?;
    let _clut_file = Clut::read(Cursor::new(data))?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::new().default_filter_or(
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "info"
        },
    ))
    .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::TestClut { directory } => test_clut_files(&directory).await,
        Commands::Download { config_args } => {
            let mut download_cmd = DownloadCommand::new(config_args.into())?;
            let (version, updated) = download_cmd.run().await?;
            if cli.gha
                && let Some(outputs_path) = std::env::var_os("GITHUB_OUTPUT")
            {
                OpenOptions::new()
                    .write(true)
                    .open(Path::new(&outputs_path))?
                    .write_all(format!("version={}\nupdated={}\n", version, updated).as_bytes())?;
            }
            Ok(())
        }
    }
}

async fn test_clut_files(directory: &str) -> anyhow::Result<()> {
    println!("Searching for CLUT files in: {}", directory);

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
                println!("✗ FAILED: {}", e);
                failed += 1;
            }
        }
    }

    println!("\n=== Test Summary ===");
    println!("Total files: {}", clut_files.len());
    println!("Successful: {}", successful);
    println!("Failed: {}", failed);
    println!(
        "Success rate: {:.1}%",
        if clut_files.len() > 0 {
            (successful as f64 / clut_files.len() as f64) * 100.0
        } else {
            0.0
        }
    );

    Ok(())
}
