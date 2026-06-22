/// When nusb enumeration returns no fastboot devices, scan `/sys/bus/usb/devices/`
/// directly and log every interface with its class/subclass/protocol + parent device
/// attributes. Helps diagnose why `nusb::probe_device()` silently dropped the device.
#[cfg(target_os = "linux")]
pub fn diagnose_fastboot_sysfs() {
    use std::fs;
    use std::path::Path;
    use tracing::warn;

    let root = Path::new("/sys/bus/usb/devices");
    let Ok(entries) = fs::read_dir(root) else {
        warn!("cannot read /sys/bus/usb/devices");
        return;
    };

    let read_attr = |p: &Path| -> String {
        fs::read_to_string(p)
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_default()
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();

        if !name.contains(':') {
            continue;
        }

        let base = entry.path();
        let class = read_attr(&base.join("bInterfaceClass"));
        let subclass = read_attr(&base.join("bInterfaceSubClass"));
        let protocol = read_attr(&base.join("bInterfaceProtocol"));
        if class == "ff" && subclass == "42" && protocol == "03" {
            warn!(iface = %name, %class, %subclass, %protocol, "found fastboot interface in sysfs");

            let parent_name = name.split(':').next().unwrap_or("");
            let parent = root.join(parent_name);
            warn!(parent = %parent_name, "fastboot interface -> parent device");
            for attr in [
                "busnum", "devnum", "idVendor", "idProduct", "bcdDevice",
                "version", "bDeviceClass", "bDeviceSubClass", "bDeviceProtocol",
            ] {
                let val = read_attr(&parent.join(attr));
                warn!(%attr, value = %val, "  parent sysfs attr");
            }
            if !parent.exists() {
                warn!("parent device directory {parent_name} does not exist under /sys/bus/usb/devices/");
            }
        }
    }
}
