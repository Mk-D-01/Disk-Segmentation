//! Drive enumeration for the picker screen. Deliberately separate from
//! `scanner-core`: listing *which* volumes exist on this machine is a
//! desktop-app / OS-integration concern the CLI has never needed, whereas
//! `scanner-core` stays focused on scanning a volume it's already been
//! told about. If a second consumer ever needs this, it moves to
//! `scanner-core::volume` then — not before.

use serde::Serialize;
use sysinfo::Disks;

#[derive(Debug, Clone, Serialize)]
pub struct DriveInfo {
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub file_system: String,
    pub is_removable: bool,
}

/// Every mounted volume this process can see, sorted by mount point.
pub fn list_drives() -> Vec<DriveInfo> {
    let disks = Disks::new_with_refreshed_list();

    let mut drives: Vec<DriveInfo> = disks
        .iter()
        .map(|disk| DriveInfo {
            mount_point: disk.mount_point().to_string_lossy().to_string(),
            total_bytes: disk.total_space(),
            available_bytes: disk.available_space(),
            file_system: disk.file_system().to_string_lossy().to_string(),
            is_removable: disk.is_removable(),
        })
        .collect();

    drives.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    drives
}
