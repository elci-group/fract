# Changelog

All notable changes to this project are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
Releases are cut automatically by [kaptaind](kaptaind.toml), which clusters
changes, runs the full quality gate (`scripts/ci.sh`), and commits with a
semver bump — version numbers are never hand-written.

## [Unreleased]

### Changed

- Finish replacing magic numbers with named constants in the daemon
  pipeline, completing the `pipeline.rs` health-entropy reduction from
  0.70 to 0.65.

## [0.4.3] - 2026-07-12

### Added

- 15 property tests built on a small deterministic generator (no new
  dependencies): JSON value-tree serialization roundtrips, entropy
  bounds and monotonicity characterizations, and walker invariants.
- ~30 named constants replacing magic numbers across the daemon,
  `engine_http`, and pipeline modules.

### Fixed

- Eliminate an environment-dependent test race found under Miri via a
  test-only `Style::detect_auto` seam. A full Miri run (266 tests) now
  passes with no undefined behavior.

### Changed

- Begin the `pipeline.rs` health-entropy reduction (0.70 toward 0.65)
  by extracting helpers and simplifying control flow.

### Documented

- Lock in characterization tests for two known behaviors: entropy is
  not globally monotone in `lines` (NaN-heavy inputs), and untracked
  files do not dirty the working-tree check. Both are deliberate
  semantics, now pinned by tests.

## [0.4.2] - 2026-07-12

### Added

- GitHub Actions CI workflow (`.github/workflows/ci.yml`) running the
  full `scripts/ci.sh` quality gate.
- ~1,700 lines of new unit tests across `config`, `events`, `git`,
  `json::ser`, the CLI (`main`), `model`, `queue`, `refactor`,
  `report::style`, `validation`, `web`, and the daemon pipeline.
  Line coverage rises from 65.86% to 92.35%; test count from 178 to 341.

### Removed

- Drop the stale `examples/` directory.

## [0.4.1] - 2026-07-12

### Added

- `ARCHITECTURE.md`: module map, data flow, and extension points.

### Changed

- Rewrite `README.md` around installation, quickstart, and pointers to
  the architecture and contributing docs.

## [0.4.0] - 2026-07-12

### Added

- `CONTRIBUTING.md`: development setup, the quality gate, and how the
  kaptaind auto-commit workflow treats working-tree changes.
- This changelog, backfilled through v0.2.10.

## [0.3.0] - 2026-07-12

### Added

- `LICENSE-MIT` and `LICENSE-APACHE` files matching the dual-license
  metadata already declared in `Cargo.toml`.
- Module-level documentation headers across every source module;
  `cargo doc` builds with zero warnings.

## [0.2.10] - 2026-07-12

### Changed

- Enforce clippy `pedantic` project-wide via `[lints.clippy]` in `Cargo.toml`,
  with a single justified exception: `cast_precision_loss` (entropy/health math
  is inherently `f64` over integer metrics).
- Fix the resulting pedantic lints across `store`, `time`, `validation`,
  `walk`, `web`, the scanners, and the integration tests.
- Bench harness cleanup: `std::hint::black_box` around measured calls,
  inline format args, and iterator tidying in the perf suite and head-to-head
  benches.

## [0.2.9] - 2026-07-12

### Changed

- Remove duplicated `Value` helpers (`as_object`, `as_array`, `get`,
  `get_str`); share them via the `json` module instead.
- Extract the repeated health recompute-and-persist sequence in the daemon
  into a single `persist_health` helper.

### Fixed

- Delete dead queue code left over from earlier pipeline revisions.

## [0.2.8] - 2026-07-12

### Fixed

- Continue the observability hardening in the daemon pipeline: warn (with
  proposal id and error) when proposal or health persistence fails instead of
  dropping the error.
- Handle `merge::diff_last_commit` failure explicitly, logging and falling
  back to an empty diff rather than unwrapping.

## [0.2.7] - 2026-07-12

### Fixed

- Replace silent `let _ = ...` error drops with structured `warn!` logging
  across the daemon, notify handler, HTTP engine, store, and validation:
  full/closed notify channels, failed journal appends, and failed socket
  timeout configuration are now visible instead of swallowed.

## [0.2.6] - 2026-07-12

### Changed

- Move blocking work off the tokio worker threads with `spawn_blocking`:
  file writes in `merge::apply`, the git2 staging/commit path in
  `merge::commit`, and the `last_modified` stat in `merge::assess`.
- Extend the journal store with additional persistence helpers used by the
  daemon pipeline.

## [0.2.5] - 2026-07-12

- Version-only release: a kaptaind race artifact that bumped `Cargo.toml` and
  `VERSION` with no code changes.

## [0.2.4] - 2026-07-12

### Added

- `cargo audit` security-advisory scan added to the `scripts/ci.sh` quality
  gate.

### Changed

- Upgrade `git2` from 0.20 to 0.21; small accompanying fixes in `git.rs` and
  `merge.rs`.

## [0.2.3] - 2026-07-11

### Changed

- Split the monolithic `model.rs` into a `model` module directory
  (`event`, `health`, `module`, `proposal`), one file per domain type.

## [0.2.2] - 2026-07-11

### Changed

- Split the monolithic `store.rs` (~560 lines) into a `store` module directory
  (`event`, `health`, `proposal` codecs plus the journal core in `mod.rs`).

## [0.2.1] - 2026-07-11

### Changed

- Split the monolithic `daemon.rs` (~660 lines) into a `daemon` module
  directory: `mod.rs` (orchestration and state), `notify.rs` (incremental
  watcher-driven reindexing), and `pipeline.rs` (refactor/validate/merge).

## [0.2.0] - 2026-07-11

### Added

- HTTP refactor engine: hand-rolled HTTP/1.1 client (`engine_http`) targeting
  OpenAI-compatible `/chat/completions` endpoints, since `reqwest` is
  amber-forbidden.
- Grounded prompt rendering and strict structured-output parsing (`prompt`):
  forces a single JSON `RefactorPlan` reply and rejects plans whose paths
  escape the project.
- Conventional Commit messages and review-ready PR body rendering (`pr`).
- Unified output model with `human`, `json`, `jsonl`, `sarif`, and `markdown`
  renderers (`report`), wired into the CLI via `--format`, `--color`,
  `--no-color`, and `-v`/`-q` flags.
- Per-language scanners split out of the monolith: `scanner::{rust, python,
  jsts, mask}` for Rust, Python, TypeScript, and JavaScript metrics.
- Append-only JSONL journal (`store`) persisting proposals, events, and the
  health trend to `.fract/state.jsonl` so state survives restarts.
- Live dashboard SSE stream and the `tests/live_daemon_sse.rs` integration
  test; expanded `validation` pipeline; split the in-tree `json` module into
  `value`/`ser`/`de` submodules.

### Changed

- Extract the core domain model out of `lib.rs` into `model.rs` (`lib.rs`
  shrinks from ~250 lines to a pure module list).

## [0.1.1] - 2026-07-09

### Added

- Deterministic single-pass daemon entry points (`Daemon::scan`,
  `Daemon::detect_proposals`) that reindex and emit proposals without
  shelling out or touching git — safe to drive from tests and offline hosts.
- `tests/integration_pipeline.rs`: end-to-end pipeline integration test built
  on those entry points.

[Unreleased]: https://github.com/elci-group/fract/compare/v0.2.10...HEAD
[0.2.10]: https://github.com/elci-group/fract/compare/v0.2.9...v0.2.10
[0.2.9]: https://github.com/elci-group/fract/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/elci-group/fract/compare/v0.2.7...v0.2.8
[0.2.7]: https://github.com/elci-group/fract/compare/v0.2.6...v0.2.7
[0.2.6]: https://github.com/elci-group/fract/compare/v0.2.5...v0.2.6
[0.2.5]: https://github.com/elci-group/fract/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/elci-group/fract/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/elci-group/fract/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/elci-group/fract/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/elci-group/fract/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/elci-group/fract/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/elci-group/fract/releases/tag/v0.1.1
