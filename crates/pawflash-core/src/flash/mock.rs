use std::collections::HashMap;

use async_trait::async_trait;

use crate::flash::error::FlashError;
use crate::flash::error::Result;
use crate::flash::transport::DownloadSender;
use crate::flash::transport::FlashTransport;

/// A mock download sink used by [`MockTransport`].
pub struct MockDownloadSink {
    data: Vec<u8>,
    finished: bool,
}

impl MockDownloadSink {
    pub(crate) fn new() -> Self {
        Self { data: Vec::new(), finished: false }
    }

    pub(crate) async fn extend_from_slice(&mut self, data: &[u8]) -> Result<()> {
        tokio::task::yield_now().await;
        self.data.extend_from_slice(data);
        Ok(())
    }

    pub(crate) async fn finish(mut self) -> Result<()> {
        tokio::task::yield_now().await;
        self.finished = true;
        Ok(())
    }
}

/// Mock fastboot transport for unit tests.
pub(crate) struct MockTransport {
    get_var_responses: HashMap<String, Result<String>>,
    pub(crate) commands: Vec<String>,
    pub(crate) fail_download: bool,
    flash_response: Option<String>,
}

impl MockTransport {
    #[must_use]
    pub fn new() -> Self {
        let mut get_var_responses: HashMap<String, Result<String>> = HashMap::new();
        get_var_responses.insert("max-download-size".to_string(), Ok("0x10000000".to_string()));
        Self { get_var_responses, commands: Vec::new(), fail_download: false, flash_response: None }
    }

    pub fn commands(&self) -> &[String] { &self.commands }
}

impl Default for MockTransport { fn default() -> Self { Self::new() } }

#[async_trait]
impl FlashTransport for MockTransport {
    async fn get_var(&mut self, var: &str) -> Result<String> {
        self.commands.push(format!("get_var:{var}"));
        self.get_var_responses
            .get(var)
            .map_or_else(
                || Err(FlashError::ActionFailed {
                    partition: var.to_string(),
                    reason: "mock: no response configured".into(),
                }),
                |v| match v {
                    Ok(s) => Ok(s.clone()),
                    Err(e) => Err(FlashError::ActionFailed {
                        partition: var.to_string(),
                        reason: e.to_string(),
                    }),
                },
            )
    }

    async fn get_all_vars(&mut self) -> Result<HashMap<String, String>> {
        self.commands.push("get_all_vars".to_string());
        Ok(self
            .get_var_responses
            .iter()
            .filter_map(|(k, v)| v.as_ref().ok().map(|v| (k.clone(), v.clone())))
            .collect())
    }

    async fn download(&mut self, size: u32) -> Result<DownloadSender<'_>> {
        self.commands.push(format!("download:{size}"));
        if self.fail_download {
            return Err(FlashError::ActionFailed {
                partition: "(download)".into(),
                reason: "mock download failure".into(),
            });
        }
        Ok(DownloadSender::Mock(MockDownloadSink::new()))
    }

    async fn flash(&mut self, partition: &str) -> Result<String> {
        self.commands.push(format!("flash:{partition}"));
        Ok(self.flash_response.clone().unwrap_or_else(|| format!("OKAY flashing {partition}")))
    }

    async fn erase(&mut self, partition: &str) -> Result<String> {
        self.commands.push(format!("erase:{partition}"));
        Ok(format!("OKAY erased {partition}"))
    }

    async fn reboot(&mut self) -> Result<String> {
        self.commands.push("reboot".to_string());
        Ok("OKAY".to_string())
    }

    async fn reboot_to(&mut self, target: &str) -> Result<String> {
        self.commands.push(format!("reboot_to:{target}"));
        Ok("OKAY".to_string())
    }

    async fn is_logical(&mut self, partition: &str) -> Result<bool> {
        self.commands.push(format!("is_logical:{partition}"));
        Ok(false)
    }

    async fn resize_logical_partition(&mut self, partition: &str, _size: u64) -> Result<()> {
        self.commands.push(format!("resize_logical:{partition}"));
        Ok(())
    }

    async fn flashing(&mut self, cmd: &str) -> Result<String> {
        self.commands.push(format!("flashing:{cmd}"));
        Ok(format!("OKAY flashing {cmd}"))
    }

    async fn set_active(&mut self, slot: &str) -> Result<String> {
        self.commands.push(format!("set_active:{slot}"));
        Ok(format!("OKAY set_active {slot}"))
    }

    async fn snapshot_update(&mut self, cmd: &str) -> Result<String> {
        self.commands.push(format!("snapshot_update:{cmd}"));
        Ok(format!("OKAY snapshot_update {cmd}"))
    }
}
