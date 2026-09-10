use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use twox_hash::XxHash64;
use std::hash::Hasher;

const CHUNK_SIZE: usize = 64 * 1024; // 64 KB

/// Instant O(1) fingerprint: Hashes first 64KB, last 64KB, and file size.
/// Takes microseconds regardless of whether the file is 10 MB or 100 GB.
pub fn compute_quick_fingerprint(path: &Path, file_size: u64) -> Option<String> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return None,
    };

    let mut hasher = XxHash64::default();
    hasher.write_u64(file_size);

    let mut buffer = [0u8; CHUNK_SIZE];

    // Read head
    if let Ok(n) = file.read(&mut buffer) {
        hasher.write(&buffer[..n]);
    }

    // Read tail if file is larger than chunk
    if file_size > CHUNK_SIZE as u64 {
        let seek_pos = file_size.saturating_sub(CHUNK_SIZE as u64);
        if file.seek(SeekFrom::Start(seek_pos)).is_ok() {
            if let Ok(n) = file.read(&mut buffer) {
                hasher.write(&buffer[..n]);
            }
        }
    }

    Some(format!("{:016x}", hasher.finish()))
}

/// Compute full xxHash64 for files within the size limit
#[allow(dead_code)]
pub fn compute_full_hash(path: &Path) -> Option<String> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(_) => return None,
    };

    let mut hasher = XxHash64::default();
    let mut buffer = [0u8; CHUNK_SIZE];

    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => hasher.write(&buffer[..n]),
            Err(_) => return None,
        }
    }

    Some(format!("{:016x}", hasher.finish()))
}
