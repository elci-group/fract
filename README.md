# Fract

Autonomous architectural maintenance daemon for Rust, Python, TypeScript, and JavaScript projects.

> **Copilot** writes code.  
> **Silverline** fixes errors.  
> **Kaptaind** manages releases.  
> **Fract** preserves architecture.

## What it does

Fract runs continuously in the background, watches your project, and asks:

- Is this module becoming too large?
- Is cohesion falling?
- Are responsibilities diverging?
- Is duplication increasing?
- Are dependency cycles emerging?

When structural entropy crosses a threshold, Fract builds a semantic context package,
invokes an LLM refactor engine, validates the result with `cargo fmt`, `clippy`,
`check`, and `test`, and—if safe—applies the change.

## Pipeline

```
Filesystem Watcher
        │
        ▼
Project Indexer ──► Complexity Engine ──► Refactor Candidate Queue
                                               │
                                               ▼
                              LLM Refactor Engine
                                               │
                                               ▼
                    Compilation ──► Test ──► Static Analysis
                                               │
                                               ▼
                                   Merge Safety Check
                                               │
                                               ▼
                                        Git Commit / Patch
```

## Quick start

```bash
# Build
cargo build --release

# Generate a config file
./target/release/fract init

# Index and inspect module health
./target/release/fract index

# Run the daemon with the dashboard
./target/release/fract run
```

Open http://127.0.0.1:7345 to see the glassmorphism dashboard.

## Modes

- **Passive** — only recommends refactors.
- **Assisted** — creates branches with proposed changes.
- **Autonomous** — refactors, tests, commits, and merges automatically.

## Configuration

See [`fract.toml`](fract.toml) for an example. Key options:

| Option | Description |
|--------|-------------|
| `mode` | `passive`, `assisted`, or `autonomous` |
| `entropy_threshold` | Modules above this score enter the queue (default `0.82`) |
| `confidence_threshold` | Minimum confidence before auto-merge (default `0.90`) |
| `quiet_period_secs` | Seconds of no edits before merge (default `120`) |
| `llm.provider` | `mock`, `openai`, `anthropic`, or `local` |

## Entropy formula

```
Entropy =
  0.3 * module_size
+ 0.2 * cyclomatic_complexity
+ 0.2 * cohesion_loss
+ 0.1 * dependency_density
+ 0.1 * public_surface
+ 0.1 * duplication
```

Each sub-score is normalised to `[0, 1]` with soft clamping.

## Architecture

- `daemon` — orchestrates watchers, indexing, queue processing, and merges.
- `indexer` — walks the project and extracts module metrics.
- `complexity` — computes the structural entropy score.
- `queue` — holds candidates and proposals.
- `refactor` — LLM refactor engine interface plus a mock implementation.
- `validation` — runs the cargo / toolchain validation pipeline.
- `merge` — safety checks and application of accepted refactors.
- `confidence` — scores proposals from validation results.
- `web` — dashboard API and static file serving.

## Status

This is a working scaffold. The mock refactor engine demonstrates the pipeline;
replace it with a real LLM backend by implementing the `RefactorEngine` trait.

## License

MIT OR Apache-2.0
