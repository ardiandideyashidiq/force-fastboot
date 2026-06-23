use std::time::Duration;

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

use crate::gsi::types::GsiEvent;
use crate::output;

fn step_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {msg}")
        .unwrap()
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
}

/// Tracks GSI workflow progress via a `MultiProgress` when not verbose.
/// When verbose, delegates to `tracing::info!` instead.
pub struct GsiProgress {
    mp: MultiProgress,
    current: Option<ProgressBar>,
}

impl Default for GsiProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl GsiProgress {
    #[must_use]
    pub fn new() -> Self {
        Self {
            mp: MultiProgress::new(),
            current: None,
        }
    }

    /// Report a GSI event. When `-v` is active, logs via `tracing::info!`.
    /// Otherwise, shows a spinner-based step tracker via `MultiProgress`.
    pub fn report(&mut self, event: &GsiEvent) {
        if output::verbosity() >= 1 {
            match event {
                GsiEvent::Step(step) => tracing::info!("[gsi] {}", step.as_str()),
                GsiEvent::ModeDetected(mode) => tracing::info!("[gsi] detected mode: {}", mode.as_str()),
                GsiEvent::ModeReady(mode) => tracing::info!("[gsi] ready in mode: {}", mode.as_str()),
                GsiEvent::ResolvedPartition { base, partition, size_bytes } => {
                    tracing::info!("[gsi] resolved {base} -> {partition} ({size_bytes} bytes)");
                }
                GsiEvent::Flashing { partition, size_bytes } => {
                    tracing::info!("[gsi] flashing {partition} ({size_bytes} bytes)");
                }
                GsiEvent::Wiping { partition } => {
                    tracing::info!("[gsi] wiping {partition}");
                }
                GsiEvent::PartitionSkipped { partition, reason } => {
                    tracing::info!("[gsi] skipped {partition}: {reason}");
                }
            }
            return;
        }

        match event {
            GsiEvent::Step(step) => {
                // Clear the previous step's spinner
                if let Some(pb) = self.current.take() {
                    pb.finish_and_clear();
                }
                let pb = self.mp.add(ProgressBar::new_spinner());
                pb.set_style(step_style());
                pb.set_message(step.as_str().to_string());
                pb.enable_steady_tick(Duration::from_millis(80));
                self.current = Some(pb);
            }
            GsiEvent::ModeDetected(mode) => {
                let _ = self.mp.println(format!("  {} detected", mode.as_str()));
            }
            GsiEvent::ModeReady(mode) => {
                let _ = self.mp.println(format!("  ✓ ready in {}", mode.as_str()));
            }
            GsiEvent::ResolvedPartition { base, partition, size_bytes } => {
                let _ = self.mp.println(format!(
                    "  ✓ resolved {base} → {partition} ({size_bytes} bytes)",
                ));
            }
            GsiEvent::Flashing { partition, size_bytes } => {
                if let Some(pb) = &self.current {
                    pb.set_message(format!("flashing {partition} ({size_bytes} bytes)"));
                }
            }
            GsiEvent::Wiping { partition } => {
                if let Some(pb) = &self.current {
                    pb.set_message(format!("wiping {partition}"));
                }
            }
            GsiEvent::PartitionSkipped { partition, reason } => {
                let _ = self.mp.println(format!("  - skipped {partition}: {reason}"));
            }
        }
    }

    /// Finish the active step spinner and clear the display.
    pub fn finish(&mut self) {
        if let Some(pb) = self.current.take() {
            pb.finish_and_clear();
        }
    }
}

impl Drop for GsiProgress {
    fn drop(&mut self) {
        self.finish();
    }
}
