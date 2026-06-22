use nusb::MaybeFuture;
use std::fs;
use std::path::Path;
use tracing::{debug, trace, warn};

pub fn list_fastboot_devices() {
    let Ok(devices) = nusb::list_devices().wait() else { return };
    for dev in devices.filter(|d| {
        d.interfaces()
            .any(|i| i.class() == 0xff && i.subclass() == 0x42 && i.protocol() == 0x03)
    }) {
        let serial = dev.serial_number().unwrap_or("?").to_string();
        let vidpid = format!("{:04x}:{:04x}", dev.vendor_id(), dev.product_id());
        debug!(serial, vidpid, "found fastboot device");
        println!("{serial:22}\tfastboot\t{vidpid}");
    }
}

pub fn in_fastboot_mode() -> bool {
    let nusb = nusb_fastboot_mode();
    let sysfs = linux_sysfs_fastboot_mode();
    let result = nusb || sysfs;
    debug!(nusb, sysfs, result, "fastboot mode check");
    result
}

fn nusb_fastboot_mode() -> bool {
    let devices = match nusb::list_devices().wait() {
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
            intf.class() == 0xff && intf.subclass() == 0x42 && intf.protocol() == 0x03
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

        trace!(%name, %class, %subclass, %protocol, "sysfs interface");

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
