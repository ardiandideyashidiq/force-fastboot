use crate::flash::error::FlashError;

/// Outcome of a single flash action.
#[derive(Debug)]
pub struct FlashOutcome {
    pub partition: String,
    pub success: bool,
    pub error: Option<FlashError>,
}

/// Overall result of executing a flash plan.
#[derive(Debug)]
pub struct FlashResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub outcomes: Vec<FlashOutcome>,
}

/// Outcome of a format-data operation on a single partition.
#[derive(Debug)]
pub struct FormatOutcome {
    pub partition: String,
    pub status: FormatStatus,
}

/// Per-partition format status.
#[derive(Debug)]
pub enum FormatStatus {
    /// Fully wiped and formatted with an empty filesystem.
    Wiped,
    /// Erased but not formatted (filesystem type not recognised).
    ErasedOnly(String),
    /// Skipped (partition does not exist or empty type).
    Skipped(String),
    /// Operation failed with the given error.
    Failed(FlashError),
}

/// Result of a full format-data run.
#[derive(Debug)]
pub struct FormatDataResult {
    pub outcomes: Vec<FormatOutcome>,
}
