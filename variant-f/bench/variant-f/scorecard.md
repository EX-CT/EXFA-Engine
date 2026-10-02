# Scorecard: variant-f

- command: `/workspace/exct-eve/lab-f/variant-f/target/release/eve-dogma-f calc`, batch: `/workspace/exct-eve/lab-f/variant-f/target/release/eve-dogma-f batch`
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
| one process per case, median ms (cold start + calc) | 2.3 |
| batch throughput (corpus x5) fits/s | 3168 |
| latency one fit (exct_rifter) ms/calc | 0.302 |
| startup + one calc ms | 2.8 |
| deterministic | True |
