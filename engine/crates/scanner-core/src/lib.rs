pub mod error;
pub mod mft_scan;
mod prefetch_reader;
mod sector_reader;
pub mod volume;
pub mod walk_scan;

pub use error::{Result, ScanError};
pub use mft_scan::{
    scan_volume as scan_mft, scan_volume_children as scan_mft_children, ChildSize,
    MftBreakdownReport, MftScanReport,
};
pub use walk_scan::{scan_walk, scan_walk_children, WalkBreakdownReport, WalkChildEntry, WalkScanReport};
