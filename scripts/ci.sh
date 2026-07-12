#!/usr/bin/env bash
# Fract quality gate. Used by the kaptaind test hook and by hand.
#
# Runs formatting, linting, tests, dependency policy (amber), a security
# advisory scan (cargo audit), and a release build, then prints the release
# binary size for tracking.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "==> cargo fmt --check"
cargo fmt --all -- --check

echo "==> cargo clippy --all-targets -- -D warnings"
cargo clippy --all-targets -- -D warnings

echo "==> cargo test"
cargo test

echo "==> amber dependency policy"
amber "$ROOT" --config "$ROOT/.amber.toml" --threshold 70 --format console >/dev/null

echo "==> cargo audit (security advisories)"
cargo audit

echo "==> cargo build --release"
cargo build --release

BIN="$ROOT/target/release/fract"
if [ -f "$BIN" ]; then
    SIZE="$(du -h "$BIN" | cut -f1)"
    echo "==> release binary size: $SIZE"
fi

echo "==> quality gate passed"
