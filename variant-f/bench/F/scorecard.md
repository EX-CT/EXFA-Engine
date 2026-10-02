# Scorecard: F

- command: `./target/eve-dogma-f-bench calc`, batch: `./target/eve-dogma-f-bench batch`
- cases fully correct: **249/249**
- values correct: **13812/13812** (100.00 %)
- engine errors: 0

| group | ok | total | % |
|---|---|---|---|
| application | 1989 | 1989 | 100.0 |
| capacitor | 871 | 871 | 100.0 |
| defense | 4480 | 4480 | 100.0 |
| fitting | 2241 | 2241 | 100.0 |
| navigation | 1243 | 1243 | 100.0 |
| offense | 996 | 996 | 100.0 |
| tank | 996 | 996 | 100.0 |
| targeting | 996 | 996 | 100.0 |

| perf | value |
|---|---|
| one process per case, median ms (cold start + calc) | 1.9 |
| batch throughput (corpus x5) fits/s | 4596 |
| latency one fit (exct_rifter) ms/calc | 0.124 |
| startup + one calc ms | 2.5 |
| deterministic | True |
