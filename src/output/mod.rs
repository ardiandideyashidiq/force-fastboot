pub mod gsi_progress;
pub mod prompts;
pub mod spinner;
pub mod status;
pub mod tables;
pub mod theme;

use std::sync::OnceLock;

static VERBOSITY: OnceLock<u8> = OnceLock::new();

pub fn set_verbosity(count: u8) {
    _ = VERBOSITY.set(count);
}

pub fn verbosity() -> u8 {
    *VERBOSITY.get().unwrap_or(&0)
}
