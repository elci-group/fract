# Architecture

Fract is an autonomous architectural maintenance daemon: it watches a project
tree, scores structural entropy per module, proposes semantic refactorings via
an LLM engine, validates them against the real toolchain, and — when safe —
branches, applies, commits, and merges them. Everything it learns is appended
to a local journal and streamed to a live dashboard.

This document covers the pipeline, the module map, data flow, and the
concurrency model. User-facing usage lives in the [README](README.md).

## Pipeline

```
notify watch ──► index (walk + per-language scanners)
                    │
                    ▼
            entropy scoring (complexity)
                    │
                    ▼
        proposal detection (queue) ──► refactor engine
                    │                  (mock | HTTP /chat/completions)
                    ▼
              validation in scratch workspace
          (fmt → clippy → check → test → api-compat)
                    │
                    ▼
             confidence score + safety assess
          (git stability, quiet period, conflicts)
                    │
                    ▼
        branch → apply → commit → merge (mode-gated)
                    │
                    ▼
        PR body render ──► journal persist (.fract/state.jsonl)
                    │
                    ▼
        dashboard (axum) + SSE live tail on 127.0.0.1:7345
```

- **Passive mode** stops at proposal detection: recommendations only.
- **Assisted mode** creates branches with proposed changes for human review.
- **Autonomous mode** runs the full loop including merge, gated by
  `confidence_threshold` and `quiet_period_secs`.

## Module map

`src/lib.rs` is a pure module list; all domain types live in `model`.
Submodules are noted under their parent.

### Pipeline modules

| Module | Responsibility |
|--------|----------------|
| `daemon` | Orchestrates watchers, the live index/health model, and the refactor pipeline. `daemon/notify` does incremental watcher-driven reindexing; `daemon/pipeline` drives refactor → validate → merge. |
| `indexer` | Walks the project tree and runs the per-language scanners to build `Module` metrics. |
| `scanner` | Metric extraction per language: `scanner/rust`, `scanner/python`, `scanner/jsts`, with `scanner/mask` stripping comments/strings and listing public symbols. |
| `complexity` | Structural entropy: six sigmoid-normalised sub-scores combined with fixed weights (formula in the README). |
| `queue` | Candidate/proposal queue: statuses, timelines, diff summaries. |
| `refactor` | `RefactorEngine` trait, the semantic context package, and the deterministic mock engine used by default and in tests. |
| `prompt` | Deterministic prompt rendering and strict `RefactorPlan` JSON parsing, including a grounding check that no planned path escapes the project. |
| `engine_http` | Hand-rolled HTTP/1.1 over `std::net::TcpStream` (http only; https is rejected with a pointer at a local TLS terminator). The only transport code in the LLM path. |
| `validation` | Runs `cargo fmt`, `clippy`, `check`, `test`, and the api-compat probe against a proposal in its scratch workspace. |
| `confidence` | Scores a proposal in `[0, 1]` from its `ValidationReport`. |
| `merge` | Safety assessment (quiet period, git stability, conflicts), then apply / commit / merge via git2. |
| `pr` | Pure presentation: Conventional Commit messages and review-ready PR bodies from a `Proposal`. |
| `web` | axum dashboard: JSON API, static file serving, and the SSE live tail over the event bus. |
| `report` | Unified `Report` model plus `human`, `json`, `jsonl`, `sarif`, and `markdown` renderers, so the CLI, HTTP API, dashboard, and PR bodies share one schema. |

### Infrastructure modules

| Module | Responsibility |
|--------|----------------|
| `config` | TOML configuration and defaults: mode, thresholds, watch/ignore patterns, bind address, output knobs, LLM settings. |
| `events` | Central event bus: tokio `broadcast` channel (capacity 1024) plus a bounded history for late subscribers. |
| `git` | Thin git2 helpers: repo discovery, current branch, stability, conflict detection. |
| `model` | Core domain types: `model/module` (metrics, language, health bands), `model/proposal` (lifecycle, diffs, validation), `model/event`, `model/health`. |
| `store` | Append-only JSONL journal. `store/{event,health,proposal}` encode/decode each record type through the in-tree `json::Value`. |

### Zero-dependency replacements (amber-enforced)

| Module | Replaces | Responsibility |
|--------|----------|----------------|
| `cli` | `clap` | Manual argument parser for the `fract` binary (subcommands, output flags, verbosity). |
| `error` | `anyhow` | Owned error type with `?` conversions and a `Context` trait. |
| `id` | `uuid` | Process-local `fract-{counter}` identifiers. |
| `json` | `serde_json` | `Value` type + writer, serde `Serializer` (`json/ser`), recursive-descent parser (`json/de`). |
| `walk` | `walkdir` | Ignore-pattern-aware recursive tree walk. |
| `time` | `chrono` | RFC 3339 timestamps over `std::time::SystemTime`, with serde helpers. |
| `scratch` | `tempfile` | Temporary validation workspaces under the system temp dir. |

## Data flow

- **Journal.** `store` appends one JSON object per line to
  `<project_root>/.fract/state.jsonl`. Every record carries a schema version
  and a discriminator: `{"v":1,"type":"proposal"|"event"|"health", ...}`.
  Proposals persist identity and lifecycle only — heavy `changed_files`
  payloads are re-derived on demand. On startup the journal is replayed;
  malformed lines are skipped with a warning, so corruption never aborts
  startup.
- **Config.** Loaded in `main.rs`: `--config <path>` if given, else
  `./fract.toml` if it exists, else defaults for the current directory.
  `fract init` writes a fully-default `fract.toml`. CLI output flags override
  the `[output]` config section.
- **Entropy.** Each module's six sub-scores (size, cyclomatic complexity,
  cohesion loss, dependency density, public surface, duplication) are
  sigmoid-normalised to `[0, 1]` and combined with fixed weights — see the
  formula in the README. Modules at or above `entropy_threshold` enter the
  queue.
- **Events.** Filesystem saves, git activity, and pipeline outcomes flow
  through `events::EventBus`: broadcast to live subscribers (the SSE tail)
  and appended to the journal for restart recovery.

## Concurrency model

- A single multi-threaded **tokio** runtime (`main.rs`) drives the daemon and
  the axum server.
- Shared state is held behind `tokio::sync::RwLock`: the module index
  (`Vec<Module>`), the `ProjectHealth` snapshot, and event history. The
  dashboard reads snapshots; only the daemon writes.
- Blocking work is pushed off the async worker threads with
  `tokio::task::spawn_blocking`: git2 staging/commit, file writes during
  `merge::apply`, and stat calls during safety assessment.
- The notify watcher feeds a bounded channel; when full or closed, events are
  dropped with a `warn!` rather than blocking the watcher.
- Validation shell-outs use `tokio::process::Command` with timeouts, so a
  hung toolchain cannot stall the pipeline.

## The zero-dependency constraint

The dependency set is fixed at nine crates (async runtime, file watching, the
web stack, config parsing, logging, git) and enforced by amber in strict mode
via `.amber.toml`; `scripts/ci.sh` fails otherwise. Everything else — errors,
JSON, CLI parsing, timestamps, ids, temp dirs, tree walking, HTTP transport —
is implemented in-tree with `std`. The `benches/` head-to-head suites and the
`scripts/perf-check.sh` guardrails exist to keep those replacements honest.
See [CONTRIBUTING.md](CONTRIBUTING.md) for how to propose an exception.
