use crate::stages::dedup::canonicalize;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

pub struct LearnResult {
    pub profile_name: String,
    pub profile_path: PathBuf,
    pub noise_count: usize,
    pub signal_count: usize,
}

/// Analyze `lines` and write a profile to `output_dir/<name>.yml`.
pub fn learn(
    lines: &[String],
    name: &str,
    output_dir: &Path,
) -> std::io::Result<LearnResult> {
    // Count canonical line frequencies
    let mut freq: HashMap<String, (usize, String)> = HashMap::new();
    for line in lines {
        let canonical = canonicalize(line);
        let entry = freq.entry(canonical).or_insert((0, line.clone()));
        entry.0 += 1;
    }

    let total = lines.len().max(1);
    let noise_threshold = total / 20; // top 5% frequency = noise candidate

    // Build noise patterns from high-frequency, non-signal lines
    let mut noise_patterns: Vec<String> = Vec::new();
    let mut signal_patterns: Vec<String> = Vec::new();

    // Detect match strings from first 50 lines
    let match_strings = extract_match_strings(lines);

    // Detect signal keywords
    let signal_re = regex::Regex::new(
        r"(?i)\b(error|warn|warning|fatal|critical|exception|traceback|panic|failed|refused)\b",
    )
    .unwrap();

    let mut entries: Vec<(usize, String, String)> = freq
        .into_iter()
        .map(|(canonical, (count, first_line))| (count, canonical, first_line))
        .collect();
    entries.sort_by(|a, b| b.0.cmp(&a.0));

    for (count, _canonical, first_line) in &entries {
        if *count < 3 {
            break; // Only look at lines appearing 3+ times
        }

        if signal_re.is_match(first_line) {
            // It's a signal line — generate a signal pattern
            let pattern = line_to_pattern(first_line);
            if !signal_patterns.contains(&pattern) {
                signal_patterns.push(pattern);
            }
        } else if *count > noise_threshold {
            // High frequency non-signal = noise
            let pattern = line_to_pattern(first_line);
            if !noise_patterns.contains(&pattern) {
                noise_patterns.push(pattern);
            }
        }
    }

    // Detect service name for auto-naming
    let profile_name = if name.is_empty() {
        detect_service_name(lines).unwrap_or_else(|| "custom".to_string())
    } else {
        name.to_string()
    };

    // Write YAML profile
    std::fs::create_dir_all(output_dir)?;
    let profile_path = output_dir.join(format!("{}.yml", profile_name));

    let noise_count = noise_patterns.len();
    let signal_count = signal_patterns.len();

    let yaml = build_yaml(&profile_name, &match_strings, &noise_patterns, &signal_patterns);

    let mut file = std::fs::File::create(&profile_path)?;
    file.write_all(yaml.as_bytes())?;

    Ok(LearnResult {
        profile_name,
        profile_path,
        noise_count,
        signal_count,
    })
}

fn line_to_pattern(line: &str) -> String {
    // Extract the "structural" part of a line as a regex pattern
    // Replace numbers with \d+ and keep text portions
    let no_ts = strip_timestamp(line);
    let words: Vec<&str> = no_ts.split_whitespace().collect();
    let key_words: Vec<&str> = words
        .iter()
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit() || c == '.' || c == ':' || c == '-'))
        .take(6)
        .copied()
        .collect();
    if key_words.is_empty() {
        return regex::escape(line.trim());
    }
    let joined = key_words.join(".*");
    format!(".*{}.*", joined)
}

fn strip_timestamp(line: &str) -> &str {
    // Quick heuristic: skip leading timestamp-like prefix
    let bytes = line.as_bytes();
    if bytes.len() > 19
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && (bytes[10] == b' ' || bytes[10] == b'T')
    {
        // ISO timestamp prefix, skip up to space after time
        if let Some(rest) = line.get(19..) {
            return rest.trim_start();
        }
    }
    line
}

fn extract_match_strings(lines: &[String]) -> Vec<String> {
    let mut candidates: HashMap<String, usize> = HashMap::new();
    // Look for common package/class name fragments in the first 100 lines
    let pkg_re = regex::Regex::new(r"\b([a-zA-Z][a-zA-Z0-9_]*(?:\.[a-zA-Z][a-zA-Z0-9_]*){2,})\b").unwrap();
    for line in lines.iter().take(100) {
        for cap in pkg_re.captures_iter(line) {
            let pkg = cap[1].to_string();
            // Only keep medium-length packages (likely class names)
            if pkg.len() > 6 && pkg.len() < 40 {
                *candidates.entry(pkg).or_insert(0) += 1;
            }
        }
    }
    let mut sorted: Vec<(String, usize)> = candidates.into_iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1));
    sorted.into_iter().take(3).map(|(s, _)| s).collect()
}

fn detect_service_name(lines: &[String]) -> Option<String> {
    let combined: String = lines.iter().take(20).map(|s| s.as_str()).collect::<Vec<_>>().join(" ");
    let combined_lower = combined.to_lowercase();

    if combined_lower.contains("nifi") { return Some("nifi".to_string()); }
    if combined_lower.contains("kafka") { return Some("kafka".to_string()); }
    if combined_lower.contains("clickhouse") { return Some("clickhouse".to_string()); }
    if combined_lower.contains("kubernetes") || combined_lower.contains("kubelet") {
        return Some("kubernetes".to_string());
    }
    if combined_lower.contains("redis") { return Some("redis".to_string()); }
    if combined_lower.contains("mongo") { return Some("mongodb".to_string()); }
    None
}

fn build_yaml(
    name: &str,
    match_strings: &[String],
    noise_patterns: &[String],
    signal_patterns: &[String],
) -> String {
    let mut yaml = String::new();
    yaml.push_str(&format!("name: {}\n", name));

    yaml.push_str("match:\n");
    if match_strings.is_empty() {
        yaml.push_str("  # Add strings that identify this service in log lines\n");
    }
    for s in match_strings {
        yaml.push_str(&format!("  - \"{}\"\n", s.replace('"', "\\\"")));
    }

    yaml.push_str("\nnoise_patterns:\n");
    for p in noise_patterns {
        yaml.push_str(&format!("  - \"{}\"\n", p.replace('"', "\\\"")));
    }

    yaml.push_str("\nsignal_patterns:\n");
    for p in signal_patterns {
        yaml.push_str(&format!("  - \"{}\"\n", p.replace('"', "\\\"")));
    }

    yaml.push_str("\nstack_collapse: true\n");
    yaml
}
