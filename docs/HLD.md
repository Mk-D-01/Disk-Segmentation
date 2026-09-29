# High-Level Design

## Scope

This covers the current codebase: the `engine/` Rust workspace (two crates)
and the `frontend/` Tauri app, which is a real, built consumer of
`scanner-core` as of this revision — not a planned/hypothetical one.

## Components

```
┌───────────────────────────┐   ┌───────────────────────────────────┐
│        scanner-cli        │   │      frontend/src-tauri            │
│  (binary — argv parsing,  │   │  (Tauri commands: list_drives,     │
│   loops over engines,     │   │   scan_summary, scan_children)     │
│   stdout formatting)      │   │            │                        │
└─────────────┬─────────────┘   │            ▼                        │
              │                 │  frontend/ui (HTML/CSS/JS,          │
              │                 │   no bundler, window.__TAURI__)     │
              │                 └────────────┬────────────────────────┘
              │ depends on                    │ depends on
              ▼                                ▼
┌─────────────────────────────────────────────────────────────────┐
│                         scanner-core                             │
│  (library crate — the only thing either consumer talks to)       │
│                                                                    │
│              ┌──────────────────────────────┐                    │
│              │            engine             │                    │
│              │  ScanEngine trait + ScanTarget/│                   │
│              │  ScanReport/ChildEntry +       │                   │
│              │  MftEngine, WalkEngine adapters │                   │
│              └───────┬───────────────┬────────┘                   │
│                      ▼               ▼                            │
│  ┌────────────────────┐          ┌────────────────────┐          │
│  │     mft_scan        │          │     walk_scan       │         │
│  │ (raw $MFT parsing,   │          │ (jwalk directory     │        │
│  │  parent/child tree,  │          │  traversal, no        │       │
│  │  aggregation)        │          │  elevation needed)   │       │
│  └──────────┬───────────┘          └────────────────────┘         │
│             ▼                                                      │
│  ┌───────────┐   ┌──────────────┐   ┌───────────┐   ┌─────────┐  │
│  │  volume   │──▶│ sector_reader│──▶│ prefetch_  │  │  error  │  │
│  │ (opens    │   │ (alignment)  │   │  reader    │  │(ScanErr │  │
│  │  \\.\D:)  │   │              │   │ (8MiB cache│  │ or type)│  │
│  └───────────┘   └──────────────┘   └───────────┘   └─────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

`sector_reader` and `prefetch_reader` are private (`mod`, not `pub mod`) —
implementation details of how `volume::open_volume` produces a readable
stream. `mft_scan` and `walk_scan` are `pub` (for `engine`'s own use and
direct access if ever needed) but are no longer the crate's *intended*
surface — `error`, `volume`, and `engine` (re-exported at the crate root)
are. Neither `scanner-cli` nor `frontend/src-tauri` calls `mft_scan`/
`walk_scan` directly; both go through `ScanEngine` only.

`frontend/src-tauri` also has its own small `drives` module (volume
enumeration for the picker screen, via `sysinfo`) that is deliberately
**not** part of `scanner-core` — see Responsibilities below.

## Responsibilities

| Component | Responsibility | Does *not* do |
|---|---|---|
| `scanner-cli` | Parse argv, hold a `Vec<Box<dyn ScanEngine>>`, loop over it, format results as text | Any NTFS/filesystem logic itself; know engine internals |
| `frontend/src-tauri` | Expose `ScanEngine` calls as Tauri commands (`engine_for` maps an id string to a boxed engine); enumerate drives for the picker | Any NTFS/filesystem logic itself; any UI rendering (that's `ui/`) |
| `frontend/ui` | Render the drive picker and results table from whatever the commands return; know it's talking to Tauri only through the `api` object | Any scanning logic, any knowledge of NTFS/MFT |
| `engine` | Define `ScanEngine` + the shared `ScanReport`/`ChildEntry` shape; adapt `mft_scan`/`walk_scan`'s native types into it | Any parsing itself — pure adapter/dispatch layer |
| `volume` | Open a raw, elevation-gated volume handle; parse `C:\sub\path` into `('C', "sub\path")` | Interpret volume contents |
| `sector_reader` | Enforce sector-aligned reads/seeks on the raw handle | Caching (explicitly documented as unbuffered) |
| `prefetch_reader` | Cache a forward window so the MFT record-by-record access pattern doesn't become one syscall per record | Sector alignment (delegates down) |
| `mft_scan` | Parse `$MFT` into an in-memory record graph once; resolve a subpath in it; aggregate sizes bottom-up (whole-subtree or per-immediate-child) | Any I/O beyond the initial volume read; no caching across process runs |
| `walk_scan` | Multithreaded directory traversal as a comparison/fallback engine, whole-subtree or per-immediate-child | Raw volume access; anything requiring elevation |
| `error` | One error enum (`ScanError`) covering both engines | Recovery/retry logic — callers decide what to do with an `Err` |

## Data flow (per invocation)

**CLI**: a Windows path string + optional flags → a `Vec<Box<dyn
ScanEngine>>` (MFT + walk) run over the same target, each producing a
`ScanReport` or `Vec<ChildEntry>` → printed to stdout as human-readable
text. No machine-readable output format (no JSON/CSV) and no return value
other than process exit code. The two engines don't depend on each other;
the CLI runs them sequentially purely because it prints one after the
other.

**Frontend**: the UI calls `list_drives` to render the picker → user clicks
a drive → the UI calls `scan_summary` and `scan_children` with that path
and whichever engine is selected → the Rust side resolves the engine id,
calls the trait methods, and returns `ScanReport`/`Vec<ChildEntry>`
serialized as JSON over Tauri's IPC → the UI renders them. This is the same
`ScanReport`/`ChildEntry` data the CLI prints as text — the frontend just
gets it as structured data instead of parsing stdout, which was the entire
point of the library/binary split.

## Key architectural decisions

- **Library/binary split, now with two real consumers.** All engine logic
  lives in `scanner-core`, a library crate with no CLI or I/O-formatting
  concerns. `scanner-cli` and `frontend/src-tauri` are both thin consumers
  that depend on `scanner-core` directly — the frontend never shells out to
  the CLI or parses its text output.
- **The `ScanEngine` trait exists *because* a second consumer showed up.**
  Before the frontend, `scanner-cli` called `scan_mft`/`scan_walk` as plain
  functions directly — fine for one consumer. Adding the frontend was the
  trigger to introduce the trait: without it, the Tauri command layer would
  have needed its own second copy of the "which engine, which report shape"
  dispatch logic the CLI already had. This is deliberately *not* done
  speculatively — see "Design principles" below for why introducing it
  earlier, for one consumer, would have been premature.
- **Two engines, not one.** The MFT engine is the actual point of the
  project (raw-metadata reads to avoid per-directory syscalls), but it only
  works on NTFS and only when elevated. The walk engine exists both as a
  correctness/performance baseline to validate the MFT engine's numbers
  against, and as the real fallback for non-NTFS volumes or non-elevated
  runs.
- **One `$MFT` read serves both "total" and "per-folder" queries.** The
  breakdown mode (`--children`) does not re-scan `$MFT`; it reuses the same
  in-memory record/edge maps built once per invocation and aggregates from a
  different set of starting points. See `docs/LLD.md` for how.
- **Errors are per-record, not per-run, inside the MFT walk.** A single
  corrupt or in-flux record does not abort the scan — only a failure at
  volume-open or `$MFT`-parse time (before the per-record loop starts) is
  a hard `Err` for the whole call.
- **No cross-run caching (yet).** Every invocation re-reads the whole
  `$MFT` from scratch. The CLI's own printed output flags this as the
  planned next step for the MFT engine's advantage to compound.

## Non-functional characteristics

- **Performance profile**: MFT engine cost ≈ O(`$MFT` size), independent of
  target path depth/width. Walk engine cost ≈ O(target subtree size). Neither
  is universally faster — see `docs/GLOSSARY.md`'s closing note.
- **Security/privilege boundary**: the MFT engine requires Administrator
  because raw volume handles are privilege-gated by Windows itself, not by
  any policy this project enforces. The walk engine deliberately needs no
  elevation and is safe to run as a normal user, at the cost of silently
  under-reporting anything it lacks permission to read into.
- **Portability**: raw volume access (`\\.\D:`) and the elevation model are
  Windows-specific; this is a Windows-only tool by design, not an
  accidental limitation.

## Design principles

Where this codebase does and doesn't follow SOLID/GoF, concretely, as of
the `engine.rs` refactor and the frontend landing:

- **Open/Closed, now actually demonstrated.** Before this revision, adding
  a third engine or a second consumer meant editing `scanner-cli::main`'s
  hardcoded call sites. Now: `scanner-cli::main` and
  `frontend/src-tauri::main` both hold/construct `Box<dyn ScanEngine>` and
  never branch on which concrete engine they have except in one place each
  (`engine_for` in the Tauri app). A third engine is a new file implementing
  the trait; neither existing consumer's loop changes.
- **Dependency Inversion, now real.** Both consumers depend on the
  `ScanEngine` trait (an abstraction defined by `scanner-core`), not on
  `mft_scan`/`walk_scan`'s concrete functions. Before, `scanner-cli`
  depended directly on `scan_mft`/`scan_walk`.
- **Adapter pattern (GoF), intentional.** `MftEngine`/`WalkEngine` in
  `engine.rs` exist specifically to adapt `mft_scan`/`walk_scan`'s
  differently-shaped native report types to the one shape `ScanEngine`
  promises. This is the same motivation as the pre-existing `Decorator`
  usage in `PrefetchReader<SectorReader<File>>` (see `docs/LLD.md`) — reach
  for a named pattern when it solves a real shape mismatch, not by default.
- **Why this wasn't done from the start.** With exactly one consumer
  (`scanner-cli`) and two engines that were always meant to run
  side-by-side for comparison (not swapped at runtime), a trait
  abstraction would have been speculative generality — SOLID/GoF applied
  because they're "best practice" rather than because anything needed
  them yet. The trigger that justified it was a second real consumer
  (the frontend) needing the same "which engine, which shape" dispatch the
  CLI had already hardcoded once.
- **Where it's still thin.** `ScanTarget` is just a path string; each
  engine re-parses/validates it independently (`MftEngine` via
  `split_drive_and_subpath`, `WalkEngine` via `Path::new`). That's
  intentional — a shared parsed-target type would need to know about
  drive letters even for the walk engine, which doesn't care about them —
  but it does mean an invalid path isn't rejected until the specific
  engine tries to use it.

## Planned/absent components (explicitly out of scope today)

- **Caching layer**: mentioned in the CLI's own comparison output as
  "build step 2" — not implemented.
- **In-app elevation.** The frontend's MFT engine option fails with the
  same `Access is denied` the CLI shows unless the whole process is
  launched elevated; no UAC re-launch flow exists.
- **Any delete/cleanup capability**, engine or UI. Both are analysis-only
  today.
- **Tests.** Neither `scanner-core` nor the frontend has an automated test
  suite.
