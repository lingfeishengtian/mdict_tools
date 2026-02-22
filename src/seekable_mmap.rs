use std::fs::File;
use std::io::{Read, Result as IoResult, Seek, SeekFrom};

use memmap2::Mmap;

/// A small wrapper around `memmap2::Mmap` that provides `Read` + `Seek` by
/// keeping an internal cursor. This is intended for single-threaded use; if
/// you need concurrent access wrap this type in `Mutex`/`RwLock` or similar.
#[derive(Debug)]
pub struct SeekableMmap {
    mmap: Mmap,
    pos: usize,
}

impl SeekableMmap {
    /// Map the given file and return a seekable handle.
    ///
    /// Safety: this uses `memmap2::Mmap::map` which is safe when the file is
    /// not concurrently modified in a way that invalidates the mapping.
    pub fn open(file: &File) -> IoResult<Self> {
        // SAFETY: memmap2::Mmap::map is safe here; caller must ensure file
        // lives long enough and isn't truncated concurrently in unsafe ways.
        let mmap = unsafe { Mmap::map(file)? };
        Ok(Self { mmap, pos: 0 })
    }

    /// Create from an existing `Mmap`.
    pub fn from_mmap(mmap: Mmap) -> Self {
        Self { mmap, pos: 0 }
    }

    /// Return the underlying bytes slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.mmap[..]
    }

    /// Return current position (in bytes).
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Return mapped length.
    pub fn len(&self) -> usize {
        self.mmap.len()
    }

    /// Is the cursor at or beyond EOF?
    pub fn is_eof(&self) -> bool {
        self.pos >= self.mmap.len()
    }
}

impl Read for SeekableMmap {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<usize> {
        if self.pos >= self.mmap.len() {
            return Ok(0);
        }
        let avail = self.mmap.len() - self.pos;
        let to_read = std::cmp::min(avail, buf.len());
        buf[..to_read].copy_from_slice(&self.mmap[self.pos..self.pos + to_read]);
        self.pos += to_read;
        Ok(to_read)
    }
}

impl Seek for SeekableMmap {
    fn seek(&mut self, how: SeekFrom) -> IoResult<u64> {
        let new = match how {
            SeekFrom::Start(off) => off as i128,
            SeekFrom::End(off) => (self.mmap.len() as i128) + (off as i128),
            SeekFrom::Current(off) => (self.pos as i128) + (off as i128),
        };

        if new < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid seek to a negative position",
            ));
        }

        // Clamp to usize::MAX if necessary, but avoid overflow.
        let new_usize = if new as u128 > usize::MAX as u128 {
            usize::MAX
        } else {
            new as usize
        };

        self.pos = new_usize;
        Ok(self.pos as u64)
    }
}
