# Disk Segmentation

A disk-usage analyzer for Windows. Computes folder sizes by parsing the NTFS
`$MFT` directly instead of walking the directory tree file-by-file — the same
trick WizTree uses to beat Explorer/TreeSize by 10–100x on large volumes.

## Status

| Part               | State                                                                 |
|--------------------|------------------------------------------------------------------------|
| `engine/` (Rust)   | Working. MFT engine + directory-walk fallback, both with per-folder breakdown. |
| `frontend/`        | Not started. Directory only contains a stray `node_modules/`, no source. |

## Why parse the MFT instead of just walking the tree

`FindFirstFile`/`FindNextFile` — what every walker, including Explorer, uses —
does one syscall per directory. A large volume nests hundreds of thousands of
directories, so wall-clock time ends up dominated by syscall count, not disk
bandwidth.

NTFS already stores every file record — name, parent, size, directory flag —
in one system file, `$MFT`. Read it once, sequentially, and the whole tree is
in memory with zero per-folder syscalls. Cost scales with `$MFT` size, not
with how deep or wide the target path is.

## Build

The Rust toolchain is vendored in `.toolchain/`, not a system install:

```powershell
$env:CARGO_HOME = "..\.toolchain\cargo"
$env:RUSTUP_HOME = "..\.toolchain\rustup"
cd engine
cargo build --release
```

## Run

```
scanner-cli.exe <path> [--max-records N] [--children]
```

- No flags — one aggregate total for `<path>`: MFT engine vs. walk fallback, side by side.
- `--children` — size per *immediate* subfolder of `<path>` instead of one total.
- `--max-records N` — caps how many MFT records get walked, for fast iteration on logic; sizes/counts are then partial, not a real result.

The MFT engine needs raw volume read access — **run from an elevated
(Administrator) terminal**, or every call fails with `Access is denied`. The
walk fallback needs no elevation but silently skips anything it can't read
(see `docs/LLD.md`).

```powershell
# from an elevated terminal
.\scanner-cli.exe D:\ --children
```

## Tech stack

- Rust 1.98 (stable), 2021 edition, 2-crate Cargo workspace (`scanner-core` + `scanner-cli`)
- [`ntfs`](https://crates.io/crates/ntfs) 0.4 — `$MFT` record/attribute parsing
- [`jwalk`](https://crates.io/crates/jwalk) 0.8 — parallel directory walk (fallback engine)
- `thiserror` 2 — error types
- Raw Win32 volume handles (`\\.\D:`) via plain `std::fs::File`, no C FFI — sector alignment and MFT-record prefetch caching are hand-rolled

## Docs

- [`docs/OVERVIEW.md`](docs/OVERVIEW.md) — how it works, module by module, full tech stack
- [`docs/GLOSSARY.md`](docs/GLOSSARY.md) — NTFS/MFT terms and the request-to-output flow
- [`docs/HLD.md`](docs/HLD.md) — high-level design
- [`docs/LLD.md`](docs/LLD.md) — low-level design: structs, algorithms, edge cases
