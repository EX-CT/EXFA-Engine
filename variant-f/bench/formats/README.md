# Formats scorecard (variant-f)

Scorer: eve-dogma-bench `formats-suite` @ 7c716e7, `tools/evaluate_formats.py --rpc "<bin> serve-stdio"`
(CONTRACT-FORMATS 0.1 DRAFT with eve's rulings: 4793 rows, 4779 scored, 4 groups × 25 %).

| build | score | scored rows | export | import | edge_export | edge |
|---|---|---|---|---|---|---|
| native (`scorecard.md`) | **100 %** | **4779/4779** (gate passed) | 3257/3257 | 1304/1304 | 125/125 | 93/93 |
| WASM wasm32-wasip1 (`wasm/scorecard.md`) | **100 %** | **4779/4779** (gate passed) | 3257/3257 | 1304/1304 | 125/125 | 93/93 |

14 rows are report-only (contract §5.1 and the rulings), and this build agrees with Pyfa on 11 of them. The 3
`shipstats` rows that depend on main-bench exclusions are among them. Before this change (bc84e2b) the score was
98.46 % (4773/4779).

Gates with this build, native and WASM: round 1 326/326 (21051/21051), EFT export 326/326. The `batch` output over
all 326 cases is byte-identical to bc84e2b.
