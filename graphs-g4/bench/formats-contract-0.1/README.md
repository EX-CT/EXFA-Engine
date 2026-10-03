# FORMATS contract 0.1 (DRAFT, with eve's rulings) scorecards

Suite: eve-dogma-bench `formats-suite` @ 7c716e7 (`formats/CONTRACT-FORMATS.md` rev 0.1 draft + rulings: 4793
rows, 4779 scored, 4 groups × 25 %), scorer `tools/evaluate_formats.py --rpc "<bin> serve-stdio"`.

| dir | binary | score (4×25 %) | scored rows | export | import | edge_export | edge (scored) | report-only agree |
|---|---|---|---|---|---|---|---|---|
| `F-native/`, `F-wasm/` | variant-f **bc84e2b** (native / wasm32-wasip1) | 98.46 % | 4773/4779 | 3257/3257 | 1304/1304 | 124/125 | 88/93 | 9/14 |
| `graphs-g4-native/`, `graphs-g4-wasm/` | graphs-g4 **22dbeb7** (formats parity fixes) | **100 %** | **4779/4779** | 3257/3257 | 1304/1304 | 125/125 | 93/93 | 11/14 |

Native and WASM give identical results.

graphs-g4 passes the gate: 100 % of scored rows. The 3 shipstats rows that depend on main-bench exclusions are
report-only (contract §5.1); `esf_items_4` agrees with Pyfa, the other two don't. The legacy name `Drone Control
Unit I` is report-only too and deliberately not mapped, because the SDE has only `Fighter Support Unit I`.

Error codes on graphs-g4 match the contract's recommended codes 34/34 (informational).

Gates for 22dbeb7, native and WASM:
- graphs 0.2: 178/178
- round 1: 326/326, 21051/21051
- EFT export: 326/326
