use crate::flash::results::{FormatDataResult, FormatStatus};

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
