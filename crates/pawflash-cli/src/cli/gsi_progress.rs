use indicatif::ProgressBar;

use pawflash_core::gsi::GsiEvent;
use pawflash_core::output::{self, spinner};

/// Tracks GSI workflow progress via a `MultiProgress` when not verbose.
/// When verbose, delegates to `tracing::info!` instead.
#[derive(Default)]
pub struct GsiProgress {
    current: Option<ProgressBar>,
}

impl GsiProgress {
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
                if let Some(pb) = self.current.take() {
                    pb.finish_and_clear();
                }
                self.current = Some(spinner::start(step.as_str()));
            }
            GsiEvent::ModeDetected(mode) => {
                let _ = spinner::print(&format!("  {} detected", mode.as_str()));
            }
            GsiEvent::ModeReady(mode) => {
                let _ = spinner::print(&format!("  ✓ ready in {}", mode.as_str()));
            }
            GsiEvent::ResolvedPartition { base, partition, size_bytes } => {
                let _ = spinner::print(&format!(
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
                let _ = spinner::print(&format!("  - skipped {partition}: {reason}"));
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
