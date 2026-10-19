//! Identifiers and content hashes (FR-030, MVP-008).

use sha2::{Digest, Sha256};

/// A new permanent ID for an authored node, edge, operation or change set: a ULID (FR-030).
pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// True if `s` looks like a full ULID (26 Crockford base-32 characters).
pub fn is_ulid(s: &str) -> bool {
    s.len() == 26 && ulid::Ulid::from_string(s).is_ok()
}

/// SHA-256 of `bytes` in lowercase hex (MVP-008).
pub fn content_hash(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest.iter() {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// The deterministic node ID of a requirement, `req:<KEY>` (MVP-008, FR-031, FR-186).
/// The kind of a requirement, from its key prefix: `NFR-` keys are non-functional.
pub fn requirement_kind(key: &str) -> &'static str {
    if key.starts_with("NFR-") {
        "non-functional"
    } else {
        "functional"
    }
}

pub fn requirement_id(key: &str) -> String {
    format!("req:{key}")
}

/// The node ID of a file. File identity is path-based (FR-035).
pub fn file_id(path: &str) -> String {
    format!("file:{path}")
}

/// The ID of a derived `contains` edge.
pub fn contains_id(parent: &str, child: &str) -> String {
    format!("contains:{parent}>{child}")
}

/// True if `s` is a requirement key such as `FR-042`, `NFR-030` or `FR-012a`.
pub fn is_requirement_key(s: &str) -> bool {
    let Some((prefix, rest)) = s.split_once('-') else {
        return false;
    };
    if prefix.is_empty() || !prefix.chars().all(|c| c.is_ascii_uppercase()) {
        return false;
    }
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return false;
    }
    let suffix = &rest[digits.len()..];
    suffix.is_empty() || (suffix.len() == 1 && suffix.chars().all(|c| c.is_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_lowercase_hex_sha256() {
        assert_eq!(
            content_hash(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn ulids_sort_by_creation() {
        let a = new_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = new_id();
        assert!(a < b);
        assert!(is_ulid(&a));
        assert!(!is_ulid("FR-042"));
    }

    #[test]
    fn requirement_keys() {
        for ok in ["FR-042", "NFR-030", "FR-012a", "OQ-04"] {
            assert!(is_requirement_key(ok), "{ok}");
        }
        for bad in ["fr-042", "FR-", "FR-12AB", "FR42", "-1", "FR-1ab"] {
            assert!(!is_requirement_key(bad), "{bad}");
        }
    }
}
