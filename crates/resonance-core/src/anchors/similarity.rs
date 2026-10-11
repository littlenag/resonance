//! Approximate quote and context matching with confidence scores (MVP-013).
//!
//! Similarity is character-based: the share of characters two texts have in common, as
//! found by a character diff (`similar`'s ratio). Windows are first screened with a cheap
//! trigram overlap so that only promising candidates are diffed.

use std::collections::HashMap;
use std::time::Duration;

use similar::TextDiffConfig;

/// Character similarity of `a` and `b`, from 0.0 to 1.0.
pub fn ratio(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    TextDiffConfig::default()
        .timeout(Duration::from_millis(250))
        .diff_chars(a, b)
        .ratio() as f64
}

/// How well a candidate's surroundings agree with the stored context: the mean similarity of
/// the sides that have stored context, or 1.0 when none does (a region at both file ends).
pub fn context_score(stored_prefix: &str, stored_suffix: &str, prefix: &str, suffix: &str) -> f64 {
    let mut total = 0.0;
    let mut sides = 0.0;
    if !stored_prefix.trim().is_empty() {
        total += ratio(stored_prefix, prefix);
        sides += 1.0;
    }
    if !stored_suffix.trim().is_empty() {
        total += ratio(stored_suffix, suffix);
        sides += 1.0;
    }
    if sides == 0.0 { 1.0 } else { total / sides }
}

/// Character trigrams of a line with runs of whitespace collapsed, as hashed keys.
pub fn line_trigrams(line: &str) -> Vec<u64> {
    let normalized: Vec<char> = {
        let mut out = Vec::new();
        let mut space = false;
        for c in line.trim().chars() {
            if c.is_whitespace() {
                if !space {
                    out.push(' ');
                }
                space = true;
            } else {
                out.push(c);
                space = false;
            }
        }
        out
    };
    if normalized.is_empty() {
        return Vec::new();
    }
    if normalized.len() < 3 {
        return vec![key(&normalized)];
    }
    normalized.windows(3).map(key).collect()
}

fn key(chars: &[char]) -> u64 {
    chars.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, c| {
        (h ^ *c as u64).wrapping_mul(0x100_0000_01b3)
    })
}

/// A multiset of trigrams.
pub fn trigram_counts<'a>(lines: impl IntoIterator<Item = &'a str>) -> HashMap<u64, u32> {
    let mut counts = HashMap::new();
    for line in lines {
        for t in line_trigrams(line) {
            *counts.entry(t).or_insert(0) += 1;
        }
    }
    counts
}

/// For every window of `width` consecutive lines, the Dice coefficient of its trigram
/// multiset against `quote`'s. Computed incrementally, in time linear in the file.
pub fn window_dice(lines: &[&str], width: usize, quote: &HashMap<u64, u32>) -> Vec<f64> {
    if width == 0 || width > lines.len() {
        return Vec::new();
    }
    let quote_total: u32 = quote.values().sum();
    let line_grams: Vec<Vec<u64>> = lines.iter().map(|l| line_trigrams(l)).collect();
    let mut window: HashMap<u64, u32> = HashMap::new();
    let mut inter: u32 = 0;
    let mut total: u32 = 0;
    let add = |t: u64, window: &mut HashMap<u64, u32>, inter: &mut u32| {
        let c = window.entry(t).or_insert(0);
        if *c < quote.get(&t).copied().unwrap_or(0) {
            *inter += 1;
        }
        *c += 1;
    };
    let remove = |t: u64, window: &mut HashMap<u64, u32>, inter: &mut u32| {
        if let Some(c) = window.get_mut(&t) {
            *c -= 1;
            if *c < quote.get(&t).copied().unwrap_or(0) {
                *inter -= 1;
            }
        }
    };
    let mut out = Vec::with_capacity(lines.len() - width + 1);
    for (i, grams) in line_grams.iter().enumerate() {
        for &t in grams {
            add(t, &mut window, &mut inter);
        }
        total += grams.len() as u32;
        if i >= width {
            for &t in &line_grams[i - width] {
                remove(t, &mut window, &mut inter);
            }
            total -= line_grams[i - width].len() as u32;
        }
        if i + 1 >= width {
            let denom = quote_total + total;
            out.push(if denom == 0 {
                0.0
            } else {
                2.0 * inter as f64 / denom as f64
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratios() {
        assert_eq!(ratio("abc", "abc"), 1.0);
        assert_eq!(ratio("", "abc"), 0.0);
        let r = ratio(
            "    let x = 1;\n    let y = 2;",
            "  let x = 1;\n  let y = 2;",
        );
        assert!(r > 0.85 && r < 1.0, "{r}");
        assert!(ratio("completely different", "nothing alike at all") < 0.5);
    }

    #[test]
    fn context_scores() {
        assert_eq!(context_score("", "", "x", "y"), 1.0);
        assert_eq!(context_score("a\n", "", "a\n", "zzz"), 1.0);
        assert!(context_score("abc\n", "def", "abc\n", "xyz") < 0.75);
    }

    #[test]
    fn windows_find_the_reindented_region() {
        let file = ["fn a() {}", "fn target() {", "  body();", "}", "fn b() {}"];
        let quote = trigram_counts(["fn target() {", "    body();", "}"]);
        let scores = window_dice(&file, 3, &quote);
        let best = scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        assert_eq!(best.0, 1);
        assert!(*best.1 > 0.9);
    }
}
