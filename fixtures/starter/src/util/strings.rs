/// Trims, lower-cases and collapses inner whitespace.
pub fn normalize(name: &str) -> String {
    name.split_whitespace()
        .map(|w| w.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}
