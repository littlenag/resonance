//! Content hashing and the large/binary file policy (FR-010, MVP-016).

/// Whether content looks binary: it contains a NUL byte (MVP-016).
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

/// Whether the graph keeps a file's bytes, or only its hash and size (MVP-016).
pub fn should_store(bytes: &[u8], max_blob_bytes: u64) -> bool {
    (bytes.len() as u64) <= max_blob_bytes && !is_binary(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn policy() {
        assert!(should_store(b"fn main() {}\n", 100));
        assert!(!should_store(b"\x00\x01", 100), "binary");
        assert!(!should_store(&[b'a'; 101], 100), "too large");
        assert!(should_store(b"", 0));
    }
}
