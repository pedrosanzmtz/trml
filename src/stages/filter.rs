use crate::stages::Stage;
use regex::Regex;
use std::collections::VecDeque;
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
    Regex::new(r"(?i)\b(TRACE|DEBUG|INFO|WARN(?:ING)?|ERROR|FATAL|CRITICAL|SEVERE)\b").unwrap()
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

/// Returns true if this line looks like a stack frame (indented, or contains markers).
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
    /// Keep 1 in N INFO lines: 0 = drop all, 1 = keep all, N>1 = keep 1 in N.
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

/// Batch filter — used in learn mode and explain instrumentation.
pub fn process(lines: Vec<String>, config: &FilterConfig) -> Vec<String> {
    let mut result = Vec::with_capacity(lines.len() / 4);
    let mut info_counter = 0usize;
    let mut debug_counter = 0usize;
    let mut in_signal_context = false;

    for line in lines {
        if is_signal(&line) {
            result.push(line);
            in_signal_context = true;
            continue;
        }

        if in_signal_context && is_stack_frame(&line) {
            result.push(line);
            continue;
        }

        if !line.starts_with(' ') && !line.starts_with('\t') {
            in_signal_context = false;
        }

        let level = detect_level(&line);
        match level {
            Level::Fatal | Level::Error | Level::Warn => {
                result.push(line);
                in_signal_context = true;
            }
            Level::Info => {
                info_counter += 1;
                match config.sample_info {
                    0 => {} // drop all
                    1 => result.push(line),
                    n => {
                        if info_counter % n == 1 {
                            result.push(line);
                        }
                    }
                }
            }
            Level::Debug | Level::Trace => {
                if config.sample_debug > 0 {
                    debug_counter += 1;
                    if debug_counter % config.sample_debug == 1 {
                        result.push(line);
                    }
                }
            }
            Level::Unknown => {
                result.push(line);
            }
        }
    }

    result
}

/// Streaming filter stage with optional context window.
///
/// When `context_lines > 0`, keeps the last N lines in a ring buffer. On
/// encountering a signal line, unemitted buffered lines are flushed as
/// pre-context. Post-context is counted explicitly.
pub struct FilterStage {
    config: FilterConfig,
    context_lines: usize,
    /// Ring buffer: (line, already_emitted_by_normal_filter).
    pre_context: VecDeque<(String, bool)>,
    post_context_remaining: usize,
    in_signal_context: bool,
    info_counter: usize,
    debug_counter: usize,
}

impl FilterStage {
    pub fn new(config: FilterConfig, context_lines: usize) -> Self {
        Self {
            config,
            context_lines,
            pre_context: VecDeque::new(),
            post_context_remaining: 0,
            in_signal_context: false,
            info_counter: 0,
            debug_counter: 0,
        }
    }

    fn is_signal_line(&self, line: &str) -> bool {
        is_signal(line)
            || matches!(
                detect_level(line),
                Level::Fatal | Level::Error | Level::Warn
            )
    }

    fn keep_by_level(&mut self, line: &str) -> bool {
        let level = detect_level(line);
        match level {
            Level::Fatal | Level::Error | Level::Warn => true,
            Level::Info => {
                self.info_counter += 1;
                match self.config.sample_info {
                    0 => false,
                    1 => true,
                    n => self.info_counter % n == 1,
                }
            }
            Level::Debug | Level::Trace => {
                if self.config.sample_debug > 0 {
                    self.debug_counter += 1;
                    self.debug_counter % self.config.sample_debug == 1
                } else {
                    false
                }
            }
            Level::Unknown => true,
        }
    }

    fn push_to_ring(&mut self, line: String, emitted: bool) {
        if self.context_lines == 0 {
            return;
        }
        if self.pre_context.len() >= self.context_lines {
            self.pre_context.pop_front();
        }
        self.pre_context.push_back((line, emitted));
    }
}

impl Stage for FilterStage {
    fn push(&mut self, line: String) -> Vec<String> {
        // Signal lines always kept; also flush unemitted pre-context.
        if self.is_signal_line(&line) {
            let mut out: Vec<String> = self
                .pre_context
                .drain(..)
                .filter_map(|(l, emitted)| if !emitted { Some(l) } else { None })
                .collect();
            out.push(line.clone());
            self.in_signal_context = true;
            self.post_context_remaining = self.context_lines;
            return out;
        }

        // Stack frames in a signal context are always kept.
        if self.in_signal_context && is_stack_frame(&line) {
            self.push_to_ring(line.clone(), true);
            return vec![line];
        }

        // Post-context lines.
        if self.post_context_remaining > 0 {
            self.post_context_remaining -= 1;
            if self.post_context_remaining == 0 {
                self.in_signal_context = false;
            }
            self.push_to_ring(line.clone(), true);
            return vec![line];
        }

        // Exit signal context on non-indented lines.
        if !line.starts_with(' ') && !line.starts_with('\t') {
            self.in_signal_context = false;
        }

        let keep = self.keep_by_level(&line);
        self.push_to_ring(line.clone(), keep);
        if keep { vec![line] } else { vec![] }
    }

    fn flush(&mut self) -> Vec<String> {
        vec![]
    }
}
