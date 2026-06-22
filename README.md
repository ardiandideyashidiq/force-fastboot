# pawflash

MTK device flashing toolkit — force fastboot via preloader serial, parse scatter manifests, plan & execute flashes, format partitions, disable vbmeta, control devices.

## Install

```sh
sudo apt install libudev-dev   # Linux
cargo build --release          # requires Rust 1.85+
```

Prebuilt binaries for Linux (x86_64) and Windows (x86_64) on [releases](https://github.com/user/pawflash/releases).

## Usage

```
pawflash force-fastboot [-v]
pawflash flash scatter <scatter> [--mode dry-run|selective|dirty-flash] [--storage auto|all|ufs|emmc] [--firmware-dir <dir>] [--check-images] [--dry-run] [--part <name>]... [--group <name>]... [-v]
pawflash flash <partition> <image> [--slot a|b] [--both]
pawflash disable-vbmeta [-v]
pawflash format-data [-v] [--fs-options casefold,projid,compress]
pawflash device info|reboot [target]|lock|unlock|set-active <a|b>|get-var <var>
```

Flash modes: `dry-run` (preview), `selective` (default, explicit parts/groups), `dirty-flash` (safe firmware + Android). Storage: `auto` (default), `all`, `ufs`, `emmc`.

## License

GPL-3.0-or-later. Vendored `fastboot-rs` Apache-2.0/MIT.
