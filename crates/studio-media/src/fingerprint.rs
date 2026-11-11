//! Fast content fingerprint for cache keys and duplicate detection:
//! BLAKE3 over (file size, first 1 MiB, last 1 MiB).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use studio_core::StudioResult;

const CHUNK: u64 = 1 << 20;

pub fn fingerprint(path: &Path) -> StudioResult<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let mut h = blake3::Hasher::new();
    h.update(&len.to_le_bytes());
    let mut buf = vec![0u8; CHUNK as usize];
    let n = f.read(&mut buf)?;
    h.update(&buf[..n]);
    if len > 2 * CHUNK {
        f.seek(SeekFrom::Start(len - CHUNK))?;
        let mut tail = Vec::with_capacity(CHUNK as usize);
        f.take(CHUNK).read_to_end(&mut tail)?;
        h.update(&tail);
    } else if len > CHUNK {
        let mut rest = Vec::new();
        f.read_to_end(&mut rest)?;
        h.update(&rest);
    }
    Ok(h.finalize().to_hex()[..32].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_and_sensitive() {
        let d = std::env::temp_dir().join("studio-fp-test");
        std::fs::create_dir_all(&d).unwrap();
        let a = d.join("a.bin");
        std::fs::write(&a, vec![1u8; 3_000_000]).unwrap();
        let f1 = fingerprint(&a).unwrap();
        assert_eq!(f1, fingerprint(&a).unwrap());
        let mut v = vec![1u8; 3_000_000];
        v[2_999_999] = 2;
        std::fs::write(&a, v).unwrap();
        assert_ne!(f1, fingerprint(&a).unwrap());
    }
}
