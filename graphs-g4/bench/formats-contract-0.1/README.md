# FORMATS contract 0.1 (DRAFT) scorecards

Suite: eve-dogma-bench `formats-suite` @ 7784ce7 (`formats/CONTRACT-FORMATS.md` rev 0.1 draft, 4792 rows), scorer
`tools/evaluate_formats.py --rpc "<bin> serve-stdio"`.

| dir | binary | rows | export | import | edge_export | edge |
|---|---|---|---|---|---|---|
| `F-native/` | variant-f **bc84e2b** release (x86_64) | 4781/4792 | 3258/3260 | 1304/1304 | 124/125 | 95/103 |
| `F-wasm/` | variant-f **bc84e2b** wasm32-wasip1 (wasmtime, precompiled) | 4781/4792 | 3258/3260 | 1304/1304 | 124/125 | 95/103 |
| `graphs-g4-native/` | graphs-g4 (same formats code) | 4781/4792 | 3258/3260 | 1304/1304 | 124/125 | 95/103 |

These are kept on graphs-g4 so that variant-f stays untouched for the 10:20 CST scoring. The same files can be
copied to `variant-f/bench/formats/` afterwards. The 11 failures (identical on native and WASM):
- shipstats ×2: the known structure-bonus / odd-item cases.
- XML import of a name containing a newline.
- 8 lenient imports where Pyfa fails:
  - lower-case hull name
  - `[Rifter,]`
  - ESI without `description`
  - malformed XML
  - XML without `<description>`
  - `<fittings count="0">`
  - garbage forced as XML
  - one missing rename (`Drone Control Unit I` → `Fighter Support Unit I`)

Error codes: F uses `IMPORT` / `UNSUPPORTED_FORMAT`, where the contract has `IMPORT_ERROR` / `UNRECOGNIZED_INPUT`
(0/42, informational in 0.1).
