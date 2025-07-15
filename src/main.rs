mod binary;
mod clut_data_ref;
mod clut_file;
mod clut_file_data;
mod clut_header;
mod clut_patch_ref;
mod types;

use clap::{Arg, Command};
use std::fs;
use std::io::Cursor;
use std::path::Path;

use crate::clut_file::ClutFile;

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
    let _clut_file = ClutFile::read(Cursor::new(data))?;
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let matches = Command::new("xiv-dl")
        .version("0.1.0")
        .about("FFXIV CLUT file reader and tester")
        .arg(
            Arg::new("directory")
                .short('d')
                .long("directory")
                .value_name("DIR")
                .help("Directory to recursively search for CLUT files")
                .default_value("/home/asriel/Downloads/cluts/"),
        )
        .get_matches();

    let directory = matches.get_one::<String>("directory").unwrap();
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
