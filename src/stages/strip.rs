use crate::stages::Stage;
use regex::Regex;
use std::sync::LazyLock;

static ANSI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\x1b\[[0-9;]*[a-zA-Z]").unwrap());

/// Process a single line: strip ANSI codes and trim trailing whitespace.
pub fn process_line(line: String) -> String {
    let stripped = ANSI_RE.replace_all(&line, "");
    stripped.trim_end().to_string()
}

/// Batch process (used in explain instrumentation).
pub fn process(lines: Vec<String>) -> Vec<String> {
    lines.into_iter().map(process_line).collect()
}

/// Streaming stage for ANSI stripping / whitespace normalization.
pub struct StripStage;

impl Stage for StripStage {
    fn push(&mut self, line: String) -> Vec<String> {
        vec![process_line(line)]
    }

    fn flush(&mut self) -> Vec<String> {
        vec![]
    }
}
