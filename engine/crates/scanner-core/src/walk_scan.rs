use std::path::Path;
use std::time::{Duration, Instant};

use jwalk::WalkDir;

pub struct WalkScanReport {
    pub elapsed: Duration,
    pub total_size: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

/// Fallback / comparison engine: a multithreaded recursive directory walk
/// using jwalk's internal work-stealing thread pool. This is also the
/// primary engine for non-NTFS volumes (exFAT/FAT32 USB drives, etc.).
pub fn scan_walk(path: &Path) -> WalkScanReport {
    let start = Instant::now();

    let mut total_size = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;

    for entry in WalkDir::new(path) {
        let Ok(entry) = entry else { continue };
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if metadata.is_dir() {
            dir_count += 1;
        } else {
            file_count += 1;
            total_size += metadata.len();
        }
    }

    WalkScanReport {
        elapsed: start.elapsed(),
        total_size,
        file_count,
        dir_count,
    }
}
