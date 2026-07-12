# Fract

Autonomous architectural maintenance daemon for Rust, Python, TypeScript, and
JavaScript projects.

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

When structural entropy crosses a threshold, Fract builds a semantic context
package, invokes an LLM refactor engine, validates the result with
`cargo fmt`, `clippy`, `check`, and `test`, and—if safe—applies the change.
Every step is journaled to `.fract/state.jsonl` and streamed to a live
dashboard.

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

The full stage-by-stage design, module map, and concurrency model live in
[ARCHITECTURE.md](ARCHITECTURE.md).

## Installation

Requires a recent stable Rust toolchain (see `rust-version` in `Cargo.toml`).

```bash
cargo build --release
# binary at target/release/fract
```

To install onto your `PATH` instead:

```bash
cargo install --path .
# installs `fract` to ~/.cargo/bin
```

## Quick start

```bash
# Generate a config file (fract.toml) in the current directory
./target/release/fract init

# Index the project and print module health
./target/release/fract index

# Run the daemon with the dashboard
./target/release/fract run
```

Open http://127.0.0.1:7345 to see the glassmorphism dashboard.

## CLI

```
fract [OPTIONS] [COMMAND]

Commands:
  run     Run the daemon (default)
  index   Index the project and print module health
  init    Generate a default configuration file (--path/-p to choose where)

Options:
  -c, --config <FILE>    Path to configuration file
  -f, --format <FORMAT>  Output format: human, json, jsonl, sarif, markdown
      --color <WHEN>     Colour output: auto, always, never
      --no-color         Disable colour output (also honors NO_COLOR)
  -v, --verbose          Increase verbosity (repeatable, e.g. -vv)
  -q, --quiet            Decrease verbosity
  -h, --help             Print help
  -V, --version          Print version
```

If no `--config` is given, Fract loads `./fract.toml` when present and falls
back to built-in defaults otherwise.

## Modes

- **Passive** — only recommends refactors.
- **Assisted** — creates branches with proposed changes.
- **Autonomous** — refactors, tests, commits, and merges automatically.

## Configuration

No example config ships in the repo — `fract init` generates a `fract.toml`
with every default written out. Key options:

| Option | Description |
|--------|-------------|
| `mode` | `passive` (default), `assisted`, or `autonomous` |
| `entropy_threshold` | Modules at or above this score enter the queue (default `0.82`) |
| `confidence_threshold` | Minimum confidence before auto-merge (default `0.90`) |
| `quiet_period_secs` | Seconds of no edits before a merge is considered safe (default `120`) |
| `watch_patterns` | Globs to watch — defaults cover `src/**/*.rs`, `**/*.py`, `**/*.ts`, `**/*.js` |
| `ignore_patterns` | Defaults skip `target/`, `.git/`, `.fract/`, `node_modules/`, `.venv/`, `dist/` |
| `bind` | Dashboard address (default `127.0.0.1:7345`) |
| `output.format` | Default report format: `human`, `json`, `jsonl`, `sarif`, or `markdown` |
| `output.max_findings` | Noise budget: cap on actionable findings (0 = unlimited) |
| `llm.provider` | `mock` (default), `openai`, `anthropic`, or `local` |
| `llm.model` | Model name (default `gpt-oss-120b`) |

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

Each sub-score is normalised to `[0, 1]` with soft (sigmoid) clamping.

## Quality gates

Fract holds itself to the same standard it enforces on your code:

- `scripts/ci.sh` — fmt, clippy pedantic with `-D warnings`, tests, the amber
  dependency policy, `cargo audit`, and a release build. This is the gate
  every change must pass.
- `scripts/perf-check.sh` — performance guardrails for the hand-rolled
  replacements (JSON, walk, scanners, time, ids, CLI parsing) against
  `bench/baseline.json`.
- A zero-dependency philosophy: only nine vetted crates; common conveniences
  (`anyhow`, `serde_json`, `walkdir`, `clap`, `chrono`, `uuid`, `tempfile`)
  are implemented in-tree and forbidden from returning.

See [CONTRIBUTING.md](CONTRIBUTING.md) for details, and
[CHANGELOG.md](CHANGELOG.md) for the release history.

## License

Licensed under either of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE) at your option.
