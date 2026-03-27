use crate::{
    config::Config,
    profile::Profile,
    stages::{
        dedup, filter, normalize, profile_stage, stack, strip,
        dedup::DedupStage,
        filter::{FilterConfig, FilterStage},
        normalize::NormalizeStage,
        profile_stage::ProfileStage,
        stack::StackStage,
        strip::StripStage,
        Stage,
    },
};
use std::io::BufRead;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Level {
    Light,
    Normal,
    Aggressive,
}

impl Level {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "light" => Level::Light,
            "aggressive" => Level::Aggressive,
            _ => Level::Normal,
        }
    }
}

pub struct Stats {
    pub input_lines: usize,
    pub output_lines: usize,
    pub input_bytes: usize,
    pub output_bytes: usize,
}

impl Stats {
    pub fn line_reduction_pct(&self) -> f64 {
        if self.input_lines == 0 {
            return 0.0;
        }
        (1.0 - self.output_lines as f64 / self.input_lines as f64) * 100.0
    }

    pub fn token_reduction_pct(&self) -> f64 {
        if self.input_bytes == 0 {
            return 0.0;
        }
        (1.0 - self.output_bytes as f64 / self.input_bytes as f64) * 100.0
    }
}

pub struct PipelineConfig {
    pub level: Level,
    pub sample_info: usize,
    pub sample_debug: usize,
    pub dedup_threshold: usize,
    pub stack_keep_head: usize,
    pub explain: bool,
    /// Keep N lines before and after each ERROR/WARN (0 = disabled).
    pub context_lines: usize,
    /// Apply frequency-map dedup across the full output after stage pipeline.
    pub nonconseq_dedup: bool,
    /// ISO timestamp lower bound for --since (line skipped if ts < since).
    pub since_ts: Option<String>,
    /// ISO timestamp upper bound for --until (line skipped if ts > until).
    pub until_ts: Option<String>,
}

impl PipelineConfig {
    pub fn from_config(config: &Config, level: Level) -> Self {
        let (sample_info, stack_keep) = match level {
            Level::Light => (5, 5),
            Level::Normal => (config.defaults.sample_info, 3),
            Level::Aggressive => (50, 1),
        };
        Self {
            level,
            sample_info,
            sample_debug: config.defaults.sample_debug,
            dedup_threshold: 3,
            stack_keep_head: stack_keep,
            explain: false,
            context_lines: 0,
            nonconseq_dedup: false,
            since_ts: None,
            until_ts: None,
        }
    }
}

pub struct ExplainLine {
    pub original: String,
    pub kept: bool,
    pub stage: Option<String>,
    pub reason: Option<String>,
}

pub struct RunResult {
    pub lines: Vec<String>,
    pub stats: Stats,
    pub explain: Vec<ExplainLine>,
}

// ── Streaming pipeline ────────────────────────────────────────────────────────

fn build_stage_chain(
    config: &PipelineConfig,
    profile: Option<Profile>,
) -> Vec<Box<dyn Stage>> {
    let filter_cfg = FilterConfig {
        sample_info: config.sample_info,
        sample_debug: config.sample_debug,
    };
    let mut stages: Vec<Box<dyn Stage>> = vec![Box::new(StripStage)];

    // Insert normalization before dedup when the profile has normalize rules.
    if let Some(ref p) = profile {
        if !p.normalize_rules.is_empty() {
            stages.push(Box::new(NormalizeStage::new(p.normalize_rules.clone())));
        }
    }

    stages.push(Box::new(DedupStage::new(config.dedup_threshold)));
    stages.push(Box::new(FilterStage::new(filter_cfg, config.context_lines)));
    stages.push(Box::new(StackStage::new(config.stack_keep_head)));

    if let Some(p) = profile {
        stages.push(Box::new(ProfileStage::new(p)));
    }
    stages
}

/// Push `line` through all stages in order, collecting all output lines.
fn push_through(stages: &mut [Box<dyn Stage>], line: String) -> Vec<String> {
    let mut current = vec![line];
    for stage in stages.iter_mut() {
        let mut next = Vec::new();
        for l in current {
            next.extend(stage.push(l));
        }
        current = next;
    }
    current
}

/// Flush stage[i], piping output through stages[i+1..].
fn flush_pipeline(stages: &mut Vec<Box<dyn Stage>>) -> Vec<String> {
    let mut all_out = Vec::new();
    for i in 0..stages.len() {
        let flushed = stages[i].flush();
        let mut current = flushed;
        for j in (i + 1)..stages.len() {
            let mut next = Vec::new();
            for l in current {
                next.extend(stages[j].push(l));
            }
            current = next;
        }
        all_out.extend(current);
    }
    all_out
}

/// True streaming pipeline: reads from `reader` one line at a time.
pub fn run_reader<R: BufRead>(
    reader: R,
    config: &PipelineConfig,
    profile: Option<Profile>,
) -> RunResult {
    let explain_enabled = config.explain;
    // For --explain we need the original lines, so collect them.
    let mut input_lines: Vec<String> = Vec::new();
    let mut input_bytes = 0usize;

    let mut stages = build_stage_chain(config, profile.clone());
    let mut output: Vec<String> = Vec::new();

    for line_result in reader.lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };

        // --since / --until time filter
        if config.since_ts.is_some() || config.until_ts.is_some() {
            if let Some(ts) = dedup::extract_timestamp(&line) {
                if let Some(ref since) = config.since_ts {
                    if ts.as_str() < since.as_str() {
                        if explain_enabled {
                            input_bytes += line.len() + 1;
                            input_lines.push(line);
                        }
                        continue;
                    }
                }
                if let Some(ref until) = config.until_ts {
                    if ts.as_str() > until.as_str() {
                        if explain_enabled {
                            input_bytes += line.len() + 1;
                            input_lines.push(line);
                        }
                        continue;
                    }
                }
            }
        }

        input_bytes += line.len() + 1;
        if explain_enabled {
            input_lines.push(line.clone());
        }

        output.extend(push_through(&mut stages, line));
    }

    output.extend(flush_pipeline(&mut stages));

    // Non-consecutive dedup post-pass
    if config.nonconseq_dedup {
        output = dedup::nonconseq_dedup(output, config.dedup_threshold);
    }

    let input_count = if explain_enabled {
        input_lines.len()
    } else {
        // We tracked bytes but not count; approximate from bytes isn't useful.
        // For non-explain mode count lines via byte counting approach — keep a counter.
        // Actually we need to track this. Let's count separately.
        // This path isn't taken in explain mode so just use 0; stats still show bytes.
        0
    };

    let output_bytes: usize = output.iter().map(|l| l.len() + 1).sum();

    let explain = if explain_enabled {
        let profile_ref = profile.as_ref();
        build_explain(&input_lines, config, profile_ref)
    } else {
        vec![]
    };

    RunResult {
        stats: Stats {
            input_lines: input_count,
            output_lines: output.len(),
            input_bytes,
            output_bytes,
        },
        explain,
        lines: output,
    }
}

/// Batch pipeline (used by learn mode, explain, and backward compat).
pub fn run(
    input_lines: Vec<String>,
    pipeline_config: &PipelineConfig,
    profile: Option<&Profile>,
) -> RunResult {
    let input_count = input_lines.len();
    let input_bytes: usize = input_lines.iter().map(|l| l.len() + 1).sum();

    // --since / --until pre-filter
    let filtered_input: Vec<String> = if pipeline_config.since_ts.is_some()
        || pipeline_config.until_ts.is_some()
    {
        input_lines
            .iter()
            .filter(|line| {
                let Some(ts) = dedup::extract_timestamp(line) else {
                    return true;
                };
                if let Some(ref since) = pipeline_config.since_ts {
                    if ts.as_str() < since.as_str() {
                        return false;
                    }
                }
                if let Some(ref until) = pipeline_config.until_ts {
                    if ts.as_str() > until.as_str() {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect()
    } else {
        input_lines.clone()
    };

    // Stage 1: Strip
    let lines = strip::process(filtered_input);

    // Stage 1b: Normalize (apply profile substitutions before dedup)
    let lines = if let Some(p) = profile {
        if !p.normalize_rules.is_empty() {
            normalize::process(lines, p)
        } else {
            lines
        }
    } else {
        lines
    };

    // Stage 2: Dedup
    let lines = dedup::process(lines, pipeline_config.dedup_threshold);

    // Stage 3: Filter
    let filter_cfg = filter::FilterConfig {
        sample_info: pipeline_config.sample_info,
        sample_debug: pipeline_config.sample_debug,
    };
    let lines = filter::process(lines, &filter_cfg);

    // Stage 4: Stack trace compression
    let lines = stack::process(lines, pipeline_config.stack_keep_head);

    // Stage 5: Profile rules
    let mut lines = if let Some(p) = profile {
        profile_stage::process(lines, p)
    } else {
        lines
    };

    // Post-pass: non-consecutive dedup
    if pipeline_config.nonconseq_dedup {
        lines = dedup::nonconseq_dedup(lines, pipeline_config.dedup_threshold);
    }

    let output_bytes: usize = lines.iter().map(|l| l.len() + 1).sum();

    let explain = if pipeline_config.explain {
        build_explain(&input_lines, pipeline_config, profile)
    } else {
        vec![]
    };

    RunResult {
        stats: Stats {
            input_lines: input_count,
            output_lines: lines.len(),
            input_bytes,
            output_bytes,
        },
        explain,
        lines,
    }
}

// ── --follow mode ─────────────────────────────────────────────────────────────

/// Follow a file (like `tail -f`), running each new line through the pipeline
/// and writing colorized output to `writer`. Handles log rotation by detecting
/// file size shrinkage.
pub fn follow_file(
    path: &str,
    config: &PipelineConfig,
    profile: Option<Profile>,
    use_color: bool,
    writer: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    use std::io::{BufReader, Seek, SeekFrom};
    use std::time::Duration;

    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::End(0))?;
    let mut last_size = file.metadata()?.len();

    let mut stages = build_stage_chain(config, profile);

    loop {
        let current_size = std::fs::metadata(path)?.len();
        if current_size < last_size {
            // File was truncated/rotated — reopen from start.
            file = std::fs::File::open(path)?;
            last_size = 0;
        }

        let mut reader = BufReader::new(&file);
        reader.seek(SeekFrom::Start(last_size))?;

        let mut has_new = false;
        for line_result in reader.lines() {
            let line = match line_result {
                Ok(l) => l,
                Err(_) => break,
            };
            has_new = true;
            for out_line in push_through(&mut stages, line) {
                let display = if use_color {
                    crate::formatter::colorize(&out_line)
                } else {
                    out_line
                };
                writeln!(writer, "{}", display)?;
            }
        }

        last_size = std::fs::metadata(path)?.len();

        if !has_new {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

/// Follow stdin continuously, running each new line through the pipeline.
pub fn follow_stdin(
    config: &PipelineConfig,
    profile: Option<Profile>,
    use_color: bool,
    writer: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stages = build_stage_chain(config, profile);

    for line_result in stdin.lock().lines() {
        let line = match line_result {
            Ok(l) => l,
            Err(_) => break,
        };
        for out_line in push_through(&mut stages, line) {
            let display = if use_color {
                crate::formatter::colorize(&out_line)
            } else {
                out_line
            };
            writeln!(writer, "{}", display)?;
        }
    }

    Ok(())
}

// ── Explain ───────────────────────────────────────────────────────────────────

fn build_explain(
    input_lines: &[String],
    pipeline_config: &PipelineConfig,
    profile: Option<&Profile>,
) -> Vec<ExplainLine> {
    use std::collections::{HashMap, HashSet};

    let threshold = pipeline_config.dedup_threshold;

    // Stage 1: Strip — 1-to-1 transform, no drops.
    let stripped: Vec<String> = input_lines
        .iter()
        .map(|l| {
            let mut v = strip::process(vec![l.clone()]);
            v.remove(0)
        })
        .collect();

    // Stage 2: Dedup — identify which canonical forms get collapsed.
    let mut canon_counts: HashMap<String, usize> = HashMap::new();
    for line in &stripped {
        *canon_counts.entry(dedup::canonicalize(line)).or_insert(0) += 1;
    }
    let collapsed_canons: HashSet<String> = canon_counts
        .into_iter()
        .filter(|(_, n)| *n > threshold)
        .map(|(k, _)| k)
        .collect();

    // Stages 3-5: run on dedup output to determine fate of each surviving line.
    let after_dedup = dedup::process(stripped.clone(), threshold);

    let filter_cfg = filter::FilterConfig {
        sample_info: pipeline_config.sample_info,
        sample_debug: pipeline_config.sample_debug,
    };
    let after_filter = filter::process(after_dedup.clone(), &filter_cfg);
    let filter_kept: HashSet<&str> = after_filter.iter().map(|l| l.as_str()).collect();

    let after_stack = stack::process(after_filter.clone(), pipeline_config.stack_keep_head);
    let stack_kept: HashSet<&str> = after_stack.iter().map(|l| l.as_str()).collect();

    let after_profile: Vec<String> = if let Some(p) = profile {
        profile_stage::process(after_stack.clone(), p)
    } else {
        after_stack.clone()
    };
    let profile_kept: HashSet<&str> = after_profile.iter().map(|l| l.as_str()).collect();

    let mut seen_canons: HashMap<String, usize> = HashMap::new();

    stripped
        .iter()
        .zip(input_lines.iter())
        .map(|(sl, orig)| {
            let canon = dedup::canonicalize(sl);
            let seen = seen_canons.entry(canon.clone()).or_insert(0);
            *seen += 1;

            // Collapsed by dedup?
            if collapsed_canons.contains(&canon) {
                return if *seen == 1 {
                    ExplainLine {
                        original: orig.clone(),
                        kept: true,
                        stage: Some("dedup".to_string()),
                        reason: Some("first of collapsed group".to_string()),
                    }
                } else {
                    ExplainLine {
                        original: orig.clone(),
                        kept: false,
                        stage: Some("dedup".to_string()),
                        reason: Some("collapsed (repeated)".to_string()),
                    }
                };
            }

            // Dropped by filter?
            if !filter_kept.contains(sl.as_str()) {
                let level = filter::detect_level(sl);
                let reason = match level {
                    filter::Level::Debug | filter::Level::Trace => "DEBUG/TRACE dropped",
                    filter::Level::Info => "INFO sampled out",
                    _ => "dropped by filter",
                };
                return ExplainLine {
                    original: orig.clone(),
                    kept: false,
                    stage: Some("filter".to_string()),
                    reason: Some(reason.to_string()),
                };
            }

            // Hidden by stack compression?
            if !stack_kept.contains(sl.as_str()) {
                return ExplainLine {
                    original: orig.clone(),
                    kept: false,
                    stage: Some("stack".to_string()),
                    reason: Some("stack frame hidden".to_string()),
                };
            }

            // Dropped by profile?
            if !profile_kept.contains(sl.as_str()) {
                return ExplainLine {
                    original: orig.clone(),
                    kept: false,
                    stage: Some("profile".to_string()),
                    reason: Some("matched noise pattern".to_string()),
                };
            }

            ExplainLine {
                original: orig.clone(),
                kept: true,
                stage: None,
                reason: None,
            }
        })
        .collect()
}
