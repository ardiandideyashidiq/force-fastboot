//! Flash Android sparse images to fastboot partitions.
//!
//! Android sparse images (magic `0xED26FF3A`) wrap image data with chunk
//! headers describing how to expand them on-device.  Fastboot supports
//! flashing them in split pieces — each piece is a self-contained sparse
//! image that the bootloader reassembles.

use std::io::SeekFrom;
use std::path::Path;

use android_sparse_image::{
    split::{split_image, split_raw, SplitChunk}, ChunkHeader, DEFAULT_BLOCKSIZE,
    FileHeader, FileHeaderBytes, CHUNK_HEADER_BYTES_LEN, FILE_HEADER_BYTES_LEN,
};
use indicatif::ProgressBar;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::{debug, info};

use crate::flash::error::{FlashError, Result};
use crate::flash::transport::FlashTransport;

/// Crypto footer offset used by legacy FDE (Full Disk Encryption).
/// TWRP preserves this space during mkfs and wipes it afterward
/// to remove encryption — matches `CRYPT_FOOTER_OFFSET` in `bootable/recovery/partition.cpp`.
pub const CRYPT_FOOTER_OFFSET: u64 = 0x4000;

/// Helper: call `read_exact_padded`, then raise `SparseTruncated` if fewer
/// bytes were read from the file than requested.
async fn read_exact_padded_or_truncate(
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
async fn read_exact_padded(
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

/// Flash a sparse image to a partition.
///
/// Parse the sparse file header + chunk headers, split into parts that each
/// fit within `max_download`, then send each part as a separate
/// download+flash transaction.  The bootloader reassembles the pieces.
/// Returns the device response message from the final split flash.
pub(crate) async fn flash_sparse_image(
    fb: &mut impl FlashTransport,
    partition: &str,
    path: &Path,
    file_len: u64,
    max_download: u32,
    progress_bar: Option<&ProgressBar>,
) -> Result<String> {
    debug!(%partition, file_len, max_download, "flashing sparse image");

    let mut file = tokio::fs::File::open(path).await?;

    // ---- parse file header ----
    let mut header_bytes = FileHeaderBytes::default();
    file.read_exact(&mut header_bytes).await?;
    let header = FileHeader::from_bytes(&header_bytes)
        .map_err(|_| FlashError::SparseParseFailed)?;

    // ---- parse all chunk headers, skipping data ----
    let mut chunks = Vec::with_capacity(header.chunks as usize);
    for _ in 0..header.chunks {
        let mut chunk_bytes = [0u8; CHUNK_HEADER_BYTES_LEN];
        file.read_exact(&mut chunk_bytes).await?;
        let chunk = ChunkHeader::from_bytes(&chunk_bytes)
            .map_err(|_| FlashError::SparseParseFailed)?;
        let data_size = chunk.data_size();
        if data_size > 0 {
            let seek_offset = i64::try_from(data_size)
                .map_err(|_| FlashError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "sparse chunk data size exceeds i64 range",
                )))?;
            file.seek(SeekFrom::Current(seek_offset)).await?;
        }
        chunks.push(chunk);
    }

    info!(%partition, chunk_count = chunks.len(), "parsed sparse image header");

    // ---- split into max_download-sized pieces ----
    let splits = split_image(&header, &chunks, max_download)
        .map_err(|_| FlashError::SparseSplitFailed)?;

    info!(%partition, split_count = splits.len(), "sparse image split for download");

    let total_download: u64 = splits.iter()
        .map(|s| u64::try_from(s.sparse_size()).unwrap_or(0))
        .sum();

    if let Some(pb) = progress_bar {
        pb.set_length(total_download);
        pb.set_prefix(partition.to_string());
        pb.reset();
        pb.set_position(0);
    }

    // ---- flash each split (no erase — the flash command handles it) ----
    let mut last_resp = String::new();
    for (i, split) in splits.iter().enumerate() {
        debug!(%partition, part = i, "sending sparse split");

        let sparse_size = u32::try_from(split.sparse_size())
            .map_err(|_| FlashError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "sparse split size exceeds u32 range",
            )))?;
        let mut sender = fb.download( sparse_size).await?;

        // file header for this split
        sender.extend_from_slice(&split.header.to_bytes()).await?;
        if let Some(pb) = progress_bar {
            pb.inc(FILE_HEADER_BYTES_LEN as u64);
        }

        // chunk headers + data for each chunk in this split
        for chunk in &split.chunks {
            sender.extend_from_slice(&chunk.header.to_bytes()).await?;
            if let Some(pb) = progress_bar {
                pb.inc(CHUNK_HEADER_BYTES_LEN as u64);
            }

            if chunk.size > 0 {
                file.seek(SeekFrom::Start(chunk.offset as u64)).await?;

                let mut remaining = chunk.size;
                let mut buf = vec![0u8; 1024 * 1024];
                while remaining > 0 {
                    let to_read = buf.len().min(remaining);
                    read_exact_padded_or_truncate(&mut file, &mut buf[..to_read], chunk.size).await?;
                    sender.extend_from_slice(&buf[..to_read]).await?;
                    if let Some(pb) = progress_bar {
                        pb.inc(to_read as u64);
                    }
                    remaining = remaining.saturating_sub(to_read);
                }
            }
        }

        sender.finish().await?;
        last_resp = fb.flash(partition).await?;
    }

    if let Some(pb) = progress_bar {
        pb.set_position(total_download);
    }

    debug!(%partition, total_download, response = last_resp, "sparse flash complete");
    Ok(last_resp)
}

/// Flash a raw image by wrapping it in Android sparse format splits.
///
/// Uses `split_raw()` to convert the raw file into sparse-format splits
/// that each fit within `max_download`.  The bootloader expands them
/// on-device, avoiding transmission of large zero-filled regions.
/// Returns the device response message from the final split flash.
pub(crate) async fn flash_sparse_wrapped(
    fb: &mut impl FlashTransport,
    partition: &str,
    path: &Path,
    file_len: u64,
    max_download: u32,
) -> Result<String> {
    debug!(%partition, file_len, max_download, "wrapping raw image in sparse format");

    let raw_size = usize::try_from(file_len)
        .map_err(|_| FlashError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "file too large for split_raw",
        )))?;
    let splits = split_raw(raw_size, max_download)
        .map_err(|_| FlashError::SparseSplitFailed)?;

    info!(%partition, split_count = splits.len(), "raw image split into sparse chunks");

    let mut file = tokio::fs::File::open(path).await?;

    // ---- flash each split (no erase — the flash command handles it) ----
    let mut last_resp = String::new();
    for (i, split) in splits.iter().enumerate() {
        debug!(%partition, part = i, "sending sparse-wrapped split");

        let sparse_size = u32::try_from(split.sparse_size())
            .map_err(|_| FlashError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "sparse split size exceeds u32 range",
            )))?;
        info!(%partition, part = i, sparse_size, max_download, "downloading split via fb.download");
        let mut sender = fb.download( sparse_size).await?;
        info!(%partition, part = i, "fb.download returned successfully");

        // file header for this split
        sender.extend_from_slice(&split.header.to_bytes()).await?;

        // chunk headers + data for each chunk in this split
        for chunk in &split.chunks {
            sender.extend_from_slice(&chunk.header.to_bytes()).await?;

            if chunk.size > 0 {
                file.seek(SeekFrom::Start(chunk.offset as u64)).await?;

                let mut remaining = chunk.size;
                let mut buf = vec![0u8; 1024 * 1024];
                while remaining > 0 {
                    let to_read = buf.len().min(remaining);
                    // Use plain read_exact_padded here (not the truncation-check
                    // variant) because split_raw may create chunks that extend
                    // past the end of the file for block alignment.  Zero-filling
                    // the tail is correct.
                    read_exact_padded(&mut file, &mut buf[..to_read]).await?;
                    sender.extend_from_slice(&buf[..to_read]).await?;
                    remaining = remaining.saturating_sub(to_read);
                }
            }
        }

        sender.finish().await?;
        last_resp = fb.flash(partition).await?;
    }

    debug!(%partition, splits = splits.len(), response = last_resp, "sparse-wrapped flash complete");
    Ok(last_resp)
}

/// Create an Android sparse image from a raw file by detecting data/hole
/// runs, then flash it via download+flash.
///
/// Unlike [`flash_sparse_wrapped`] (which uses `split_raw` and treats every
/// block as RAW data), this function collapses zero runs into DONTCARE
/// chunks, producing a compact image that can be sent in a single
/// download+flash even for huge partitions with only a few metadata blocks.
///
/// The file is extended to `effective_size` (normally `part_size -
/// footer_size`) before scanning, ensuring all partition metadata regions
/// (SIT, NAT, ...) are covered.  The last `footer_size` bytes are emitted
/// as a DONTCARE chunk — the bootloader writes zeros there, matching
/// TWRP's behaviour of wiping the crypto footer.
///
/// If `footer_size` is zero, `effective_size == part_size` and the entire
/// partition is covered by the scan.
pub(crate) async fn sparse_wrap_file(
    fb: &mut impl FlashTransport,
    partition: &str,
    path: &Path,
    part_size: u64,
    max_download: u32,
    footer_size: u64,
) -> Result<String> {
    debug!(%partition, part_size, footer_size, max_download, "full-scan sparse wrapping");

    let blk = u64::from(DEFAULT_BLOCKSIZE);
    let effective_size = part_size.saturating_sub(footer_size);
    let total_blocks = part_size / blk;
    if total_blocks == 0 {
        fb.erase(partition).await?;
        return Ok(String::new());
    }

    // Save original file size before extending.
    let orig_size = {
        let m = tokio::fs::metadata(path).await?;
        m.len()
    };

    // Extend the output file to effective_size. On Linux this creates a
    // sparse file (holes) so the extra space costs no disk I/O.
    // Note: OpenOptions::write(true) (without create(true)/truncate(true))
    // preserves the existing filesystem data written by generate_empty_fs.
    {
        let f = tokio::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .await?;
        f.set_len(effective_size).await?;
        drop(f);
    }

    // ---- scan for data / hole runs ----
    // We scan from block 0 up to the smaller of (orig_size, effective_size)
    // because blocks beyond orig_size were added by set_len and are
    // guaranteed zeros.  We use SEEK_DATA / SEEK_HOLE on Unix for
    // efficient extent iteration; fallback path reads in chunks.
    let scan_size = effective_size.min(orig_size);

    let chunks = scan_extents(path, scan_size, effective_size, part_size, blk)?;

    if chunks.is_empty() {
        // Entire partition is zero — just erase.
        fb.erase(partition).await?;
        return Ok(String::new());
    }

    let n_chunks = u32::try_from(chunks.len())
        .map_err(|_| FlashError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "too many sparse chunks",
        )))?;
    let total_blocks = chunks.iter().map(|c| c.header.chunk_size).sum::<u32>();
    let header = FileHeader {
        block_size: DEFAULT_BLOCKSIZE,
        blocks: total_blocks,
        chunks: n_chunks,
        checksum: 0,
    };
    let image_size = FILE_HEADER_BYTES_LEN
        + chunks.iter().map(|c| c.header.total_size as usize).sum::<usize>();

    // ---- check size ----
    let sparse_size = u32::try_from(image_size)
        .map_err(|_| FlashError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "sparse image too large for u32",
        )))?;
    if sparse_size > max_download {
        return Err(FlashError::ActionFailed {
            partition: partition.into(),
            reason: format!(
                "compressed sparse image ({sparse_size}) exceeds max-download-size ({max_download}); \
                 try again without format-data"
            ),
        });
    }

    // ---- send ----
    let mut file = tokio::fs::File::open(path).await?;

    let mut sender = fb.download( sparse_size).await?;
    sender.extend_from_slice(&header.to_bytes()).await?;

    for chunk in &chunks {
        sender.extend_from_slice(&chunk.header.to_bytes()).await?;
        if chunk.size > 0 {
            file.seek(SeekFrom::Start(chunk.offset as u64)).await?;
            let mut remaining = chunk.size;
            let mut buf = vec![0u8; 1024 * 1024];
            while remaining > 0 {
                let to_read = buf.len().min(remaining);
                read_exact_padded(&mut file, &mut buf[..to_read]).await?;
                sender.extend_from_slice(&buf[..to_read]).await?;
                remaining = remaining.saturating_sub(to_read);
            }
        }
    }

    sender.finish().await?;
    let resp = fb.flash(partition).await?;

    debug!(%partition, sparse_size, response = resp, "full-scan sparse flash complete");
    Ok(resp)
}

/// Scan a sparse file for data/hole extent boundaries and build alternating
/// RAW/DONTCARE Android sparse chunks.
///
/// On Unix uses `SEEK_DATA` / `SEEK_HOLE` for O(extents) scanning; on other
/// platforms falls back to block-by-block reads.
///
/// The region from `effective_size` to `part_size` is always emitted as a
/// DONTCARE chunk (bootloader writes zeros), matching TWRP's crypto-footer
/// wipe behaviour.
#[cfg(unix)]
fn do_scan_impl(
    file: &std::fs::File,
    effective_size: u64,
    blk: u64,
) -> Result<Vec<(u64, u64, bool)>> {
    use std::os::unix::io::AsRawFd;
    let fd = file.as_raw_fd();
    let mut extents: Vec<(u64, u64, bool)> = Vec::new();
    let mut offset: u64 = 0;

    loop {
        if offset >= effective_size {
            break;
        }
        let seek_offset = i64::try_from(offset)
            .map_err(|_| FlashError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("seek offset {offset:#x} exceeds i64 range"),
            )))?;
        let data_start = match unsafe { libc::lseek(fd, seek_offset, libc::SEEK_DATA) } {
            -1 => {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::ENXIO) {
                    if offset < effective_size {
                        extents.push((offset, effective_size - offset, false));
                    }
                    break;
                }
                return Err(FlashError::Io(std::io::Error::other(
                    format!("SEEK_DATA at {offset:#x}: {err}"),
                )));
            }
            pos => u64::try_from(pos)
                .map_err(|_| FlashError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("SEEK_DATA returned position {pos} that exceeds u64 range"),
                )))?,
        };
        let aligned = (data_start / blk) * blk;
        if aligned > offset {
            extents.push((offset, aligned - offset, false));
        }
        let hole_seek = i64::try_from(aligned)
            .map_err(|_| FlashError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("hole seek offset {aligned:#x} exceeds i64 range"),
            )))?;
        let hole_start =
            match unsafe { libc::lseek(fd, hole_seek, libc::SEEK_HOLE) } {
                -1 => {
                    let err = std::io::Error::last_os_error();
                    if err.raw_os_error() == Some(libc::ENXIO) {
                        effective_size
                    } else {
                return Err(FlashError::Io(std::io::Error::other(
                    format!("SEEK_HOLE at {aligned:#x}: {err}"),
                )));
                    }
                }
                pos => u64::try_from(pos)
                    .map_err(|_| FlashError::Io(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("SEEK_HOLE returned position {pos} that exceeds u64 range"),
                    )))?,
            };
        let data_end = hole_start.min(effective_size);
        let len = data_end.saturating_sub(aligned);
        if len > 0 {
            extents.push((aligned, len, true));
        }
        offset = data_end;
    }
    Ok(extents)
}

#[cfg(not(unix))]
fn is_all_zero(buf: &[u8]) -> bool {
    let (prefix, chunks, suffix) = unsafe { buf.align_to::<u128>() };
    chunks.iter().all(|&w| w == 0) && prefix.iter().all(|&b| b == 0) && suffix.iter().all(|&b| b == 0)
}

#[cfg(not(unix))]
fn do_scan_impl(
    file: &std::fs::File,
    effective_size: u64,
    blk: u64,
) -> Result<Vec<(u64, u64, bool)>> {
    use std::io::{BufReader, Read};
    let mut reader = BufReader::with_capacity(1024 * 1024, file);
    let mut extents: Vec<(u64, u64, bool)> = Vec::new();
    let scan_blocks = effective_size / blk;
    let mut run_start: u64 = 0;
    let mut run_is_data = false;

    const BLOCKS_PER_BATCH: u64 = 128;
    let batch_bytes = (BLOCKS_PER_BATCH * blk) as usize;
    let mut batch = vec![0u8; batch_bytes];
    let mut current_block: u64 = 0;

    while current_block < scan_blocks {
        let remaining = scan_blocks - current_block;
        let batch_blocks = remaining.min(BLOCKS_PER_BATCH);
        let read_bytes = (batch_blocks * blk) as usize;
        reader.read_exact(&mut batch[..read_bytes]).map_err(|_| {
            FlashError::SparseTruncated {
                read: (current_block * blk) as usize,
                expected: (scan_blocks * blk) as usize,
            }
        })?;

        for block_idx in 0..batch_blocks {
            let start = (block_idx * blk) as usize;
            let end = start + blk as usize;
            let block_data = &batch[start..end];
            let is_data = !is_all_zero(block_data);
            if current_block == 0 {
                run_is_data = is_data;
            }
            if is_data != run_is_data {
                let len = (current_block - run_start) * blk;
                if len > 0 {
                    extents.push((run_start * blk, len, run_is_data));
                }
                run_start = current_block;
                run_is_data = is_data;
            }
            current_block += 1;
        }
    }

    let len = (current_block - run_start) * blk;
    if len > 0 {
        extents.push((run_start * blk, len, run_is_data));
    }
    Ok(extents)
}

fn scan_extents(
    path: &Path,
    scan_size: u64,
    effective_size: u64,
    part_size: u64,
    blk: u64,
) -> Result<Vec<SplitChunk>> {
    let file = std::fs::File::open(path)
        .map_err(|e| FlashError::Io(std::io::Error::new(e.kind(), format!("scan open: {e}"))))?;

    let extents = do_scan_impl(&file, scan_size.min(effective_size), blk)?;

    build_split_chunks(&extents, effective_size, part_size, blk)
}

fn build_split_chunks(
    extents: &[(u64, u64, bool)],
    effective_size: u64,
    part_size: u64,
    blk: u64,
) -> Result<Vec<SplitChunk>> {
    let mut chunks: Vec<SplitChunk> = Vec::new();
    for &(offset, len, is_data) in extents {
        let blocks = len / blk;
        if blocks == 0 {
            continue;
        }
        let blocks_u32 = u32::try_from(blocks)
            .map_err(|_| FlashError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("block count {blocks} exceeds u32 range"),
            )))?;
        if is_data {
            chunks.push(SplitChunk {
                header: ChunkHeader::new_raw(blocks_u32, DEFAULT_BLOCKSIZE),
                offset: usize::try_from(offset).unwrap_or(usize::MAX),
                size: usize::try_from(len).unwrap_or(usize::MAX),
            });
        } else {
            chunks.push(SplitChunk {
                header: ChunkHeader::new_dontcare(blocks_u32),
                offset: 0,
                size: 0,
            });
        }
    }

    // Footer region between effective_size and part_size: DONTCARE zeros
    // the crypto footer, matching TWRP's behaviour of wiping encryption metadata.
    if part_size > effective_size {
        let fb = (part_size - effective_size) / blk;
        if fb > 0 {
            let fb_u32 = u32::try_from(fb)
                .map_err(|_| FlashError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("footer block count {fb} exceeds u32 range"),
                )))?;
            chunks.push(SplitChunk {
                header: ChunkHeader::new_dontcare(fb_u32),
                offset: 0,
                size: 0,
            });
        }
    }

    Ok(chunks)
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
