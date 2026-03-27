use crate::{profile::Profile, stages::Stage};
use regex::Regex;

/// Batch normalize: apply profile rules to every line (used in batch pipeline).
pub fn process(lines: Vec<String>, profile: &Profile) -> Vec<String> {
    lines.into_iter().map(|l| profile.normalize(&l)).collect()
}

/// Applies profile normalization rules to each line before the dedup stage.
///
/// This replaces variable parts (IPs, usernames, port numbers, PIDs) with
/// stable placeholders so that semantically identical lines — which differ
/// only in those variable fields — get the same canonical form and collapse
/// in the dedup stage.
pub struct NormalizeStage {
    rules: Vec<(Regex, String)>,
}

impl NormalizeStage {
    pub fn new(rules: Vec<(Regex, String)>) -> Self {
        Self { rules }
    }

    fn apply(&self, line: &str) -> String {
        let mut result = line.to_string();
        for (re, replacement) in &self.rules {
            result = re.replace_all(&result, replacement.as_str()).into_owned();
        }
        result
    }
}

impl Stage for NormalizeStage {
    fn push(&mut self, line: String) -> Vec<String> {
        vec![self.apply(&line)]
    }

    fn flush(&mut self) -> Vec<String> {
        vec![]
    }
}
