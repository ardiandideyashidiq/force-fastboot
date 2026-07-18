use std::path::Path;

use android_sparse_image::{
    split::SplitChunk, ChunkHeader, DEFAULT_BLOCKSIZE,
};

use crate::flash::error::{FlashError, Result};

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
        // SAFETY: `fd` is a valid file descriptor from `AsRawFd` on a real file.
        // `lseek` with SEEK_DATA is safe per POSIX; we check for -1 return which
        // covers both errors (checked via errno/ENXIO) and absence of data.
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
        // SAFETY: same fd as above; SEEK_HOLE after a data region is valid per POSIX.
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
    buf.chunks_exact(16).all(|c| c.iter().all(|&b| b == 0))
        && buf.iter().rev().take(buf.len() % 16).all(|&b| b == 0)
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

pub(super) fn scan_extents(
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
