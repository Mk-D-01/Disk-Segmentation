#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod drives;

use scanner_core::{ChildrenReport, MftEngine, ScanEngine, ScanReport, ScanTarget, WalkEngine};

use drives::DriveInfo;

/// Maps the engine id chosen in the UI to a concrete [`ScanEngine`]. This is
/// the *only* place in the app that knows the two engines by name — adding a
/// third engine means adding a match arm here, nowhere else.
fn engine_for(id: &str) -> Result<Box<dyn ScanEngine>, String> {
    match id {
        "mft" => Ok(Box::new(MftEngine::new())),
        "walk" => Ok(Box::new(WalkEngine)),
        other => Err(format!("unknown engine id '{other}'")),
    }
}

#[tauri::command]
fn list_drives() -> Vec<DriveInfo> {
    drives::list_drives()
}

#[tauri::command]
fn scan_summary(path: String, engine: String) -> Result<ScanReport, String> {
    let engine = engine_for(&engine)?;
    let target = ScanTarget::new(path);
    engine.scan(&target).map_err(|e| e.to_string())
}

#[tauri::command]
fn scan_children(path: String, engine: String) -> Result<ChildrenReport, String> {
    let engine = engine_for(&engine)?;
    let target = ScanTarget::new(path);
    engine.scan_children(&target).map_err(|e| e.to_string())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            list_drives,
            scan_summary,
            scan_children
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Tauri application");
}
