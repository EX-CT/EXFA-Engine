# Scorecard: G4r1

- command: `/workspace/exct-eve/lab-g4/graphs-g4/target/release/eve-dogma-f calc`, batch: `/workspace/exct-eve/lab-g4/graphs-g4/target/release/eve-dogma-f batch`
- cases fully correct: **326/326**
- values correct: **21051/21051** (100.00 %)
- engine errors: 0

| group | ok | total | % |
|---|---|---|---|
| application | 3943 | 3943 | 100.0 |
| capacitor | 1147 | 1147 | 100.0 |
| defense | 5866 | 5866 | 100.0 |
| fitting | 2934 | 2934 | 100.0 |
| navigation | 1945 | 1945 | 100.0 |
| offense | 1304 | 1304 | 100.0 |
| tank | 2282 | 2282 | 100.0 |
| targeting | 1630 | 1630 | 100.0 |

| perf | value |
|---|---|
| one process per case, median ms (cold start + calc) | 2.2 |
| batch throughput (corpus x5) fits/s | 12233 |
| latency one fit (exct_rifter) ms/calc | 0.981 |
| startup + one calc ms | 3.6 |
| deterministic | True |
