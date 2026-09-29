# High-Level Design

## Scope

This covers the current codebase: a Rust workspace with two crates. There is
no frontend yet — this document treats that as a planned consumer, not a
built component.

## Components

```
┌─────────────────────────────────────────────────────────────────┐
│                          scanner-cli                            │
│  (binary crate — argv parsing, dispatch, stdout formatting)     │
└───────────────────────────────┬─────────────────────────────────┘
                                 │ depends on
                                 ▼
┌─────────────────────────────────────────────────────────────────┐
│                         scanner-core                            │
│  (library crate — everything reusable by a future frontend)     │
│                                                                   │
│  ┌───────────┐   ┌──────────────┐   ┌───────────┐   ┌─────────┐ │
│  │  volume   │──▶│ sector_reader│──▶│ prefetch_  │  │  error  │ │
│  │ (opens    │   │ (alignment)  │   │  reader    │  │(ScanErr │ │
│  │  \\.\D:)  │   │              │   │ (8MiB cache│  │ or type)│ │
│  └───────────┘   └──────────────┘   └───────────┘   └─────────┘ │
│         │                                    │                   │
│         ▼                                    ▼                   │
│  ┌────────────────────┐          ┌────────────────────┐         │
│  │     mft_scan        │          │     walk_scan       │        │
│  │ (raw $MFT parsing,   │          │ (jwalk directory     │       │
│  │  parent/child tree,  │          │  traversal, no        │       │
│  │  aggregation)        │          │  elevation needed)   │       │
│  └────────────────────┘          └────────────────────┘         │
└─────────────────────────────────────────────────────────────────┘
```

`sector_reader` and `prefetch_reader` are private (`mod`, not `pub mod`) —
they're implementation details of how `volume::open_volume` produces a
readable stream, not part of the crate's public surface. `error`, `volume`,
`mft_scan`, and `walk_scan` are `pub`.

## Responsibilities

| Component | Responsibility | Does *not* do |
|---|---|---|
| `scanner-cli` | Parse argv, call into `scanner-core`, format results as text | Any NTFS/filesystem logic itself |
| `volume` | Open a raw, elevation-gated volume handle; parse `C:\sub\path` into `('C', "sub\path")` | Interpret volume contents |
| `sector_reader` | Enforce sector-aligned reads/seeks on the raw handle | Caching (explicitly documented as unbuffered) |
| `prefetch_reader` | Cache a forward window so the MFT record-by-record access pattern doesn't become one syscall per record | Sector alignment (delegates down) |
| `mft_scan` | Parse `$MFT` into an in-memory record graph once; resolve a subpath in it; aggregate sizes bottom-up (whole-subtree or per-immediate-child) | Any I/O beyond the initial volume read; no caching across process runs |
| `walk_scan` | Multithreaded directory traversal as a comparison/fallback engine, whole-subtree or per-immediate-child | Raw volume access; anything requiring elevation |
| `error` | One error enum (`ScanError`) covering both engines | Recovery/retry logic — callers decide what to do with an `Err` |

## Data flow (per invocation)

1. **Input**: a Windows path string + optional flags.
2. **Two independent pipelines** run over the same target — MFT and walk —
   producing structurally identical report types (`MftScanReport` /
   `WalkScanReport`, or their `*BreakdownReport` counterparts). Neither
   pipeline depends on the other; `scanner-cli` runs them sequentially today
   purely because it prints one after the other, not because of a data
   dependency.
3. **Output**: printed to stdout as human-readable text. There is currently
   no machine-readable output format (no JSON/CSV) and no return value other
   than process exit code.

## Key architectural decisions

- **Library/binary split.** All engine logic lives in `scanner-core`, a
  library crate with no CLI or I/O-formatting concerns. `scanner-cli` is a
  thin consumer. This is the seam a future GUI frontend plugs into —
  it would depend on `scanner-core` directly rather than shelling out to the
  CLI.
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

## Planned/absent components (explicitly out of scope today)

- **Frontend** (`frontend/`): no source exists. `.gitignore` has a
  pre-staged WPF/.NET build-output section, which is the only hint of
  intended direction.
- **Caching layer**: mentioned in the CLI's own comparison output as
  "build step 2" — not implemented.
- **Structured output / API surface** for a frontend to consume
  programmatically instead of parsing stdout text.
