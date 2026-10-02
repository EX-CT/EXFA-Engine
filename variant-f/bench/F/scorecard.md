# Scorecard: F

- command: `./target/eve-dogma-f-bench calc`, batch: `./target/eve-dogma-f-bench batch`
- cases fully correct: **289/289**
- values correct: **18591/18591** (100.00 %)
- engine errors: 0

| group | ok | total | % |
|---|---|---|---|
| application | 3428 | 3428 | 100.0 |
| capacitor | 1015 | 1015 | 100.0 |
| defense | 5200 | 5200 | 100.0 |
| fitting | 2601 | 2601 | 100.0 |
| navigation | 1723 | 1723 | 100.0 |
| offense | 1156 | 1156 | 100.0 |
| tank | 2023 | 2023 | 100.0 |
| targeting | 1445 | 1445 | 100.0 |

| perf | value |
|---|---|
| one process per case, median ms (cold start + calc) | 4.8 |
| batch throughput (corpus x5) fits/s | 11311 |
| latency one fit (exct_rifter) ms/calc | 0.056 |
| startup + one calc ms | 3.1 |
| deterministic | True |
