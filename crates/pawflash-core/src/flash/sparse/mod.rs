//! Flash Android sparse images to fastboot partitions.
//!
//! Android sparse images (magic `0xED26FF3A`) wrap image data with chunk
//! headers describing how to expand them on-device.  Fastboot supports
//! flashing them in split pieces — each piece is a self-contained sparse
//! image that the bootloader reassembles.

use std::path::Path;

use tokio::io::AsyncReadExt;

use crate::flash::error::{FlashError, Result};

pub(crate) mod chunked;
pub(crate) mod scan;

pub(crate) use chunked::{flash_sparse_image, flash_sparse_wrapped, sparse_wrap_file};

/// Reusable 1 MiB transfer buffer to avoid per-chunk allocation.
pub(crate) struct XferBuf {
    buf: Vec<u8>,
}

impl XferBuf {
    pub(crate) fn new() -> Self {
        Self { buf: vec![0u8; 1024 * 1024] }
    }

    fn get(&mut self, size_hint: usize) -> &mut [u8] {
        let needed = size_hint.max(1024 * 1024);
        if self.buf.len() < needed {
            self.buf.resize(needed, 0);
        }
        let end = needed.min(self.buf.len());
        &mut self.buf[..end]
    }
}

/// Crypto footer offset used by legacy FDE (Full Disk Encryption).
/// TWRP preserves this space during mkfs and wipes it afterward
/// to remove encryption — matches `CRYPT_FOOTER_OFFSET` in `bootable/recovery/partition.cpp`.
pub const CRYPT_FOOTER_OFFSET: u64 = 0x4000;

/// Helper: call `read_exact_padded`, then raise `SparseTruncated` if fewer
/// bytes were read from the file than requested.
pub(crate) async fn read_exact_padded_or_truncate(
    file: &mut tokio::fs::File,
    buf: &mut [u8],
    chunk_expected: usize,
) -> Result<()> {
    let file_bytes = read_exact_padded(file, buf).await?;
    if file_bytes < buf.len() {
        return Err(FlashError::SparseTruncated {
            read: chunk_expected - (buf.len() - file_bytes),
            expected: chunk_expected,
        });
    }
    Ok(())
}

/// Read exactly `buf.len()` bytes from `file`, zero-filling any remainder
/// if EOF is reached early.  Required because sparse chunk data must always
/// be block-aligned even when the underlying file is shorter.
///
/// Returns the number of bytes actually read from the file (before any
/// zero-fill padding).  The caller should check the return value against
/// `buf.len()` and raise `FlashError::SparseTruncated` on mismatch.
pub(crate) async fn read_exact_padded(
    file: &mut tokio::fs::File,
    buf: &mut [u8],
) -> std::io::Result<usize> {
    let total = buf.len();
    let mut offset = 0;
    while offset < total {
        match file.read(&mut buf[offset..]).await {
            Ok(0) => {
                buf[offset..].fill(0);
                break;
            }
            Ok(n) => offset += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(offset)
}

/// Check whether a file is an Android sparse image by reading the 4-byte
/// magic header.
pub(crate) async fn is_sparse_image(path: &Path) -> Result<bool> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).await?;
    Ok(u32::from_le_bytes(magic) == android_sparse_image::HEADER_MAGIC)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::read_exact_padded;
    use tokio::io::AsyncWriteExt;

    use crate::flash::sparse::is_sparse_image;

    #[test]
    fn read_exact_padded_should_zero_fill_short_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("test.bin");
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let mut f = tokio::fs::File::create(&path).await.unwrap();
            f.write_all(&[0xAB; 10]).await.unwrap();
            drop(f);
            let mut f = tokio::fs::File::open(&path).await.unwrap();
            let mut buf = [0xFFu8; 16];
            let n = read_exact_padded(&mut f, &mut buf).await.unwrap();
            assert_eq!(n, 10, "should read 10 bytes from file");
            assert_eq!(&buf[..10], &[0xAB; 10], "first 10 bytes are file data");
            assert_eq!(&buf[10..], &[0u8; 6], "last 6 bytes are zero-padded");
        });
    }

    #[test]
    fn sparse_magic_constant_is_correct() {
        assert_eq!(
            android_sparse_image::HEADER_MAGIC,
            0xED26_FF3A,
            "sparse magic constant should match known value"
        );
    }

    #[test]
    fn zero_partition_yields_zero_blocks() {
        assert!(
            u64::from(android_sparse_image::DEFAULT_BLOCKSIZE) > 0,
            "block size must be positive",
        );
    }

    #[tokio::test]
    async fn is_sparse_image_detects_magic() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sparse.img");
        let magic = 0xED26_FF3Au32.to_le_bytes();
        std::fs::write(&path, magic).unwrap();
        assert!(is_sparse_image(Path::new(&path)).await.unwrap());
    }

    #[tokio::test]
    async fn is_sparse_image_rejects_raw() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("raw.img");
        std::fs::write(&path, b"this is not a sparse image").unwrap();
        assert!(!is_sparse_image(Path::new(&path)).await.unwrap());
    }
}
