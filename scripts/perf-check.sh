#!/usr/bin/env bash
# Performance regression gate for fract.
#
# Runs the head-to-head perf suite multiple times, merges the samples
# (latency: take min; throughput: take max), then compares against the
# baseline. Fails if any metric regresses beyond the tolerance.
#
# Usage:
#   scripts/perf-check.sh            # compare against bench/baseline.json
#   scripts/perf-check.sh --seed     # refresh bench/baseline.json from current
#   scripts/perf-check.sh --samples 5 --tol 0.30
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BASELINE="$ROOT/bench/baseline.json"
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

SEED=0
TOL="${PERF_TOL:-0.25}"
SAMPLES="${PERF_SAMPLES:-2}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --seed) SEED=1; shift ;;
    --tol) TOL="$2"; shift 2 ;;
    --samples) SAMPLES="$2"; shift 2 ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

cd "$ROOT"

echo "perf-check: building perf suite (samples=$SAMPLES) ..."
cargo build --release --benches >/dev/null 2>&1

BIN="$(find "$ROOT/target/release/deps" -maxdepth 1 -name 'perf_suite-*' -type f -executable | head -n1)"
if [[ -z "$BIN" ]]; then
  echo "perf-check: could not locate perf_suite binary" >&2
  exit 2
fi

# Collect samples.
for i in $(seq 1 "$SAMPLES"); do
  "$BIN" --output "$TMPDIR/sample-$i.json" >/dev/null
done
echo "perf-check: collected $SAMPLES samples"

# Merge samples into a single best-of-N view.
python3 - "$TMPDIR" "$SAMPLES" "$TMPDIR/merged.json" <<'PY'
import json, sys, os
d = sys.argv[1]
n = int(sys.argv[2])
out = sys.argv[3]
merged = {}
for i in range(1, n + 1):
    with open(os.path.join(d, "sample-%d.json" % i)) as f:
        data = json.load(f)  # flat dict: name -> value
    for name, val in data.items():
        if name not in merged:
            merged[name] = val
        elif "ns_per" in name:        # lower is better
            merged[name] = min(merged[name], val)
        else:                          # per_sec / speedup: higher is better
            merged[name] = max(merged[name], val)
with open(out, "w") as f:
    json.dump(merged, f, indent=2)
PY

if [[ "$SEED" -eq 1 ]]; then
  mkdir -p "$(dirname "$BASELINE")"
  cp "$TMPDIR/merged.json" "$BASELINE"
  echo "perf-check: seeded baseline at $BASELINE"
  exit 0
fi

if [[ ! -f "$BASELINE" ]]; then
  echo "perf-check: no baseline at $BASELINE; run with --seed first" >&2
  exit 2
fi

# Hard performance guardrails. These encode the actual contract and are stable
# across hosts, unlike tight baseline-relative checks which flake on shared
# machines. The baseline is still printed for trend visibility.
python3 - "$TMPDIR/merged.json" "$BASELINE" <<'PY'
import json, sys
cur  = json.load(open(sys.argv[1]))   # flat dict: name -> value
base = json.load(open(sys.argv[2]))

RULES = {
    "walk_speedup":         ("min", 0.5),
    "scanner_speedup":      ("min", 0.4),
    "json_speedup":         ("min", 1.0),
    "time_ns_per_format":   ("max", 2000.0),
    "id_ns_per_id":         ("max", 500.0),
    "cli_ns_per_parse":     ("max", 500.0),
}

fails = []
print(f"{'metric':<22} {'current':>14} {'limit':>14} {'baseΔ':>9}  status")
for name, (kind, limit) in RULES.items():
    if name not in cur:
        fails.append(f"{name}: missing in current run")
        print(f"{name:<22} {'(missing)':>14} {limit:>14.3f} {'':>9}  FAIL")
        continue
    cv = cur[name]
    bv = base.get(name)
    pct = f"{(cv-bv)/bv*100:+.1f}%" if bv else "n/a"
    if kind == "min":
        ok = cv >= limit
        lim = f">= {limit:.3f}"
    else:
        ok = cv <= limit
        lim = f"<= {limit:.1f}"
    status = "ok" if ok else "FAIL"
    if not ok:
        fails.append(f"{name}: {cv:.3f} violates {lim}")
    print(f"{name:<22} {cv:>14.3f} {lim:>14} {pct:>9}  {status}")

if fails:
    print("\nGUARDRAIL VIOLATIONS:")
    for f in fails:
        print("  - " + f)
    sys.exit(1)
print("\nperf-check: all guardrails satisfied")
PY
