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


