use regex::Regex;
use std::sync::LazyLock;

/// Keywords that indicate a line must always be kept.
static SIGNAL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(error|warn|warning|fatal|critical|exception|traceback|panic|oom|out of memory|killed|failed|refused|failure|crash|abort|assert|illegal|invalid|corrupt|timeout|deadlock|connection reset|broken pipe)\b",
    )
    .unwrap()
});

/// Detect log level from common patterns.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
    Unknown,
}

static LEVEL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)\b(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL|CRITICAL|SEVERE)\b",
    )
    .unwrap()
});

pub fn detect_level(line: &str) -> Level {
    if let Some(cap) = LEVEL_RE.find(line) {
        match cap.as_str().to_uppercase().as_str() {
            "TRACE" => Level::Trace,
            "DEBUG" => Level::Debug,
            "INFO" => Level::Info,
            "WARN" | "WARNING" => Level::Warn,
            "ERROR" | "SEVERE" => Level::Error,
            "FATAL" | "CRITICAL" => Level::Fatal,
            _ => Level::Unknown,
        }
    } else {
        Level::Unknown
    }
}

pub fn is_signal(line: &str) -> bool {
    SIGNAL_RE.is_match(line)
}

/// Returns true if this line looks like a stack frame (indented, or contains stack-frame markers).
pub fn is_stack_frame(line: &str) -> bool {
    line.starts_with('\t')
        || (line.len() > 1 && line.starts_with("  "))
        || line.trim_start().starts_with("at ")
        || line.trim_start().starts_with("File \"")
        || line.trim_start().starts_with("caused by")
        || line.trim_start().starts_with("Caused by")
        || line.trim_start().starts_with("...")
}

pub struct FilterConfig {
    /// Keep 1 in N INFO lines (1 = keep all, 0 = drop all, N = sample).
    pub sample_info: usize,
    /// Keep 1 in N DEBUG lines (0 = drop all).
    pub sample_debug: usize,
}

impl Default for FilterConfig {
    fn default() -> Self {
        Self {
            sample_info: 20,
            sample_debug: 0,
        }
    }
}

/// Filter lines by severity, sampling INFO and dropping DEBUG by default.
/// Always keeps signal lines (ERROR/WARN/Exception/etc) and stack frames following them.
pub fn process(lines: Vec<String>, config: &FilterConfig) -> Vec<String> {
    let mut result = Vec::with_capacity(lines.len() / 4);
    let mut info_counter = 0usize;
    let mut debug_counter = 0usize;
    let mut in_signal_context = false; // true if last kept line was a signal

    for line in lines {
        // Always keep signal lines regardless of level
        if is_signal(&line) {
            result.push(line);
            in_signal_context = true;
            continue;
        }

        // Keep stack frames that follow a signal line
        if in_signal_context && is_stack_frame(&line) {
            result.push(line);
            continue;
        }

        // Non-indented line: exit stack trace context
        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_signal_context = false;
        }

        let level = detect_level(&line);
        match level {
            Level::Fatal | Level::Error | Level::Warn => {
                // Caught by signal check above, but catch level-labelled lines too
                result.push(line);
                in_signal_context = true;
            }
            Level::Info => {
                info_counter += 1;
                if config.sample_info == 0 {
                    // keep all
                    result.push(line);
                } else if config.sample_info == 1 {
                    // keep all (1 in 1)
                    result.push(line);
                } else if info_counter % config.sample_info == 1 {
                    result.push(line);
                }
            }
            Level::Debug | Level::Trace => {
                if config.sample_debug > 0 {
                    debug_counter += 1;
                    if debug_counter % config.sample_debug == 1 {
                        result.push(line);
                    }
                }
                // else drop
            }
            Level::Unknown => {
                // Unknown level: keep it (don't risk losing signal)
                result.push(line);
            }
        }
    }

    result
}
