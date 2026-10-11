//! Re-locates anchors after a change: diff mapping, exact quote, cross-file move, approximate,
//! orphan (FR-032, FR-033, MVP-013).
//!
//! Everything here is a pure function of the anchor and file contents, so it can be tested
//! on its own and gives the same answer on every machine.

use similar::{Algorithm, DiffOp, capture_diff_slices};

use super::similarity::{context_score, ratio, trigram_counts, window_dice};
use super::{Anchor, Method, context_after, context_before, line_starts, lines_to_bytes};

/// Minimum similarity for an approximate match (MVP-013 step 4).
pub const APPROXIMATE_MIN: f64 = 0.8;
/// Approximate matches below this go on the repair list (MVP-013, FR-038).
pub const CONFIDENT: f64 = 0.9;
/// Minimum context agreement for an approximate match.
pub const CONTEXT_MIN: f64 = 0.5;
/// Minimum context agreement for restoring an orphaned anchor (MVP-005).
pub const RESTORE_CONTEXT_MIN: f64 = 0.8;
/// Confidence given to an exact quote found more than once with equally good context.
pub const AMBIGUOUS_CONFIDENCE: f64 = 0.85;

/// Which MVP-013 step placed the anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The file did not change.
    Unchanged,
    Diff,
    Exact,
    Move,
    Approximate,
    Orphaned,
}

/// The result of re-locating one anchor.
#[derive(Debug, Clone, PartialEq)]
pub struct Relocation {
    pub anchor: Anchor,
    pub step: Step,
}

/// Re-locate `anchor` after its file changed from `old` to `new` (`None` when the file was
/// deleted, or its old content is not stored). `others` are the other files that changed in
/// the same reconcile, as `(path, content)`, searched for a move (step 3).
pub fn relocate(
    anchor: &Anchor,
    old: Option<&str>,
    new: Option<&str>,
    others: &[(&str, &str)],
) -> Relocation {
    relocate_with(anchor, old, new, others, &mut |_| Vec::new())
}

/// [`relocate`], with `unchanged` supplying, on demand, files that did not change in this
/// reconcile but may contain the quote (step 3b, MVP-024).
pub fn relocate_with(
    anchor: &Anchor,
    old: Option<&str>,
    new: Option<&str>,
    others: &[(&str, &str)],
    unchanged: &mut dyn FnMut(&Anchor) -> Vec<(String, String)>,
) -> Relocation {
    if let (Some(o), Some(n)) = (old, new)
        && o == n
    {
        return Relocation {
            anchor: anchor.clone(),
            step: Step::Unchanged,
        };
    }
    // Step 1: map the range through the line diff.
    if let (Some(o), Some(n)) = (old, new)
        && let Some(a) = diff_map(anchor, o, n)
    {
        return Relocation {
            anchor: a,
            step: Step::Diff,
        };
    }
    // Step 2: exact quote search in the same file.
    if let Some(n) = new
        && let Some(a) = exact_in(anchor, &anchor.path, n, Method::Exact)
    {
        return Relocation {
            anchor: a,
            step: Step::Exact,
        };
    }
    // Step 3: exact quote search in the other changed files (a move).
    let candidates: Vec<(&str, &str)> = others
        .iter()
        .copied()
        .filter(|(p, _)| *p != anchor.path)
        .collect();
    if let Some(a) = best_exact(anchor, &candidates, Method::Move, 0.0) {
        return Relocation {
            anchor: a,
            step: Step::Move,
        };
    }
    // Step 3b: exact quote search in files that did not change (MVP-024).
    let extra = unchanged(anchor);
    let extra: Vec<(&str, &str)> = extra
        .iter()
        .filter(|(p, _)| *p != anchor.path && !others.iter().any(|(o, _)| o == p))
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    if let Some(a) = best_exact(anchor, &extra, Method::Move, 0.0) {
        return Relocation {
            anchor: a,
            step: Step::Move,
        };
    }
    // Step 4: approximate match in the same file.
    if let Some(n) = new
        && let Some(a) = approximate(anchor, n)
    {
        return Relocation {
            anchor: a,
            step: Step::Approximate,
        };
    }
    // Step 5: orphan, keeping the last quote and location (FR-034).
    Relocation {
        anchor: anchor.orphaned(),
        step: Step::Orphaned,
    }
}

/// MVP-005: an orphaned anchor is restored when its quote reappears exactly, with matching
/// context, in one of `files`.
pub fn restore(anchor: &Anchor, files: &[(&str, &str)]) -> Option<Anchor> {
    best_exact(anchor, files, Method::Restored, RESTORE_CONTEXT_MIN)
}

fn split_lines(s: &str) -> Vec<&str> {
    s.split_inclusive('\n').collect()
}

/// A changed line paired with an old one counts as surviving at this similarity (MVP-024).
pub const LINE_SURVIVAL_MIN: f64 = 0.6;

/// What happened to one old line.
#[derive(Debug, Clone, Copy)]
enum LineFate {
    /// Unchanged, now at this new line.
    Same(usize),
    /// Changed, paired with this new line at this similarity.
    Modified(usize, f64),
    /// Deleted, or rewritten beyond recognition; the new text continues at this line.
    Gone(usize),
}

/// Step 1. Follows the range through the line diff if at least half the anchored lines
/// survive, including lines inserted inside it. A changed line survives when it is paired
/// with a new line at least [`LINE_SURVIVAL_MIN`] similar; boundaries inside changed lines are
/// mapped through a character diff, so snippet anchors keep their exact columns (MVP-024).
///
/// When unchanged lines alone are at least half, confidence is 1.0. Otherwise it is the mean
/// similarity of the anchored lines, and a mapping below [`APPROXIMATE_MIN`] is refused.
pub fn diff_map(anchor: &Anchor, old: &str, new: &str) -> Option<Anchor> {
    if old.get(anchor.start_byte..anchor.end_byte) != Some(anchor.quote.as_str()) {
        return None; // The stored old content does not match the anchor; no basis for mapping.
    }
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let old_starts = line_starts(old);
    let new_starts = line_starts(new);
    if old_lines.is_empty() || new_lines.is_empty() {
        return None;
    }
    let first = anchor.start_line - 1;
    let last = (anchor.end_line - 1).min(old_lines.len() - 1);
    let ops = capture_diff_slices(Algorithm::Patience, &old_lines, &new_lines);

    let mut fate: Vec<LineFate> = vec![LineFate::Gone(0); old_lines.len()];
    for op in &ops {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => {
                for k in 0..len {
                    fate[old_index + k] = LineFate::Same(new_index + k);
                }
            }
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => {
                for k in 0..old_len {
                    fate[old_index + k] = LineFate::Gone(new_index);
                }
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => {
                // Pair each old line with the most similar remaining new line, in order.
                let mut next = new_index;
                for k in 0..old_len {
                    let o = old_lines[old_index + k];
                    let mut best: Option<(usize, f64)> = None;
                    for (j, line) in new_lines
                        .iter()
                        .enumerate()
                        .take(new_index + new_len)
                        .skip(next)
                    {
                        let sim = ratio(o.trim_end(), line.trim_end());
                        if best.is_none_or(|(_, b)| sim > b) {
                            best = Some((j, sim));
                        }
                    }
                    fate[old_index + k] = match best {
                        Some((j, sim)) if sim >= LINE_SURVIVAL_MIN => {
                            next = j + 1;
                            LineFate::Modified(j, sim)
                        }
                        _ => LineFate::Gone(next.min(new_index + new_len)),
                    };
                }
            }
            DiffOp::Insert { .. } => {}
        }
    }

    let total = last - first + 1;
    let anchored = &fate[first..=last];
    let exact = anchored
        .iter()
        .filter(|f| matches!(f, LineFate::Same(_)))
        .count();
    let surviving = anchored
        .iter()
        .filter(|f| !matches!(f, LineFate::Gone(_)))
        .count();
    if surviving * 2 < total || surviving == 0 {
        return None;
    }
    let confidence = if exact * 2 >= total {
        1.0
    } else {
        let sum: f64 = anchored
            .iter()
            .map(|f| match f {
                LineFate::Same(_) => 1.0,
                LineFate::Modified(_, s) => *s,
                LineFate::Gone(_) => 0.0,
            })
            .sum();
        sum / total as f64
    };
    if confidence < APPROXIMATE_MIN {
        return None;
    }

    let new_first = match fate[first] {
        LineFate::Same(n) | LineFate::Modified(n, _) | LineFate::Gone(n) => n,
    };
    let new_last = match fate[last] {
        LineFate::Same(n) | LineFate::Modified(n, _) => n,
        LineFate::Gone(n) => n.saturating_sub(1),
    };
    let new_last = new_last.max(new_first).min(new_lines.len() - 1);
    if new_first >= new_lines.len() {
        return None;
    }

    let start_col = anchor.start_byte - old_starts[first];
    let end_col = anchor.end_byte - old_starts[last];
    let start = new_starts[new_first]
        + match fate[first] {
            LineFate::Same(_) => start_col,
            LineFate::Modified(n, _) => {
                map_offset(old_lines[first], new_lines[n], start_col, false)
            }
            LineFate::Gone(_) => 0,
        };
    let end = match fate[last] {
        LineFate::Same(_) => new_starts[new_last] + end_col,
        LineFate::Modified(n, _) => {
            new_starts[new_last] + map_offset(old_lines[last], new_lines[n], end_col, true)
        }
        LineFate::Gone(_) => lines_to_bytes(new, new_last + 1, new_last + 1).ok()?.1,
    };
    if end <= start || !new.is_char_boundary(start) || !new.is_char_boundary(end) {
        return None;
    }
    let mut found = Anchor::at(&anchor.path, new, start, end, Method::Diff);
    found.confidence = (confidence * 1000.0).round() / 1000.0;
    Some(found)
}

/// Map a byte offset in `old` to the corresponding byte offset in `new` through a character
/// diff. An `end` offset (exclusive) that follows unchanged text stays after it.
pub fn map_offset(old: &str, new: &str, offset: usize, end: bool) -> usize {
    let old_chars: Vec<(usize, char)> = old.char_indices().collect();
    let new_chars: Vec<(usize, char)> = new.char_indices().collect();
    let to_char = |byte: usize| {
        old_chars
            .iter()
            .position(|(b, _)| *b >= byte)
            .unwrap_or(old_chars.len())
    };
    let to_byte = |ci: usize| new_chars.get(ci).map(|(b, _)| *b).unwrap_or(new.len());
    let ci = to_char(offset);
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(100))
        .diff_chars(old, new);
    // For an end offset, map the last character before it and step past it.
    let probe = if end && ci > 0 { ci - 1 } else { ci };
    for op in diff.ops() {
        match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } if (old_index..old_index + len).contains(&probe) => {
                let n = new_index + (probe - old_index);
                return to_byte(if end && ci > 0 { n + 1 } else { n });
            }
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } if (old_index..old_index + old_len).contains(&probe) => {
                return to_byte(new_index);
            }
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } if (old_index..old_index + old_len).contains(&probe) => {
                return to_byte(if end { new_index + new_len } else { new_index });
            }
            _ => {}
        }
    }
    if ci >= old_chars.len() {
        new.len()
    } else {
        to_byte(ci)
    }
}

/// Step 2 (and the per-file part of steps 3 and MVP-005): exact quote search in one file.
pub fn exact_in(anchor: &Anchor, path: &str, content: &str, method: Method) -> Option<Anchor> {
    best_exact(anchor, &[(path, content)], method, 0.0)
}

/// The best exact occurrence of the quote across `files`, judged by context. Occurrences
/// whose context agrees less than `min_context` are ignored. If two occurrences are equally
/// good, the match is ambiguous and gets [`AMBIGUOUS_CONFIDENCE`], which puts it on the
/// repair list.
fn best_exact(
    anchor: &Anchor,
    files: &[(&str, &str)],
    method: Method,
    min_context: f64,
) -> Option<Anchor> {
    if anchor.quote.is_empty() {
        return None;
    }
    let mut hits: Vec<(f64, usize, &str, &str, usize)> = Vec::new();
    for (path, content) in files {
        for (start, _) in content.match_indices(anchor.quote.as_str()) {
            let end = start + anchor.quote.len();
            let score = context_score(
                &anchor.prefix,
                &anchor.suffix,
                &context_before(content, start),
                &context_after(content, end),
            );
            if score < min_context {
                continue;
            }
            let distance = if *path == anchor.path {
                start.abs_diff(anchor.start_byte)
            } else {
                usize::MAX
            };
            hits.push((score, distance, path, content, start));
        }
    }
    hits.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    let (score, _, path, content, start) = *hits.first()?;
    let ambiguous = hits.get(1).is_some_and(|h| (h.0 - score).abs() < 1e-9);
    let mut found = Anchor::at(path, content, start, start + anchor.quote.len(), method);
    if ambiguous {
        found.confidence = AMBIGUOUS_CONFIDENCE;
    }
    Some(found)
}

/// Step 4. The best window of whole lines whose similarity to the quote is at least
/// [`APPROXIMATE_MIN`] and whose context also agrees. Confidence is the similarity.
pub fn approximate(anchor: &Anchor, content: &str) -> Option<Anchor> {
    let starts = line_starts(content);
    let file_lines: Vec<&str> = (0..starts.len() - 1)
        .map(|i| content[starts[i]..starts[i + 1]].trim_end_matches(['\n', '\r']))
        .collect();
    if file_lines.is_empty() || anchor.quote.trim().is_empty() {
        return None;
    }
    let quote_lines: Vec<&str> = anchor.quote.split('\n').collect();
    let n = quote_lines.len();
    let slack = (n / 4).clamp(1, 20);
    let quote_grams = trigram_counts(quote_lines.iter().copied());

    let mut windows: Vec<(f64, usize, usize)> = Vec::new();
    for width in n.saturating_sub(slack).max(1)..=n + slack {
        for (i, dice) in window_dice(&file_lines, width, &quote_grams)
            .into_iter()
            .enumerate()
        {
            if dice >= 0.5 {
                windows.push((dice, i, width));
            }
        }
    }
    windows.sort_by(|a, b| b.0.total_cmp(&a.0));
    windows.truncate(12);

    // A snippet anchor (starting or ending inside a line) is compared with the text in the
    // window that aligns with its quote, rather than whole lines (MVP-024).
    let starts_mid_line = !anchor.prefix.is_empty() && !anchor.prefix.ends_with('\n');
    let ends_mid_line = !anchor.suffix.is_empty() && !anchor.suffix.starts_with(['\n', '\r']);
    let snippet = starts_mid_line || ends_mid_line;
    let mut best: Option<(f64, f64, usize, usize)> = None;
    for (_, i, width) in windows {
        let (mut s, mut e) = lines_to_bytes(content, i + 1, i + width).ok()?;
        let mut sim = ratio(&anchor.quote, &content[s..e]);
        if snippet && let Some((rs, re)) = align(&anchor.quote, &content[s..e]) {
            let refined = ratio(&anchor.quote, &content[s + rs..s + re]);
            if refined >= sim {
                sim = refined;
                (s, e) = (s + rs, s + re);
            }
        }
        if sim < APPROXIMATE_MIN {
            continue;
        }
        let ctx = context_score(
            &anchor.prefix,
            &anchor.suffix,
            &context_before(content, s),
            &context_after(content, e),
        );
        if ctx < CONTEXT_MIN {
            continue;
        }
        let better = match best {
            None => true,
            Some((bs, bc, _, _)) => sim > bs + 1e-9 || ((sim - bs).abs() < 1e-9 && ctx > bc),
        };
        if better {
            best = Some((sim, ctx, s, e));
        }
    }
    let (sim, _, s, e) = best?;
    let mut found = Anchor::at(&anchor.path, content, s, e, Method::Approximate);
    found.confidence = (sim * 1000.0).round() / 1000.0;
    Some(found)
}

/// The byte range of `window` that aligns with `quote`: from the first to the last character
/// a character diff finds in common.
fn align(quote: &str, window: &str) -> Option<(usize, usize)> {
    let new_chars: Vec<usize> = window.char_indices().map(|(b, _)| b).collect();
    let diff = similar::TextDiff::configure()
        .timeout(std::time::Duration::from_millis(100))
        .diff_chars(quote, window);
    let equal: Vec<(usize, usize)> = diff
        .ops()
        .iter()
        .filter_map(|op| match *op {
            DiffOp::Equal { new_index, len, .. } => Some((new_index, new_index + len)),
            _ => None,
        })
        .collect();
    let (first, _) = *equal.first()?;
    let (_, last) = *equal.last()?;
    let to_byte = |ci: usize| new_chars.get(ci).copied().unwrap_or(window.len());
    Some((to_byte(first), to_byte(last)))
}
