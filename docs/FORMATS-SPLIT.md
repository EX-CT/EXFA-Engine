# Fit formats moved out of the engine (2026-10-03)

Architecture ruling (user, via eve, 2026-10-03): fit formats (EFT, DNA, XML, ESI fitting JSON, EFS/HTML export,
Pyfa saved-fit import, …) are **not** part of the engine. The engine takes the structured fit plus the skills
input (`exfa_core::model` `FitRequest`) and only calculates.

## Crates

| crate | role |
|---|---|
| `exfa_core::sde` | static data tables (from `exfa-codegen`, `tables.rs`); no engine code. Was the `exfa-sde` crate |
| `exfa_core::model` | `FitRequest` v1 serde types (still re-exported as `exfa_core::request`). Was the `exfa-model` crate |
| `exfa-core` | engine; compiled effect code (`effects.rs`) over `sde` tables; **no format code** |
| `exfa-formats` | all fit formats; links `exfa-core` for `sde`/`model` only (engine eval dead-stripped) |
| `exfa-formats-wasm` | formats C ABI for the frontend (`alloc`, `dealloc`, `rpc`) |
| `exfa-cli` (`exfa`) | convenience tool linking engine + formats |
| `exfa-wasm` | engine C ABI (`alloc`, `dealloc`, `calc`, `rpc`); no formats |

## What moved

| before (exfa-core) | now |
|---|---|
| `exfa_core::eft` (`parse`, `export`, `export_opts`, `EftOpts`, `py_float`, `float_unerr`, `mutator_lines`) | `exfa_formats::eft` |
| `exfa_core::formats` (DNA, ESI, XML, multibuy, shipstats, EFT import, EFT cfg, detect, items lists, …) | `exfa_formats::formats` |
| `exfa_core::request` (FitRequest types) | `exfa_core::model` (re-exported as `exfa_core::request`) |
| `exfa_core::data` static tables | `exfa_core::sde` (re-exported by `exfa_core::data` together with the effect code) |
| engine RPC `eft_parse {text}` | `exfa_formats::eft_parse` / `rpc_method("eft_parse", ..)` |
| engine RPC `eft_export {fit, name}` | `exfa_formats::eft_export` |
| engine RPC `format_import {text, format, path?}` | `exfa_formats::format_import` |
| engine RPC `format_export {fit, name, format, options}` | `exfa_formats::format_export(params, stats)` |
| `engine::infer_slot` use in formats | `exfa_formats::infer_slot` (the engine keeps its own) |
| formats' `Fit::build` calls (subsystem slot/hardpoint counts, fighter squadron size) | `exfa_formats::fitting::StaticFit` (static data: base + subsystem modAdd; same "unknown type → no fit" rule) |

`exfa_core::rpc` now serves `calc`, `graph`, `graph_specs`, `search`, `type`, `meta`. The four format methods answer
`UNKNOWN_METHOD` there. `exfa serve-stdio` serves both tables (format methods → `exfa-formats`, the rest →
engine), so the RPC seen through the CLI is unchanged, byte for byte.

`format_export` with `format: "shipstats"` needs engine stats. `exfa` passes the engine as a callback (stats of
`exfa_formats::shipstats_request(fit)`: `include_attributes: "all"`, no spool-up, `full_precision: true`). Without
the engine (formats wasm), run engine `calc` on that request and pass its output text as `params.stats_json`: the
formats layer reads it with correctly rounded floats, so the text is byte-identical to the linked-engine result (CI
checks 20 cases incl. the 3 rounding-boundary ones). `params.stats` (an already parsed object) is also accepted, but
serde_json's default float parsing can be one ulp off and flip a rounded digit (3/335 formats-suite cases). Without
any of these the answer is `{"error": {"code": "NEEDS_STATS"}}`.

New request option `options.full_precision` (engine, default false): floats are written unrounded (shortest
round-trip form) instead of rounded to 6 decimals. Default output is unchanged (not even serialized when false).

## Interface changes for consumers

* **exfa-wasm C ABI (`exfa_wasm.wasm`)**: `calc` is unchanged (it never accepted EFT text: FitRequest JSON only). `rpc`
  no longer serves `eft_parse`, `eft_export`, `format_import`, `format_export` (they return
  `{"id", "result": {"error": {"code": "UNKNOWN_METHOD", "message": "<method>"}}}`). Load
  `exfa_formats_wasm.wasm` and call its `rpc` export with the same JSONL lines instead; responses are
  byte-identical to the previous engine responses (CI checks a sample against `exfa serve-stdio`). `shipstats`
  export: call engine `calc` on the shipstats request (`include_attributes: "all"`, `default_spool` spool_scale 0,
  no per-module `spool`, `full_precision: true`) and pass the output text as `params.stats_json`.
* **CLI / serve-stdio / MCP via the CLI**: no change.
* **Rust**: `exfa_core::eft` / `exfa_core::formats` paths → `exfa_formats::{eft, formats}`.

## Output identity

No engine output key came from the formats, so no engine output changed: round-1 sha256 stays
`214f6192…`; bench 1.9.0, EFT 1.8.0, cap, mutated (+EFT), formats-suite and graphs scores are unchanged, native and
wasm. The bench EFT and formats suites run through `exfa serve-stdio` (CLI = formats + engine).
