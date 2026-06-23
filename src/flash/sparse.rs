//! Flash Android sparse images to fastboot partitions.
//!
//! Android sparse images (magic `0xED26FF3A`) wrap image data with chunk
//! headers describing how to expand them on-device.  Fastboot supports
//! flashing them in split pieces — each piece is a self-contained sparse
//! image that the bootloader reassembles.

use std::io::SeekFrom;
use std::path::Path;

use android_sparse_image::{
    split::{split_image, split_raw}, ChunkHeader, FileHeader, FileHeaderBytes,
    CHUNK_HEADER_BYTES_LEN, FILE_HEADER_BYTES_LEN,
};
use indicatif::ProgressBar;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tracing::{debug, info};

use crate::flash::error::{FlashError, Result};

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
    fb: &mut fastboot_protocol::nusb::NusbFastBoot,
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
        let mut sender = fb.download(sparse_size).await?;

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
    fb: &mut fastboot_protocol::nusb::NusbFastBoot,
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
        let mut sender = fb.download(sparse_size).await?;

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
