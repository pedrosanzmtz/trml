# trml

**Compress logs before they hit your LLM context window.**

`trml` is a fast, single-binary Rust CLI that reads log output from stdin or a file, removes noise, collapses repetition, and writes a compact, agent-readable summary to stdout. It keeps every actionable signal — errors, warnings, stack traces, unique events — while dropping heartbeats, health checks, sampled info lines, and duplicate noise that wastes tokens.

```
cat nifi-app.log | trml --stats
# [trml]    127,430 lines →    312 lines (99.8% reduction, ~97% token reduction)
```

---

## Why trml

Log files are among the most token-expensive artifacts an AI coding agent reads. A single NiFi or Kafka log can be hundreds of thousands of lines, 99% of which are repeating heartbeats, checkpoint confirmations, and INFO sampling noise. Sending that raw to an LLM costs tokens without adding signal.

`trml` applies a five-stage pipeline — strip, dedup, filter, stack-trace compression, service profile — to reduce logs by 70–99% while guaranteeing that every error, every unique warning, and every stack trace survives intact.

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
# Binary at: target/release/trml
```

Requires Rust 1.85+ (2024 edition).

---

## Usage

### Pipe from stdin

```bash
cat nifi-app.log | trml
kubectl logs my-pod | trml
tail -f kafka.log | trml
```

### Read a file directly

```bash
trml nifi-app.log
trml /var/log/kafka/server.log
```

### Compression level

```bash
trml --level light nifi-app.log      # minimal filtering (keep more)
trml --level normal nifi-app.log     # default: 1-in-20 INFO sampling
trml --level aggressive nifi-app.log # 1-in-50 INFO sampling, 1 stack frame
```

### Show what was removed

```bash
trml --explain nifi-app.log
# [KEEP] 2024-01-15 10:00:03 ERROR Failed to send FlowFile to Kafka
# [DROP] 2024-01-15 10:00:04 INFO heartbeat check OK
# ...
```

### Print reduction stats

```bash
trml --stats nifi-app.log
# [trml]         26 lines →      6 lines (76.9% reduction, ~81% token reduction)
```

### Force a specific service profile

```bash
trml --profile nifi nifi-app.log
trml --profile kafka kafka-server.log
trml --profile kubernetes pod.log
```

### Learn a profile from a sample log

```bash
trml --learn nifi-app.log                         # auto-names from detected service
trml --learn nifi-app.log --profile-name nifi-dc2 # explicit name
# Writes ~/.trml/profiles/nifi.yml
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

trml ships with bundled profiles for the services most likely to produce noisy logs. Profiles are auto-detected from the first 200 lines of input.

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

Drop custom profiles in `~/.trml/profiles/` — they are loaded alongside the bundled ones and take priority.

### Config file

`~/.trml/config.toml` (all fields optional):

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

Install the `PreToolUse` hook so that any Bash command Claude runs against a log file is automatically piped through `trml`:

```bash
trml hook install
```

This patches `~/.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "~/.cargo/bin/trml-hook" }]
      }
    ]
  }
}
```

After this, commands like `cat nifi-app.log`, `tail -f kafka.log`, and `kubectl logs my-pod` are automatically rewritten to pipe through `trml` before their output reaches the context window — zero changes to your workflow.

---

## Comparison with RTK

[RTK](https://github.com/rtk-ai/rtk) ("Rust Token Killer") is a general-purpose CLI proxy that wraps developer commands — `git status`, `cargo test`, `kubectl` — and compresses their output. It includes a `log` subcommand. Both tools reduce log token usage; they make different trade-offs.

### Real-world benchmark

Tested against four public log datasets from [loghub](https://zenodo.org/records/8196385), ranging from 51k to 655k lines. Both tools run on the same input file; time is wall-clock on an M-series Mac.

| Log file | Input lines | Input size | trml lines | trml reduction | `rtk log` lines | `rtk log` reduction | trml time | `rtk log` time |
|---|---|---|---|---|---|---|---|---|
| **Nginx** (access log) | 51,462 | 7.6 MB | 22,723 | 55.8% ↓ | 4 | ~100% ↓ | 172ms | 87ms |
| **Apache** (error log) | 56,481 | 4.9 MB | 31,266 | 44.6% ↓ | 25 | ~100% ↓ | 87ms | 91ms |
| **Zookeeper** | 74,380 | 9.9 MB | 53,779 | 27.7% ↓ | 25 | ~100% ↓ | 170ms | 84ms |
| **SSH** (auth log) | 655,146 | 70 MB | 654,968 | ~0% ↓ | 25 | ~100% ↓ | 1,464ms | 555ms |

### What the outputs actually look like

The tools solve fundamentally different problems. The Apache error log (56k lines) illustrates the contrast clearly.

**`rtk log` output** (Apache, 25 lines total):
```
Log Summary
   [error] 38081 errors (14890 unique)
   [warn] 168 warnings (31 unique)
   [info] 0 info messages

[ERRORS]
   [×200] [Tue Nov 29 06:12:12 2005] [error] [client 210.245.233.251] File does not exist: /var/www/html/log
   [×136] [Tue Nov 15 12:48:08 2005] [error] [client 201.6.53.88] File does not exist: /var/www/html/cp
   [×105] [Thu Nov 17 21:46:53 2005] [error] [client 220.194.61.230] File does not exist: /var/www/html/blog
   ... +14880 more unique errors
```

**`trml` output** (Apache, 31,266 lines — chronological, deduped):
```
[Thu Jun 09 06:07:05 2005] [error] env.createBean2(): Factory error creating channel.jni:jni
[Thu Jun 09 06:07:05 2005] [error] config.update(): Can't create channel.jni:jni
[Thu Jun 09 06:07:19 2005] [notice] jk2_init() Found child 2330 in scoreboard slot 0 [repeated x8]
[Thu Jun 09 07:11:21 2005] [error] [client 204.100.200.22] Directory index forbidden by rule [repeated x12]
[Thu Jun 09 19:23:31 2005] [error] [client 81.199.21.119] File does not exist: /var/www/html/sumthin [repeated x16]
...
```

### The core trade-off

> **`rtk log` answers "how bad is it?"** — a structured digest: total error count, unique error count, top errors by frequency. Fits in ~25 lines regardless of input size.
>
> **trml answers "what happened?"** — a filtered, deduplicated log stream that preserves temporal sequence, event context, and the full text of every unique error. An agent can follow the chain of events that led to a failure.

| | trml | rtk log |
|---|---|---|
| **Output format** | Filtered log stream (original lines) | Structured digest (counts + top N) |
| **Temporal context** | Preserved — LLM can follow event sequences | Lost — only aggregate counts remain |
| **Dedup style** | `[repeated x8]` inline, in sequence | `[×200]` aggregated across whole file |
| **Line fidelity** | Full text preserved | Truncated at ~80 chars |
| **Stack traces** | Head + tail frames kept | Exception message only |
| **INFO context** | Sampled (startup, state changes) | All INFO dropped |
| **Service profiles** | NiFi, Kafka, ClickHouse, k8s, Redis | None |
| **Scope** | Log files and streams | Any CLI command |

### Known gaps (profiles needed)

trml's heuristics depend on severity keywords and exact-line deduplication. Three log types expose the limits without a dedicated profile:

- **SSH / auth logs** — every line is unique (different IPs, usernames, PIDs). The pattern `Failed password for invalid user X from Y` is semantically repetitive but textually unique, so dedup doesn't fire. Result: ~0% reduction on 655k lines.
- **Nginx access logs** — no `ERROR`/`WARN` keywords in access log format. The severity filter passes almost everything through; only exact-duplicate lines get collapsed. A profile treating 2xx as noise and 4xx/5xx as signal is needed.
- **Zookeeper** — `WARN`/`ERROR` lines each contain a unique thread ID (`RecvWorker:188978561024`), so identical events from different threads don't deduplicate. Stripping thread IDs before comparison would collapse these.

---

## Design constraints

- **Single binary, zero runtime dependencies** — runs on air-gapped on-premises servers without Python, Node, or internet access
- **Stdin → stdout** — composable Unix pipe; no opinion about what's upstream or downstream
- **Never lose signal** — ERRORs, WARNings, stack traces, unique events always pass through
- **Fast** — streaming line-by-line; does not load the full file into memory

---

## License

MIT
