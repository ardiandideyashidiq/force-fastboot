use std::path::PathBuf;
use miette::Diagnostic;
use thiserror::Error;

#[derive(Error, Debug, Diagnostic)]
pub enum FlashError {
    #[error("no fastboot device found")]
    #[diagnostic(help("connect your device via USB and check that it is in fastboot mode"))]
    NoDevice,

    #[error("device mismatch: expected {expected}, got {actual}")]
    #[diagnostic(help("use --serial SERIAL to target the correct device"))]
    DeviceMismatch { expected: String, actual: String },

    #[error("fastboot protocol: {0}")]
    Protocol(#[from] fastboot_protocol::nusb::NusbFastBootError),

    #[error("failed to open fastboot device: {0}")]
    Open(#[from] fastboot_protocol::nusb::NusbFastBootOpenError),

    #[error("image not found: {0}")]
    #[diagnostic(help("verify the image path and --firmware-dir"))]
    ImageNotFound(PathBuf),

    #[error("image {name} too large ({image_size}) > partition size ({partition_size})")]
    #[diagnostic(severity(Warning))]
    ImageTooLarge { name: String, image_size: u64, partition_size: i64 },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("download error: {0}")]
    Download(#[from] fastboot_protocol::nusb::DownloadError),

    #[error("flash action failed: {partition}: {reason}")]
    ActionFailed { partition: String, reason: String },

    #[error("filesystem generator failed: {reason}")]
    #[diagnostic(help("the format tools may be corrupted; reinstall pawflash"))]
    GeneratorFailed { reason: String },

    #[error("failed to parse sparse image header")]
    SparseParseFailed,

    #[error("failed to split sparse image for download")]
    SparseSplitFailed,

    #[error("sparse image truncated: read {read} of {expected} bytes")]
    SparseTruncated { read: usize, expected: usize },
}

impl serde::Serialize for FlashError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, FlashError>;
