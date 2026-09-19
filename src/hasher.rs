use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use xxhash_rust::xxh3::xxh3_64_with_seed;

const CHUNK_SIZE: usize = 64 * 1024; // 64 KB

/// Deterministic O(1) fingerprint: xxh3_64 over (file_size || head 64KB || tail 64KB).
/// Stable across builds, platforms, and compiler versions. Takes microseconds
/// regardless of whether the file is 10 MB or 100 GB.
pub fn compute_quick_fingerprint(path: &Path, file_size: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;

    let mut head = [0u8; CHUNK_SIZE];
    let n_head = file.read(&mut head).ok()?;

    let mut tail = [0u8; CHUNK_SIZE];
    let n_tail = if file_size > CHUNK_SIZE as u64 {
        let seek_pos = file_size.saturating_sub(CHUNK_SIZE as u64);
        if file.seek(SeekFrom::Start(seek_pos)).is_ok() {
            file.read(&mut tail).ok().unwrap_or(0)
        } else {
            0
        }
    } else {
        0
    };

    // Feed size + head + tail into a single xxh3_64 pass for determinism.
    let mut buf = Vec::with_capacity(8 + n_head + n_tail);
    buf.extend_from_slice(&file_size.to_le_bytes());
    buf.extend_from_slice(&head[..n_head]);
    if n_tail > 0 {
        buf.extend_from_slice(&tail[..n_tail]);
    }
    Some(format!("{:016x}", xxh3_64_with_seed(&buf, 0)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn fingerprint_is_deterministic() {
        let dir = std::env::temp_dir().join(format!("rinode_hash_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.bin");
        let mut f = File::create(&p).unwrap();
        f.write_all(&vec![0xABu8; 200 * 1024]).unwrap();
        drop(f);
        let size = std::fs::metadata(&p).unwrap().len();
        let a = compute_quick_fingerprint(&p, size).unwrap();
        let b = compute_quick_fingerprint(&p, size).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        std::fs::remove_dir_all(&dir).ok();
    }
}
