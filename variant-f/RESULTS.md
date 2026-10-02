# Variant F: results

Bench: EX-CT/eve-dogma-bench @ b687270 (249 cases, 13 812 values, expected from Pyfa). The raw scorecards are in `bench/`.

| run | cases | values | ms/fit (latency) | fits/s (batch) | cold ms (process+calc) |
|---|---|---|---|---|---|
| F native (`bench.py --only F --quick`, official harness) | 249/249 | 13812/13812 | 0.452 | 2627 | 6.6 |
| F native (`run.py`, corpus ×5) | 249/249 | 13812/13812 | 0.302 | 3168 | 2.3 |
| F wasm32-wasip1 under wasmtime (precompiled .cwasm) | 249/249 | 13812/13812 | 0.280 | 2633 | 6.6 |
| reference A (same box, `results/A`) | 249/249 | 13812/13812 | 1.430 | 460 | — |

All accuracy groups are at 100 %: application, capacitor, defense, fitting, navigation, offense, tank, targeting.
Timings come from a shared, noisy box, so expect ±30 %.

Sizes: native 3.8 MB, wasm32-wasip1 3.4 MB, wasm32-unknown-unknown (release-small) 2.9 MB.
In-process breakdown (`eve-dogma-f bench case.json -n N`, Rifter): build ≈ 110 µs, stats ≈ 130 µs, serialize ≈ 20 µs.
