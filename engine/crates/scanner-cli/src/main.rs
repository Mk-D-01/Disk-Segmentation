use std::env;
use std::process::ExitCode;

use scanner_core::{ChildEntry, MftEngine, ScanEngine, ScanReport, ScanTarget, WalkEngine};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let Some(target_path) = args.get(1) else {
        print_usage();
        return ExitCode::FAILURE;
    };

    let max_records: Option<u64> = args
        .iter()
        .position(|a| a == "--max-records")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());
    let children_mode = args.iter().any(|a| a == "--children");

    let target = ScanTarget::new(target_path.as_str());
    let engines: Vec<Box<dyn ScanEngine>> = vec![
        Box::new(MftEngine::with_max_records(max_records)),
        Box::new(WalkEngine),
    ];

    println!("Target: {target_path}");
    println!();

    for engine in &engines {
        let elevation_note = if engine.requires_elevation() {
            " (needs Administrator)"
        } else {
            " (no elevation needed)"
        };
        println!("--- {}{elevation_note} ---", engine_label(engine.id()));

        if children_mode {
            match engine.scan_children(&target) {
                Ok(report) => {
                    if let Some(note) = &report.note {
                        println!("  ** {note} **");
                    }
                    print_children_table(&report.children);
                }
                Err(e) => println!("  FAILED: {e}"),
            }
        } else {
            match engine.scan(&target) {
                Ok(report) => print_report(&report),
                Err(e) => println!("  FAILED: {e}"),
            }
        }
        println!();
    }

    if !children_mode {
        print_comparison_note();
    }

    ExitCode::SUCCESS
}

fn engine_label(id: &str) -> &'static str {
    match id {
        "mft" => "MFT scan (one read of the whole volume's $MFT; subpath size is a byproduct)",
        "walk" => "Directory walk (multithreaded, target path only — fallback engine)",
        _ => "Scan",
    }
}

fn print_usage() {
    eprintln!("usage: scanner-cli <path> [--max-records N] [--children]   e.g. scanner-cli C:\\Users");
    eprintln!("       (run from an elevated / Administrator terminal —");
    eprintln!("        the MFT engine needs raw volume read access)");
    eprintln!("       --max-records caps how many MFT records are walked, for fast");
    eprintln!("       iteration on logic/correctness — sizes/counts are then partial.");
    eprintln!("       --children lists each immediate subfolder's size instead of one total.");
}

fn print_report(report: &ScanReport) {
    if let Some(note) = &report.note {
        println!("  ** {note} **");
    }
    println!("  elapsed:        {} ms", report.elapsed_ms);
    println!("  files:          {}", report.file_count);
    println!("  dirs:           {}", report.dir_count);
    println!("  logical size:   {} bytes", report.logical_size);
    println!("  allocated size: {} bytes", report.allocated_size);
}

fn print_children_table(children: &[ChildEntry]) {
    if children.is_empty() {
        println!("  (no entries)");
        return;
    }
    // Already sorted by allocated_size descending — every ScanEngine
    // guarantees that ordering for scan_children.
    for c in children {
        let kind = if c.is_directory { "dir " } else { "file" };
        println!(
            "  [{kind}] {:>14} bytes   ({:>8} files, {:>6} dirs)   {}",
            c.allocated_size, c.file_count, c.dir_count, c.name
        );
    }
}

fn print_comparison_note() {
    println!("Note: the two numbers above are NOT a fair race unless <path> is a");
    println!("drive root (e.g. C:\\) — the MFT engine always reads the whole volume's");
    println!("$MFT, while the walk only covers the target path. For a small subfolder");
    println!("on a large, mostly-unrelated volume, the walk can win; the MFT approach");
    println!("wins once caching (build step 2) removes the per-run full-$MFT-read cost.");
    println!();
    println!("To compare against Explorer: right-click the same folder -> Properties");
    println!("and check its reported 'Size' against the numbers above (manual step).");
}
