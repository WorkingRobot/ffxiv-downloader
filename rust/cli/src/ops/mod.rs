use std::io::{Cursor, Write};

mod file_ops;
mod filtered;
mod io;
mod persistent;

pub use file_ops::{FileOperations, TargetFile, TargetFileExt};
pub use filtered::FilteredFileOperations;
pub use persistent::PersistentFileOperations;

fn create_empty_file_block(block_count: i64) -> Vec<u8> {
    let mut ret = vec![0; 24];
    let mut cursor = Cursor::new(&mut ret);

    // FileBlockHeader - the 0 writes are technically unnecessary but are in for illustrative purposes
    // Block size
    cursor.write_all(&(1u32 << 7).to_le_bytes()).unwrap();
    // ????
    cursor.write_all(&0u32.to_le_bytes()).unwrap();
    // File size
    cursor.write_all(&0u64.to_le_bytes()).unwrap();
    // Total number of blocks?
    cursor.write_all(&(block_count - 1).to_le_bytes()).unwrap();
    // Used number of blocks?
    cursor.write_all(&0u32.to_le_bytes()).unwrap();

    ret
}
