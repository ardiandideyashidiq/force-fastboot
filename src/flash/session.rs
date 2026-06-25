//! Shared download→send→flash helper.
//!
//! Consolidates the common pattern:
//! `fb.download(size) → sender.extend_from_slice(data) → sender.finish() → fb.flash(partition)`

use fastboot_protocol::nusb::{DataDownload, NusbFastBoot};

use crate::flash::error::Result;

/// Initiates a fastboot download and streams data.
///
/// Usage:
/// ```ignore
/// let mut dl = FlashDownload::begin(&mut fb, size).await?;
/// dl.extend(data).await?;
/// dl.finish().await?;
/// let resp = fb.flash("partition").await?;
/// ```
pub(crate) struct FlashDownload<'a> {
    sender: Option<DataDownload<'a>>,
}

impl<'a> FlashDownload<'a> {
    /// Start a fastboot download transaction for `total_size` bytes.
    pub async fn begin(fb: &'a mut NusbFastBoot, total_size: u32) -> Result<Self> {
        let sender = fb.download(total_size).await?;
        Ok(Self {
            sender: Some(sender),
        })
    }

    /// Append a slice of data to the download buffer.
    pub async fn extend(&mut self, data: &[u8]) -> Result<()> {
        self.sender
            .as_mut()
            .expect("FlashDownload already finished")
            .extend_from_slice(data)
            .await?;
        Ok(())
    }

    /// Finalize the download.
    /// Consumes `self` so the mutable borrow on `fb` is released.
    pub async fn finish(mut self) -> Result<()> {
        self.sender
            .take()
            .expect("FlashDownload already finished")
            .finish()
            .await?;
        Ok(())
    }
}
