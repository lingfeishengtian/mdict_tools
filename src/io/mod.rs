use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Minimal byte-source abstraction used by the new API.
pub trait ByteSource {
    /// Read `buf.len()` bytes starting at `offset` into `buf`.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()>;
}

/// Filesystem-backed byte source.
pub struct FsSource {
    file: File,
}

impl FsSource {
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(FsSource { file })
    }
}

impl ByteSource for FsSource {
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> io::Result<()> {
        self.file.seek(SeekFrom::Start(offset))?;
        self.file.read_exact(buf)
    }
}
