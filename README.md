# logslim

**Compress logs before they hit your LLM context window.**

`logslim` is a fast, single-binary Rust CLI that reads log output from stdin or a file, removes noise, collapses repetition, and writes a compact, agent-readable summary to stdout. It keeps every actionable signal — errors, warnings, stack traces, unique events — while dropping heartbeats, health checks, sampled info lines, and duplicate noise that wastes tokens.

```
cat nifi-app.log | logslim --stats
# [logslim]    127,430 lines →    312 lines (99.8% reduction, ~97% token reduction)
```

---

## Why logslim

Log files are among the most token-expensive artifacts an AI coding agent reads. A single NiFi or Kafka log can be hundreds of thousands of lines, 99% of which are repeating heartbeats, checkpoint confirmations, and INFO sampling noise. Sending that raw to an LLM costs tokens without adding signal.

`logslim` applies a five-stage pipeline — strip, dedup, filter, stack-trace compression, service profile — to reduce logs by 70–99% while guaranteeing that every error, every unique warning, and every stack trace survives intact.

---

## Installation

```bash
cargo install --git https://github.com/pedrosanzmtz/trml
```

Or clone and build locally:

```bash
git clone https://github.com/pedrosanzmtz/trml
cd trml
cargo build --release
# Binary at: target/release/logslim
```

Requires Rust 1.85+ (2024 edition).

---

## Usage

### Pipe from stdin

```bash
cat nifi-app.log | logslim
kubectl logs my-pod | logslim
tail -f kafka.log | logslim
```

### Read a file directly

```bash
logslim nifi-app.log
logslim /var/log/kafka/server.log
```

### Compression level

```bash
logslim --level light nifi-app.log      # minimal filtering (keep more)
logslim --level normal nifi-app.log     # default: 1-in-20 INFO sampling
logslim --level aggressive nifi-app.log # 1-in-50 INFO sampling, 1 stack frame
```

### Show what was removed

```bash
logslim --explain nifi-app.log
# [KEEP] 2024-01-15 10:00:03 ERROR Failed to send FlowFile to Kafka
# [DROP] 2024-01-15 10:00:04 INFO heartbeat check OK
# ...
```

### Print reduction stats

```bash
logslim --stats nifi-app.log
# [logslim]         26 lines →      6 lines (76.9% reduction, ~81% token reduction)
```

### Force a specific service profile

```bash
logslim --profile nifi nifi-app.log
logslim --profile kafka kafka-server.log
logslim --profile kubernetes pod.log
```

### Learn a profile from a sample log

```bash
logslim --learn nifi-app.log                         # auto-names from detected service
logslim --learn nifi-app.log --profile-name nifi-dc2 # explicit name
# Writes ~/.logslim/profiles/nifi.yml
```

---

## The compression pipeline

Each line passes through five stages in order:

| Stage | What it does |
|---|---|
| **Strip** | Remove ANSI escape codes, trim trailing whitespace |
| **Dedup** | Collapse consecutive identical lines (after stripping timestamps and numbers) into `first occurrence [repeated xN]` |
| **Filter** | Always keep ERROR / WARN / FATAL / Exception / Traceback lines. Sample INFO at 1-in-20. Drop DEBUG. Preserve stack frame lines that follow a signal. |
| **Stack** | Compress stack trace blocks: keep the trigger line + first 3 frames + last frame + `[N frames hidden]` |
| **Profile** | Apply service-specific noise and signal patterns (see below) |

### What always survives

- Lines containing `ERROR`, `WARN`, `WARNING`, `FATAL`, `CRITICAL`
- Lines containing `Exception`, `Traceback`, `panic`, `OOM`
- Lines containing `failed`, `refused`, `killed`, `timeout`, `deadlock`
- Stack trace frames immediately following any of the above
- The first and last frames of every stack trace block
- Lines matching a profile's `signal_patterns`

### What gets removed

- Lines matching a profile's `noise_patterns` (heartbeats, health checks, etc.)
- DEBUG lines (by default; configurable)
- 19 out of every 20 INFO lines (configurable via `--level`)
- Consecutive duplicate lines beyond the threshold (collapsed with a count)

---

## Service profiles

logslim ships with bundled profiles for the services most likely to produce noisy logs. Profiles are auto-detected from the first 200 lines of input.

| Profile | Matched by | What gets dropped |
|---|---|---|
| **NiFi** | `o.a.nifi`, `FlowController`, `WriteAheadFlowFileRepository` | Heartbeats, checkpoint confirmations, ReportingTaskNode status |
| **Kafka** | `kafka`, `KafkaController`, `[Producer clientId` | Coordinator heartbeats, fetch requests, UpdateMetadata, ISR metadata |
| **ClickHouse** | `ClickHouse`, `MergeTree`, `ReplicatedMergeTree` | Background merges, part moves, ZooKeeper keepalives, health pings |
| **Kubernetes** | `kubelet`, `k8s.io`, `kube-proxy` | Liveness/readiness probes, node lease renewals, SyncLoop |
| **Redis** | `redis`, `# Server` | RDB snapshots, AOF rewrites, accepted/closed client connections |

Profile YAML format — easy to write your own:

```yaml
name: nifi
match:
  - "o.a.nifi"
  - "FlowController"

noise_patterns:
  - ".*StandardProcessorNode.*heartbeat.*"
  - ".*WriteAheadFlowFileRepository.*checkpoint.*"

signal_patterns:
  - ".*BackPressure.*"
  - ".*Failed to.*"

stack_collapse: true
```

Drop custom profiles in `~/.logslim/profiles/` — they are loaded alongside the bundled ones and take priority.

### Config file

`~/.logslim/config.toml` (all fields optional):

```toml
[defaults]
level = "normal"     # normal | aggressive | light
sample_info = 20     # keep 1 in N INFO lines
sample_debug = 0     # 0 = drop all DEBUG

[profiles]
auto_detect = true

[output]
show_stats = true    # always print reduction stats to stderr
```

---

## Claude Code hook

Install the `PreToolUse` hook so that any Bash command Claude runs against a log file is automatically piped through `logslim`:

```bash
logslim hook install
```

This patches `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "~/.cargo/bin/logslim-hook" }]
      }
    ]
  }
}
```

After this, commands like `cat nifi-app.log`, `tail -f kafka.log`, and `kubectl logs my-pod` are automatically rewritten to pipe through `logslim` before their output reaches the context window — zero changes to your workflow.

---

## Comparison with RTK

[RTK](https://github.com/rtk-ai/rtk) ("Rust Token Killer") is a general-purpose CLI proxy that wraps developer commands — `git status`, `cargo test`, `kubectl` — and compresses their output. It includes a `log` subcommand. Both tools reduce log token usage; they make different trade-offs.

### Benchmark — same three log files, same input

| File | Input | logslim lines | rtk log lines | logslim bytes | rtk log bytes |
|---|---|---|---|---|---|
| NiFi (26 lines) | 2,540 B | **6 (77% ↓)** | 11 (58% ↓) | 474 B (81% ↓) | 426 B **(83% ↓)** |
| Generic (28 lines) | 1,526 B | **7 (75% ↓)** | 10 (65% ↓) | 422 B (73% ↓) | 256 B **(84% ↓)** |
| Kafka (21 lines) | 2,791 B | **7 (67% ↓)** | 11 (48% ↓) | 694 B (76% ↓) | 411 B **(86% ↓)** |

### What the outputs actually look like

**`rtk log` output** (NiFi, 11 lines):
```
Log Summary
   [error] 2 errors (2 unique)
   [warn] 1 warnings (1 unique)
   [info] 20 info messages

[ERRORS]
   2024-01-15 10:00:03,001 ERROR [FlowEngine-1] o.a.nifi.processor.PutKafka Failed to send FlowFile ...
   org.apache.kafka.common.errors.TimeoutException: Failed to update metadata after 60000 ms.

[WARNINGS]
   2024-01-15 10:00:05,001 WARN [FlowEngine-1] o.a.nifi.controller.BackPressure Queue is full (10000...
```

**`logslim` output** (NiFi, 6 lines):
```
2024-01-15 10:00:00,001 INFO [main] o.a.nifi.NiFiServer NiFi has started.
2024-01-15 10:00:03,001 ERROR [FlowEngine-1] o.a.nifi.processor.PutKafka Failed to send FlowFile to Kafka
org.apache.kafka.common.errors.TimeoutException: Failed to update metadata after 60000 ms.
	at org.apache.kafka.clients.producer.internals.Sender.run(Sender.java:238)
	at java.lang.Thread.run(Thread.java:748)
2024-01-15 10:00:05,001 WARN [FlowEngine-1] o.a.nifi.controller.BackPressure Queue is full (10000/10000)
```

### Trade-off summary

| | logslim | rtk log |
|---|---|---|
| **Line reduction** | Better (67–77%) | Weaker (48–65%) |
| **Byte reduction** | Good (73–81%) | Better (83–86%) |
| **Line fidelity** | Full text preserved | Truncated at ~80 chars |
| **Stack traces** | Head + tail frames kept | Exception message only |
| **INFO context** | Sampled (startup, state changes) | All INFO dropped |
| **Output format** | Original log format | Reformatted digest |
| **Service profiles** | NiFi, Kafka, ClickHouse, k8s, Redis | None |
| **Scope** | Log files and streams | Any CLI command |

RTK achieves smaller byte counts by truncating long lines and dropping all INFO. logslim's goal is the smallest output that remains **fully actionable**: an agent reading logslim output can identify the error class, locate the failing frame, and understand the application state at the time of failure — without needing to re-fetch the original log.

---

## Design constraints

- **Single binary, zero runtime dependencies** — runs on air-gapped on-premises servers without Python, Node, or internet access
- **Stdin → stdout** — composable Unix pipe; no opinion about what's upstream or downstream
- **Never lose signal** — ERRORs, WARNings, stack traces, unique events always pass through
- **Fast** — streaming line-by-line; does not load the full file into memory

---

## License

MIT
