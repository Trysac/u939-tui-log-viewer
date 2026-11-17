use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// Random-access byte source that can be queried for length and read at arbitrary offsets.
pub trait Source: Send {
    /// Returns the current total length of the source in bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if the underlying storage metadata cannot be queried.
    fn len(&self) -> io::Result<u64>;

    /// Returns `true` if the source has zero bytes.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if querying [`Source::len`] fails.
    fn is_empty(&self) -> io::Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Reads bytes starting at `offset` into `buf`, returning the number of bytes read.
    ///
    /// Reading at or past the end of the source returns `Ok(0)`.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if seeking or reading fails.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize>;
}

/// File-backed [`Source`] guarded by a [`Mutex`] for thread-safe random access.
#[derive(Debug)]
pub struct FileSource {
    file: Mutex<File>,
}

impl FileSource {
    /// Opens a file at `path` for random-access reading.
    ///
    /// # Errors
    ///
    /// Returns an [`io::Error`] if the file does not exist or cannot be opened.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(path)?;
        Ok(Self {
            file: Mutex::new(file),
        })
    }
}

impl Source for FileSource {
    fn len(&self) -> io::Result<u64> {
        let file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("file lock poisoned"))?;
        Ok(file.metadata()?.len())
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut file = self
            .file
            .lock()
            .map_err(|_| io::Error::other("file lock poisoned"))?;
        file.seek(SeekFrom::Start(offset))?;
        let mut total = 0;
        while total < buf.len() {
            match file.read(&mut buf[total..]) {
                Ok(0) => break,
                Ok(n) => total += n,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(total)
    }
}

/// In-memory [`Source`] backed by a shared byte buffer, supporting dynamic append and truncation.
#[derive(Debug, Clone, Default)]
pub struct MemSource {
    data: Arc<Mutex<Vec<u8>>>,
}

impl MemSource {
    /// Creates a new [`MemSource`] initialized with `bytes`.
    #[must_use]
    pub fn new(bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            data: Arc::new(Mutex::new(bytes.into())),
        }
    }

    /// Appends bytes to the end of the in-memory source.
    ///
    /// # Panics
    ///
    /// Panics only if the internal mutex has been poisoned by a panic in another thread.
    pub fn append(&self, bytes: &[u8]) {
        let mut guard = self.data.lock().expect("mem lock poisoned");
        guard.extend_from_slice(bytes);
    }

    /// Replaces the entire buffer with `bytes` (simulating truncation and rewrite).
    ///
    /// # Panics
    ///
    /// Panics only if the internal mutex has been poisoned by a panic in another thread.
    pub fn set(&self, bytes: &[u8]) {
        let mut guard = self.data.lock().expect("mem lock poisoned");
        guard.clear();
        guard.extend_from_slice(bytes);
    }

    /// Truncates the buffer to `new_len` bytes.
    ///
    /// # Panics
    ///
    /// Panics only if the internal mutex has been poisoned by a panic in another thread.
    pub fn truncate(&self, new_len: usize) {
        let mut guard = self.data.lock().expect("mem lock poisoned");
        guard.truncate(new_len);
    }
}

impl Source for MemSource {
    fn len(&self) -> io::Result<u64> {
        let guard = self
            .data
            .lock()
            .map_err(|_| io::Error::other("mem lock poisoned"))?;
        Ok(guard.len() as u64)
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
        let guard = self
            .data
            .lock()
            .map_err(|_| io::Error::other("mem lock poisoned"))?;
        let Ok(start) = usize::try_from(offset) else {
            return Ok(0);
        };
        if start >= guard.len() {
            return Ok(0);
        }
        let available = &guard[start..];
        let n = available.len().min(buf.len());
        buf[..n].copy_from_slice(&available[..n]);
        Ok(n)
    }
}
