use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use ntfs::structured_values::{NtfsFileName, NtfsFileNamespace};
use ntfs::Ntfs;

use crate::error::{Result, ScanError};
use crate::volume::open_volume;

const ROOT_RECORD_NUMBER: u64 = 5;

struct RecordInfo {
    is_directory: bool,
    allocated_size: u64,
    logical_size: u64,
}

pub struct MftScanReport {
    pub elapsed: Duration,
    pub records_scanned: u64,
    pub records_total: u64,
    /// Number of records actually walked this run — equals `records_total`
    /// unless `max_records` capped it (fast-iteration mode; sizes/counts
    /// below are then necessarily partial, not a real scan result).
    pub records_walked: u64,
    pub total_allocated: u64,
    pub total_logical: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

/// One immediate child of the scanned target, with its own subtree totals.
pub struct ChildSize {
    pub name: String,
    pub is_directory: bool,
    pub allocated_size: u64,
    pub logical_size: u64,
    pub file_count: u64,
    pub dir_count: u64,
}

pub struct MftBreakdownReport {
    pub elapsed: Duration,
    pub records_scanned: u64,
    pub records_total: u64,
    pub records_walked: u64,
    /// Immediate children of the target, sorted by `allocated_size` descending.
    pub children: Vec<ChildSize>,
}

/// Result of the shared MFT parse: every in-use record plus the
/// parent -> children edges, before any subpath resolution or aggregation.
struct ScanTree {
    records: HashMap<u64, RecordInfo>,
    children: HashMap<u64, Vec<(u64, String)>>,
    records_scanned: u64,
    records_total: u64,
    records_walked: u64,
}

/// Lower is preferred when a record has two `$FILE_NAME` attributes for the
/// *same* parent (a long Win32 name plus its separate 8.3 alias).
fn namespace_rank(ns: NtfsFileNamespace) -> u8 {
    match ns {
        NtfsFileNamespace::Win32AndDos => 0,
        NtfsFileNamespace::Win32 => 1,
        NtfsFileNamespace::Posix => 2,
        NtfsFileNamespace::Dos => 3,
    }
}

/// Scans `drive_letter`'s `$MFT` directly (raw volume read + NTFS record
/// parsing) and aggregates sizes bottom-up for `subpath` (relative to the
/// drive root; pass "" for the whole volume).
///
/// This reads every in-use MFT record once — the cost is dominated by
/// $MFT size, not by how deep or wide `subpath` is. That's the real
/// source of the speed win over FindFirstFile/FindNextFile: one
/// semi-sequential read of file records with parent pointers already
/// attached, instead of a directory-index walk with a syscall per folder.
///
/// A record can carry *multiple* `$FILE_NAME` attributes with *different*
/// parents — that's a real hardlink, not a duplicate (Windows does this
/// heavily for files serviced from the WinSxS component store, e.g. most
/// of `C:\Windows\Fonts`). Every parent is registered as a distinct edge;
/// only same-parent duplicates (an 8.3 alias next to the long name) are
/// deduplicated.
pub fn scan_volume(
    drive_letter: char,
    subpath: &str,
    max_records: Option<u64>,
) -> Result<MftScanReport> {
    let start = Instant::now();
    let tree = build_tree(drive_letter, max_records, start)?;

    let target_record = resolve_subpath(&tree.children, subpath)?;
    let (total_allocated, total_logical, file_count, dir_count) =
        aggregate(&tree.records, &tree.children, target_record);

    Ok(MftScanReport {
        elapsed: start.elapsed(),
        records_scanned: tree.records_scanned,
        records_total: tree.records_total,
        records_walked: tree.records_walked,
        total_allocated,
        total_logical,
        file_count,
        dir_count,
    })
}

/// Like [`scan_volume`], but instead of one aggregate total for `subpath`,
/// reports a separate total per *immediate* child of `subpath` — one $MFT
/// read still covers the whole thing, since [`build_tree`] already has
/// every record and edge in memory; each child just gets its own
/// [`aggregate`] call over the same maps.
pub fn scan_volume_children(
    drive_letter: char,
    subpath: &str,
    max_records: Option<u64>,
) -> Result<MftBreakdownReport> {
    let start = Instant::now();
    let tree = build_tree(drive_letter, max_records, start)?;

    let target_record = resolve_subpath(&tree.children, subpath)?;
    let kids = tree.children.get(&target_record).cloned().unwrap_or_default();

    let mut children: Vec<ChildSize> = kids
        .into_iter()
        .map(|(record_number, name)| {
            let is_directory = tree
                .records
                .get(&record_number)
                .map(|r| r.is_directory)
                .unwrap_or(false);
            let (allocated_size, logical_size, file_count, dir_count) =
                aggregate(&tree.records, &tree.children, record_number);
            ChildSize {
                name,
                is_directory,
                allocated_size,
                logical_size,
                file_count,
                dir_count,
            }
        })
        .collect();
    children.sort_by(|a, b| b.allocated_size.cmp(&a.allocated_size));

    Ok(MftBreakdownReport {
        elapsed: start.elapsed(),
        records_scanned: tree.records_scanned,
        records_total: tree.records_total,
        records_walked: tree.records_walked,
        children,
    })
}

/// Opens the volume and parses every in-use `$MFT` record into `records` +
/// the parent -> children edge map. Shared by [`scan_volume`] and
/// [`scan_volume_children`], which only differ in what they do with the
/// resulting tree.
fn build_tree(drive_letter: char, max_records: Option<u64>, start: Instant) -> Result<ScanTree> {
    let mut fs = open_volume(drive_letter)?;

    let mut ntfs = Ntfs::new(&mut fs)?;
    ntfs.read_upcase_table(&mut fs)?;
    let record_size = ntfs.file_record_size() as u64;

    // $MFT is always file record 0. NtfsFile::data_size()/allocated_size()
    // are NOT what they sound like — they describe the ~1024-byte file
    // *record segment* itself, not the file's content. The actual $MFT
    // content size comes from its unnamed $DATA attribute.
    let mft_file = ntfs.file(&mut fs, 0)?;
    let mft_data_item = match mft_file.data(&mut fs, "") {
        Some(item) => item?,
        None => return Err(ScanError::NotNtfs("$MFT has no unnamed $DATA attribute".into())),
    };
    let mft_total_size = mft_data_item.to_attribute()?.value_length();
    let records_total: u64 = mft_total_size / record_size;

    let mut records: HashMap<u64, RecordInfo> = HashMap::new();
    // parent_record_number -> [(child_record_number, child_name), ...]
    let mut children: HashMap<u64, Vec<(u64, String)>> = HashMap::new();
    let mut records_scanned = 0u64;

    let records_walked = max_records.unwrap_or(records_total).min(records_total);

    for record_number in 0..records_walked {
        if record_number % 5_000 == 0 {
            eprintln!(
                "[progress] {record_number}/{records_total} records, {} cache hits / {} misses ({:?} elapsed)",
                fs.hit_count(),
                fs.miss_count(),
                start.elapsed()
            );
        }

        let file = match ntfs.file(&mut fs, record_number) {
            Ok(f) => f,
            Err(_) => continue, // unused / free / corrupt record slot
        };

        // Collect every $FILE_NAME attribute, keeping at most one per
        // distinct parent (the best-ranked namespace for that parent).
        //
        // $FILE_NAME is always resident in the base record — it never
        // needs $ATTRIBUTE_LIST traversal into other MFT records — so
        // `attributes_raw()` (a plain, bounded Iterator over this record's
        // own already-loaded buffer) is both correct and the only variant
        // that can't be dragged into chasing a malformed/cyclic attribute
        // list on some other record.
        let mut names_by_parent: HashMap<u64, NtfsFileName> = HashMap::new();
        for attribute in file.attributes_raw() {
            let Ok(attribute) = attribute else { continue };
            let Ok(file_name) = attribute.structured_value::<_, NtfsFileName>(&mut fs) else {
                continue;
            };

            let parent = file_name.parent_directory_reference().file_record_number();
            let keep = match names_by_parent.get(&parent) {
                None => true,
                Some(existing) => namespace_rank(file_name.namespace()) < namespace_rank(existing.namespace()),
            };
            if keep {
                names_by_parent.insert(parent, file_name);
            }
        }

        if names_by_parent.is_empty() {
            continue; // e.g. an $ATTRIBUTE_LIST extension record with no name of its own
        }

        let is_directory = file.is_directory();
        let (allocated_size, logical_size) = names_by_parent
            .values()
            .next()
            .map(|n| (n.allocated_size(), n.data_size()))
            .unwrap_or((0, 0));

        records.insert(
            record_number,
            RecordInfo {
                is_directory,
                allocated_size,
                logical_size,
            },
        );

        if record_number != ROOT_RECORD_NUMBER {
            for (parent_record_number, file_name) in &names_by_parent {
                let name = file_name.name().to_string_lossy();
                children
                    .entry(*parent_record_number)
                    .or_default()
                    .push((record_number, name));
            }
        }

        records_scanned += 1;
    }

    Ok(ScanTree {
        records,
        children,
        records_scanned,
        records_total,
        records_walked,
    })
}

/// Walks the requested subpath component-by-component through the
/// already-scanned parent/child map, matching names case-insensitively —
/// mirrors NTFS's own case-insensitive Win32 namespace semantics.
fn resolve_subpath(children: &HashMap<u64, Vec<(u64, String)>>, subpath: &str) -> Result<u64> {
    let mut current = ROOT_RECORD_NUMBER;
    if subpath.trim().is_empty() {
        return Ok(current);
    }

    for component in subpath.split(['\\', '/']).filter(|c| !c.is_empty()) {
        let Some(kids) = children.get(&current) else {
            return Err(ScanError::PathNotFound(subpath.into()));
        };
        let found = kids
            .iter()
            .find(|(_, name)| name.eq_ignore_ascii_case(component));
        match found {
            Some(&(record_number, _)) => current = record_number,
            None => return Err(ScanError::PathNotFound(subpath.into())),
        }
    }

    Ok(current)
}

/// Iterative post-order aggregation (explicit stack) so depth is bounded
/// only by available memory, not by the call stack. `visited` also
/// de-duplicates a hardlinked record reachable via more than one parent
/// edge within the same subtree, so its bytes are only counted once.
fn aggregate(
    records: &HashMap<u64, RecordInfo>,
    children: &HashMap<u64, Vec<(u64, String)>>,
    root: u64,
) -> (u64, u64, u64, u64) {
    let mut total_allocated = 0u64;
    let mut total_logical = 0u64;
    let mut file_count = 0u64;
    let mut dir_count = 0u64;

    let mut stack = vec![root];
    let mut visited = HashSet::new();

    while let Some(record_number) = stack.pop() {
        if !visited.insert(record_number) {
            continue;
        }
        let Some(info) = records.get(&record_number) else {
            continue;
        };

        if info.is_directory {
            dir_count += 1;
            if let Some(kids) = children.get(&record_number) {
                stack.extend(kids.iter().map(|(child, _)| *child));
            }
        } else {
            file_count += 1;
            total_allocated += info.allocated_size;
            total_logical += info.logical_size;
        }
    }

    (total_allocated, total_logical, file_count, dir_count)
}
