//! Unifies the two scan implementations (`mft_scan`, `walk_scan`) behind one
//! interface, so a caller — the CLI, or a future GUI — can hold a list of
//! engines and loop over them generically instead of hand-writing one call
//! site per engine. Adding a third engine means adding a new type that
//! implements [`ScanEngine`]; it does not require touching any existing
//! caller.
//!
//! [`MftEngine`] and [`WalkEngine`] are adapters: `mft_scan`/`walk_scan` keep
//! their own richer, engine-specific result types (e.g. the MFT engine's
//! `records_total`/`records_walked` diagnostics have no walk-engine
//! equivalent), and these two types translate between that and the common
//! [`ScanReport`]/[`ChildEntry`] shape this module exposes.

use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::error::{Result, ScanError};
use crate::volume::split_drive_and_subpath;
use crate::{mft_scan, walk_scan};

/// What to scan: a single filesystem path, in whatever form the caller has
/// it (`D:\Games`, `D:\`, ...). Each engine interprets it as needed — the
/// MFT engine splits it into a drive letter + subpath, the walk engine uses
/// it as-is.
#[derive(Debug, Clone)]
pub struct ScanTarget {
    pub path: String,
}

impl ScanTarget {
    pub fn new(path: impl Into<String>) -> Self {
        Self { path: path.into() }
    }
}

/// One engine's total for a [`ScanTarget`].
///
/// The walk engine cannot distinguish allocated (on-disk, cluster-rounded)
/// size from logical (content) size without attribute reads it doesn't do —
/// for it, `allocated_size == logical_size`. Only the MFT engine reports a
/// real difference between the two.
#[derive(Debug, Clone, Serialize)]
pub struct ScanReport {
    pub elapsed_ms: u64,
    pub allocated_size: u64,
    pub logical_size: u64,
    pub file_count: u64,
    pub dir_count: u64,
    /// An engine-specific caveat about this result, if any (e.g. the MFT
    /// engine flagging a `max_records`-capped partial scan). `None` means
    /// the result is complete as far as the engine can tell.
    pub note: Option<String>,
}

/// One immediate child of a scanned target, with its own subtree totals.
/// Same allocated/logical caveat as [`ScanReport`] applies per engine.
#[derive(Debug, Clone, Serialize)]
pub struct ChildEntry {
    pub name: String,
    pub is_directory: bool,
    pub allocated_size: u64,
    pub logical_size: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

/// Result of [`ScanEngine::scan_children`]: the per-child breakdown plus the
/// same kind of engine-specific caveat [`ScanReport`] carries — kept
/// separate from `Vec<ChildEntry>` itself so a partial-scan warning isn't
/// lost just because the caller wanted a breakdown instead of one total.
#[derive(Debug, Clone, Serialize)]
pub struct ChildrenReport {
    /// Sorted by `allocated_size` descending.
    pub children: Vec<ChildEntry>,
    pub note: Option<String>,
}

/// A folder-size scanning strategy. Implementations are stateless enough to
/// be constructed fresh per call (`MftEngine`/`WalkEngine` are both plain
/// data), so callers are expected to hold them as `Box<dyn ScanEngine>` or
/// similar and iterate rather than special-casing each one by name.
pub trait ScanEngine: Send + Sync {
    /// Short, stable identifier (e.g. `"mft"`, `"walk"`) — for logging/UI
    /// labeling, not for callers to branch on.
    fn id(&self) -> &'static str;

    /// Whether this engine needs an elevated (Administrator) process to
    /// succeed. Callers can use this to skip a doomed attempt instead of
    /// reading it off a returned error.
    fn requires_elevation(&self) -> bool;

    /// One aggregate total for the whole target.
    fn scan(&self, target: &ScanTarget) -> Result<ScanReport>;

    /// One total per *immediate* child of the target.
    fn scan_children(&self, target: &ScanTarget) -> Result<ChildrenReport>;
}

fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

/// Raw `$MFT` parsing engine. Needs Administrator; NTFS volumes only.
pub struct MftEngine {
    /// Caps how many MFT records get walked, for fast iteration on
    /// logic/correctness. `None` walks the whole table (a real result).
    pub max_records: Option<u64>,
}

impl MftEngine {
    pub fn new() -> Self {
        Self { max_records: None }
    }

    pub fn with_max_records(max_records: Option<u64>) -> Self {
        Self { max_records }
    }

    fn split(&self, target: &ScanTarget) -> Result<(char, String)> {
        split_drive_and_subpath(&target.path)
            .ok_or_else(|| ScanError::InvalidTarget(target.path.clone()))
    }
}

impl Default for MftEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanEngine for MftEngine {
    fn id(&self) -> &'static str {
        "mft"
    }

    fn requires_elevation(&self) -> bool {
        true
    }

    fn scan(&self, target: &ScanTarget) -> Result<ScanReport> {
        let (drive, subpath) = self.split(target)?;
        let report = mft_scan::scan_volume(drive, &subpath, self.max_records)?;
        let note = (report.records_walked < report.records_total).then(|| {
            format!(
                "partial scan: {} / {} MFT records walked (--max-records set)",
                report.records_walked, report.records_total
            )
        });
        Ok(ScanReport {
            elapsed_ms: duration_ms(report.elapsed),
            allocated_size: report.total_allocated,
            logical_size: report.total_logical,
            file_count: report.file_count,
            dir_count: report.dir_count,
            note,
        })
    }

    fn scan_children(&self, target: &ScanTarget) -> Result<ChildrenReport> {
        let (drive, subpath) = self.split(target)?;
        let report = mft_scan::scan_volume_children(drive, &subpath, self.max_records)?;
        let note = (report.records_walked < report.records_total).then(|| {
            format!(
                "partial scan: {} / {} MFT records walked (--max-records set)",
                report.records_walked, report.records_total
            )
        });
        let children = report
            .children
            .into_iter()
            .map(|c| ChildEntry {
                name: c.name,
                is_directory: c.is_directory,
                allocated_size: c.allocated_size,
                logical_size: c.logical_size,
                file_count: c.file_count,
                dir_count: c.dir_count,
            })
            .collect();
        Ok(ChildrenReport { children, note })
    }
}

/// Multithreaded directory-walk engine. No elevation needed; works on any
/// filesystem, but silently under-counts anything it lacks permission to
/// read (see `docs/LLD.md`).
pub struct WalkEngine;

impl ScanEngine for WalkEngine {
    fn id(&self) -> &'static str {
        "walk"
    }

    fn requires_elevation(&self) -> bool {
        false
    }

    fn scan(&self, target: &ScanTarget) -> Result<ScanReport> {
        let report = walk_scan::scan_walk(Path::new(&target.path));
        Ok(ScanReport {
            elapsed_ms: duration_ms(report.elapsed),
            allocated_size: report.total_size,
            logical_size: report.total_size,
            file_count: report.file_count,
            dir_count: report.dir_count,
            note: None,
        })
    }

    fn scan_children(&self, target: &ScanTarget) -> Result<ChildrenReport> {
        let report = walk_scan::scan_walk_children(Path::new(&target.path));
        let children = report
            .children
            .into_iter()
            .map(|c| ChildEntry {
                name: c.name,
                is_directory: c.is_directory,
                allocated_size: c.size,
                logical_size: c.size,
                file_count: c.file_count,
                dir_count: c.dir_count,
            })
            .collect();
        Ok(ChildrenReport { children, note: None })
    }
}
