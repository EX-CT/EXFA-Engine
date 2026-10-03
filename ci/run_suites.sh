#!/usr/bin/env bash
# Run every regression suite against one engine command and fail on any score below the gate.
# usage: ci/run_suites.sh NAME "ENGINE_CMD"   (ENGINE_CMD = native binary, or "wasmtime run ... eve-fit.cwasm")
# needs: $BENCH = eve-dogma-bench checkout (full history), $EVE_DOGMA_DATASET, python3
set -uo pipefail
NAME=$1; E=$2; D=$EVE_DOGMA_DATASET
T=${SUITES_DIR:-$PWD/_suites}; mkdir -p "$T"; OUT=$PWD/ci-results/$NAME; mkdir -p "$OUT"
stage() { # dir ref
  [ -d "$T/$1" ] || { mkdir -p "$T/$1" && git -C "$BENCH" archive "$2" | tar -x -C "$T/$1"; }
}
# pins (docs: eve-dogma-bench results/F-coverage)
stage b18 3da9671da11379de6ae17d3c3285505e95267f29   # bench 1.8.0 (round 1): byte-identical output check
stage b19 d2edf98      # bench v1.9.0
stage cap d80cc38      # cap-suite
stage mut 2ac7c00      # mutated-suite
stage fmt 7c716e7      # formats-suite (FORMATS 0.1)
stage gr  84f7c2e      # graphs-round2, contract 0.2
fail=0
note() { echo "$1" | tee -a "$OUT/summary.md"; }
# round-1 corpus, batch output sha256 (pure refactors must not change a byte)
for f in "$T"/b18/cases/*.json; do python3 -c "import json,sys;print(json.dumps(json.load(open(sys.argv[1]))))" "$f"; done > "$OUT/round1.jsonl"
sha=$($E batch < "$OUT/round1.jsonl" | sha256sum | cut -d' ' -f1)
want=$(cat ci/round1.sha256)
if [ "$sha" = "$want" ]; then note "- round-1 batch sha256 $sha (unchanged)"; else note "- round-1 batch sha256 $sha != ci/round1.sha256 $want (output changed: update ci/round1.sha256 only for an intended output change)"; fail=1; fi
(cd "$T/b19" && python3 run.py --name "$NAME" --cmd "$E calc" --batch-cmd "$E batch" --batch-repeat 1 --latency-n 3 > "$OUT/b19.log" 2>&1)
(cd "$T/b18" && python3 "$T/b18/tools/check_eft_export.py" --rpc-cmd "$E serve-stdio" > "$OUT/eft18.log" 2>&1)
(cd "$T/cap" && python3 cap/run_cap.py --name "$NAME" --batch-cmd "$E batch" > "$OUT/cap.log" 2>&1)
(cd "$T/mut" && python3 mutated/run_mutated.py --name "$NAME" --cmd "$E calc" --batch-cmd "$E batch" > "$OUT/mut.log" 2>&1;
 python3 mutated/tools/check_eft.py --rpc-cmd "$E serve-stdio" --dataset "$D" > "$OUT/mut-eft.json" 2> "$OUT/mut-eft.log")
(cd "$T/fmt" && python3 tools/evaluate_formats.py --rpc "$E serve-stdio" --name "$NAME" --out "$OUT/fmt" > "$OUT/fmt.log" 2>&1)
(cd "$T/gr" && python3 graphs/run_graphs.py --name "$NAME" --rpc-cmd "$E serve-stdio" --timeout 60 > "$OUT/gr.log" 2>&1)
python3 ci/gate.py "$NAME" "$T" "$OUT" | tee -a "$OUT/summary.md"; [ "${PIPESTATUS[0]}" = 0 ] || fail=1
exit $fail
