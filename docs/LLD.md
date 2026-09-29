# Low-Level Design

Concrete types, function signatures, algorithms, and edge cases, module by
module. Line numbers are omitted since they drift; struct/function names are
exact as of this writing.

## `scanner-core::error`

```rust
pub enum ScanError {
    OpenVolume(char, std::io::Error),  // volume open failed (usually: not elevated)
    Io(#[from] std::io::Error),
    Ntfs(#[from] ntfs::NtfsError),
    PathNotFound(PathBuf),
    NotNtfs(String),
    InvalidTarget(String),             // target string isn't a path this engine can use (e.g. no drive letter, for MftEngine)
}
pub type Result<T> = std::result::Result<T, ScanError>;
```

One enum for both engines. `#[from]` on `Io` and `Ntfs` means most low-level
failures propagate via `?` without manual wrapping. `Display` messages are
derived via `thiserror`; `OpenVolume`'s message explicitly hints at the
Administrator requirement since that's by far the most common cause.

## `scanner-core::volume`

```rust
pub type VolumeReader = PrefetchReader<SectorReader<File>>;
pub fn open_volume(drive_letter: char) -> Result<VolumeReader>;
pub fn split_drive_and_subpath(path: &str) -> Option<(char, String)>;
```

- `open_volume`: builds `\\.\<DRIVE>:` (uppercased), opens it read-only,
  wraps it `SectorReader::new(file, 4096)` then
  `PrefetchReader::new(sector_reader, 8 * 1024 * 1024)`. The 4096 sector
  size and 8 MiB chunk size are both hardcoded constants at the call site /
  in `mft_scan.rs` (`PREFETCH_CHUNK_SIZE`), not configurable.
- `split_drive_and_subpath`: takes the first char, requires it be
  ASCII-alphabetic and followed by `:`; the rest of the string has any
  leading `\` or `/` trimmed. Returns `None` (not an error) for anything that
  doesn't look like `X:...` — the caller (`main.rs`) turns that into a
  user-facing message itself.

## `scanner-core::sector_reader::SectorReader<R>`

Vendored unmodified from the `ntfs` crate's own `ntfs-shell` example
(MIT/Apache-2.0, attributed in the file header) — not original code in this
project, kept as-is because raw-volume sector alignment is a solved problem
that shouldn't be re-solved.

```rust
pub struct SectorReader<R: Read + Seek> { inner: R, sector_size: usize, stream_position: u64, temp_buf: Vec<u8> }
pub fn new(inner: R, sector_size: usize) -> io::Result<Self>;  // errors if sector_size isn't a power of two
```

- `read`: computes `aligned_position = floor(stream_position / sector_size) * sector_size`,
  reads a temp buffer sized up to the next sector boundary past the request,
  and copies out just the requested slice. Every call re-reads from
  `aligned_position`, even for a request already covered by a previous
  read — this type keeps **no cache**, by design (that's `PrefetchReader`'s
  job, layered on top).
- `seek`: `SeekFrom::End` is unconditionally `Err` ("unsupported") — the
  volume's total size was never needed by this reader's only consumer.
  `SeekFrom::Start`/`Current` align down to the sector boundary before
  seeking the inner reader, but track the *unaligned* logical
  `stream_position` for `read()` to compute its slice offset from.

## `scanner-core::prefetch_reader::PrefetchReader<R>`

```rust
pub struct PrefetchReader<R: Read + Seek> {
    inner: R, chunk_size: usize, buffer: Vec<u8>,
    buffer_start: u64, position: u64, miss_count: u64, hit_count: u64,
}
pub fn new(inner: R, chunk_size: usize) -> Self;
pub fn hit_count(&self) -> u64;
pub fn miss_count(&self) -> u64;
```

**Why this exists at all** (documented in the source): `ntfs::Ntfs::file()`
issues `seek(Start(pos)); read_exact(1024 bytes)` once per MFT record.
`std::io::BufReader` discards its entire internal buffer on *every*
`seek()` call, even a no-op seek to a position already inside the buffer —
so stacking a `BufReader` under this access pattern degrades to one raw
device read per record: measured at ~590µs/record, i.e. minutes for a
multi-million-record volume.

- `seek()` here is **purely logical** — it only updates `self.position`,
  never touches `inner`. This is what makes the pattern above cheap.
- `read()` checks whether `[position, position+len)` falls fully inside
  `[buffer_start, buffer_start+buffer.len())`. If yes: `hit_count += 1`,
  copy straight out of `buffer`. If no: `miss_count += 1`, seek `inner` to
  `position`, fetch `max(chunk_size, len)` bytes (so a read larger than one
  chunk still succeeds in one fetch), replace the buffer, then copy out.
- Cache is a single window, not an LRU of chunks — a read pattern that
  jumps backward past the current window on every call would thrash (miss
  every time). The MFT walk's access pattern (record 0, 1, 2, ... in order)
  is exactly the forward-sequential case this is built for.
- `hit_count`/`miss_count` are read by `mft_scan`'s progress logging
  (every 5,000 records) purely as instrumentation — "the cache isn't
  helping" is distinguishable from "something external is slow."

## `scanner-core::mft_scan`

```rust
const ROOT_RECORD_NUMBER: u64 = 5;          // NTFS volume root is always record 5
const PREFETCH_CHUNK_SIZE: usize = 8 * 1024 * 1024;

struct RecordInfo { is_directory: bool, allocated_size: u64, logical_size: u64 }

pub struct MftScanReport {
    pub elapsed: Duration, pub records_scanned: u64, pub records_total: u64,
    pub records_walked: u64, pub total_allocated: u64, pub total_logical: u64,
    pub file_count: u64, pub dir_count: u64,
}
pub struct ChildSize {
    pub name: String, pub is_directory: bool,
    pub allocated_size: u64, pub logical_size: u64,
    pub file_count: u64, pub dir_count: u64,
}
pub struct MftBreakdownReport {
    pub elapsed: Duration, pub records_scanned: u64, pub records_total: u64,
    pub records_walked: u64, pub children: Vec<ChildSize>,  // sorted by allocated_size, descending
}

struct ScanTree {
    records: HashMap<u64, RecordInfo>,
    children: HashMap<u64, Vec<(u64, String)>>,  // parent record# -> [(child record#, name)]
    records_scanned: u64, records_total: u64, records_walked: u64,
}

pub fn scan_volume(drive_letter: char, subpath: &str, max_records: Option<u64>) -> Result<MftScanReport>;
pub fn scan_volume_children(drive_letter: char, subpath: &str, max_records: Option<u64>) -> Result<MftBreakdownReport>;
fn build_tree(drive_letter: char, max_records: Option<u64>, start: Instant) -> Result<ScanTree>;
fn resolve_subpath(children: &HashMap<u64, Vec<(u64, String)>>, subpath: &str) -> Result<u64>;
fn aggregate(records: &HashMap<u64, RecordInfo>, children: &HashMap<u64, Vec<(u64, String)>>, root: u64) -> (u64, u64, u64, u64); // (allocated, logical, files, dirs)
fn namespace_rank(ns: NtfsFileNamespace) -> u8;  // Win32AndDos=0, Win32=1, Posix=2, Dos=3 (lower wins)
```

### `build_tree` — the one pass over `$MFT`

Shared by both public entry points (extracted specifically so
`scan_volume_children` doesn't duplicate the parse loop).

1. `open_volume(drive_letter)?` — the only fallible, elevation-gated step.
2. `Ntfs::new(&mut fs)?` then `ntfs.read_upcase_table(&mut fs)?` (needed for
   case-insensitive name comparisons downstream) — parser setup, not yet
   reading records.
3. Read record 0 (`$MFT` describing itself), get its **unnamed** `$DATA`
   attribute, and use `value_length()` — explicitly *not*
   `NtfsFile::data_size()`/`allocated_size()`, which describe the ~1024-byte
   record segment itself, not file content — divided by
   `ntfs.file_record_size()` to get `records_total`.
4. `records_walked = min(max_records.unwrap_or(records_total), records_total)`
   — `--max-records` only ever *shrinks* the walk, never extends past the
   real total.
5. Loop `record_number` in `0..records_walked`:
   - Every 5,000 records, `eprintln!` progress (record count, cache
     hit/miss counts, elapsed) — goes to stderr so it doesn't pollute the
     CLI's stdout report.
   - `ntfs.file(&mut fs, record_number)` — `Err` here (unused/free/corrupt
     slot) is `continue`d, not propagated. A bad record does not fail the
     scan.
   - Iterate `file.attributes_raw()` (a bounded, non-attribute-list-chasing
     iterator over just this record's own resident attributes — chosen
     specifically so a malformed/cyclic `$ATTRIBUTE_LIST` elsewhere can't
     hang this loop), collecting one `NtfsFileName` per distinct parent via
     `namespace_rank` tie-breaking. A record with **zero** `$FILE_NAME`
     attributes (e.g. an `$ATTRIBUTE_LIST` extension record) is skipped
     entirely — it isn't inserted into `records` or `children`.
   - `is_directory` and (`allocated_size`, `logical_size`) come from
     whichever single `NtfsFileName` survives tie-breaking per parent
     (arbitrary pick among equally-ranked parents — sizes don't actually
     vary by parent, only the name does).
   - Insert into `records`. Then, **unless this is the root record itself**
     (record 5 has no meaningful "parent" edge to register), push a
     `(record_number, name)` edge into `children` for every surviving
     parent — this is where a hardlink gets more than one edge.
6. Return the populated `ScanTree`.

### `resolve_subpath`

Starts at `ROOT_RECORD_NUMBER`. Empty/whitespace-only subpath returns the
root immediately (whole-volume query). Otherwise splits on `\` or `/`,
drops empty components (handles `C:\\Users` or a trailing slash), and for
each component does a **linear scan** of that level's children vector for a
case-insensitive name match (`eq_ignore_ascii_case`) — not a hash lookup,
since children are stored as `Vec`, not keyed by name; fine at directory
fan-out sizes, would need revisiting for pathologically wide directories.
First unmatched component or a parent with no children entry at all ⇒
`Err(PathNotFound)`.

### `aggregate` — iterative post-order sum

```
stack = [root]; visited = {}
while stack not empty:
    n = stack.pop()
    if n in visited: continue        // hardlink already counted in this subtree
    visited.insert(n)
    info = records.get(n)
    if info missing: continue        // shouldn't normally happen; defensive
    if info.is_directory:
        dir_count += 1
        stack.extend(children_of(n)) // push, don't recurse
    else:
        file_count += 1
        total_allocated += info.allocated_size
        total_logical += info.logical_size
return (total_allocated, total_logical, file_count, dir_count)
```

Explicit `Vec`-backed stack, not recursion — depth is bounded by available
memory, not by the OS thread's call-stack size, so a pathologically deep
directory tree can't stack-overflow this. `visited` is what makes a
hardlink correct: reachable via two parent edges inside the same subtree,
its bytes are added exactly once, on whichever edge is popped first.

### `scan_volume_children`

After `build_tree` + `resolve_subpath`, looks up `tree.children[target]`
(cloned — a small `Vec`, one clone per call, not per record), and for each
`(record_number, name)` pair calls `aggregate` rooted at that child
specifically — so a subfolder's own total includes *its* whole subtree, but
siblings don't leak into each other. `is_directory` for the row is read
directly off `tree.records[record_number]`, defaulting to `false` if
somehow absent (defensive, not expected in practice). Result sorted by
`allocated_size` descending before returning.

**Cost note**: this is `O(total records in scanned subtree)` — the same
total work as one whole-tree `aggregate` call, just partitioned per child
instead of summed into one number. No additional `$MFT` I/O versus
`scan_volume`.

## `scanner-core::walk_scan`

```rust
pub struct WalkScanReport { pub elapsed: Duration, pub total_size: u64, pub file_count: u64, pub dir_count: u64 }
pub struct WalkChildEntry { pub name: String, pub is_directory: bool, pub size: u64, pub file_count: u64, pub dir_count: u64 }
pub struct WalkBreakdownReport { pub elapsed: Duration, pub children: Vec<WalkChildEntry> } // sorted by size, descending

pub fn scan_walk(path: &Path) -> WalkScanReport;
pub fn scan_walk_children(path: &Path) -> WalkBreakdownReport;
```

- `scan_walk`: `jwalk::WalkDir::new(path)` iterated to completion. Any
  entry whose `Result` is `Err`, or whose `.metadata()` call errors
  (permission denied is the common case), is **silently skipped** — not
  counted as a file, not counted as an error, not logged. This is the
  single biggest accuracy caveat of the whole tool: a protected system
  folder scanned without elevation reports as `0 bytes`, indistinguishable
  from actually empty, unless the caller already knows to be suspicious of
  a round zero.
- `scan_walk_children`: `std::fs::read_dir(path)` (one level, not
  recursive) — entries that error are `.flatten()`-dropped the same way.
  For each surviving entry: if it's a directory, run a full `scan_walk` on
  it and report that subtree's totals, plus `+ 1` to `dir_count` for the
  folder itself (since `scan_walk`'s own `dir_count` only counts entries
  *found inside* the walked path, not the root of the walk). If it's a
  file, report its own `metadata().len()` directly with `file_count: 1,
  dir_count: 0` — no walk needed.
- Neither function needs Administrator — this is the only engine that
  works on non-NTFS volumes and the only one safe to run unelevated, at the
  cost of the silent-skip behavior above.

## `scanner-core::engine`

```rust
pub struct ScanTarget { pub path: String }
impl ScanTarget { pub fn new(path: impl Into<String>) -> Self; }

pub struct ScanReport {
    pub elapsed_ms: u64, pub allocated_size: u64, pub logical_size: u64,
    pub file_count: u64, pub dir_count: u64, pub note: Option<String>,
} // #[derive(Serialize)] — crosses the Tauri IPC boundary as-is

pub struct ChildEntry {
    pub name: String, pub is_directory: bool,
    pub allocated_size: u64, pub logical_size: u64,
    pub file_count: u64, pub dir_count: u64,
} // #[derive(Serialize)]

pub struct ChildrenReport {
    pub children: Vec<ChildEntry>,  // sorted by allocated_size, descending
    pub note: Option<String>,       // same caveat mechanism as ScanReport::note
} // #[derive(Serialize)]

pub trait ScanEngine: Send + Sync {
    fn id(&self) -> &'static str;
    fn requires_elevation(&self) -> bool;
    fn scan(&self, target: &ScanTarget) -> Result<ScanReport>;
    fn scan_children(&self, target: &ScanTarget) -> Result<ChildrenReport>;
}

pub struct MftEngine { pub max_records: Option<u64> }   // id() == "mft",  requires_elevation() == true
pub struct WalkEngine;                                   // id() == "walk", requires_elevation() == false
```

This module is the adapter layer between `mft_scan`/`walk_scan`'s own
richer, differently-shaped report types and one common shape every caller
can loop over.

- `MftEngine::scan`/`scan_children` first call the private `split` helper
  (`self.split(target)`), which wraps `volume::split_drive_and_subpath` and
  turns a `None` into `Err(ScanError::InvalidTarget(target.path.clone()))`
  — the one new error variant this refactor added. Note this moved
  *out* of `scanner-cli` (which used to do this parse itself before even
  calling into `scanner-core`) and *into* the engine — a real correctness
  fix, not just a refactor: the old CLI code required every target to look
  like a drive path even when only the walk engine would run, which the
  walk engine never actually needed.
- `MftEngine::scan` calls `mft_scan::scan_volume`, then maps
  `records_walked < records_total` into `ScanReport::note` as a formatted
  string (`"partial scan: X / Y MFT records walked (--max-records set)"`)
  rather than a separate field — deliberately, so a caller that doesn't
  care about MFT-specific diagnostics (the frontend, today) doesn't need to
  know they exist; a caller that does can still read the note.
  `MftEngine::scan_children` does the identical mapping into
  `ChildrenReport::note` — the breakdown path carries the same caveat the
  aggregate-total path does; this was the entire reason `ChildrenReport`
  wraps `Vec<ChildEntry>` instead of `scan_children` returning the bare
  `Vec` (an earlier version of this method did exactly that, and silently
  dropped the partial-scan warning in `--children` mode — a real
  regression that surfaced during doc review, not an intentional
  simplification).
- `WalkEngine::scan`/`scan_children` map `WalkScanReport::total_size` /
  `WalkChildEntry::size` into **both** `allocated_size` and `logical_size`
  on the shared type — the walk engine has no distinct allocated-size
  reading (see `walk_scan` below), so both fields hold the identical
  number. This is a documented modeling choice on the struct's own doc
  comment, not a bug to fix later.
- Neither engine struct holds any mutable state; both are cheap to
  construct fresh per call (`MftEngine::new()`/`with_max_records(..)`,
  `WalkEngine` is a unit struct). Callers are expected to build a fresh
  `Box<dyn ScanEngine>` per request rather than caching one long-lived.

## `scanner-cli::main`

```rust
fn main() -> ExitCode
fn engine_label(id: &str) -> &'static str
fn print_report(report: &ScanReport)
fn print_children_table(children: &[ChildEntry])
fn print_comparison_note()
```

- Argv parsing is manual, no crate: `args.get(1)` is the target path
  (missing ⇒ print usage, exit failure); `--max-records N` is found by
  position + parsed as `u64` (a malformed or missing value silently yields
  `None`, i.e. "no cap", rather than an error — a deliberate leniency
  trade-off, not currently surfaced to the user); `--children` is a bare
  boolean flag (`args.iter().any(...)`).
- Builds `engines: Vec<Box<dyn ScanEngine>> = vec![Box::new(MftEngine::with_max_records(max_records)), Box::new(WalkEngine)]`
  once, then loops over it — this loop is the concrete demonstration of
  Open/Closed from `docs/HLD.md`'s "Design principles" section: a third
  engine added to this `vec!` needs no other change here.
- No upfront path validation — unlike before the `engine` refactor, `main`
  no longer calls `split_drive_and_subpath` itself; each engine validates
  the target when it actually tries to use it (see `engine::MftEngine`
  above). `main` just prints the raw target string.
- Default mode calls `engine.scan(&target)` per engine and prints via
  `print_report` (which prints `report.note` first, if present, then
  elapsed/counts/sizes), followed by `print_comparison_note`'s fixed
  explanation of why the two numbers aren't a fair race unless `<path>` is
  a drive root.
- `--children` mode calls `engine.scan_children(&target)` per engine,
  prints `report.note` first if present (same partial-scan caveat as
  `scan`), then prints `report.children` via `print_children_table`, which
  trusts the `ScanEngine` contract that the `Vec` is already sorted by
  `allocated_size` descending (no re-sort). An empty list prints
  `(no entries)`.
- An engine's `Err` (e.g. the MFT engine unelevated) prints `FAILED: {e}`
  and the loop continues to the next engine — not fatal to the process.
- No output format other than `println!`/`eprintln!` text — no JSON, no
  exit-code differentiation between "MFT failed but walk succeeded" and
  "both succeeded" (both currently return `ExitCode::SUCCESS` as long as
  the process didn't panic).

## `frontend/src-tauri::main` and `drives`

```rust
// main.rs
fn engine_for(id: &str) -> Result<Box<dyn ScanEngine>, String>;   // "mft" | "walk" | _ => Err

#[tauri::command] fn list_drives() -> Vec<DriveInfo>;
#[tauri::command] fn scan_summary(path: String, engine: String) -> Result<ScanReport, String>;
#[tauri::command] fn scan_children(path: String, engine: String) -> Result<ChildrenReport, String>;

// drives.rs
pub struct DriveInfo {
    pub mount_point: String, pub total_bytes: u64, pub available_bytes: u64,
    pub file_system: String, pub is_removable: bool,
} // #[derive(Serialize)]
pub fn list_drives() -> Vec<DriveInfo>;  // via sysinfo::Disks::new_with_refreshed_list(), sorted by mount_point
```

- `engine_for` is the **only** place in the app that matches on the two
  engine ids by name — the same "one dispatch point" property `scanner-cli`
  has via its `Vec<Box<dyn ScanEngine>>`, just shaped differently here
  because the UI picks *one* engine per request rather than running both.
- All three commands return `Result<_, String>`, not `Result<_, ScanError>`
  — `ScanError` isn't `Serialize` (it wraps `std::io::Error`/`ntfs::NtfsError`,
  neither of which are either), so the error is flattened to its `Display`
  text (`.map_err(|e| e.to_string())`) at the command boundary. This loses
  the ability to match on error *kind* in the UI (e.g. distinguish
  "not elevated" from "path not found" programmatically) — today the UI
  only ever displays the string, so this hasn't mattered yet.
- `drives::list_drives` lives in the Tauri app, not `scanner-core` —
  deliberately: enumerating OS volumes for a picker screen is a desktop-app
  concern the CLI has never needed, and adding it to `scanner-core` before
  a second consumer needed it would have been the same premature-generality
  mistake the "Design principles" discussion in `docs/HLD.md` calls out for
  the engine trait itself. It has no fallible path — `sysinfo` doesn't
  return a `Result` for disk listing — so the command wraps it in `Ok(...)`
  trivially rather than propagating an error that can't occur.
- `list_drives` takes no target and can't fail; it's the only command that
  isn't just "call a `ScanEngine` method."

## `frontend/ui` (plain HTML/CSS/JS, no bundler)

- `app.js` defines a single `api` object (`listDrives`/`scanSummary`/
  `scanChildren`) wrapping `window.__TAURI__.core.invoke` — the *only*
  place in the UI code that knows it's talking to Tauri specifically. Every
  other function operates on plain JS values already shaped like the
  Rust side's `Serialize` output (`camelCase` isn't needed since every
  field name here happens to already be a single word or already
  snake_case-compatible with JS property access).
- Drive cards (`renderDriveCard`) and result rows (`renderChildRow`) are
  pure render functions: given a `DriveInfo`/`ChildEntry` object, return a
  DOM element. Clicking a drive card calls `openResults(mount_point)`,
  which fires `scan_summary` and `scan_children` concurrently via
  `Promise.allSettled` (not `Promise.all` — one succeeding and the other
  failing is a real, expected case, e.g. the MFT engine failing unelevated
  while the walk engine's own call for the same path would still succeed
  if selected) and renders whichever settled.
- The size bar in each result row (`renderChildRow`'s `size-bar` width) is
  relative to the *largest* child's `allocated_size` in that same listing
  (`maxSize`, recomputed per `openResults` call) — not relative to the
  parent folder's total, so it visually ranks siblings against each other
  rather than showing what fraction of the parent each one is.
- `tauri.conf.json` sets `"withGlobalTauri": true` specifically so this
  plain-script UI can reach `window.__TAURI__` with no npm package, no
  bundler, and no build step — `frontendDist` points straight at `ui/` as
  static files Tauri serves as-is.

## Known edge cases and their current handling

| Case | Handling |
|---|---|
| Not elevated | MFT engine's `open_volume` returns `Err(OpenVolume)`; CLI prints `FAILED: ...` and continues to the walk engine. Not a panic, not a process-level failure. |
| Non-NTFS volume | `Ntfs::new` or later parsing returns an `Err` (surfaces as `ScanError::Ntfs` or `NotNtfs`); same non-fatal handling as above. |
| Corrupt/unused MFT record slot | Skipped in the per-record loop (`continue`), not counted, not an error. |
| Hardlinked file (multiple parents) | Counted once per distinct parent as a tree edge; de-duplicated by `visited` so its size is added exactly once per aggregate call, even if reachable twice in the same subtree. |
| 8.3 short-name alias (same parent) | Deduplicated via `namespace_rank`; does not produce a second entry. |
| Path component not found | `ScanError::PathNotFound`, surfaced as the MFT engine's `Err`. |
| Requested folder unreadable by walk engine (no permission) | Silently reported as `0 bytes` / no entries under it — not an error, not distinguishable from "genuinely empty" without independent knowledge. |
| `--max-records` set below `records_total` | Both scan reports flag this explicitly (`records_walked < records_total`) and the CLI prints a `** PARTIAL **` warning; sizes/counts are real for the subset walked but not the whole answer. |
| Malformed `--max-records` value | Parsed with `.and_then(|v| v.parse().ok())` — silently becomes "no cap" rather than erroring. |
| Target string `MftEngine` can't parse a drive letter from (e.g. a bare UNC path) | `ScanError::InvalidTarget`, raised lazily inside `MftEngine::scan`/`scan_children` rather than upfront by the caller — `WalkEngine` never rejects such a target, since it doesn't need a drive letter at all. |
