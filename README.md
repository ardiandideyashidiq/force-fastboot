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
pawflash flash scatter <scatter-path> [--mode dry-run|selective|dirty-flash] [--storage auto|all|ufs|emmc] [--part <name>]... [--group <name>]... [--firmware-dir <dir>] [--check-images] [--dry-run] [--json] [--exclude <name>]... [--image-search] [--allow-incomplete-slots] [--include-preloader] [--clean] [--no-format] [--clean-test] [-v]

pawflash flash <partition> <image> [--slot a|b] [--both]
pawflash disable-vbmeta [-v]
pawflash device info
pawflash device reboot [system|bootloader|fastbootd|recovery]
pawflash device lock|unlock
pawflash device set-active <a|b>
pawflash device get-var <var-name>
```

Flash modes: `dry-run` (preview via `--dry-run` flag), `selective` (explicit `--part`/`--group`, the default), `dirty-flash` (safe firmware + Android). Storage: `auto` (default, prefers UFS), `all`, `ufs`, `emmc`.

## License

GPL-3.0-or-later. Vendored `fastboot-rs` Apache-2.0/MIT.
