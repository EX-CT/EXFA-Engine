# Fit formats moved out of the engine (2026-10-03)

Architecture ruling (user, via eve, 2026-10-03): fit formats (EFT, DNA, XML, ESI fitting JSON, EFS/HTML export,
Pyfa saved-fit import, …) are **not** part of the engine. The engine takes the structured fit plus the skills
input (`eve-fit-model` `FitRequest`) and only calculates.

## Crates

| crate | role |
|---|---|
| `eve-sde` | static data tables (from `eve-dogma-codegen`, `tables.rs`); no engine code |
| `eve-fit-model` | `FitRequest` v1 serde types (formerly `eve_dogma::request`, still re-exported there) |
| `eve-dogma` | engine; compiled effect code (`effects.rs`) over `eve-sde` tables; **no format code** |
| `eve-fit-formats` | all fit formats; depends on `eve-sde` + `eve-fit-model` only |
| `eve-fit-formats-wasm` | formats C ABI for the frontend (`alloc`, `dealloc`, `rpc`) |
| `eve-cli` (`eve-fit`) | convenience tool linking engine + formats |
| `eve-wasm` | engine C ABI (`alloc`, `dealloc`, `calc`, `rpc`); no formats |

## What moved

| before (eve-dogma) | now |
|---|---|
| `eve_dogma::eft` (`parse`, `export`, `export_opts`, `EftOpts`, `py_float`, `float_unerr`, `mutator_lines`) | `eve_fit_formats::eft` |
| `eve_dogma::formats` (DNA, ESI, XML, multibuy, shipstats, EFT import, EFT cfg, detect, items lists, …) | `eve_fit_formats::formats` |
| `eve_dogma::request` (FitRequest types) | `eve_fit_model` (re-exported as `eve_dogma::request`) |
| `eve_dogma::data` static tables | `eve_sde` (re-exported by `eve_dogma::data` together with the effect code) |
| engine RPC `eft_parse {text}` | `eve_fit_formats::eft_parse` / `rpc_method("eft_parse", ..)` |
| engine RPC `eft_export {fit, name}` | `eve_fit_formats::eft_export` |
| engine RPC `format_import {text, format, path?}` | `eve_fit_formats::format_import` |
| engine RPC `format_export {fit, name, format, options}` | `eve_fit_formats::format_export(params, stats)` |
| `engine::infer_slot` use in formats | `eve_fit_formats::infer_slot` (the engine keeps its own) |
| formats' `Fit::build` calls (subsystem slot/hardpoint counts, fighter squadron size) | `eve_fit_formats::fitting::StaticFit` (static data: base + subsystem modAdd; same "unknown type → no fit" rule) |

`eve_dogma::rpc` now serves `calc`, `graph`, `graph_specs`, `search`, `type`, `meta`. The four format methods answer
`UNKNOWN_METHOD` there. `eve-fit serve-stdio` serves both tables (format methods → `eve-fit-formats`, the rest →
engine), so the RPC seen through the CLI is unchanged, byte for byte.

`format_export` with `format: "shipstats"` needs engine stats. `eve-fit` passes the engine as a callback (stats of
`eve_fit_formats::shipstats_request(fit)`: `include_attributes: "all"`, no spool-up). Without the engine (formats
wasm), pass them as `params.stats` (FitStats JSON of that request); without either the answer is
`{"error": {"code": "NEEDS_STATS"}}`. Caveat: the callback path uses the engine's unrounded values
(`J::to_value_raw`); stats passed as JSON text are the serialized FitStats, so a value on a display rounding
boundary can print differently (formats-suite shipstats inputs: 332/335 texts identical through `params.stats`, 3
differ by one digit in a percentage/last place).

## Interface changes for consumers

* **eve-wasm C ABI (`eve_wasm.wasm`)**: `calc` is unchanged (it never accepted EFT text: FitRequest JSON only). `rpc`
  no longer serves `eft_parse`, `eft_export`, `format_import`, `format_export` (they return
  `{"id", "result": {"error": {"code": "UNKNOWN_METHOD", "message": "<method>"}}}`). Load
  `eve_fit_formats_wasm.wasm` and call its `rpc` export with the same JSONL lines instead; responses are
  byte-identical to the previous engine responses (CI checks a sample against `eve-fit serve-stdio`). `shipstats`
  export: call engine `calc` on the shipstats request first and pass the result as `params.stats`.
* **CLI / serve-stdio / MCP via the CLI**: no change.
* **Rust**: `eve_dogma::eft` / `eve_dogma::formats` paths → `eve_fit_formats::{eft, formats}`.

## Output identity

No engine output key came from the formats, so no engine output changed: round-1 sha256 stays
`214f6192…`; bench 1.9.0, EFT 1.8.0, cap, mutated (+EFT), formats-suite and graphs scores are unchanged, native and
wasm. The bench EFT and formats suites run through `eve-fit serve-stdio` (CLI = formats + engine).
