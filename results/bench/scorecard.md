# Scorecard: J

- command: `./build/eve-dogma-j calc --dataset /workspace/exct-eve/data/dataset-3569502.json.gz`, batch: `./build/eve-dogma-j batch --dataset /workspace/exct-eve/data/dataset-3569502.json.gz`
- cases fully correct: **297/297**
- values correct: **19103/19103** (100.00 %)
- engine errors: 0

| group | ok | total | % |
|---|---|---|---|
| application | 3520 | 3520 | 100.0 |
| capacitor | 1043 | 1043 | 100.0 |
| defense | 5344 | 5344 | 100.0 |
| fitting | 2673 | 2673 | 100.0 |
| navigation | 1771 | 1771 | 100.0 |
| offense | 1188 | 1188 | 100.0 |
| tank | 2079 | 2079 | 100.0 |
| targeting | 1485 | 1485 | 100.0 |

| perf | value |
|---|---|
| one process per case, median ms (cold start + calc) | 2.2 |
| batch throughput (corpus x5) fits/s | 18082 |
| latency one fit (exct_rifter) ms/calc | 0.039 |
| startup + one calc ms | 4.8 |
| deterministic | True |
