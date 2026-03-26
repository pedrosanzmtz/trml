use regex::Regex;
use std::sync::LazyLock;

static ANSI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());

/// Remove ANSI escape codes and normalize whitespace.
pub fn process(lines: Vec<String>) -> Vec<String> {
    lines
        .into_iter()
        .map(|line| {
            let stripped = ANSI_RE.replace_all(&line, "");
            // Trim trailing whitespace but preserve leading (indentation matters for stack traces)
            stripped.trim_end().to_string()
        })
        .collect()
}
