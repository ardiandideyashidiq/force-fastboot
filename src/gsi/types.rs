use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Fastboot mode: bootloader (fastboot) or userspace fastbootd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FastbootMode {
    Bootloader,
    Fastbootd,
}

impl FastbootMode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bootloader => "bootloader",
            Self::Fastbootd => "fastbootd",
        }
    }
}

/// Step in the GSI flash workflow (human-readable progress).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GsiStep {
    StartingBootloaderPhase,
    StartingFastbootdPhase,
    PreparingVbmetaFlash,
    FlashingVbmeta,
    CheckingSystemPartition,
    CheckingProductGsiFallback,
    GeneratingProductGsiImage,
    FlashingProductGsi,
    ProductGsiFallbackNotNeeded,
    FlashingSystemGsi,
    WipingUserdata,
    RebootingToBootloader,
    RebootingToFastbootd,
    GsiFlowComplete,
}

impl GsiStep {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StartingBootloaderPhase => "starting bootloader phase",
            Self::StartingFastbootdPhase => "starting fastbootd phase",
            Self::PreparingVbmetaFlash => "preparing vbmeta flash",
            Self::FlashingVbmeta => "flashing vbmeta",
            Self::CheckingSystemPartition => "checking system partition",
            Self::CheckingProductGsiFallback => "checking product GSI fallback",
            Self::GeneratingProductGsiImage => "generating product GSI image",
            Self::FlashingProductGsi => "flashing product GSI",
            Self::ProductGsiFallbackNotNeeded => "product GSI fallback not needed",
            Self::FlashingSystemGsi => "flashing system GSI",
            Self::WipingUserdata => "wiping userdata",
            Self::RebootingToBootloader => "rebooting to bootloader",
            Self::RebootingToFastbootd => "rebooting to fastbootd",
            Self::GsiFlowComplete => "GSI flow complete",
        }
    }
}

/// Progress events emitted during the GSI flash workflow.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GsiEvent {
    Step(GsiStep),
    ModeDetected(FastbootMode),
    ModeReady(FastbootMode),
    ResolvedPartition {
        base: &'static str,
        partition: String,
        size_bytes: u64,
    },
    Flashing {
        partition: String,
        size_bytes: u64,
    },
    Wiping {
        partition: String,
    },
    PartitionSkipped {
        partition: String,
        reason: String,
    },
}

/// Options for the GSI flash workflow.
#[derive(Debug, Clone)]
pub struct GsiFlashOptions {
    pub wipe_data: bool,
    pub cancel_token: Option<Arc<AtomicBool>>,
}

impl Default for GsiFlashOptions {
    fn default() -> Self {
        Self {
            wipe_data: true,
            cancel_token: None,
        }
    }
}

/// Summary statistics from a completed GSI flash.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GsiFlashSummary {
    pub flash_count: usize,
    pub wipe_count: usize,
    pub skipped_count: usize,
    pub total_bytes: u64,
}

/// Outcome of a GSI flash operation.
pub struct GsiFlashOutcome {
    pub summary: GsiFlashSummary,
}
