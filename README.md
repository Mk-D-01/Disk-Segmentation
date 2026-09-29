# Disk Segmentation

A disk-usage analyzer for Windows. Computes folder sizes by parsing the NTFS
`$MFT` directly instead of walking the directory tree file-by-file — the same
trick WizTree uses to beat Explorer/TreeSize by 10–100x on large volumes.

## Status

| Part               | State                                                                 |
|--------------------|------------------------------------------------------------------------|
| `engine/` (Rust)   | Working. MFT engine + directory-walk fallback, both with per-folder breakdown, unified behind a `ScanEngine` trait. |
| `frontend/`        | Working. Tauri desktop app — pick a drive, see its folder-size breakdown. Analysis only, no delete/cleanup. |

## Why parse the MFT instead of just walking the tree

`FindFirstFile`/`FindNextFile` — what every walker, including Explorer, uses —
does one syscall per directory. A large volume nests hundreds of thousands of
directories, so wall-clock time ends up dominated by syscall count, not disk
bandwidth.

NTFS already stores every file record — name, parent, size, directory flag —
in one system file, `$MFT`. Read it once, sequentially, and the whole tree is
in memory with zero per-folder syscalls. Cost scales with `$MFT` size, not
with how deep or wide the target path is.

## Quick start

`start.ps1` sets up the vendored toolchain and runs either the app or the
CLI, in release mode:

```powershell
.\start.ps1                            # launches the desktop app
.\start.ps1 -Target app -Elevate       # ...so the MFT engine option works too
.\start.ps1 -Target cli D:\ --children # runs the CLI engine directly
```

The MFT engine needs raw volume read access and fails with `Access is
denied` unless the process is elevated (Administrator) — `-Elevate`
relaunches the script elevated for you. The walk engine/fallback needs no
elevation but silently skips anything it can't read (see `docs/LLD.md`).

## Build manually (without `start.ps1`)

The Rust toolchain is vendored in `.toolchain/`, not a system install.
Building `frontend/` also needs MinGW's `windres` on `PATH` (its
tauri-build step embeds a Windows icon resource):

```powershell
$env:CARGO_HOME = "..\.toolchain\cargo"
$env:RUSTUP_HOME = "..\.toolchain\rustup"

cd engine
cargo build --release          # scanner-cli.exe

cd ..\frontend\src-tauri
cargo build --release          # disk-segmentation-app.exe
```

`scanner-cli.exe <path> [--max-records N] [--children]`:
- No flags — one aggregate total for `<path>`: MFT engine vs. walk fallback, side by side.
- `--children` — size per *immediate* subfolder of `<path>` instead of one total.
- `--max-records N` — caps how many MFT records get walked, for fast iteration on logic; sizes/counts are then partial, not a real result.

## Tech stack

- Rust 1.98 (stable), 2021 edition — `engine/` (2-crate workspace: `scanner-core` + `scanner-cli`) and `frontend/src-tauri` (standalone, depends on `scanner-core` by path)
- [`ntfs`](https://crates.io/crates/ntfs) 0.4 — `$MFT` record/attribute parsing
- [`jwalk`](https://crates.io/crates/jwalk) 0.8 — parallel directory walk (fallback engine)
- `thiserror` 2 / `serde` 1 — error types / IPC-serializable report types
- Raw Win32 volume handles (`\\.\D:`) via plain `std::fs::File`, no C FFI — sector alignment and MFT-record prefetch caching are hand-rolled
- [`tauri`](https://crates.io/crates/tauri) 2 — desktop window (WebView2) around `scanner-core`, called directly, not shelled out to
- Plain HTML/CSS/JS UI (`frontend/ui`), no npm/bundler
- [`sysinfo`](https://crates.io/crates/sysinfo) 0.32 — drive enumeration for the picker screen

## Docs

- [`docs/OVERVIEW.md`](docs/OVERVIEW.md) — how it works, module by module, full tech stack
- [`docs/GLOSSARY.md`](docs/GLOSSARY.md) — NTFS/MFT terms and the request-to-output flow
- [`docs/HLD.md`](docs/HLD.md) — high-level design
- [`docs/LLD.md`](docs/LLD.md) — low-level design: structs, algorithms, edge cases
