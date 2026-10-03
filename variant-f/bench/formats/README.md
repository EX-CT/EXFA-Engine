# Formats scorecard (variant-f)

Scorer: eve-dogma-bench `formats-suite` @ af4180f, `tools/evaluate_formats.py --rpc "<bin> serve-stdio"`
(CONTRACT-FORMATS 0.1 DRAFT with eve's rulings: 4793 rows, 4782 scored, 4 groups × 25 %).

| build | score | scored rows | export | import | edge_export | edge |
|---|---|---|---|---|---|---|
| native (`scorecard.md`) | 99.98 % | 4780/4782 | 3258/3260 | 1304/1304 | 125/125 | 93/93 |
| WASM wasm32-wasip1 (`wasm/scorecard.md`) | 99.98 % | 4780/4782 | 3258/3260 | 1304/1304 | 125/125 | 93/93 |

The 2 rows left are the shipstats texts of `esf_structure_bonus_1` and `esf_items_7`, which are stat differences
the main bench excludes. Before this change (bc84e2b) the score was 98.44 % (4774/4782).

Gates with this build, native and WASM: round 1 326/326 (21051/21051), EFT export 326/326. The `batch` output over
all 326 cases is byte-identical to bc84e2b.
