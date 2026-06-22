use nusb::MaybeFuture;
use std::fs;
use std::path::Path;
use tracing::warn;

pub fn in_fastboot_mode() -> bool {
    nusb_fastboot_mode() || linux_sysfs_fastboot_mode()
}

fn nusb_fastboot_mode() -> bool {
    let devices = match nusb::list_devices().wait() {
        Ok(devices) => devices,
        Err(err) => {
            warn!("failed to enumerate USB devices with nusb: {err}");
            return false;
        }
    };

    for dev in devices {
        let interface_match = dev.interfaces().any(|intf| {
            intf.class() == 0xff && intf.subclass() == 0x42 && intf.protocol() == 0x03
        });

        if interface_match {
            return true;
        }

        let product = dev.product_string().unwrap_or("").to_ascii_lowercase();
        if product.contains("fastboot") || product.contains("bootloader") {
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

        if class == "ff" && subclass == "42" && protocol == "03" {
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
