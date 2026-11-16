//! SHA-256 content hashing.
//!
//! Every file stored by Persist is identified by the SHA-256 digest of its
//! contents. The digest is used both as the object address (content-addressable
//! storage) and as the integrity check performed after upload and download.

use crate::error::Result;
use sha2::{Digest, Sha256};
use std::path::Path;
use tokio::io::AsyncReadExt;

/// Hash an in-memory buffer.
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Hash a file by streaming it, without loading it fully into memory.
pub async fn hash_file(path: &Path) -> Result<String> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 128 * 1024];
    loop {
        let n = file.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex::encode(hasher.finalize()))
}

/// Validate that a string looks like a lowercase hex SHA-256 digest.
pub fn is_valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_sha256() {
        // sha256("abc")
        assert_eq!(
            hash_bytes(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn hash_validation() {
        assert!(is_valid_hash(&hash_bytes(b"abc")));
        assert!(!is_valid_hash("abc"));
        assert!(!is_valid_hash(&"A".repeat(64)));
    }

    #[tokio::test]
    async fn hash_file_matches_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        tokio::fs::write(&p, b"hello world").await.unwrap();
        assert_eq!(hash_file(&p).await.unwrap(), hash_bytes(b"hello world"));
    }
}
