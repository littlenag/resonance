//! Code units and tests as textual anchors: range, quote and context (MVP-013). No parsing.
//!
//! An anchor records where a declared region is (byte range and line range), what it says
//! (the quote and its hash), and what surrounds it (up to 3 lines, at most 256 bytes, on each
//! side). [`locate`] re-locates anchors after a file changes.

pub mod locate;
pub mod similarity;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::model::content_hash;

/// Lines of context kept on each side of a quote.
pub const CONTEXT_LINES: usize = 3;
/// Bytes of context kept on each side of a quote, at most.
pub const CONTEXT_BYTES: usize = 256;

/// Whether an anchor is attached to text, or orphaned with its last known location (FR-034).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnchorState {
    Attached,
    Orphaned,
}

/// How an anchor's current location was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Method {
    /// Declared by a person or agent, or re-anchored explicitly (FR-037).
    Declared,
    /// Followed through the line diff (MVP-013 step 1).
    Diff,
    /// Found by exact quote search in the same file (step 2).
    Exact,
    /// Found by exact quote search in another changed file (step 3).
    Move,
    /// Approximate match (step 4).
    Approximate,
    /// Could not be matched (step 5).
    Orphaned,
    /// An orphaned anchor whose quote reappeared with matching context (MVP-005).
    Restored,
    /// A low-confidence match confirmed by a person or agent.
    Confirmed,
}

/// A textual anchor (MVP-013).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Anchor {
    /// Repository-relative path of the file.
    pub path: String,
    pub start_byte: usize,
    pub end_byte: usize,
    /// 1-based, inclusive.
    pub start_line: usize,
    /// 1-based, inclusive.
    pub end_line: usize,
    pub quote: String,
    pub prefix: String,
    pub suffix: String,
    pub quote_hash: String,
    /// 1.0 for exact matches; the similarity for approximate ones (step 4).
    pub confidence: f64,
    pub state: AnchorState,
    pub method: Method,
}

impl Anchor {
    /// The anchor of `content[start..end]` in the file at `path`.
    pub fn at(path: &str, content: &str, start: usize, end: usize, method: Method) -> Anchor {
        let quote = &content[start..end];
        let (start_line, end_line) = line_span(content, start, end);
        Anchor {
            path: path.to_string(),
            start_byte: start,
            end_byte: end,
            start_line,
            end_line,
            quote: quote.to_string(),
            prefix: context_before(content, start),
            suffix: context_after(content, end),
            quote_hash: content_hash(quote.as_bytes()),
            confidence: 1.0,
            state: AnchorState::Attached,
            method,
        }
    }

    pub fn is_attached(&self) -> bool {
        self.state == AnchorState::Attached
    }

    /// The same anchor, orphaned: it keeps its last quote and location (FR-034).
    pub fn orphaned(&self) -> Anchor {
        Anchor {
            state: AnchorState::Orphaned,
            method: Method::Orphaned,
            ..self.clone()
        }
    }

    /// `path:start-end` (or `path:line`) for messages.
    pub fn location(&self) -> String {
        if self.start_line == self.end_line {
            format!("{}:{}", self.path, self.start_line)
        } else {
            format!("{}:{}-{}", self.path, self.start_line, self.end_line)
        }
    }
}

/// Byte offsets of the start of each line, plus `content.len()` at the end.
pub fn line_starts(content: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (i, b) in content.bytes().enumerate() {
        if b == b'\n' && i + 1 < content.len() {
            starts.push(i + 1);
        }
    }
    starts.push(content.len());
    starts
}

/// The number of lines in `content` (a trailing newline does not start a new line).
pub fn line_count(content: &str) -> usize {
    if content.is_empty() {
        0
    } else {
        line_starts(content).len() - 1
    }
}

/// The 0-based line containing byte `offset`.
fn line_index(starts: &[usize], offset: usize) -> usize {
    match starts[..starts.len() - 1].binary_search(&offset) {
        Ok(i) => i,
        Err(i) => i.saturating_sub(1),
    }
}

/// The 1-based inclusive line span of `[start, end)`.
pub fn line_span(content: &str, start: usize, end: usize) -> (usize, usize) {
    let starts = line_starts(content);
    let first = line_index(&starts, start);
    let last = line_index(&starts, end.saturating_sub(1).max(start));
    (first + 1, last + 1)
}

/// The byte range of lines `start..=end` (1-based), excluding the final line's newline.
pub fn lines_to_bytes(content: &str, start: usize, end: usize) -> Result<(usize, usize)> {
    let count = line_count(content);
    if start == 0 || end < start {
        return Err(Error::Usage(format!(
            "line range {start}-{end} is not valid; lines are numbered from 1"
        )));
    }
    if end > count {
        return Err(Error::Invalid(format!(
            "line {end} is past the end of the file, which has {count} line{}",
            if count == 1 { "" } else { "s" }
        )));
    }
    let starts = line_starts(content);
    let s = starts[start - 1];
    let mut e = starts[end];
    if e > s && content.as_bytes()[e - 1] == b'\n' {
        e -= 1;
    }
    if e > s && content.as_bytes()[e - 1] == b'\r' {
        e -= 1;
    }
    Ok((s, e))
}

/// Find the only occurrence of `snippet` in `content`.
pub fn find_unique(content: &str, snippet: &str) -> Result<(usize, usize)> {
    if snippet.is_empty() {
        return Err(Error::Usage("the snippet to match is empty".into()));
    }
    let hits: Vec<usize> = content.match_indices(snippet).map(|(i, _)| i).collect();
    match hits.as_slice() {
        [one] => Ok((*one, one + snippet.len())),
        [] => Err(Error::NotFound(format!(
            "the snippet {snippet:?} in this file"
        ))),
        many => Err(Error::Invalid(format!(
            "the snippet occurs {} times in this file; give a longer snippet or a line range",
            many.len()
        ))),
    }
}

fn floor_char_boundary(s: &str, mut i: usize) -> usize {
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn ceil_char_boundary(s: &str, mut i: usize) -> usize {
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

/// Up to [`CONTEXT_LINES`] lines (at most [`CONTEXT_BYTES`]) before byte `start`.
pub fn context_before(content: &str, start: usize) -> String {
    let before = &content[..start];
    // The partial line before `start`, plus up to CONTEXT_LINES whole lines.
    let mut cut = before.len();
    let mut newlines = 0;
    for (i, b) in before.bytes().enumerate().rev() {
        if b == b'\n' {
            newlines += 1;
            if newlines > CONTEXT_LINES {
                cut = i + 1;
                break;
            }
        }
        cut = i;
    }
    let from = cut.max(before.len().saturating_sub(CONTEXT_BYTES));
    let from = ceil_char_boundary(content, from);
    content[from..start].to_string()
}

/// Up to [`CONTEXT_LINES`] lines (at most [`CONTEXT_BYTES`]) after byte `end`.
pub fn context_after(content: &str, end: usize) -> String {
    let after = &content[end..];
    let mut take = after.len();
    let mut newlines = 0;
    for (i, b) in after.bytes().enumerate() {
        if b == b'\n' {
            newlines += 1;
            if newlines > CONTEXT_LINES {
                take = i;
                break;
            }
        }
    }
    let to = end + take.min(CONTEXT_BYTES);
    let to = floor_char_boundary(content, to);
    content[end..to].to_string()
}

/// A region typed as `path:START-END`, `path:LINE` or just `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionSpec {
    pub path: String,
    pub lines: Option<(usize, usize)>,
}

/// Parse `path:START-END` or `path:LINE`. A path without a line suffix is returned as is
/// (to be combined with `--match`).
pub fn parse_region(spec: &str) -> Result<RegionSpec> {
    if let Some((path, range)) = spec.rsplit_once(':') {
        let parsed = match range.split_once('-') {
            Some((a, b)) => a.trim().parse().ok().zip(b.trim().parse().ok()),
            None => range.trim().parse().ok().map(|n| (n, n)),
        };
        if let Some((a, b)) = parsed {
            if path.is_empty() {
                return Err(Error::Usage(format!("`{spec}` has no file path")));
            }
            return Ok(RegionSpec {
                path: path.to_string(),
                lines: Some((a, b)),
            });
        }
    }
    Ok(RegionSpec {
        path: spec.to_string(),
        lines: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "l1\nl2\nl3\nl4\nl5\nl6\nl7\nl8\n";

    #[test]
    fn line_ranges_map_to_bytes_without_the_last_newline() {
        let (s, e) = lines_to_bytes(TEXT, 2, 3).unwrap();
        assert_eq!(&TEXT[s..e], "l2\nl3");
        assert_eq!(line_span(TEXT, s, e), (2, 3));
        assert_eq!(line_count(TEXT), 8);
        assert_eq!(line_count("a\nb"), 2);
        assert!(lines_to_bytes(TEXT, 3, 9).is_err());
        assert!(lines_to_bytes(TEXT, 0, 1).is_err());
        let (s, e) = lines_to_bytes("only", 1, 1).unwrap();
        assert_eq!((s, e), (0, 4));
    }

    #[test]
    fn context_is_three_lines_each_side() {
        let (s, e) = lines_to_bytes(TEXT, 5, 5).unwrap();
        let a = Anchor::at("f", TEXT, s, e, Method::Declared);
        assert_eq!(a.prefix, "l2\nl3\nl4\n");
        assert_eq!(a.suffix, "\nl6\nl7\nl8");
        assert_eq!(a.quote, "l5");
        assert_eq!(a.location(), "f:5");
    }

    #[test]
    fn context_is_capped_at_256_bytes() {
        let long = format!("{}\nX\n{}\n", "a".repeat(400), "b".repeat(400));
        let start = long.find('X').unwrap();
        let a = Anchor::at("f", &long, start, start + 1, Method::Declared);
        assert_eq!(a.prefix.len(), CONTEXT_BYTES);
        assert_eq!(a.suffix.len(), CONTEXT_BYTES);
    }

    #[test]
    fn regions() {
        assert_eq!(
            parse_region("src/a.rs:10-20").unwrap(),
            RegionSpec {
                path: "src/a.rs".into(),
                lines: Some((10, 20))
            }
        );
        assert_eq!(parse_region("a.rs:7").unwrap().lines, Some((7, 7)));
        assert_eq!(parse_region("a.rs").unwrap().lines, None);
        assert_eq!(parse_region("c:/x").unwrap().lines, None);
    }

    #[test]
    fn unique_snippets() {
        assert_eq!(find_unique("ab cd ab", "cd").unwrap(), (3, 5));
        assert!(find_unique("ab cd ab", "ab").is_err());
        assert!(find_unique("ab", "zz").is_err());
    }
}
