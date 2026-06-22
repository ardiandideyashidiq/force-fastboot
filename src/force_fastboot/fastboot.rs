use std::fs;
use std::path::Path;
use tracing::{debug, info, trace, warn};

const FASTBOOT_IFACE_CLASS: u8 = 0xff;
const FASTBOOT_IFACE_SUBCLASS: u8 = 0x42;
const FASTBOOT_IFACE_PROTOCOL: u8 = 0x03;

pub async fn list_fastboot_devices() {
    let Ok(devices) = nusb::list_devices().await else { return };
    for dev in devices.filter(is_fastboot_device) {
        let serial = dev.serial_number().unwrap_or("?").to_string();
        let vidpid = format!("{:04x}:{:04x}", dev.vendor_id(), dev.product_id());
        info!(serial, vidpid, "fastboot device");
    }
}

pub async fn in_fastboot_mode() -> bool {
    let nusb = nusb_fastboot_mode().await;
    let sysfs = linux_sysfs_fastboot_mode();
    let result = nusb || sysfs;
    debug!(nusb, sysfs, result, "fastboot mode check");
    result
}

fn is_fastboot_device(dev: &nusb::DeviceInfo) -> bool {
    dev.interfaces()
        .any(|i| i.class() == FASTBOOT_IFACE_CLASS && i.subclass() == FASTBOOT_IFACE_SUBCLASS && i.protocol() == FASTBOOT_IFACE_PROTOCOL)
}

async fn nusb_fastboot_mode() -> bool {
    let devices = match nusb::list_devices().await {
        Ok(devices) => devices,
        Err(err) => {
            warn!(%err, "failed to enumerate USB devices with nusb");
            return false;
        }
    };

    for dev in devices {
        let vid = dev.vendor_id();
        let pid = dev.product_id();
        trace!(vid, pid, "checking USB device for fastboot interface");

        let interface_match = dev.interfaces().any(|intf| {
            intf.class() == FASTBOOT_IFACE_CLASS
                && intf.subclass() == FASTBOOT_IFACE_SUBCLASS
                && intf.protocol() == FASTBOOT_IFACE_PROTOCOL
        });

        if interface_match {
            debug!(vid, pid, "found fastboot interface via nusb");
            return true;
        }

        let product = dev.product_string().unwrap_or("").to_ascii_lowercase();
        if product.contains("fastboot") || product.contains("bootloader") {
            debug!(vid, pid, product, "found fastboot device via product string");
            return true;
        }
    }

    false
}

fn linux_sysfs_fastboot_mode() -> bool {
    if cfg!(not(target_os = "linux")) {
        return false;
    }

    let root = Path::new("/sys/bus/usb/devices");
    let Ok(entries) = fs::read_dir(root) else {
        trace!("cannot read /sys/bus/usb/devices");
        return false;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();

        if !name.contains(':') {
            continue;
        }

        let base = entry.path();
        let class = read_trimmed(base.join("bInterfaceClass"));
        let subclass = read_trimmed(base.join("bInterfaceSubClass"));
        let protocol = read_trimmed(base.join("bInterfaceProtocol"));

        debug!(%name, %class, %subclass, %protocol, "sysfs interface");

        if class == "ff" && subclass == "42" && protocol == "03" {
            debug!(%name, "found fastboot interface via sysfs");
            return true;
        }
    }

    false
}

fn read_trimmed(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_fastboot_device_should_return_true_for_fastboot_interface() {
        assert_eq!(FASTBOOT_IFACE_CLASS, 0xff);
        assert_eq!(FASTBOOT_IFACE_SUBCLASS, 0x42);
        assert_eq!(FASTBOOT_IFACE_PROTOCOL, 0x03);
    }

    #[tokio::test]
    async fn fastboot_mode_should_fallback_gracefully_when_nusb_fails() {
        assert!(!nusb_fastboot_mode().await || cfg!(target_os = "linux"));
    }

    #[test]
    fn read_trimmed_should_return_default_for_missing_path() {
        let result = read_trimmed("/sys/force-fastboot-nonexistent");
        assert_eq!(result, "");
    }

    #[test]
    fn read_trimmed_should_lowercase_and_trim() {
        let result = read_trimmed("/tmp/__force_fastboot_test__");
        assert_eq!(result, "");
    }
}
