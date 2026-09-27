use std::io;
use std::io::{Read, Seek, SeekFrom};

/// Wraps a `Read + Seek` source with a large forward-biased cache.
///
/// `ntfs::Ntfs::file()` issues `seek(Start(pos)); read_exact(1024 bytes)` for
/// every single MFT record. `std::io::BufReader` discards its internal
/// buffer on *every* `seek()` call — even a no-op one to a position already
/// inside the buffer — so layering it under that access pattern turns a
/// ~1.5GB sequential scan into one raw-device syscall per record (measured:
/// ~590us/record, i.e. minutes for a volume with a few million records).
///
/// This reader tracks a purely logical position on `seek()` (no I/O) and
/// only touches `inner` when a `read()` actually falls outside the current
/// cached window, fetching `chunk_size` bytes at a time.
pub struct PrefetchReader<R: Read + Seek> {
    inner: R,
    chunk_size: usize,
    buffer: Vec<u8>,
    buffer_start: u64,
    position: u64,
    /// Real fetches from `inner` (cache misses) — instrumentation to tell
    /// "the cache isn't helping" apart from "something external is slow".
    miss_count: u64,
    hit_count: u64,
}

impl<R: Read + Seek> PrefetchReader<R> {
    pub fn new(inner: R, chunk_size: usize) -> Self {
        Self {
            inner,
            chunk_size,
            buffer: Vec::new(),
            buffer_start: 0,
            position: 0,
            miss_count: 0,
            hit_count: 0,
        }
    }

    pub fn miss_count(&self) -> u64 {
        self.miss_count
    }

    pub fn hit_count(&self) -> u64 {
        self.hit_count
    }
}

impl<R: Read + Seek> Read for PrefetchReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let want_start = self.position;
        let want_end = want_start + buf.len() as u64;
        let have_start = self.buffer_start;
        let have_end = self.buffer_start + self.buffer.len() as u64;

        let cached = !self.buffer.is_empty() && want_start >= have_start && want_end <= have_end;

        if !cached {
            self.miss_count += 1;
            let fetch_len = self.chunk_size.max(buf.len());
            self.inner.seek(SeekFrom::Start(want_start))?;
            self.buffer.resize(fetch_len, 0);
            self.inner.read_exact(&mut self.buffer)?;
            self.buffer_start = want_start;
        } else {
            self.hit_count += 1;
        }

        let offset = (self.position - self.buffer_start) as usize;
        let n = buf.len().min(self.buffer.len() - offset);
        buf[..n].copy_from_slice(&self.buffer[offset..offset + n]);
        self.position += n as u64;
        Ok(n)
    }
}

impl<R: Read + Seek> Seek for PrefetchReader<R> {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        self.position = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(n) => {
                if n >= 0 {
                    self.position.checked_add(n as u64)
                } else {
                    self.position.checked_sub(n.wrapping_neg() as u64)
                }
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek overflow"))?
            }
            SeekFrom::End(_) => {
                return Err(io::Error::other("SeekFrom::End is unsupported for PrefetchReader"));
            }
        };
        Ok(self.position)
    }
}
