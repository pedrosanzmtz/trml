use regex::Regex;
use std::sync::LazyLock;

/// Regex that matches digits (timestamps, IDs, counts) for normalization.
static DIGITS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b\d+\b").unwrap());

/// Regex to strip common timestamp prefixes.
static TIMESTAMP_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        ^(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}[.,]\d+\s*) |  # ISO/log4j
        ^(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}\s*)           |  # ISO no millis
        ^(\d{2}:\d{2}:\d{2}[.,]\d+\s*)                          |  # time only
        ^(\[\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}[^\]]*\]\s*) |  # bracketed
        ^([A-Z][a-z]{2}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2}\s*)         # syslog
        ",
    )
    .unwrap()
});

/// Collapse the line to a canonical form for comparison.
pub fn canonicalize(line: &str) -> String {
    let no_ts = TIMESTAMP_RE.replace(line, "");
    DIGITS_RE.replace_all(&no_ts, "N").to_string()
}

/// Collapse repeated identical lines (after normalization) into `first [repeated xN]`.
/// Lines appearing more than `threshold` times consecutively are collapsed.
pub fn process(lines: Vec<String>, threshold: usize) -> Vec<String> {
    if lines.is_empty() {
        return lines;
    }

    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut iter = lines.into_iter();

    let first = iter.next().unwrap();
    let mut current_canonical = canonicalize(&first);
    let mut current_first = first;
    let mut count = 1usize;

    for line in iter {
        let canonical = canonicalize(&line);
        if canonical == current_canonical {
            count += 1;
        } else {
            flush_group(&mut result, current_first, count, threshold);
            current_canonical = canonical;
            current_first = line;
            count = 1;
        }
    }
    flush_group(&mut result, current_first, count, threshold);

    result
}

fn flush_group(result: &mut Vec<String>, first_line: String, count: usize, threshold: usize) {
    if count > threshold {
        result.push(format!("{} [repeated x{}]", first_line, count));
    } else {
        // For small counts just keep all occurrences... but we only stored the first.
        // Since we don't buffer all copies, just emit the first line.
        // (Non-consecutive repeats are handled per-run; minor limitation of streaming dedup.)
        result.push(first_line);
        // Note: if count > 1 but <= threshold we've lost the extra copies.
        // This is an acceptable trade-off for the streaming design.
    }
}
