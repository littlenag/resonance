//! An in-memory index from lines of text to the files that contain them, for finding regions
//! that moved into files which did not change in the same reconcile (MVP-024, step 3b).
//!
//! It is derived data: built lazily from the stored file contents the first time it is
//! needed, kept up to date as files change, and never persisted.

use std::collections::{BTreeSet, HashMap};

use crate::error::Result;
use crate::model::ids;
use crate::store::GraphRead;

/// Lines shorter than this (trimmed) are too common to identify a region.
const MIN_LINE: usize = 8;
/// At most this many candidate files are searched for one quote.
const MAX_CANDIDATES: usize = 25;

#[derive(Debug, Default)]
pub struct LineIndex {
    built: bool,
    files: HashMap<u64, BTreeSet<String>>,
    by_path: HashMap<String, Vec<u64>>,
}

fn key(line: &str) -> Option<u64> {
    let t = line.trim();
    if t.chars().count() < MIN_LINE {
        return None;
    }
    Some(t.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
    }))
}

impl LineIndex {
    pub fn is_built(&self) -> bool {
        self.built
    }

    /// Index every stored text file.
    pub fn build(&mut self, r: &dyn GraphRead) -> Result<()> {
        self.files.clear();
        self.by_path.clear();
        for record in r.list_files()? {
            let Some(node) = r.get_node(&ids::file_id(&record.path))? else {
                continue;
            };
            if node.retired {
                continue;
            }
            let text = node
                .str_prop("blob")
                .map(|b| r.get_blob(b))
                .transpose()?
                .flatten()
                .and_then(|bytes| String::from_utf8(bytes).ok());
            self.set(&record.path, text.as_deref());
        }
        self.built = true;
        Ok(())
    }

    /// Replace what is indexed for `path` (`None` when it is gone or not text).
    pub fn set(&mut self, path: &str, text: Option<&str>) {
        if let Some(old) = self.by_path.remove(path) {
            for k in old {
                if let Some(set) = self.files.get_mut(&k) {
                    set.remove(path);
                    if set.is_empty() {
                        self.files.remove(&k);
                    }
                }
            }
        }
        let Some(text) = text else { return };
        let mut keys: Vec<u64> = text.lines().filter_map(key).collect();
        keys.sort_unstable();
        keys.dedup();
        for k in &keys {
            self.files.entry(*k).or_default().insert(path.to_string());
        }
        self.by_path.insert(path.to_string(), keys);
    }

    /// Files that contain the quote's rarest distinctive line.
    pub fn candidates(&self, quote: &str) -> Vec<String> {
        quote
            .lines()
            .filter_map(key)
            .filter_map(|k| self.files.get(&k))
            .min_by_key(|set| set.len())
            .map(|set| set.iter().take(MAX_CANDIDATES).cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_by_their_rarest_line() {
        let mut idx = LineIndex::default();
        idx.set("a.rs", Some("fn shared_helper() {}\nfn only_in_a() {}\n"));
        idx.set("b.rs", Some("fn shared_helper() {}\n"));
        assert_eq!(
            idx.candidates("fn shared_helper() {}\nfn only_in_a() {}"),
            ["a.rs"]
        );
        assert_eq!(idx.candidates("fn shared_helper() {}").len(), 2);
        idx.set("a.rs", None);
        assert!(idx.candidates("fn only_in_a() {}").is_empty());
        assert!(
            idx.candidates("x").is_empty(),
            "short lines are not indexed"
        );
    }
}
