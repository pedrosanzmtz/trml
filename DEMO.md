# Built Live on Stage

This project was created in a single session — live, on stage, in front of an audience.

---

## The Event

**"Agentic AI: El nuevo modelo de desarrollo de software"**
Organized by [Python Monterrey (@pymty)](https://social.org.mx/events/f27cbcb3-8786-4dcb-8865-504f4fbfbd14)

- **When:** Thursday, March 26, 2026 — 6:30 PM to 10:15 PM
- **Where:** Spectrum Effect R&D — Torre Comercial América (Piso 30), Av. Batallon de San Patricio No. 111, San Pedro Garza García

The talk was about how AI agents are changing software development — the Reasoning → Action → Observation loop, how tools like Claude Code shift the developer's role, and what it looks like in practice. The live demo was the whole point.

---

## The Setup

Pedro had a spec. It was a markdown file sitting in his Downloads folder describing a log compression tool for LLM context windows — an idea that came from a real problem at work: log files are enormous, and dumping raw logs into an agent context is wasteful and slow.

He had the idea. He had a spec. He had his iPhone and iPad. And he had Claude.

While Pedro chatted from his phone and tablet, Claude was running autonomously on his MacBook Air M1 (8GB RAM) — reading files, writing code, running commands, pushing to GitHub, fetching web pages, running benchmarks. Pedro never touched the keyboard on the laptop. He just described what he wanted in plain language and watched it happen.

---

## The Session

### Step 1 — A name

Before writing a single line of code, Pedro asked Claude to brainstorm short names in the style of RTK or jq — something terse and memorable. A handful of candidates came up: `slg`, `lsq`, `lsm`, `lcr`, `lpc`, `trml`. Claude checked availability on GitHub and crates.io for each one.

`trml` — short for *trim log* — was available on both. That was the name.

### Step 2 — Project setup

The spec moved from Downloads to `~/Documents/projects/trml/`. Claude initialized a Rust project with `cargo init`, then created the GitHub repository at `pedrosanzmtz/trml` — all from the conversation.

### Step 3 — Building v0.1

This is where it got interesting. Over the course of the session, Claude implemented the entire first version:

- Reading from stdin and file paths
- ANSI escape code stripping
- Repetition deduplication with `[repeated xN]` annotations
- Severity filtering — ERRORs, WARNINGs, and stack traces always preserved
- Stack trace compression — keep the first 3 frames and the last, hide the rest
- A full profile system with auto-detection for NiFi, Kafka, ClickHouse, Kubernetes, and Redis
- `--level` (light / normal / aggressive), `--profile`, `--stats`, `--explain`, `--learn` mode
- Claude Code hook integration via `trml hook install`
- 19 passing tests

One conversation. No switching to an IDE. No context-switching to a terminal. Just describing the behavior and watching it materialize.

### Step 4 — Benchmarking against RTK

To give the audience a concrete sense of what the tool actually does, Claude installed RTK (a competing log reduction tool), ran a side-by-side comparison on the test fixtures, and reported the results. `trml` held its own.

### Step 5 — The README

Claude wrote a comprehensive README — usage examples, profile docs, architecture overview, and the RTK comparison table — and committed it.

### Step 6 — Finding the cracks

After v0.1 was done, Claude analyzed the codebase and identified bugs and improvement areas:

- **Bug:** dedup was silently dropping data in some edge cases
- **Bug:** `sample_info` semantics were inverted
- **Bug:** `--explain` mode wasn't instrumented through the pipeline correctly
- **14 additional improvement areas** across features, profiles, and architecture

### Step 7 — v0.2

Claude implemented all of them:

- Bug fixes for dedup data loss, sample_info, and --explain
- New flags: `--context N`, `--tail N`, `--since`, `--until`
- Non-consecutive dedup (same pattern, non-adjacent lines)
- Color output on TTY with `--color`
- `--follow` mode for streaming tail
- New bundled profiles: MongoDB, Nginx/Apache, Java GC, Elasticsearch
- Architecture overhaul: a true streaming pipeline built on a `Stage` trait

### Step 8 — Real-world benchmarks

Pedro wanted real numbers, not toy examples. Claude downloaded four real-world log datasets from the [loghub](https://github.com/logpai/loghub) collection:

| Dataset    | Lines   | Size  |
|------------|---------|-------|
| Nginx      | 51,000  | ~4MB  |
| Apache     | 56,000  | ~4MB  |
| Zookeeper  | 74,000  | ~6MB  |
| SSH        | 655,000 | 70MB  |

Claude ran `trml` and RTK against each, measured reduction ratios and throughput, and updated the README with the results.

### Step 9 — GitHub issues

The benchmarks revealed gaps — places where the profiles needed tuning for SSH, Nginx, and Zookeeper logs. Claude opened 3 GitHub issues, each with specific observations and suggested improvements.

---

## What this demonstrates

The entire arc — from a spec file to a working Rust CLI with benchmarks, a README, and filed GitHub issues — happened in one session. Pedro's role was to describe the goal and make decisions. Claude's role was everything else: naming, research, implementation, testing, benchmarking, documentation, and project management.

This is what the Reasoning → Action → Observation loop looks like when it's working. The developer stays at the level of intent. The agent handles execution.

The laptop was just a machine running in the background. The work happened in the conversation.
