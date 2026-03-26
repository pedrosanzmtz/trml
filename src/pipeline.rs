use crate::{
    config::Config,
    profile::Profile,
    stages::{dedup, filter, profile_stage, stack, strip},
};

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
        // Tokens ≈ bytes / 4 for log content; reduction ratio is the same.
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

/// Run the full compression pipeline on `input_lines`.
pub fn run(
    input_lines: Vec<String>,
    pipeline_config: &PipelineConfig,
    profile: Option<&Profile>,
) -> RunResult {
    let input_count = input_lines.len();
    let input_bytes: usize = input_lines.iter().map(|l| l.len() + 1).sum();

    // Stage 1: Strip
    let lines = strip::process(input_lines.clone());

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

    // Stage 5: Profile rules (if a profile is active)
    let lines = if let Some(p) = profile {
        profile_stage::process(lines, p)
    } else {
        lines
    };

    let output_bytes: usize = lines.iter().map(|l| l.len() + 1).sum();

    // Build explain output if requested
    let explain = if pipeline_config.explain {
        build_explain(&input_lines, &lines)
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

fn build_explain(original: &[String], output: &[String]) -> Vec<ExplainLine> {
    // Simple explain: mark which originals survived.
    // This is a best-effort diff since stages can rewrite lines.
    let output_set: std::collections::HashSet<&str> =
        output.iter().map(|s| s.as_str()).collect();

    original
        .iter()
        .map(|line| {
            let kept = output_set.contains(line.trim_end());
            ExplainLine {
                original: line.clone(),
                kept,
                stage: None,
                reason: None,
            }
        })
        .collect()
}
