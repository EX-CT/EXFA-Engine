#!/usr/bin/env bash
# ci/run_suites.sh MODE "ENGINE_CMD" [skip-round1]
# Run the EXFA-Bench contract suites against one engine command (native binary or "wasmtime run ... exfa.wasm --").
#   BENCH:         EXFA-Bench checkout (default ./_bench), pinned by bench.lock
#   EXFA_DATASET:  compiled dataset path (required)
#   skip-round1:   skip the two round-1 sha256 checks (SDE update flow: new data legitimately changes the bytes)
# The gate is check_no_regress.py against $BENCH/baselines/exfa-engine.json plus the explicit EFT gates
# in ci/gate.json (round-1 EFT export 326/326, mutated EFT export/import 93/99).
set -uo pipefail
NAME=$1; E=$2; SKIP_R1=${3:-}
# a plain relative binary path must stay valid when the suite tools spawn it with cwd=<suite dir>
if [ -f "$E" ]; then E=$(cd "$(dirname "$E")" && pwd)/$(basename "$E"); fi
BENCH=${BENCH:-_bench}
OUT=$PWD/ci-results/$NAME; mkdir -p "$OUT"
D=$EXFA_DATASET
export EXFA_DATASET=$D
export EXFA_DATASET_R5=${EXFA_DATASET_R5:-$D}
fail=0
note() { echo "$1" | tee -a "$OUT/summary.md"; }

[ -d "$BENCH/suites/round1" ] || { echo "::error::EXFA-Bench checkout not found at $BENCH (set BENCH)"; exit 1; }
[ -f "$BENCH/baselines/exfa-engine.json" ] || { echo "::error::baseline missing in $BENCH"; exit 1; }

# round-1 corpus, batch output sha256 (pure refactors must not change a byte)
for f in "$BENCH"/suites/round1/cases/*.json; do python3 -c "import json,sys;print(json.dumps(json.load(open(sys.argv[1]))))" "$f"; done > "$OUT/round1.jsonl"
$E batch < "$OUT/round1.jsonl" > "$OUT/round1.out.jsonl"
sha=$(sha256sum < "$OUT/round1.out.jsonl" | cut -d' ' -f1)
# additive output changes: with the new stats-ext keys stripped, the output must still be the base bytes
base=$(python3 ci/strip_keys.py $(grep -v '^#' ci/round1-new-keys.txt) < "$OUT/round1.out.jsonl" | sha256sum | cut -d' ' -f1)
if [ -z "$SKIP_R1" ]; then
  if [ "$base" = "$(cat ci/round1-base.sha256)" ]; then note "- round-1 minus new keys ($(grep -v '^#' ci/round1-new-keys.txt | tr '\n' ' ')) sha256 $base = ci/round1-base.sha256 (pre-existing output byte-identical)"; else note "- round-1 minus new keys sha256 $base != ci/round1-base.sha256 (a pre-existing field changed)"; fail=1; fi
  want=$(cat ci/round1.sha256)
  if [ "$sha" = "$want" ]; then note "- round-1 batch sha256 $sha (unchanged)"; else note "- round-1 batch sha256 $sha != ci/round1.sha256 $want (output changed: update ci/round1.sha256 only for an intended output change)"; fail=1; fi
else
  note "- round-1 sha checks skipped (skip-round1); sha256 $sha / base $base recorded only"
fi

(cd "$BENCH/suites/round1" && python3 tools/check_eft_export.py --rpc-cmd "$E serve-stdio" > "$OUT/eft18.log" 2>&1)
(cd "$BENCH/suites/mutated" && python3 mutated/tools/check_eft.py --rpc-cmd "$E serve-stdio" --dataset "$D" > "$OUT/mut-eft.json" 2> "$OUT/mut-eft.log")

bash "$BENCH/tools/run_all_suites.sh" "$E" "$OUT" "$NAME" || note "suite runner reported failures (gate decides)"

# no-regress is THE suite gate. Write its report to a file first so its exit code decides the step
# (a `| tee` pipeline would mask it behind tee's status).
python3 "$BENCH/tools/check_no_regress.py" --baseline "$BENCH/baselines/exfa-engine.json" --run-dir "$OUT" > "$OUT/nrg.txt" 2>&1
rc=$?
cat "$OUT/nrg.txt"
[ $rc -eq 0 ] || fail=1

python3 ci/gate.py "$NAME" "$OUT" | tee -a "$OUT/summary.md"; [ "${PIPESTATUS[0]}" = 0 ] || fail=1
exit $fail
