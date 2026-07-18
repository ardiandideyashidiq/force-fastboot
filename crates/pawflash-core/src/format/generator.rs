use std::io;
use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tracing::debug;

use crate::flash::error::FlashError;
use crate::flash::error::Result;

/// Filesystem types that can be generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsType {
    Ext4,
    F2fs,
}

impl FsType {
    /// Maps a fastboot partition type string to a known `FsType`.
    /// Returns `None` for unsupported types (e.g. "raw", "swap").
    #[must_use]
    pub fn from_partition_type(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "ext4" => Some(Self::Ext4),
            "f2fs" => Some(Self::F2fs),
            _ => None,
        }
    }
}

// ── Embedded binaries (platform-specific) ────────────────────────────

#[cfg(target_os = "linux")]
mod embedded {
    pub const MKE2FS: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/linux/mke2fs"
    ));
    pub const MAKE_F2FS: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/linux/make_f2fs"
    ));
    pub const MAKE_F2FS_CASEFOLD: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/linux/make_f2fs_casefold"
    ));
    pub const MKE2FS_CONF: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/linux/mke2fs.conf"
    ));
    pub const LIBCPP_SO: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/linux/lib64/libc++.so"
    ));
}

#[cfg(target_os = "windows")]
mod embedded {
    pub const MKE2FS: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/windows/mke2fs.exe"
    ));
    pub const MAKE_F2FS: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/windows/make_f2fs.exe"
    ));
    pub const MAKE_F2FS_CASEFOLD: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/windows/make_f2fs_casefold.exe"
    ));
    pub const MKE2FS_CONF: &[u8] = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../vendor/format-tools/windows/mke2fs.conf"
    ));
}

// ── Extraction ───────────────────────────────────────────────────────

/// Extract embedded format-tools to a temporary directory.
/// Returns the `TempDir` (keeps the dir alive) and the path to the tool root.
/// Extract embedded format-tools to a temporary directory.
///
/// # Errors
///
/// Returns an error if the temporary directory cannot be created or
/// any embedded binary cannot be written to disk.
pub fn extract_format_tools() -> io::Result<(TempDir, PathBuf)> {
    let dir = TempDir::new()?;
    let root = dir.path().to_path_buf();

    let write_file = |name: &str, data: &[u8]| -> io::Result<()> {
        let path = root.join(name);
        std::fs::write(&path, data)?;
        #[cfg(unix)]
        std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        Ok(())
    };

    write_file("mke2fs", embedded::MKE2FS)?;
    write_file("make_f2fs", embedded::MAKE_F2FS)?;
    write_file("make_f2fs_casefold", embedded::MAKE_F2FS_CASEFOLD)?;
    write_file("mke2fs.conf", embedded::MKE2FS_CONF)?;

    #[cfg(target_os = "linux")]
    {
        let lib64 = root.join("lib64");
        std::fs::create_dir_all(&lib64)?;
        std::fs::write(lib64.join("libc++.so"), embedded::LIBCPP_SO)?;
    }

    debug!(path = %root.display(), "format-tools extracted");
    Ok((dir, root))
}

// ── Filesystem image generation ──────────────────────────────────────

/// Generate an empty filesystem image at `output` using the bundled tools.
///
/// # Errors
///
/// Returns an error if filesystem generation fails or the bundled tools
/// cannot be spawned.
pub async fn generate_empty_fs(
    tools_dir: &Path,
    output: &Path,
    fs_type: FsType,
    part_size: u64,
    erase_blk_size: u32,
    logical_blk_size: u32,
    fs_options: u32,
) -> Result<()> {
    match fs_type {
        FsType::Ext4 => {
            generate_ext4(tools_dir, output, part_size, erase_blk_size, logical_blk_size, fs_options)
                .await
        }
        FsType::F2fs => generate_f2fs(tools_dir, output, part_size, fs_options).await,
    }
}

fn apply_tool_env(cmd: &mut tokio::process::Command, tools_dir: &Path) {
    cmd.current_dir(tools_dir);

    #[cfg(target_os = "linux")]
    {
        let lib_dir = tools_dir.join("lib64");
        if lib_dir.is_dir() {
            let new_path = if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
                let mut paths: Vec<_> = std::env::split_paths(&existing).collect();
                paths.insert(0, lib_dir);
                match std::env::join_paths(paths) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(%e, "failed to join LD_LIBRARY_PATH with tool dir");
                        existing
                    }
                }
            } else {
                lib_dir.into_os_string()
            };
            cmd.env("LD_LIBRARY_PATH", new_path);
        }
    }
}

async fn generate_ext4(
    tools_dir: &Path,
    output: &Path,
    part_size: u64,
    erase_blk_size: u32,
    logical_blk_size: u32,
    fs_options: u32,
) -> Result<()> {
    const BLOCK_SIZE: u64 = 4096;
    let blocks = part_size / BLOCK_SIZE;

    if blocks < 1 {
        return Err(FlashError::GeneratorFailed {
            reason: format!("partition size {part_size} is smaller than minimum block size {BLOCK_SIZE}"),
        });
    }

    debug!(
        part_size, blocks, erase_blk_size, logical_blk_size, fs_options,
        "generating ext4 filesystem",
    );

    let mke2fs = tools_dir.join("mke2fs");
    let conf = tools_dir.join("mke2fs.conf");

    let mut cmd = tokio::process::Command::new(&mke2fs);
    apply_tool_env(&mut cmd, tools_dir);
    cmd.env("MKE2FS_CONFIG", &conf);
    cmd.arg("-F");                // force (recovery uses this)
    cmd.arg("-t").arg("ext4");
    cmd.arg("-b").arg(BLOCK_SIZE.to_string());

    // Match recovery's feature flags: metadata_csum, 64bit, extent
    // (not uninit_bg — recovery initialises block groups fully).
    cmd.arg("-O").arg("metadata_csum,64bit,extent");

    if erase_blk_size != 0 && logical_blk_size != 0 {
        let mut raid_stride = u64::from(logical_blk_size / 4096);
        let mut raid_stripe_width = u64::from(erase_blk_size / 4096);
        if logical_blk_size < 8192 {
            raid_stride = 8192 / BLOCK_SIZE;
        }
        if raid_stripe_width < raid_stride {
            raid_stripe_width = raid_stride;
        }
        let ext_attr =
            format!("stride={raid_stride},stripe-width={raid_stripe_width}");
        cmd.arg("-E").arg(&ext_attr);
    }

    // Always use wider inodes for project quotas (recovery always uses needs_projid=true).
    // The --fs-options projid bit becomes a no-op.
    cmd.arg("-I").arg("512");
    // FS_OPT_CASEFOLD = bit 0
    if fs_options & (1 << 0) != 0 {
        cmd.arg("-O").arg("casefold");
        cmd.arg("-E").arg("encoding=utf8");
    }

    cmd.arg(output);
    cmd.arg(blocks.to_string());

    let result = cmd.output().await.map_err(|e| FlashError::GeneratorFailed {
        reason: format!("failed to spawn mke2fs: {e}"),
    })?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(FlashError::GeneratorFailed {
            reason: format!("mke2fs failed: {stderr}"),
        });
    }

    Ok(())
}

async fn generate_f2fs(
    tools_dir: &Path,
    output: &Path,
    part_size: u64,
    fs_options: u32,
) -> Result<()> {
    let mkf2fs = tools_dir.join("make_f2fs");

    debug!(part_size, fs_options, "generating f2fs filesystem");

    let mut cmd = tokio::process::Command::new(&mkf2fs);
    apply_tool_env(&mut cmd, tools_dir);
    cmd.arg("-S").arg(part_size.to_string());
    cmd.arg("-g").arg("android");

    // Always add project_quota + extra_attr (recovery always does this for /data).
    // The --fs-options projid bit becomes a no-op.
    cmd.arg("-O").arg("project_quota,extra_attr");
    // FS_OPT_CASEFOLD = bit 0
    if fs_options & (1 << 0) != 0 {
        cmd.arg("-O").arg("casefold");
        cmd.arg("-C").arg("utf8");
    }
    // FS_OPT_COMPRESS = bit 2
    if fs_options & (1 << 2) != 0 {
        cmd.arg("-O").arg("compression");
        cmd.arg("-O").arg("extra_attr");
    }

    cmd.arg(output);

    let result = cmd.output().await.map_err(|e| FlashError::GeneratorFailed {
        reason: format!("failed to spawn make_f2fs: {e}"),
    })?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(FlashError::GeneratorFailed {
            reason: format!("make_f2fs failed: {stderr}"),
        });
    }

    Ok(())
}

/// Parse comma-separated fs-options string into a bitmask.
/// Options: casefold, projid, compress
#[must_use]
pub fn parse_fs_options(options: &[String]) -> u32 {
    let mut flags = 0u32;
    for opt in options {
        match opt.as_str() {
            "casefold" => flags |= 1 << 0,
            "projid" => flags |= 1 << 1,
            "compress" => flags |= 1 << 2,
            other => {
                tracing::warn!(unknown = %other, "ignoring unknown fs-option");
            }
        }
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fs_options_empty() {
        let opts: Vec<String> = vec![];
        assert_eq!(parse_fs_options(&opts), 0);
    }

    #[test]
    fn parse_fs_options_casefold() {
        let opts = vec!["casefold".to_string()];
        assert_eq!(parse_fs_options(&opts), 1 << 0);
    }

    #[test]
    fn parse_fs_options_projid() {
        let opts = vec!["projid".to_string()];
        assert_eq!(parse_fs_options(&opts), 1 << 1);
    }

    #[test]
    fn parse_fs_options_compress() {
        let opts = vec!["compress".to_string()];
        assert_eq!(parse_fs_options(&opts), 1 << 2);
    }

    #[test]
    fn parse_fs_options_combined() {
        let opts = vec!["casefold".to_string(), "compress".to_string()];
        assert_eq!(parse_fs_options(&opts), (1 << 0) | (1 << 2));
    }

    #[test]
    fn parse_fs_options_ignores_unknown() {
        let opts = vec!["unknown_option".to_string()];
        assert_eq!(parse_fs_options(&opts), 0);
    }

    #[test]
    fn fs_type_from_partition_type_should_accept_ext4() {
        assert_eq!(FsType::from_partition_type("ext4"), Some(FsType::Ext4));
        assert_eq!(FsType::from_partition_type("EXT4"), Some(FsType::Ext4));
    }

    #[test]
    fn fs_type_from_partition_type_should_accept_f2fs() {
        assert_eq!(FsType::from_partition_type("f2fs"), Some(FsType::F2fs));
    }

    #[test]
    fn fs_type_from_partition_type_should_return_none_for_raw() {
        assert_eq!(FsType::from_partition_type("raw"), None);
    }

    #[test]
    fn fs_type_from_partition_type_should_return_none_for_empty() {
        assert_eq!(FsType::from_partition_type(""), None);
    }
}
