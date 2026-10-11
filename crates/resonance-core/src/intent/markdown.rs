//! Imports `FR-`/`NFR-` requirements from Markdown lists and tables (MVP-011).
//!
//! Recognized forms, where `KEY` matches `(FR|NFR)-\d+[a-z]?`:
//!
//! - list items `- **KEY** text`, including indented continuation lines and sub-lists;
//! - table rows `| **KEY** | text | … |`.
//!
//! A trailing status label (`[Decided]`, `[Proposed]`, `[Open]`, or a qualified form such
//! as `[Decided in substance; Proposed: details]`) is recorded as `specStatus`.

/// One requirement found in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequirement {
    pub key: String,
    pub text: String,
    pub spec_status: Option<String>,
    /// 1-based line of the definition.
    pub line: usize,
}

/// Whether `key` is an MVP-011 requirement key: `(FR|NFR)-\d+[a-z]?`.
pub fn is_import_key(key: &str) -> bool {
    let rest = if let Some(r) = key.strip_prefix("NFR-") {
        r
    } else if let Some(r) = key.strip_prefix("FR-") {
        r
    } else {
        return false;
    };
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return false;
    }
    let tail = &rest[digits..];
    tail.is_empty() || (tail.len() == 1 && tail.chars().all(|c| c.is_ascii_lowercase()))
}

/// `**KEY**` at the start of `s`: returns the key and what follows.
fn bold_key(s: &str) -> Option<(&str, &str)> {
    let inner = s.strip_prefix("**")?;
    let end = inner.find("**")?;
    let key = &inner[..end];
    is_import_key(key).then(|| (key, &inner[end + 2..]))
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn list_marker(line: &str) -> Option<&str> {
    let t = line.trim_start();
    ["- ", "* ", "+ "].iter().find_map(|m| t.strip_prefix(m))
}

fn is_status_label(inner: &str) -> bool {
    let t = inner.trim_start();
    t.starts_with("Decided") || t.starts_with("Proposed") || t.starts_with("Open")
}

/// Split a trailing status label off `text`.
pub fn split_status(text: &str) -> (String, Option<String>) {
    let trimmed = text.trim_end();
    if !trimmed.ends_with(']') {
        return (trimmed.to_string(), None);
    }
    // Find the `[` that matches the final `]`.
    let mut depth = 0i32;
    for (i, c) in trimmed.char_indices().rev() {
        match c {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    let inner = &trimmed[i + 1..trimmed.len() - 1];
                    if is_status_label(inner) {
                        return (
                            trimmed[..i].trim_end().to_string(),
                            Some(inner.trim().to_string()),
                        );
                    }
                    return (trimmed.to_string(), None);
                }
            }
            _ => {}
        }
    }
    (trimmed.to_string(), None)
}

/// Split a table row into cells, honoring `\|` escapes.
fn table_cells(line: &str) -> Vec<String> {
    let t = line.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = t.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => cells.push(std::mem::take(&mut cur).trim().to_string()),
            _ => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

/// Every requirement defined in `text`, in document order.
pub fn parse(text: &str) -> Vec<ParsedRequirement> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut fence: Option<&str> = None;
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        // Skip fenced code blocks.
        if let Some(f) = fence {
            if trimmed.starts_with(f) {
                fence = None;
            }
            i += 1;
            continue;
        }
        if trimmed.starts_with("```") {
            fence = Some("```");
            i += 1;
            continue;
        }
        if trimmed.starts_with("~~~") {
            fence = Some("~~~");
            i += 1;
            continue;
        }

        if let Some((key, rest)) = list_marker(line).and_then(bold_key) {
            let base = indent_of(line);
            let mut body = vec![rest.trim().to_string()];
            let mut j = i + 1;
            while j < lines.len() {
                let l = lines[j];
                if l.trim().is_empty() {
                    // A blank line continues the item only if indented content follows.
                    let next = lines[j + 1..].iter().find(|n| !n.trim().is_empty());
                    match next {
                        Some(n) if indent_of(n) > base => {
                            body.push(String::new());
                            j += 1;
                            continue;
                        }
                        _ => break,
                    }
                }
                if indent_of(l) <= base {
                    break;
                }
                body.push(l.trim().to_string());
                j += 1;
            }
            let joined = body.join("\n").trim().to_string();
            let (text, spec_status) = split_status(&joined);
            out.push(ParsedRequirement {
                key: key.to_string(),
                text,
                spec_status,
                line: i + 1,
            });
            i = j;
            continue;
        }

        if trimmed.starts_with('|') {
            let cells = table_cells(line);
            if let Some((key, after)) = cells.first().and_then(|c| bold_key(c))
                && after.trim().is_empty()
            {
                let body = cells.get(1).cloned().unwrap_or_default();
                let (text, mut spec_status) = split_status(&body);
                if spec_status.is_none() {
                    spec_status = cells[2..].iter().rev().find_map(|c| {
                        let inner = c.strip_prefix('[')?.strip_suffix(']')?;
                        is_status_label(inner).then(|| inner.trim().to_string())
                    });
                }
                out.push(ParsedRequirement {
                    key: key.to_string(),
                    text,
                    spec_status,
                    line: i + 1,
                });
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        for ok in ["FR-001", "NFR-030", "FR-012a"] {
            assert!(is_import_key(ok), "{ok}");
        }
        for bad in ["OQ-01", "FR-", "FR-1AB", "XFR-1", "MVP-001", "FR-12ab"] {
            assert!(!is_import_key(bad), "{bad}");
        }
    }

    #[test]
    fn list_items_with_continuations_and_labels() {
        let doc = "\
# Title

- **FR-001** Every edge MUST have exactly one status. [Proposed]
- **FR-032** After each change:
  1. Map the range.
  2. Search the file.

  No parsing is involved. [Proposed]
- **FR-033 (partial)** Not an MVP-011 key form.
- not a requirement **FR-099** here
  - **NFR-013** Nested items count too. [Decided in substance; Proposed: details]

Paragraph.
";
        let reqs = parse(doc);
        let keys: Vec<&str> = reqs.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["FR-001", "FR-032", "NFR-013"]);
        assert_eq!(reqs[0].text, "Every edge MUST have exactly one status.");
        assert_eq!(reqs[0].spec_status.as_deref(), Some("Proposed"));
        assert_eq!(reqs[0].line, 3);
        assert_eq!(
            reqs[1].text,
            "After each change:\n1. Map the range.\n2. Search the file.\n\nNo parsing is involved."
        );
        assert_eq!(reqs[1].spec_status.as_deref(), Some("Proposed"));
        assert_eq!(
            reqs[2].spec_status.as_deref(),
            Some("Decided in substance; Proposed: details")
        );
    }

    #[test]
    fn table_rows() {
        let doc = "\
| ID | Requirement | Target | Status |
| --- | --- | --- | --- |
| **NFR-030** | Single-file ingest latency | p95 under 200 ms | [Open] |
| **NFR-036** | No loss of a text write \\| ever | always | [Proposed] |
| FR-001 | not bold | x | [Open] |
";
        let reqs = parse(doc);
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].key, "NFR-030");
        assert_eq!(reqs[0].text, "Single-file ingest latency");
        assert_eq!(reqs[0].spec_status.as_deref(), Some("Open"));
        assert_eq!(reqs[1].text, "No loss of a text write | ever");
    }

    #[test]
    fn code_fences_are_skipped() {
        let doc = "```\n- **FR-001** inside a fence\n```\n- **FR-002** outside\n";
        let reqs = parse(doc);
        assert_eq!(reqs.len(), 1);
        assert_eq!(reqs[0].key, "FR-002");
    }

    #[test]
    fn labels_need_a_status_word() {
        assert_eq!(
            split_status("text [see §8]"),
            ("text [see §8]".into(), None)
        );
        assert_eq!(
            split_status("text [Decided: a [nested] thing]"),
            ("text".into(), Some("Decided: a [nested] thing".into()))
        );
    }
}
