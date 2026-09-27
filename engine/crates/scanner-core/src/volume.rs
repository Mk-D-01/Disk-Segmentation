use std::fs::{File, OpenOptions};

use crate::error::{Result, ScanError};
use crate::prefetch_reader::PrefetchReader;
use crate::sector_reader::SectorReader;

/// 8 MiB per fetch: a multiple of both the record size (1024) and the
/// sector-alignment size (4096) below, so no record read ever straddles
/// a chunk boundary awkwardly, and large enough that ~1.4M records only
/// cost a few hundred real reads instead of one syscall each.
const PREFETCH_CHUNK_SIZE: usize = 8 * 1024 * 1024;

/// Raw Windows volume handles only permit sector-aligned reads/seeks, so
/// every consumer of an open volume gets this type — never the bare
/// `File`. [`PrefetchReader`] is the outermost/only cache (see its own
/// docs for why `BufReader` doesn't work here); [`SectorReader`] under it
/// satisfies the alignment requirement on the rare underlying reads.
pub type VolumeReader = PrefetchReader<SectorReader<File>>;

/// Opens a raw handle to an NTFS volume (e.g. `C`) for reading its `$MFT`.
///
/// Requires the process to be running elevated (Administrator) — a normal
/// user token gets `ERROR_ACCESS_DENIED` opening `\\.\<drive>:` for read.
pub fn open_volume(drive_letter: char) -> Result<VolumeReader> {
    let path = format!(r"\\.\{}:", drive_letter.to_ascii_uppercase());
    let file = OpenOptions::new()
        .read(true)
        .open(&path)
        .map_err(|e| ScanError::OpenVolume(drive_letter.to_ascii_uppercase(), e))?;
    let sector_reader = SectorReader::new(file, 4096)?;
    Ok(PrefetchReader::new(sector_reader, PREFETCH_CHUNK_SIZE))
}

/// Splits an absolute Windows path like `C:\Users\Bob\Docs` into
/// (`'C'`, `"Users\Bob\Docs"`). The remainder is empty for a bare drive root.
pub fn split_drive_and_subpath(path: &str) -> Option<(char, String)> {
    let mut chars = path.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    if chars.next() != Some(':') {
        return None;
    }
    let rest = chars.as_str().trim_start_matches(['\\', '/']);
    Some((drive, rest.to_string()))
}
