use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("failed to open volume \\\\.\\{0}: (are you running as Administrator?): {1}")]
    OpenVolume(char, std::io::Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("NTFS parse error: {0}")]
    Ntfs(#[from] ntfs::NtfsError),

    #[error("path not found in MFT tree: {0}")]
    PathNotFound(PathBuf),

    #[error("volume does not look like NTFS (or record could not be parsed): {0}")]
    NotNtfs(String),

    #[error("'{0}' is not a path this engine can scan")]
    InvalidTarget(String),
}

pub type Result<T> = std::result::Result<T, ScanError>;
