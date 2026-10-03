# Graph scorecards (graphs-g4)

Scorer: eve-dogma-bench `graphs-round2` `graphs/run_graphs.py`.

| contract | build | command | cases | values | informational charge ids | dir |
|---|---|---|---|---|---|---|
| 0.2 (fbcfd2d, 178 cases / 2437 values) | native (x86_64 release) | `target/release/eve-dogma-f graph-batch` | 178/178 | 2437/2437 | 103/120 | `native/` |
| 0.2 | WASM (wasm32-wasip1, wasmtime, precompiled) | `wasmtime run --allow-precompiled eve-dogma-f.cwasm graph-batch` | 178/178 | 2437/2437 | 103/120 | `wasm/` |
| 0.1 (b010e97, 111 cases / 1843 values) | native | as above | 111/111 | 1843/1843 | 103/120 | `native-c01/` |
| 0.1 | WASM | as above | 111/111 | 1843/1843 | 103/120 | `wasm-c01/` |

Charge ids are informational (Pyfa breaks exact DPS ties between equal-stat faction charges by set order).
Round-1 stats corpus with this build: 326/326 cases, 21051/21051 values (`../round1/scorecard.md`).
