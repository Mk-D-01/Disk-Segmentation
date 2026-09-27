// Copyright 2021 Colin Finck <colin@reactos.org>
// SPDX-License-Identifier: MIT OR Apache-2.0
//
// Vendored from the `ntfs` crate's `ntfs-shell` example
// (examples/ntfs-shell/sector_reader.rs), unmodified.
//
// Raw Windows volume handles (`\\.\C:`) only permit reads/seeks on sector
// boundaries — `binrw`'s small field-by-field reads inside the `ntfs` crate
// are not sector-aligned, which fails with ERROR_INVALID_PARAMETER (os
// error 87) without this wrapper.

use std::io;
use std::io::{Read, Seek, SeekFrom};

/// `SectorReader` encapsulates any reader and only performs read and seek operations on it
/// on boundaries of the given sector size.
///
/// This can be very useful for readers that only accept sector-sized reads (like reading
/// from a raw partition on Windows).
/// The sector size must be a power of two.
///
/// This reader does not keep any buffer.
/// You are advised to encapsulate `SectorReader` in a buffered reader, as unbuffered reads of
/// just a few bytes here and there are highly inefficient.
pub struct SectorReader<R>
where
    R: Read + Seek,
{
    inner: R,
    sector_size: usize,
    stream_position: u64,
    temp_buf: Vec<u8>,
}

impl<R> SectorReader<R>
where
    R: Read + Seek,
{
    pub fn new(inner: R, sector_size: usize) -> io::Result<Self> {
        if !sector_size.is_power_of_two() {
            return Err(io::Error::other("sector_size is not a power of two"));
        }

        Ok(Self {
            inner,
            sector_size,
            stream_position: 0,
            temp_buf: Vec::new(),
        })
    }

    fn align_down_to_sector_size(&self, n: u64) -> u64 {
        n / self.sector_size as u64 * self.sector_size as u64
    }

    fn align_up_to_sector_size(&self, n: u64) -> u64 {
        self.align_down_to_sector_size(n) + self.sector_size as u64
    }
}

impl<R> Read for SectorReader<R>
where
    R: Read + Seek,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let aligned_position = self.align_down_to_sector_size(self.stream_position);

        let start = (self.stream_position - aligned_position) as usize;
        let end = start + buf.len();
        let aligned_bytes_to_read = self.align_up_to_sector_size(end as u64) as usize;

        self.temp_buf.resize(aligned_bytes_to_read, 0);
        self.inner.read_exact(&mut self.temp_buf)?;
        buf.copy_from_slice(&self.temp_buf[start..end]);

        self.stream_position += buf.len() as u64;
        Ok(buf.len())
    }
}

impl<R> Seek for SectorReader<R>
where
    R: Read + Seek,
{
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(n) => Some(n),
            SeekFrom::End(_n) => {
                return Err(io::Error::other(
                    "SeekFrom::End is unsupported for SectorReader",
                ));
            }
            SeekFrom::Current(n) => {
                if n >= 0 {
                    self.stream_position.checked_add(n as u64)
                } else {
                    self.stream_position.checked_sub(n.wrapping_neg() as u64)
                }
            }
        };

        match new_pos {
            Some(n) => {
                let aligned_n = self.align_down_to_sector_size(n);
                self.inner.seek(SeekFrom::Start(aligned_n))?;
                self.stream_position = n;
                Ok(self.stream_position)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            )),
        }
    }
}
