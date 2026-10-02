# Scorecard: F

- command: `./target/eve-dogma-f-bench calc`, batch: `./target/eve-dogma-f-bench batch`
- cases fully correct: **306/306**
- values correct: **19621/19621** (100.00 %)
- engine errors: 0

| group | ok | total | % |
|---|---|---|---|
| application | 3566 | 3566 | 100.0 |
| capacitor | 1074 | 1074 | 100.0 |
| defense | 5506 | 5506 | 100.0 |
| fitting | 2754 | 2754 | 100.0 |
| navigation | 1825 | 1825 | 100.0 |
| offense | 1224 | 1224 | 100.0 |
| tank | 2142 | 2142 | 100.0 |
| targeting | 1530 | 1530 | 100.0 |

| perf | value |
|---|---|
| one process per case, median ms (cold start + calc) | 2.7 |
| batch throughput (corpus x5) fits/s | 10072 |
| latency one fit (exct_rifter) ms/calc | 0.081 |
| startup + one calc ms | 5.7 |
| deterministic | True |
