# FORMATS contract 0.1 (DRAFT, with eve's rulings) scorecards

Suite: eve-dogma-bench `formats-suite` @ af4180f (`formats/CONTRACT-FORMATS.md` rev 0.1 draft + rulings: 4793
rows, 4782 scored, 4 groups × 25 %), scorer `tools/evaluate_formats.py --rpc "<bin> serve-stdio"`.

| dir | binary | score (4×25 %) | scored rows | export | import | edge_export | edge (scored) | report-only agree |
|---|---|---|---|---|---|---|---|---|
| `F-native/`, `F-wasm/` | variant-f **bc84e2b** (native / wasm32-wasip1) | 98.44 % | 4774/4782 | 3258/3260 | 1304/1304 | 124/125 | 88/93 | 8/11 |
| `graphs-g4-native/`, `graphs-g4-wasm/` | graphs-g4 **22dbeb7** (formats parity fixes) | 99.98 % | 4780/4782 | 3258/3260 | 1304/1304 | 125/125 | 93/93 | 10/11 |

Native and WASM give identical results.

The 2 failures left on graphs-g4 are the known shipstats cases: `esf_structure_bonus_1` (a structure-bonus
exclusion, excluded in the main bench too) and `esf_items_7` (capacitor). The one report-only disagreement is
the legacy `Drone Control Unit I` name. The SDE has only `Fighter Support Unit I`, and that is deliberately not
mapped.

Error codes on graphs-g4 match the contract's recommended codes 34/34 (informational).

Gates for 22dbeb7, native and WASM:
- graphs 0.2: 178/178
- round 1: 326/326, 21051/21051
- EFT export: 326/326
