# Contributing to Fract

Thanks for helping keep Fract healthy. The project has a few unusual
constraints — a near-zero dependency policy and fully automated releases — so
please read this before opening a PR.

## Building and testing

```bash
cargo build          # debug build
cargo test           # unit + integration tests
cargo build --release  # optimized binary at target/release/fract
```

**The quality gate is `scripts/ci.sh`.** It runs, in order:

1. `cargo fmt --all -- --check`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test`
4. `amber` dependency-policy check (threshold 70, strict mode)
5. `cargo audit` (security advisories)
6. `cargo build --release` (and prints the release binary size)

A change is only "done" when `bash scripts/ci.sh` passes. This is the same
hook kaptaind runs before it will commit anything.

## Dependency policy (amber)

Fract pursues a **zero-dependency philosophy**: anything that can be
implemented in-tree with `std`, without meaningful loss, is. `anyhow`,
`serde_json`, `walkdir`, `clap`, `chrono`, `uuid`, and `tempfile` were all
removed and replaced with small internal modules (`error`, `json`, `walk`,
`cli`, `time`, `id`, `scratch`). This keeps compile times, supply-chain
surface, and binary size minimal — and the head-to-head benches in `benches/`
prove the replacements are competitive.

The policy is enforced by [amber](https://github.com/anthropics/amber) using
[`.amber.toml`](.amber.toml) with `strict = true`:

- **Required (allowed):** `tokio`, `notify`, `axum`, `futures-util`, `serde`,
  `toml`, `tracing`, `tracing-subscriber`, `git2` — the async runtime, file
  watching, the dashboard web stack, config parsing, logging, and git.
- **Forbidden:** `anyhow`, `chrono`, `walkdir`, `regex`, `clap`,
  `serde_json`, `uuid`, `tempfile`, `async-trait`, `tower-http`, `thiserror`,
  `sha2`, `hex`, `reqwest`, `tokio-test` — everything we replaced in-tree.

Reintroducing a forbidden crate fails CI.

**Proposing a new dependency:** open an issue first explaining why the
capability cannot reasonably live in-tree (size, correctness risk, security
criticality). If accepted, add the crate to `required` in `.amber.toml` in
the same PR as the dependency, with a comment justifying it — the same way
`futures-util` is annotated today.

## Lint policy

- `pedantic = warn` is enabled project-wide in `Cargo.toml` `[lints.clippy]`,
  and CI promotes everything to errors with `-D warnings`.
- The one deliberate exception is `cast_precision_loss = "allow"`: entropy and
  health math is inherently `f64` over integer metrics, and precision loss at
  2^53 is irrelevant for 0.0–1.0 scores and line counts. The rationale lives
  inline in `Cargo.toml`; keep it there.
- If a pedantic lint genuinely does not apply, prefer a targeted
  `#[allow(...)]` with a comment over weakening the workspace-wide config.

## Formatting

Default `rustfmt` — no custom `rustfmt.toml`. Run `cargo fmt --all` before
pushing; CI checks with `--check`.

## Test conventions

- **No external test dependencies.** `[dev-dependencies]` is intentionally
  empty. Use the hand-rolled helpers (`scratch` for temp dirs, `id` for
  deterministic ids, in-tree fixtures) instead of `tempfile`, `tokio-test`,
  etc.
- Integration tests live in `tests/` and must be **deterministic**: fixed
  inputs, no real network, no git remotes, no reliance on wall-clock timing
  beyond generous timeouts. `Daemon::scan` / `detect_proposals` exist
  precisely so the pipeline can be driven offline — use them.
- Unit tests live next to the code in `#[cfg(test)] mod tests`.

## Releases: kaptaind auto-commit workflow

Contributors **never hand-write version numbers** — not in `Cargo.toml`, not
in `VERSION`. Releases are fully automated by kaptaind
([`kaptaind.toml`](kaptaind.toml)):

- The daemon watches the working tree and **clusters** related edits
  (`window = 5`).
- Each cluster is **scored** (size/API/dependency/runtime weights:
  `s = 0.35, a = 0.30, d = 0.20, r = 0.15`).
- It runs `scripts/ci.sh` as a required test hook. If the gate is red, no
  commit happens.
- On green, it commits with a **semver bump** derived from the score:
  patch ≥ 0.15, minor ≥ 0.45, major ≥ 0.75 (`[version_thresholds]`).

Just make focused changes and let the daemon do the bookkeeping. See
`CHANGELOG.md` for the resulting release history.

## Performance gates

`scripts/perf-check.sh` runs the `perf_suite` bench multiple times, merges
samples (min latency / max throughput), and enforces hard guardrails — e.g.
the in-tree JSON writer must be at least at `serde_json` parity, and
id/CLI/time operations have absolute ns ceilings. The trend baseline lives in
[`bench/baseline.json`](bench/baseline.json).

- If your change touches `walk`, `scanner`, `json`, `time`, `id`, or `cli`,
  run `bash scripts/perf-check.sh` before submitting.
- Only refresh the baseline (`--seed`) when a metric legitimately and
  permanently improves, and call it out in the PR.

## PR expectations

- `bash scripts/ci.sh` green (fmt, clippy `-D warnings`, tests, amber, audit,
  release build).
- Amber policy green — no new dependencies without the `.amber.toml`
  discussion above.
- Docs updated when behavior changes: `README.md` for user-facing surface,
  `ARCHITECTURE.md` for pipeline/module changes, `CHANGELOG.md` entries are
  written by maintainers at release time.
- Tests for new behavior, following the conventions above.
