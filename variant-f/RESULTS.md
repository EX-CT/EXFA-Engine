# Variant F — results

Bench: EX-CT/eve-dogma-bench **1.4.0** (289 cases, 18 591 values; expected values from the Pyfa oracle). Raw scorecards are
in `bench/` (`bench/F` = official harness `bench.py --only F`, `bench/variant-f-wasm` = same corpus on the WASI build).

| run | cases | values | ms/fit (latency, 500× rifter via batch) | fits/s (batch, corpus ×5) | cold ms (process + calc) |
|---|---|---|---|---|---|
| F native, `bench.py --only F` (static glibc, parallel batch) | **289/289** | **18591/18591** | 0.043–0.058 | 11 000–19 000 | 1.6–2.1 |
| F wasm32-wasip1, wasmtime (precompiled .cwasm, single thread) | **289/289** | **18591/18591** | 0.17–0.36 | 2 000–3 200 | 6.6–10 |
| reference A (eve-dogma-rs, combined.md, same box) | 289/289 | 18591/18591 | 1.787 | 615 | 158 |

All accuracy groups are 100 %: application, capacitor, defense, fitting, navigation, offense, tank, targeting.
The box is shared (load average ≈ 9 on 8 cores during these runs), so ranges show spread across runs.

Single-thread engine cost (`eve-dogma-f bench case.json -n N`, valgrind instruction counts as a stable metric):
* Rifter: ≈ 1.5 M instructions/calc (≈ 90–140 µs) — build ≈ 35–50 µs, stats ≈ 40 µs, JSON ≈ 15 µs.
* heaviest cases are capacitor simulations with many staggered modules (Vexor/neut cases, 20–30 k sim events, ≈ 0.8 ms).

Speed history (Rifter, single thread, instructions/calc): 3.0 M (first folded-skills build) → 2.3 M (modifier arena)
→ 1.55 M (direct JSON writer) ; capsim heap/compare rewrite: Vexor 25.7 M → 16.3 M.

Sizes: native 4.0 MB (dynamic) / 4.8 MB (static glibc bench build), wasm32-wasip1 3.5 MB, wasm32-unknown-unknown
(release-small, C-ABI) 3.0 MB. Build: native ≈ 35–45 s, wasip1 ≈ 30 s, unknown-unknown ≈ 10 s (fresh target dir longer).
