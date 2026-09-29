# Glossary & Project Flow

## Terms

**NTFS**
The default filesystem on Windows. Unlike FAT-family filesystems, it tracks
every file as a *record* in one central table rather than in scattered
directory index entries alone.

**`$MFT` (Master File Table)**
A system file, itself stored on the volume, containing one fixed-size record
per file/folder that has ever existed on the volume (including deleted ones,
until reused). Record 0 is `$MFT` describing itself; record 5 is the volume
root directory. This project reads it directly instead of asking Windows for
directory listings one folder at a time.

**MFT record / file record segment**
One fixed-size slot in `$MFT` (its size is stored in the filesystem's own
boot parameters — read via `ntfs.file_record_size()` — not necessarily
exactly 1024 bytes, though that's the common default). Holds a header plus
a sequence of *attributes* describing one file or folder. A record's own
segment size is **not** the size of the file it describes — see `$DATA`
below.

**Attribute**
A typed sub-structure inside an MFT record. The two this project reads:
- **`$FILE_NAME`** — one per hardlink; carries the name, parent directory
  reference, and cached size fields. A file can have more than one (see
  *namespace* below).
- **`$DATA`** — the actual file content stream. Its *unnamed* `$DATA`
  attribute is the file's primary content; `value_length()` on it is the
  real logical size of the file (or, for `$MFT` itself, the real size of the
  master file table — used to compute how many record slots exist).

**Namespace (`Win32`, `Dos`, `Posix`, `Win32AndDos`)**
NTFS can store more than one name for the same file under the same parent:
a long name and an auto-generated 8.3 short name. When a record has both for
the same parent, this project keeps only the higher-ranked one
(`Win32AndDos` > `Win32` > `Posix` > `Dos`) so a file isn't double-counted.

**Hardlink**
The *same* file record reachable from more than one parent directory, each
with its own `$FILE_NAME` attribute pointing to a different parent. Real on
Windows — heavily used by the WinSxS component store (e.g. most of
`C:\Windows\Fonts`). Each parent edge is kept; only same-parent duplicates
(the namespace case above) are collapsed. When aggregating, a hardlinked
record reachable twice within the same subtree is still only counted once
(size de-duplication via a `visited` set).

**Allocated size vs. logical size**
*Logical size* is how many bytes of actual content the file has (what
`$DATA`'s `value_length()` reports). *Allocated size* is how much disk space
is reserved for it, rounded up to cluster size — always ≥ logical size. Both
are reported because they answer different questions ("how much data is
there" vs. "how much space would I get back by deleting it").

**Raw volume handle**
An open handle to an entire volume device (`\\.\D:`) rather than to a file
on it — the only way to read `$MFT` directly instead of through the normal
filesystem API. Windows requires the opening process to be elevated
(Administrator).

**Sector alignment**
Raw volume handles only permit reads and seeks that start and end on sector
boundaries (4096 bytes here). A naive small unaligned read fails outright.

**Prefetch / cache window**
Reading one MFT record at a time from the raw device, unbuffered, would be
one real disk read per ~1024-byte record — for a volume with millions of
records, that's minutes of overhead alone. This project instead pulls a
large forward-biased chunk (8 MiB) into memory at a time and serves reads
from it until a request falls outside the cached window.

**Post-order aggregation**
To get a folder's total size, every descendant must be summed *before* the
total is known — children before parents. This project does that
iteratively with an explicit stack rather than recursively, so it isn't
bounded by call-stack depth on a very deeply nested tree.

## Project flow

```
scanner-cli.exe D:\Some\Path --children
        │
        ▼
1. Parse argv: target path, --max-records, --children flag
        │
        ▼
2. Split "D:\Some\Path" -> drive='D', subpath="Some\Path"
        │
        ▼
3a. MFT engine path                         3b. Walk engine path
    (needs Administrator)                       (no elevation needed)
    │                                            │
    ├─ open \\.\D: (raw volume handle)           ├─ std::fs::read_dir on the
    ├─ wrap in SectorReader (4096-byte            │  target path directly
    │  aligned reads)                             ├─ for each entry: if it's a
    ├─ wrap in PrefetchReader (8 MiB cache)        │  directory, recurse a full
    ├─ parse $MFT's own $DATA attribute            │  jwalk walk under it and
    │  -> total record count                       │  sum sizes; if a file,
    ├─ walk every record slot once:                │  read its size directly
    │    - skip unused/corrupt slots               │
    │    - read $FILE_NAME attribute(s)            │
    │    - dedupe same-parent aliases,             │
    │      keep distinct hardlink parents          │
    │    - store size + type per record            │
    │    - store parent -> child edges             │
    ├─ resolve "Some\Path" through the              │
    │  parent/child map to a target record          │
    └─ per immediate child of the target:           └─ per immediate child of
       run post-order aggregate over the                the target: aggregate
       already-built maps (no extra I/O)                that child's own subtree
        │                                            │
        ▼                                            ▼
4. Sort each engine's children by size, descending
        │
        ▼
5. Print both tables to stdout for comparison
```

Two things this flow makes concrete:
- The MFT engine pays for parsing the *entire* volume's `$MFT` exactly once
  per run, regardless of how deep or narrow the target subpath is — that
  cost is fixed, and everything after it (subpath resolution, per-child
  breakdown) is free in-memory graph traversal.
- The walk engine pays proportional to the target subtree's own size and
  depth, with no fixed up-front cost — it can beat the MFT engine on a small
  folder inside a huge, otherwise-irrelevant volume, and lose badly on a
  drive-root scan. The CLI's own default-mode output says this explicitly.
