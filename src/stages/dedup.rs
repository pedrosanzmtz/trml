use crate::stages::Stage;
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

/// Extract the timestamp prefix from a line (for annotations).
pub fn extract_timestamp(line: &str) -> Option<String> {
    TIMESTAMP_RE.find(line).map(|m| m.as_str().trim().to_string())
}

/// Collapse repeated identical lines (after normalization) into `first [repeated xN]`.
/// Lines appearing more than `threshold` times consecutively are collapsed.
/// Lines at or below the threshold are all emitted (no silent drops).
pub fn process(lines: Vec<String>, threshold: usize) -> Vec<String> {
    if lines.is_empty() {
        return lines;
    }

    let mut result: Vec<String> = Vec::with_capacity(lines.len());
    let mut current_canonical = String::new();
    let mut current_group: Vec<String> = Vec::new();

    for line in lines {
        let canonical = canonicalize(&line);
        if current_group.is_empty() {
            current_canonical = canonical;
            current_group.push(line);
        } else if canonical == current_canonical {
            current_group.push(line);
        } else {
            flush_group(&mut result, &mut current_group, threshold);
            current_canonical = canonical;
            current_group.push(line);
        }
    }
    flush_group(&mut result, &mut current_group, threshold);

    result
}

fn flush_group(result: &mut Vec<String>, group: &mut Vec<String>, threshold: usize) {
    let count = group.len();
    if count == 0 {
        return;
    }
    if count > threshold {
        let first = group[0].clone();
        result.push(format!("{} [repeated x{}]", first, count));
    } else {
        // Emit all copies when count is at or below threshold.
        result.extend(group.drain(..));
        return;
    }
    group.clear();
}

/// Non-consecutive dedup: collapse interleaved repetition across the whole input.
/// Lines whose canonical form appears more than `threshold` times anywhere in `lines`
/// are collapsed to a single entry annotated with count and time range.
pub fn nonconseq_dedup(lines: Vec<String>, threshold: usize) -> Vec<String> {
    use std::collections::{HashMap, HashSet};

    if threshold == 0 {
        return lines;
    }

    // First pass: count occurrences and collect first/last timestamps per canonical.
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut first_ts: HashMap<String, String> = HashMap::new();
    let mut last_ts: HashMap<String, String> = HashMap::new();

    for line in &lines {
        let canon = canonicalize(line);
        let ts = extract_timestamp(line).unwrap_or_default();
        let n = counts.entry(canon.clone()).or_insert(0);
        *n += 1;
        first_ts.entry(canon.clone()).or_insert_with(|| ts.clone());
        last_ts.insert(canon, ts);
    }

    // Second pass: emit, collapsing high-frequency canonicals.
    let mut seen: HashSet<String> = HashSet::new();
    let mut result: Vec<String> = Vec::with_capacity(lines.len());

    for line in lines {
        let canon = canonicalize(&line);
        let count = counts[&canon];

        if count > threshold {
            if seen.insert(canon.clone()) {
                let first = first_ts.get(&canon).map(|s| s.as_str()).unwrap_or("");
                let last = last_ts.get(&canon).map(|s| s.as_str()).unwrap_or("");
                let annotation = if !first.is_empty() && first != last {
                    format!(" [x{}, {}–{}]", count, first, last)
                } else {
                    format!(" [x{}]", count)
                };
                result.push(format!("{}{}", line, annotation));
            }
            // subsequent occurrences: silently dropped
        } else {
            result.push(line);
        }
    }

    result
}

/// Streaming stage for consecutive dedup.
pub struct DedupStage {
    threshold: usize,
    current_canonical: String,
    current_group: Vec<String>,
}

impl DedupStage {
    pub fn new(threshold: usize) -> Self {
        Self {
            threshold,
            current_canonical: String::new(),
            current_group: Vec::new(),
        }
    }

    fn drain_group(&mut self) -> Vec<String> {
        let count = self.current_group.len();
        if count == 0 {
            return vec![];
        }
        let result = if count > self.threshold {
            let first = self.current_group[0].clone();
            vec![format!("{} [repeated x{}]", first, count)]
        } else {
            self.current_group.clone()
        };
        self.current_group.clear();
        self.current_canonical.clear();
        result
    }
}

impl Stage for DedupStage {
    fn push(&mut self, line: String) -> Vec<String> {
        let canonical = canonicalize(&line);
        if self.current_group.is_empty() {
            self.current_canonical = canonical;
            self.current_group.push(line);
            vec![]
        } else if canonical == self.current_canonical {
            self.current_group.push(line);
            vec![]
        } else {
            let flushed = self.drain_group();
            self.current_canonical = canonical;
            self.current_group.push(line);
            flushed
        }
    }

    fn flush(&mut self) -> Vec<String> {
        self.drain_group()
    }
}
