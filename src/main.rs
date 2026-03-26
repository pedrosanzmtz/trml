mod config;
mod formatter;
mod hook;
mod learn;
mod pipeline;
mod probe;
mod profile;
mod stages;

use clap::{Parser, Subcommand};
use std::io::{self, BufRead, IsTerminal};

#[derive(Parser)]
#[command(name = "logslim", about = "Compress logs before they reach an LLM context window")]
struct Cli {
    /// Input file (defaults to stdin)
    file: Option<String>,

    /// Compression level
    #[arg(long, value_name = "LEVEL", default_value = "normal")]
    level: String,

    /// Force a specific profile (e.g. nifi, kafka)
    #[arg(long, value_name = "NAME")]
    profile: Option<String>,

    /// Infer a profile from the input and write to ~/.logslim/profiles/
    #[arg(long)]
    learn: bool,

    /// Explicit name for the learned profile
    #[arg(long, value_name = "NAME")]
    profile_name: Option<String>,

    /// Print reduction stats to stderr
    #[arg(long)]
    stats: bool,

    /// Show what was removed (for debugging the tool itself)
    #[arg(long)]
    explain: bool,

    /// Keep N lines of context before and after each ERROR/WARN
    #[arg(long, value_name = "N")]
    context: Option<usize>,

    /// Process only the last N lines of the input file
    #[arg(long, value_name = "N")]
    tail: Option<usize>,

    /// Skip lines with timestamps before this value (e.g. "2024-01-15 10:00:00")
    #[arg(long, value_name = "TIMESTAMP")]
    since: Option<String>,

    /// Skip lines with timestamps after this value
    #[arg(long, value_name = "TIMESTAMP")]
    until: Option<String>,

    /// Collapse interleaved repeated lines using a frequency map (non-consecutive dedup)
    #[arg(long)]
    nonconseq_dedup: bool,

    /// Follow the file/stdin for new lines (like tail -f), with live filtering
    #[arg(long)]
    follow: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Manage the Claude Code hook
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Patch ~/.claude/settings.json to intercept log-reading Bash commands
    Install {
        /// Custom path to settings.json
        #[arg(long)]
        settings: Option<String>,
    },
}

fn main() {
    let cli = Cli::parse();

    // Handle subcommands first
    if let Some(Commands::Hook { action }) = cli.command {
        match action {
            HookAction::Install { settings } => {
                let path = settings.as_deref().map(std::path::Path::new);
                if let Err(e) = hook::install(path) {
                    eprintln!("[logslim] Error installing hook: {}", e);
                    std::process::exit(1);
                }
            }
        }
        return;
    }

    let cfg = config::load();
    let level = pipeline::Level::from_str(&cli.level);

    // Determine common pipeline config
    let mut pipeline_cfg = pipeline::PipelineConfig::from_config(&cfg, level);
    pipeline_cfg.explain = cli.explain;
    pipeline_cfg.context_lines = cli.context.unwrap_or(0);
    pipeline_cfg.nonconseq_dedup = cli.nonconseq_dedup;
    pipeline_cfg.since_ts = cli.since.clone();
    pipeline_cfg.until_ts = cli.until.clone();

    // Profile list (bundled + user)
    let profiles_dir = config::profiles_dir();
    let all_profiles: Vec<profile::Profile> = {
        let mut p = profile::bundled_profiles();
        p.extend(profile::user_profiles(&profiles_dir));
        p
    };

    // --follow mode: live tail with filtering
    if cli.follow {
        let use_color = io::stdout().is_terminal();
        let stdout = io::stdout();
        let mut out = io::BufWriter::new(stdout.lock());

        let active_profile: Option<profile::Profile> = if let Some(name) = &cli.profile {
            all_profiles.into_iter().find(|p| p.name == *name)
        } else {
            None // profile auto-detection not supported in follow mode
        };

        let result = if let Some(ref path) = cli.file {
            pipeline::follow_file(path, &pipeline_cfg, active_profile, use_color, &mut out)
        } else {
            pipeline::follow_stdin(&pipeline_cfg, active_profile, use_color, &mut out)
        };
        if let Err(e) = result {
            eprintln!("[logslim] {}", e);
        }
        return;
    }

    // Read all lines for learn/batch modes
    let lines = if let Some(tail_n) = cli.tail {
        read_tail(cli.file.as_deref(), tail_n)
    } else {
        read_input(cli.file.as_deref())
    };

    // Learn mode
    if cli.learn {
        let name = cli.profile_name.as_deref().unwrap_or("");
        let dir = config::profiles_dir();
        match learn::learn(&lines, name, &dir) {
            Ok(result) => {
                eprintln!(
                    "[logslim] Learned profile '{}' → {}",
                    result.profile_name,
                    result.profile_path.display()
                );
                eprintln!(
                    "[logslim] {} noise patterns, {} signal patterns",
                    result.noise_count, result.signal_count
                );
            }
            Err(e) => {
                eprintln!("[logslim] Error writing profile: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // Detect active profile
    let active_profile: Option<&profile::Profile> = if let Some(name) = &cli.profile {
        all_profiles.iter().find(|p| p.name == *name)
    } else if cfg.profiles.auto_detect {
        profile::detect_profile(&all_profiles, &lines)
    } else {
        None
    };

    // Run pipeline
    let result = pipeline::run(lines, &pipeline_cfg, active_profile);

    // Output
    let use_color = io::stdout().is_terminal();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if cli.explain {
        let stderr = io::stderr();
        let mut err = io::BufWriter::new(stderr.lock());
        if let Err(e) = formatter::write_explain(&result.explain, &mut err) {
            eprintln!("Error writing explain: {}", e);
        }
    }

    if let Err(e) = formatter::write_output(&result.lines, &mut out, use_color) {
        eprintln!("[logslim] Error writing output: {}", e);
        std::process::exit(1);
    }

    let show_stats = cli.stats || cfg.output.show_stats;
    if show_stats {
        let stderr = io::stderr();
        let mut err = io::BufWriter::new(stderr.lock());
        if let Err(e) = formatter::write_stats(&result.stats, &mut err) {
            eprintln!("[logslim] Error writing stats: {}", e);
        }
    }
}

fn read_input(file: Option<&str>) -> Vec<String> {
    match file {
        Some(path) => {
            let file = std::fs::File::open(path).unwrap_or_else(|e| {
                eprintln!("[logslim] Cannot open '{}': {}", path, e);
                std::process::exit(1);
            });
            io::BufReader::new(file)
                .lines()
                .filter_map(|l| l.ok())
                .collect()
        }
        None => {
            let stdin = io::stdin();
            stdin.lock().lines().filter_map(|l| l.ok()).collect()
        }
    }
}

/// Read only the last N lines using a ring buffer (memory-efficient for large files).
fn read_tail(file: Option<&str>, n: usize) -> Vec<String> {
    use std::collections::VecDeque;

    let reader: Box<dyn BufRead> = match file {
        Some(path) => {
            let f = std::fs::File::open(path).unwrap_or_else(|e| {
                eprintln!("[logslim] Cannot open '{}': {}", path, e);
                std::process::exit(1);
            });
            Box::new(io::BufReader::new(f))
        }
        None => Box::new(io::BufReader::new(io::stdin())),
    };

    let mut ring: VecDeque<String> = VecDeque::with_capacity(n + 1);
    for line in reader.lines().filter_map(|l| l.ok()) {
        if ring.len() >= n {
            ring.pop_front();
        }
        ring.push_back(line);
    }
    ring.into_iter().collect()
}
