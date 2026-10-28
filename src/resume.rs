//! Progress recovery and validation engine for Fast-Arnis.
//!
//! Audits existing Minecraft region (.mca) files, verifies their headers,
//! chunk allocation tables, and compression schemes, purges corrupted/truncated
//! files from interrupted runs, and allows generation to seamlessly pick up
//! exactly where it left off without starting from zero.

use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Result of an audit of existing region files in a world directory.
#[derive(Debug, Default, Clone)]
pub struct ResumeReport {
    /// Set of validated (rx, rz) region coordinates that are complete and uncorrupted.
    pub valid_regions: HashSet<(i32, i32)>,
    /// Number of verified, complete regions recovered.
    pub recovered_regions_count: usize,
    /// Number of corrupt, zero-byte, or truncated regions discarded.
    pub corrupted_regions_count: usize,
}

/// Header size of a standard Minecraft Anvil region file (8 KiB).
const MCA_HEADER_SIZE: u64 = 8192;

/// Validates whether a file is a genuine, uncorrupted Minecraft Anvil region (.mca) file.
///
/// An Anvil region file consists of:
/// - 4,096 bytes: Chunk Locations Table (1024 4-byte entries: 3 bytes sector offset, 1 byte sector count)
/// - 4,096 bytes: Chunk Timestamps Table (1024 4-byte timestamps)
/// - Followed by 4,096-byte chunk sectors.
pub fn validate_region_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };

    let file_len = metadata.len();
    if file_len < MCA_HEADER_SIZE {
        return false;
    }

    let Ok(mut file) = File::open(path) else {
        return false;
    };

    let mut locations = [0u8; 4096];
    if file.read_exact(&mut locations).is_err() {
        return false;
    }

    let mut chunk_count = 0usize;
    let mut max_sector_offset = 0u64;

    for chunk_idx in 0..1024 {
        let entry_offset = chunk_idx * 4;
        let sector_offset = ((locations[entry_offset] as u64) << 16)
            | ((locations[entry_offset + 1] as u64) << 8)
            | (locations[entry_offset + 2] as u64);
        let sector_count = locations[entry_offset + 3] as u64;

        if sector_offset == 0 && sector_count == 0 {
            // Unpopulated chunk slot
            continue;
        }

        // Sectors 0 and 1 are reserved for the header, so chunk sectors must start at offset >= 2
        if sector_offset < 2 {
            return false;
        }

        let byte_offset = sector_offset * 4096;
        let chunk_end = byte_offset + sector_count * 4096;

        if chunk_end > file_len {
            // Truncated file: chunk allocation extends beyond physical file length
            return false;
        }

        chunk_count += 1;
        max_sector_offset = max_sector_offset.max(sector_offset);
    }

    // A valid region must contain at least one populated chunk
    if chunk_count == 0 {
        return false;
    }

    // Deep check the first non-trivial chunk to verify compression scheme
    for chunk_idx in 0..1024 {
        let entry_offset = chunk_idx * 4;
        let sector_offset = ((locations[entry_offset] as u64) << 16)
            | ((locations[entry_offset + 1] as u64) << 8)
            | (locations[entry_offset + 2] as u64);

        if sector_offset >= 2 {
            let byte_offset = sector_offset * 4096;
            if file.seek(SeekFrom::Start(byte_offset)).is_ok() {
                let mut chunk_header = [0u8; 5];
                if file.read_exact(&mut chunk_header).is_ok() {
                    let chunk_len = u32::from_be_bytes([
                        chunk_header[0],
                        chunk_header[1],
                        chunk_header[2],
                        chunk_header[3],
                    ]);
                    let compression_scheme = chunk_header[4];

                    // Schemes: 1 = GZip, 2 = Zlib (Standard), 3 = Uncompressed, 4 = LZ4
                    if compression_scheme < 1 || compression_scheme > 4 {
                        return false;
                    }
                    if chunk_len == 0 || (chunk_len as u64) > file_len {
                        return false;
                    }
                }
            }
            break;
        }
    }

    true
}

/// Parses the (rx, rz) coordinates from an MCA filename like "r.12.-5.mca".
pub fn parse_region_filename(name: &str) -> Option<(i32, i32)> {
    if !name.starts_with("r.") || !name.ends_with(".mca") {
        return None;
    }
    let parts: Vec<&str> = name.strip_prefix("r.")?.strip_suffix(".mca")?.split('.').collect();
    if parts.len() != 2 {
        return None;
    }
    let rx = parts[0].parse::<i32>().ok()?;
    let rz = parts[1].parse::<i32>().ok()?;
    Some((rx, rz))
}

/// Audits an existing world's `region/` directory.
///
/// Validates all existing `.mca` files:
/// - Good regions are retained in `valid_regions`.
/// - Corrupt, zero-byte, or truncated regions are purged to ensure clean restarts.
pub fn audit_existing_regions(region_dir: &Path) -> ResumeReport {
    let mut report = ResumeReport::default();

    if !region_dir.is_dir() {
        return report;
    }

    let Ok(entries) = fs::read_dir(region_dir) else {
        return report;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                if let Some((rx, rz)) = parse_region_filename(file_name) {
                    if validate_region_file(&path) {
                        report.valid_regions.insert((rx, rz));
                        report.recovered_regions_count += 1;
                    } else {
                        // Incomplete or corrupted file from previous abort; remove it
                        let _ = fs::remove_file(&path);
                        report.corrupted_regions_count += 1;
                    }
                }
            }
        }
    }

    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_region_filename() {
        assert_eq!(parse_region_filename("r.0.0.mca"), Some((0, 0)));
        assert_eq!(parse_region_filename("r.-5.12.mca"), Some((-5, 12)));
        assert_eq!(parse_region_filename("r.100.-200.mca"), Some((100, -200)));
        assert_eq!(parse_region_filename("invalid.txt"), None);
        assert_eq!(parse_region_filename("r.0.mca"), None);
        assert_eq!(parse_region_filename("r.0.0.0.mca"), None);
    }
}
