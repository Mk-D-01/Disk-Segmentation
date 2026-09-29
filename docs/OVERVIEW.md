# Project Overview

## What this is

A Windows disk-usage analyzer: a Rust scanning engine (`engine/`) plus a
Tauri desktop app (`frontend/`) that lets a user pick a drive and see its
folder-size breakdown. Given a path, the engine reports how much space each
file and folder underneath it actually uses, using two different strategies
so they can be measured against each other:

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

### 0. The `ScanEngine` trait — one interface, two implementations

`scanner-core::engine` defines a `ScanEngine` trait (`scan`, `scan_children`,
`requires_elevation`, `id`) plus two implementations: `MftEngine` and
`WalkEngine`. Every caller — the CLI and the Tauri app — talks to engines
only through this trait, holding them as `Box<dyn ScanEngine>` and looping
generically rather than hand-writing one call site per engine. `MftEngine`
and `WalkEngine` are thin adapters over `mft_scan`/`walk_scan`'s own richer,
engine-specific result types, translating them into the shared
`ScanReport`/`ChildEntry` shape (see `docs/LLD.md` for exact fields). Adding
a third engine means adding a new type that implements the trait — no
existing caller changes.

### 1. Entry points

Two consumers sit on top of `scanner-core`, both only through `ScanEngine`:

- **`scanner-cli`** — `scanner-cli.exe <path> [--max-records N]
  [--children]`, argv parsed by hand (no CLI-parsing crate — three flags
  didn't justify one). Builds a `Vec<Box<dyn ScanEngine>>` of both engines
  and loops over it, printing either one aggregate total each (default) or
  the per-immediate-subfolder breakdown (`--children`).
- **`frontend` (Tauri app)** — a desktop window (`src-tauri`) exposing three
  `#[tauri::command]`s (`list_drives`, `scan_summary`, `scan_children`) that
  a plain HTML/CSS/JS UI (`ui/`) calls into. See §6 below.

Neither consumer calls `mft_scan`/`walk_scan` directly — those modules are
`pub` for internal reuse by `engine.rs`, but the crate's intended surface is
`ScanEngine` and friends, re-exported at the crate root.

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

### 6. The frontend (`frontend/`)

A Tauri v2 desktop app, split the same way the engine is: `src-tauri/`
(Rust backend) and `ui/` (static HTML/CSS/JS — no npm, no bundler, no
build step; `tauri.conf.json`'s `withGlobalTauri: true` exposes
`window.__TAURI__.core.invoke` directly to plain `<script>` code).

- `src-tauri/src/main.rs` registers three commands. `list_drives` calls a
  small `drives` module (backed by the `sysinfo` crate) to enumerate mounted
  volumes with their used/free space for the picker screen — deliberately
  *not* part of `scanner-core`, since enumerating OS volumes is a desktop-app
  concern the CLI has never needed (see that module's own doc comment for
  the "move it once a second consumer needs it" reasoning). `scan_summary`
  and `scan_children` each take a `path` and an `engine` id (`"mft"` or
  `"walk"`), resolve it to a `Box<dyn ScanEngine>` via one match statement
  (`engine_for`, the *only* place in the app that knows the two engines by
  name), and call the trait method — errors are mapped to `String` for the
  IPC boundary rather than needing `ScanError` to be serializable.
- `ui/app.js` renders a drive-picker grid (`list_drives`), then a
  per-folder breakdown table (`scan_children`, sorted by size — the
  `ScanEngine` contract guarantees that ordering) for whichever drive is
  clicked, with a summary strip from `scan_summary` above it. An `api`
  object is the only thing in the UI code that knows it's talking to Tauri;
  everything else works against plain JS values.
- **The MFT engine option in the UI's engine picker will fail unless the
  whole app is running elevated** — there is no in-app elevation prompt
  yet, matching the CLI's own behavior. The walk engine (the UI's default)
  needs no elevation.
- **Deliberately not implemented**: any delete/cleanup action. The engine
  has no deletion capability at all — this build reports sizes, it doesn't
  free space. The UI says so directly (footer disclaimer) rather than
  implying a capability that isn't there.

## Tech stack

| Layer | Choice | Why |
|---|---|---|
| Language | Rust, 2021 edition, stable 1.98 | Predictable performance for a syscall-count-sensitive workload; no GC pause; `std::fs`/Win32 raw-handle access without an FFI layer. |
| Build | Two independent Cargo trees: `engine/` (workspace, 2 crates) and `frontend/src-tauri` (standalone crate with a path dependency on `../../engine/crates/scanner-core`) | `scanner-core` has no idea the frontend exists; the frontend depends *on* it, never the reverse. `scanner-cli` and the Tauri app are siblings, both consumers of the same library. |
| NTFS parsing | [`ntfs`](https://crates.io/crates/ntfs) 0.4 | Does the low-level `$MFT` record/attribute/data-run parsing; this project only supplies the raw sector-aligned reader it needs and the aggregation logic on top. |
| String handling | [`nt-string`](https://crates.io/crates/nt-string) 0.1 (`alloc` feature) | NTFS file names are UTF-16; required by `ntfs` for `NtfsFileName` decoding. |
| Directory walk | [`jwalk`](https://crates.io/crates/jwalk) 0.8 | Multithreaded fallback walker (pulls in `rayon` transitively for its internal work-stealing pool — not used directly by this project's own code). |
| Errors | [`thiserror`](https://crates.io/crates/thiserror) 2 | Derive-based error enum (`ScanError`) instead of hand-written `Display`/`Error` impls. |
| Serialization | [`serde`](https://crates.io/crates/serde) 1 (`derive`) | `ScanReport`/`ChildEntry` need `Serialize` to cross the Tauri IPC boundary into JS; added to `scanner-core` itself rather than wrapped at the app layer, since it's a property of the data, not the transport. |
| Desktop app shell | [`tauri`](https://crates.io/crates/tauri) 2 | Rust backend + OS-native WebView2 window; chosen specifically so the UI could depend on `scanner-core` directly instead of shelling out to `scanner-cli` and parsing text. |
| Drive enumeration | [`sysinfo`](https://crates.io/crates/sysinfo) 0.32 | Lists mounted volumes with used/free space for the picker screen — lives in the Tauri app, not `scanner-core` (see §6). |
| Frontend UI | Plain HTML/CSS/JS, no framework, no bundler | The previous attempt at a frontend left behind an npm `node_modules/` with no committed source (now removed) — this build deliberately has zero npm dependency. |
| Toolchain | Vendored under `.toolchain/` (`cargo`, `rustup`), not a system install; target is `stable-x86_64-pc-windows-gnu` (MinGW, not MSVC) | Keeps the build reproducible regardless of what's installed machine-wide; see `.gitignore` — never commit this directory. The GNU target means Windows resource (icon) compilation goes through `windres`, not `rc.exe` — MinGW (e.g. an MSYS2 UCRT64 install) must be on `PATH` for `frontend/src-tauri` to build. |

## What's deliberately not here yet

- No caching between runs — every invocation re-reads the whole `$MFT`.
  The CLI's own output notes this is where the MFT engine's advantage grows
  once added.
- No tests.
- No delete/cleanup action anywhere, engine or UI — this is an analyzer,
  not a cleaner, despite the project's name. See §6.
- No in-app elevation request — the MFT engine option in the UI fails with
  the same `Access is denied` the CLI shows unless the whole process was
  launched elevated.
