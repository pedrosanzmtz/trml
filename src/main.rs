mod config;
mod formatter;
mod hook;
mod learn;
mod pipeline;
mod probe;
mod profile;
mod stages;

use clap::{Parser, Subcommand};
use std::io::{self, BufRead};

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

    // Read input lines
    let lines = read_input(cli.file.as_deref());

    // Learn mode
    if cli.learn {
        let name = cli
            .profile_name
            .as_deref()
            .unwrap_or("");
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

    // Determine active profile
    let profiles_dir = config::profiles_dir();
    let all_profiles: Vec<profile::Profile> = {
        let mut p = profile::bundled_profiles();
        p.extend(profile::user_profiles(&profiles_dir));
        p
    };

    let active_profile: Option<&profile::Profile> = if let Some(name) = &cli.profile {
        all_profiles.iter().find(|p| p.name == *name)
    } else if cfg.profiles.auto_detect {
        profile::detect_profile(&all_profiles, &lines)
    } else {
        None
    };

    // Build pipeline config
    let mut pipeline_cfg = pipeline::PipelineConfig::from_config(&cfg, level);
    pipeline_cfg.explain = cli.explain;

    // Run pipeline
    let result = pipeline::run(lines, &pipeline_cfg, active_profile);

    // Output
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());

    if cli.explain {
        let stderr = io::stderr();
        let mut err = io::BufWriter::new(stderr.lock());
        if let Err(e) = formatter::write_explain(&result.explain, &mut err) {
            eprintln!("Error writing explain: {}", e);
        }
    }

    if let Err(e) = formatter::write_output(&result.lines, &mut out) {
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
