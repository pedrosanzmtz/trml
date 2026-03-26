# logslim

A Rust CLI tool that compresses logs before they reach an LLM context window.
Works as a Unix pipe (`cat nifi-app.log | logslim`) and as a Claude Code hook.

## What this tool does

Reads log data from stdin or a file, compresses it using heuristics + optional
learned profiles, and writes a compact, agent-readable summary to stdout.

Target: 70-90% token reduction while preserving all actionable signal.

## Core design principles

- **Single binary, zero runtime dependencies** — must run on on-premises servers
  that may not have Python, Node, or internet access
- **Stdin → stdout** — works as a Unix pipe, no opinion about what's on either side
- **Generic heuristics as the floor** — works on any log format on day one
- **Learned profiles as the ceiling** — domain-specific compression after `--learn`
- **Never lose signal** — ERRORs, WARNINGs, stack traces, unique events always preserved
- **Fast** — <10ms startup, streaming line-by-line, never loads full file into memory

## Target stack (Spectrum Effect)

Services whose logs this tool must handle well:
- Apache NiFi (noisy: heartbeats, WriteAheadFlowFileRepository, StandardProcessorNode)
- Apache Kafka (noisy: coordinator heartbeats, fetch requests, ISR metadata)
- ClickHouse (noisy: background merges, part moves)
- Kubernetes / kubectl logs (noisy: liveness probes, readiness checks)
- Docker container logs
- Python microservices (structured JSON logs via standard logging)
- MongoDB
- Redis
- Any unknown service (generic heuristics apply)

## Architecture

```
stdin / file
     │
     ▼
┌─────────────┐
│ FormatProbe │  — sniff first 200 lines, detect format + known service
└─────────────┘
     │
     ▼
┌─────────────┐
│  Pipeline   │  — apply stages in order:
│  1. Strip   │    remove ANSI codes, normalize whitespace
│  2. Dedup   │    collapse repeated lines with [x23] count
│  3. Filter  │    keep ERROR/WARN/FATAL, sample INFO at 1:N
│  4. Stack   │    compress stack traces to signature + count
│  5. Profile │    apply service-specific rules if profile matched
└─────────────┘
     │
     ▼
┌─────────────┐
│  Formatter  │  — write compact summary to stdout
└─────────────┘
```

## CLI interface

```bash
# Basic pipe usage
cat nifi-app.log | logslim
kubectl logs my-pod | logslim
tail -f kafka.log | logslim

# File input
logslim nifi-app.log

# Learn mode — infer profile from a sample log, write to ~/.logslim/profiles/
logslim --learn nifi-app.log
logslim --learn nifi-app.log --profile-name nifi  # explicit name

# Compression level
logslim --level aggressive nifi-app.log   # signatures only
logslim --level normal nifi-app.log       # default
logslim --level light nifi-app.log        # minimal filtering

# Force a specific profile
logslim --profile nifi nifi-app.log
logslim --profile kafka kafka.log

# Show what was removed (for debugging the tool itself)
logslim --explain nifi-app.log

# Output stats to stderr
logslim --stats nifi-app.log
# → [logslim] 127,430 lines → 312 lines (99.7% reduction, ~94% token reduction)
```

## Config file

`~/.logslim/config.toml`

```toml
[defaults]
level = "normal"          # normal | aggressive | light
sample_info = 20          # keep 1 in N INFO lines
sample_debug = 0          # 0 = drop all DEBUG

[profiles]
auto_detect = true        # match known profiles automatically
profiles_dir = "~/.logslim/profiles"

[output]
show_stats = true         # print reduction stats to stderr
```

## Profile format

`~/.logslim/profiles/nifi.yml`

```yaml
name: nifi
match:
  - "o.a.nifi"
  - "NiFi"
  - "FlowController"

noise_patterns:
  - ".*StandardProcessorNode.*heartbeat.*"
  - ".*WriteAheadFlowFileRepository.*checkpoint.*"
  - ".*FlowController.*Starting to run.*"
  - ".*ReportingTaskNode.*"

signal_patterns:
  - ".*BackPressure.*"
  - ".*PutKafka.*"
  - ".*Failed to.*"
  - ".*Connection refused.*"

stack_collapse: true
```

## Heuristics (the 90% layer — no profile needed)

These apply to every log regardless of format:

1. **Severity detection** — keywords: ERROR, WARN, WARNING, FATAL, CRITICAL,
   Exception, Traceback, panic, OOM, killed, failed, refused → always kept
2. **Repetition collapse** — identical lines (after stripping timestamp + numbers)
   appearing >3 times → keep first + `[repeated xN]`
3. **Stack trace compression** — indented block after Exception/Error line →
   keep first 3 frames + last frame + `[N frames hidden]`
4. **Timestamp normalization** — detect and strip timestamp prefix so dedup works
5. **ANSI stripping** — remove color codes
6. **INFO sampling** — keep 1 in 20 INFO lines (configurable)
7. **DEBUG dropping** — drop all DEBUG by default (configurable)

## Claude Code hook setup

After install, run:

```bash
logslim hook install
```

This patches `~/.claude/settings.json` to intercept Bash commands that read
log files and pipe them through logslim automatically.

Or manually in `.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "~/.cargo/bin/logslim-hook"
          }
        ]
      }
    ]
  }
}
```

## Crate structure

```
logslim/
├── Cargo.toml
├── CLAUDE.md
├── src/
│   ├── main.rs             — CLI entrypoint, arg parsing (clap)
│   ├── probe.rs            — format detection, service fingerprinting
│   ├── pipeline.rs         — compression pipeline orchestrator
│   ├── stages/
│   │   ├── strip.rs        — ANSI + whitespace normalization
│   │   ├── dedup.rs        — repetition detection + collapsing
│   │   ├── filter.rs       — severity-based line filtering
│   │   ├── stack.rs        — stack trace compression
│   │   └── profile.rs      — profile-based rule application
│   ├── profile.rs          — profile loading + matching
│   ├── learn.rs            — --learn mode, profile inference
│   ├── formatter.rs        — output formatting
│   ├── hook.rs             — Claude Code hook rewrite logic
│   └── config.rs           — config file loading
├── profiles/               — bundled profiles for known services
│   ├── nifi.yml
│   ├── kafka.yml
│   ├── clickhouse.yml
│   ├── kubernetes.yml
│   └── redis.yml
└── tests/
    ├── fixtures/
    │   ├── nifi-sample.log
    │   ├── kafka-sample.log
    │   └── generic-sample.log
    └── integration.rs
```

## Key dependencies

- `clap` — CLI argument parsing
- `regex` — pattern matching for noise/signal rules
- `serde` + `serde_yaml` — profile file parsing
- `toml` — config file parsing
- `dirs` — cross-platform config/data dirs

No ML. No network. No async needed.

## Definition of done for v0.1

- [ ] Reads from stdin and file
- [ ] Generic heuristics: dedup, severity filter, stack collapse, ANSI strip
- [ ] NiFi profile bundled
- [ ] Kafka profile bundled
- [ ] `--stats` flag showing reduction metrics on stderr
- [ ] `--learn` mode writing a profile file
- [ ] `logslim hook install` patches Claude Code settings
- [ ] Single binary installable via `cargo install`
- [ ] Works on Linux x86_64 (primary target: on-premises servers)

## Non-goals for v0.1

- Web UI or dashboard
- Daemon / watch mode
- Remote log ingestion
- ML-based compression
- Windows support
