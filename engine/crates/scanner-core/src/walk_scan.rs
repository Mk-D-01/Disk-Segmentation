use std::path::Path;
use std::time::{Duration, Instant};

use jwalk::WalkDir;

pub struct WalkScanReport {
    pub elapsed: Duration,
    pub total_size: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

pub struct WalkChildEntry {
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

pub struct WalkBreakdownReport {
    pub elapsed: Duration,
    /// Immediate children of the target, sorted by `size` descending.
    pub children: Vec<WalkChildEntry>,
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

/// Like [`scan_walk`], but reports a separate total per immediate child of
/// `path` instead of one grand total. No raw-volume access, so this runs
/// without Administrator — each child directory gets its own full
/// [`scan_walk`], and each child file just reads its own metadata.
pub fn scan_walk_children(path: &Path) -> WalkBreakdownReport {
    let start = Instant::now();
    let mut children = Vec::new();

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let Ok(metadata) = entry.metadata() else { continue };

            if metadata.is_dir() {
                let report = scan_walk(&entry.path());
                children.push(WalkChildEntry {
                    name,
                    is_directory: true,
                    size: report.total_size,
                    file_count: report.file_count,
                    dir_count: report.dir_count + 1, // + the folder itself
                });
            } else {
                children.push(WalkChildEntry {
                    name,
                    is_directory: false,
                    size: metadata.len(),
                    file_count: 1,
                    dir_count: 0,
                });
            }
        }
    }

    children.sort_by(|a, b| b.size.cmp(&a.size));
    WalkBreakdownReport {
        elapsed: start.elapsed(),
        children,
    }
}
