# Project Overview

## What this is

A Windows disk-usage analyzer, currently just the analysis engine (no UI yet).
Given a path, it reports how much space each file and folder underneath it
actually uses, using two different strategies so they can be measured against
each other:

1. **MFT engine** — reads the NTFS master file table (`$MFT`) directly off the
   raw volume.
2. **Directory-walk engine** — a conventional, multithreaded recursive
   directory walk (`FindFirstFile`/`FindNextFile` under the hood). Also the
   only engine available on non-NTFS volumes (exFAT/FAT32 USB drives, etc.),
   and the only one that needs no elevation.

Both report the same shape of result — total size, file count, directory
count, either as one number for a path or broken down per immediate
subfolder — so their outputs are directly comparable.

## How it works, end to end

### 1. Entry point (`scanner-cli`)

`scanner-cli.exe <path> [--max-records N] [--children]` parses argv by hand
(no CLI-parsing crate — three flags didn't justify one), splits `<path>` into
a drive letter and a relative subpath (`volume::split_drive_and_subpath`),
and dispatches to one of two code paths in `main.rs`:

- default: run both engines once, print one aggregate total each.
- `--children`: run both engines' breakdown variant, print one row per
  immediate subfolder, sorted by size descending.

### 2. Opening the volume (`volume.rs`)

The MFT engine needs a raw handle to the volume device (`\\.\D:`), which
Windows only grants to an elevated process. Raw volume handles also only
permit **sector-aligned** reads and seeks — a bare read of 60 bytes at an
arbitrary offset fails with `ERROR_INVALID_PARAMETER`. Two wrapper layers sit
between the raw `File` and the NTFS parser to make that transparent:

- **`SectorReader`** (vendored unmodified from the `ntfs` crate's own
  example) rounds every read up to sector boundaries (4096 bytes here) and
  copies out just the requested slice.
- **`PrefetchReader`** sits on top, because `std::io::BufReader` doesn't work
  for this access pattern (see `docs/LLD.md`) — it caches an 8 MiB forward
  window and only touches the sector reader when a read actually falls
  outside it, turning ~1.4M tiny per-record reads into a few hundred real
  ones.

### 3. Parsing `$MFT` (`mft_scan.rs`)

`$MFT` is itself just file record 0 — the code opens it through the `ntfs`
crate, reads its unnamed `$DATA` attribute to find the *real* content size
(not the record-segment size, which is a fixed ~1024 bytes and describes
something else entirely), and derives how many record slots exist.

It then walks every record slot once (`build_tree`):
- Deleted/unused/corrupt slots error out per-record and are skipped, not
  fatal.
- Every `$FILE_NAME` attribute on a record is read. A record can have several
  — real hardlinks (different parents) are kept as distinct edges; same-parent
  duplicates (an 8.3 short-name alias next to the long Win32 name) are
  deduplicated by namespace preference.
- The result is two in-memory maps: every in-use record's size/type
  (`records`), and a parent → children adjacency list (`children`). This is
  the entire result of the one `$MFT` read — everything downstream is
  in-memory graph work, no further I/O.

### 4. Resolving the target and aggregating

`resolve_subpath` walks the requested path component-by-component through the
`children` map (case-insensitive, matching NTFS's own Win32-namespace
semantics) to find the target record. `aggregate` then does an iterative
(explicit-stack, not recursive — depth is bounded by heap, not call stack)
post-order sum of allocated/logical size and file/dir counts under that
record, de-duplicating any hardlink reachable via more than one edge in the
same subtree.

For `--children`, the same two maps are reused: instead of one `aggregate`
call over the whole target, each immediate child gets its own `aggregate`
call. This is why the breakdown mode costs no extra `$MFT` reads — the tree
was already fully in memory.

### 5. The walk fallback (`walk_scan.rs`)

`scan_walk` is a plain `jwalk::WalkDir` traversal summing `metadata().len()`
for every entry — no admin rights needed, but one syscall per directory, and
any entry that errors on `metadata()` (permission denied, mostly) is silently
skipped rather than counted. `scan_walk_children` runs one such walk per
immediate subdirectory of the target, so a system folder the process can't
read into shows up as `0 bytes`, not an error.

## Tech stack

| Layer | Choice | Why |
|---|---|---|
| Language | Rust, 2021 edition, stable 1.98 | Predictable performance for a syscall-count-sensitive workload; no GC pause; `std::fs`/Win32 raw-handle access without an FFI layer. |
| Build | Cargo workspace, 2 crates | `scanner-core` (library, the actual engines) is kept independent of `scanner-cli` (thin binary) so a future GUI frontend can depend on the same core without pulling in a CLI. |
| NTFS parsing | [`ntfs`](https://crates.io/crates/ntfs) 0.4 | Does the low-level `$MFT` record/attribute/data-run parsing; this project only supplies the raw sector-aligned reader it needs and the aggregation logic on top. |
| String handling | [`nt-string`](https://crates.io/crates/nt-string) 0.1 (`alloc` feature) | NTFS file names are UTF-16; required by `ntfs` for `NtfsFileName` decoding. |
| Directory walk | [`jwalk`](https://crates.io/crates/jwalk) 0.8 | Multithreaded fallback walker (pulls in `rayon` transitively for its internal work-stealing pool — not used directly by this project's own code). |
| Errors | [`thiserror`](https://crates.io/crates/thiserror) 2 | Derive-based error enum (`ScanError`) instead of hand-written `Display`/`Error` impls. |
| Toolchain | Vendored under `.toolchain/` (`cargo`, `rustup`), not a system install | Keeps the build reproducible regardless of what's installed machine-wide; see `.gitignore` — never commit this directory. |
| Frontend | Not started | `frontend/` currently holds only a stray `node_modules/` with no `package.json` or source — a placeholder, not a scaffold. |

## What's deliberately not here yet

- No caching between runs — every invocation re-reads the whole `$MFT`.
  The CLI's own output notes this is where the MFT engine's advantage grows
  once added.
- No tests.
- No UI. The `.gitignore` has a WPF/.NET section pre-staged, suggesting a
  native Windows UI is the intended direction, but nothing has been built.
