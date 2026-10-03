# Graph scorecards (graphs-g4)

Suite: eve-dogma-bench `graphs-round2` @ b010e97 (111 cases / 1843 values), scorer `graphs/run_graphs.py`.

| build | command | cases | values | informational charge ids |
|---|---|---|---|---|
| native (x86_64 release) | `target/release/eve-dogma-f graph-batch` | 111/111 | 1843/1843 | 103/120 |
| WASM (wasm32-wasip1, wasmtime, precompiled) | `wasmtime run --allow-precompiled eve-dogma-f.cwasm graph-batch` | 111/111 | 1843/1843 | 103/120 |

Charge ids are informational (Pyfa breaks exact DPS ties between equal-stat faction charges by set order).
Round-1 stats corpus with this build: 326/326 cases, 21051/21051 values (`../round1/scorecard.md`).
