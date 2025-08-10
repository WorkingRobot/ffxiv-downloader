use std::{fs::OpenOptions, io, path::Path};

use crate::ops::io::PositionedFile;

pub trait OpenOptionsExt {
    fn open_positioned(&self, path: impl AsRef<Path>) -> io::Result<PositionedFile>;
}

impl OpenOptionsExt for OpenOptions {
    fn open_positioned(&self, path: impl AsRef<Path>) -> io::Result<PositionedFile> {
        PositionedFile::new(self, path)
    }
}
