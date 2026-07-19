use std::collections::HashMap;

use async_trait::async_trait;

use crate::flash::error::FlashError;
use crate::flash::error::Result;

/// Sender handle returned by [`FlashTransport::download`].
///
/// The real variant wraps a device `DataDownload`; the mock variant
/// is only compiled in test builds; the simulated variant is used
/// by [`SimulatedTransport`](crate::flash::simulate::SimulatedTransport).
pub enum DownloadSender<'s> {
    Real(fastboot_protocol::nusb::DataDownload<'s>),
    #[cfg(test)]
    Mock(super::mock::MockDownloadSink),
    Simulated(super::simulate::SimulatedDownloadSink),
}

impl DownloadSender<'_> {
    /// Send data to the device.
    ///
    /// # Errors
    /// Returns an error if the data transfer fails.
    pub async fn extend_from_slice(&mut self, data: &[u8]) -> Result<()> {
        match self {
            Self::Real(inner) => inner.extend_from_slice(data).await.map_err(FlashError::from),
            #[cfg(test)]
            Self::Mock(inner) => inner.extend_from_slice(data).await,
            Self::Simulated(inner) => inner.extend_from_slice(data).await,
        }
    }

    /// Finalise the download.
    ///
    /// # Errors
    /// Returns an error if the download finalisation fails.
    pub async fn finish(self) -> Result<()> {
        match self {
            Self::Real(inner) => inner.finish().await.map_err(FlashError::from),
            #[cfg(test)]
            Self::Mock(inner) => inner.finish().await,
            Self::Simulated(inner) => inner.finish().await,
        }
    }
}

/// Abstract fastboot transport for testability.
///
/// The default implementation delegates to [`NusbFastBoot`]; tests
/// can use [`MockTransport`](super::mock::MockTransport) instead.
#[async_trait]
pub trait FlashTransport {
    async fn get_var(&mut self, var: &str) -> Result<String>;
    async fn get_all_vars(&mut self) -> Result<HashMap<String, String>>;
    async fn download(&mut self, size: u32) -> Result<DownloadSender<'_>>;
    async fn flash(&mut self, partition: &str) -> Result<String>;
    async fn erase(&mut self, partition: &str) -> Result<String>;
    async fn reboot(&mut self) -> Result<String>;
    async fn reboot_to(&mut self, target: &str) -> Result<String>;
    async fn is_logical(&mut self, partition: &str) -> Result<bool>;
    async fn resize_logical_partition(&mut self, partition: &str, size: u64) -> Result<()>;
    async fn flashing(&mut self, cmd: &str) -> Result<String>;
    async fn set_active(&mut self, slot: &str) -> Result<String>;
    async fn snapshot_update(&mut self, cmd: &str) -> Result<String>;
}

#[async_trait]
impl FlashTransport for fastboot_protocol::nusb::NusbFastBoot {
    async fn get_var(&mut self, var: &str) -> Result<String> {
        self.get_var(var).await.map_err(FlashError::from)
    }

    async fn get_all_vars(&mut self) -> Result<HashMap<String, String>> {
        self.get_all_vars().await.map_err(FlashError::from)
    }

    async fn download(&mut self, size: u32) -> Result<DownloadSender<'_>> {
        let inner = fastboot_protocol::nusb::NusbFastBoot::download(self, size).await?;
        Ok(DownloadSender::Real(inner))
    }

    async fn flash(&mut self, partition: &str) -> Result<String> {
        self.flash(partition).await.map_err(FlashError::from)
    }

    async fn erase(&mut self, partition: &str) -> Result<String> {
        self.erase(partition).await.map_err(FlashError::from)
    }

    async fn reboot(&mut self) -> Result<String> {
        fastboot_protocol::nusb::NusbFastBoot::reboot(self).await.map_err(FlashError::from)
    }

    async fn reboot_to(&mut self, target: &str) -> Result<String> {
        self.reboot_to(target).await.map_err(FlashError::from)
    }

    async fn is_logical(&mut self, partition: &str) -> Result<bool> {
        self.is_logical(partition).await.map_err(FlashError::from)
    }

    async fn resize_logical_partition(&mut self, partition: &str, size: u64) -> Result<()> {
        self.resize_logical_partition(partition, size).await.map_err(FlashError::from).map(drop)
    }

    async fn flashing(&mut self, cmd: &str) -> Result<String> {
        self.flashing(cmd).await.map_err(FlashError::from)
    }

    async fn set_active(&mut self, slot: &str) -> Result<String> {
        self.set_active(slot).await.map_err(FlashError::from)
    }

    async fn snapshot_update(&mut self, cmd: &str) -> Result<String> {
        self.snapshot_update(cmd).await.map_err(FlashError::from)
    }
}
