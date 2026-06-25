use std::time::Duration;

use serde::Serialize;

use crate::flash::error::FlashError;

fn serialize_duration<S: serde::Serializer>(d: &Duration, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_f64(d.as_secs_f64())
}

/// Outcome of a single flash action.
#[derive(Debug, Serialize)]
pub struct FlashOutcome {
    pub partition: String,
    pub success: bool,
    /// The device response message (e.g. "Flashing succeeded").
    pub response: Option<String>,
    /// Wall-clock duration of this flash operation (in seconds as f64).
    #[serde(serialize_with = "serialize_duration")]
    pub duration: Duration,
    pub error: Option<FlashError>,
}

/// Overall result of executing a flash plan.
#[derive(Debug, Serialize)]
pub struct FlashResult {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub outcomes: Vec<FlashOutcome>,
}

/// Outcome of a format-data operation on a single partition.
#[derive(Debug, Serialize)]
pub struct FormatOutcome {
    pub partition: String,
    pub status: FormatStatus,
}

/// Per-partition format status.
#[derive(Debug, Serialize)]
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
#[derive(Debug, Serialize)]
pub struct FormatDataResult {
    pub outcomes: Vec<FormatOutcome>,
}

/// Print format-data outcomes for each partition and return the number of
/// failures. Output is routed through `output::status` (indicatif when not
/// verbose, tracing when `-v` is active).
#[must_use]
pub fn print_format_results(result: &FormatDataResult) -> usize {
    for outcome in &result.outcomes {
        match &outcome.status {
            FormatStatus::Wiped => {
                crate::output::status::ok("OKAY", &outcome.partition);
            }
            FormatStatus::ErasedOnly(fs) => {
                crate::output::status::warn(
                    "WARN",
                    format!("{} (erased, unrecognised fs: {fs})", outcome.partition),
                );
            }
            FormatStatus::Skipped(reason) => {
                crate::output::status::dim(format!("  SKIP {} ({reason})", outcome.partition));
            }
            FormatStatus::Failed(e) => {
                tracing::warn!(partition = %outcome.partition, error = %e, "format failed");
                crate::output::status::fail("FAIL", format!("{} ({e})", outcome.partition));
            }
        }
    }

    result
        .outcomes
        .iter()
        .filter(|o| matches!(o.status, FormatStatus::Failed(_)))
        .count()
}
