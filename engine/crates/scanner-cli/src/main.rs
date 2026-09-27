use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use scanner_core::volume::split_drive_and_subpath;
use scanner_core::{scan_mft, scan_walk};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let Some(target) = args.get(1) else {
        eprintln!("usage: scanner-cli <path> [--max-records N]   e.g. scanner-cli C:\\Users");
        eprintln!("       (run from an elevated / Administrator terminal —");
        eprintln!("        the MFT engine needs raw volume read access)");
        eprintln!("       --max-records caps how many MFT records are walked, for fast");
        eprintln!("       iteration on logic/correctness — sizes/counts are then partial.");
        return ExitCode::FAILURE;
    };

    let max_records: Option<u64> = args
        .iter()
        .position(|a| a == "--max-records")
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok());

    let Some((drive, subpath)) = split_drive_and_subpath(target) else {
        eprintln!("could not parse a drive letter out of '{target}' (expected e.g. C:\\Users)");
        return ExitCode::FAILURE;
    };

    println!("Target: {drive}:\\{subpath}");
    println!();

    println!("--- MFT scan (reads all of {drive}:'s $MFT once; subpath size is a byproduct) ---");
    match scan_mft(drive, &subpath, max_records) {
        Ok(report) => {
            if report.records_walked < report.records_total {
                println!(
                    "  ** PARTIAL: only {} / {} total records walked (--max-records) — sizes/counts below are not the real answer **",
                    report.records_walked, report.records_total
                );
            }
            println!("  elapsed:         {:?}", report.elapsed);
            println!(
                "  records scanned: {} / {} walked ({} total MFT slots)",
                report.records_scanned, report.records_walked, report.records_total
            );
            println!("  files:           {}", report.file_count);
            println!("  dirs:            {}", report.dir_count);
            println!("  logical size:    {} bytes", report.total_logical);
            println!("  allocated size:  {} bytes", report.total_allocated);
        }
        Err(e) => {
            println!("  FAILED: {e}");
        }
    }

    println!();
    println!("--- Directory walk (multithreaded, target path only — fallback engine) ---");
    let walk_report = scan_walk(&PathBuf::from(target));
    println!("  elapsed:      {:?}", walk_report.elapsed);
    println!("  files:        {}", walk_report.file_count);
    println!("  dirs:         {}", walk_report.dir_count);
    println!("  logical size: {} bytes", walk_report.total_size);

    println!();
    println!("Note: the two numbers above are NOT a fair race unless <path> is a");
    println!("drive root (e.g. C:\\) — the MFT engine always reads the whole volume's");
    println!("$MFT, while the walk only covers the target path. For a small subfolder");
    println!("on a large, mostly-unrelated volume, the walk can win; the MFT approach");
    println!("wins once caching (build step 2) removes the per-run full-$MFT-read cost.");
    println!();
    println!("To compare against Explorer: right-click the same folder -> Properties");
    println!("and check its reported 'Size' against the numbers above (manual step).");

    ExitCode::SUCCESS
}
