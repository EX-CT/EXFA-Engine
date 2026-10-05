# EXFA-Engine — EXFA · 精密装配助理 解算引擎

Stateless EVE Online fitting engine (Rust, native + WASM), migrated 2026-10-05 from
`EX-CT/eve-dogma@d70e371` with full history. Provenance line before that: `eve-dogma-lab`
`variant-f-features` (engine variant F + graphs layer); the previous C++ engine (variant J) is in
this repository's history (`dd97e12`) and in eve-dogma-lab tag `j-backup-2026-10-03`.
Architecture: `EX-CT/EXFA-Docs` `docs/00-architecture-plan.md`.
Licence: LGPL-3.0-or-later (`LICENSE`, with the GPL text it extends in `LICENSE.GPL-3.0`). Pyfa is used only as a
black-box oracle and behaviour reference; no Pyfa code.

One JSON `FitRequest` on stdin → one JSON `FitStats` on stdout, stateless and deterministic
(contract: [contract/](contract/README.md)).

The SDE dataset (`dataset-3569502-r5.json.gz`, eve-sde-pipeline format v1, release `sde-3569502-r5`) is **compiled into the binary**: the code
generator turns every effect's modifier list into straight-line Rust code and every type/attribute/group into static
tables. The runtime never loads or parses dataset JSON. See [DESIGN.md](DESIGN.md).

## Workspace

| crate | what | depends on |
|---|---|---|
| `crates/exfa-codegen` | build-time generator: dataset → `tables.rs` (for exfa-sde) + `effects.rs` (for exfa-core) | — |
| `crates/exfa-sde` | static data compiled in: types, attributes, groups, names (en/zh), mutaplasmids, … (no engine code) | (build: codegen) |
| `crates/exfa-model` | structured fit input: `FitRequest` v1 serde types | serde |
| `crates/exfa-capsim` | capacitor simulator (+ Pyfa `round`) | — |
| `crates/exfa-core` | **engine**: core, stats, graphs, JSON-RPC (calc, graph, graph_specs, search, type, meta); structured input only | exfa-sde, exfa-model, exfa-capsim |
| `crates/exfa-formats` | fit formats: EFT (+cfg, mutations), DNA (+alt, link), ESI JSON, XML, multibuy, ship-stats text, item lists, auto-detect; RPC eft_parse, eft_export, format_import, format_export | exfa-sde, exfa-model (**not** exfa-core) |
| `crates/exfa-optimizer` | skill-based fit optimizer (eve-fit-docs docs/21 v1): `optimize`, `Evaluator` trait, RPC `optimize` | exfa-core, exfa-model |
| `crates/exfa-cli` | binary `exfa` (calc, batch, serve-stdio, optimize, graph, graph-batch, eft, search, type, meta, bench): links engine + formats + optimizer | exfa-core, exfa-formats, exfa-optimizer |
| `crates/exfa-wasm` | engine wasm32-unknown-unknown C ABI (`alloc`, `dealloc`, `calc`, `rpc` incl. `optimize`) | exfa-core, exfa-optimizer |
| `crates/exfa-formats-wasm` | formats wasm32-unknown-unknown C ABI (`alloc`, `dealloc`, `rpc`), for the frontend | exfa-formats |

```
exfa-model ──┬──────────────► exfa-formats ──► exfa-formats-wasm
exfa-sde ────┤                        │
             └──► exfa-core ──┬───────┴──► exfa-cli (exfa)
exfa-capsim ──────►┘          └──► exfa-wasm
exfa-codegen (build-time) ──► exfa-sde, exfa-core
```

Fit formats are not part of the engine (architecture ruling 2026-10-03): the engine takes the structured fit and the
skills input and only calculates. What moved out of the engine: [docs/FORMATS-SPLIT.md](docs/FORMATS-SPLIT.md).
CI checks the boundary (`cargo tree`: no exfa-core under exfa-formats, no formats under exfa-core / exfa-wasm).

## Build & run

```bash
export EXFA_DATASET=/abs/path/dataset-3569502-r5.json.gz   # default ../../../data/… relative to crates/exfa-sde, crates/exfa-core
cargo build --release
cargo install --path crates/exfa-cli --locked                       # installs the binary `exfa` (package exfa-cli)
./target/release/exfa calc < request.json > response.json
./target/release/exfa batch < requests.jsonl > responses.jsonl      # one FitRequest per line, parallel, ordered
# EXFA_THREADS=N limits batch worker threads (default: all cores)
./target/release/exfa serve-stdio                                   # JSONL RPC: calc | graph | search | type | meta (engine) + optimize + eft_parse | eft_export | format_export | format_import (formats)
./target/release/exfa optimize request.json                         # OptimizeRequest (eve-fit-docs docs/21) -> ranked fits
./target/release/exfa batch --request batch.json                    # BatchRequest (eve-fit-docs docs/23) -> BatchResponse
./target/release/exfa --prices prices.json calc fit.json            # price table / eve-price-snapshot -> "price" block
./target/release/exfa meta | search QUERY [--limit N --kinds k,..] | type ID|NAME | eft ... | bench FILE -n N
```

`meta.engine` / `provenance.engine` / `version` report `exfa-engine 0.1.0` (renamed from `eve-dogma-f`
at migration; otherwise byte-identical output — see `ci/round1.sha256`).

### WASM

```bash
rustup target add wasm32-wasip1 wasm32-unknown-unknown
cargo build --release --target wasm32-wasip1 -p exfa-cli                                   # CLI as WASI module
wasmtime run target/wasm32-wasip1/release/exfa.wasm calc < request.json
cargo build --profile release-small --target wasm32-unknown-unknown -p exfa-wasm           # engine C-ABI exports
node crates/exfa-wasm/examples/node-calc.mjs target/wasm32-unknown-unknown/release-small/exfa_wasm.wasm < request.json
cargo build --profile release-small --target wasm32-unknown-unknown -p exfa-formats-wasm   # formats C-ABI exports
node crates/exfa-formats-wasm/examples/node-formats.mjs target/wasm32-unknown-unknown/release-small/exfa_formats_wasm.wasm < rpc.jsonl
```

### CI

`.github/workflows/ci.yml`: native + both WASM targets; then, native and wasip1, every suite at its pinned
eve-dogma-bench ref (`ci/run_suites.sh`) against the minimum scores in `ci/gate.json` (bench 1.9.0 331/331, EFT
1.8.0 326/326, cap-suite 150/150, mutated-suite 93/93 + EFT 93/93 / 99/99, formats-suite 4779/4779, graphs 0.2
178/178), plus the round-1 batch output sha256 (`ci/round1.sha256`). Unit tests run first: `exfa-formats`
(`crates/exfa-formats/tests/roundtrip.rs`: per format import→export and export→import round trips and error
paths), `exfa-optimizer` property tests, then the whole workspace. Locally:
`BENCH=/path/to/eve-dogma-bench ci/run_suites.sh native ./target/release/exfa`.

### Bench

`bench.yaml` is the eve-dogma-bench manifest. Bench 1.8.0: **326/326 cases, 21 051/21 051 values, EFT export 326/326**,
0.064 ms/fit, 10 500 fits/s batch, 4 ms cold (see RESULTS.md, `bench/`).

### Graphs (G4, declarative graph spec)

Round-2 graph contract ([contract/CONTRACT-GRAPHS.md](contract/CONTRACT-GRAPHS.md), originally eve-dogma-bench
`graphs-round2`): all 9 Pyfa graph types.

```bash
./target/release/exfa graph < graph_request.json          # one GraphRequest -> GraphResult
./target/release/exfa graph-batch < requests.jsonl        # JSONL, parallel, ordered
./target/release/exfa graph-specs                         # the catalogue (graphs.json)
# RPC (serve-stdio and the WASM `rpc` export): {"method":"graph","params":GraphRequest}, {"method":"graph_specs"}
```

The catalogue `graphs.json` (compiled in) declares per graph its axes + validity limiters, params with defaults
and one formula per (series, axis); formulas are expression trees over engine observables (`ship.<attr>`,
`stat.<path>`, `p.<param>`, `s.<setting>`, `x`) and named kernels (capacitor simulation history, sub-warp speed,
EWAR source tables, remote-rep and damage time lines, application, application profile, ECM burst). Scores:
contract 0.2 (178 cases / 2 437 values) **178/178, 2 437/2 437**; contract 0.1 (111 / 1 843) **111/111,
1 843/1 843** — native and WASM (wasip1) alike, `bench/graphs/README.md`. Behaviour follows the contract and Pyfa's graph outputs as oracle; no Pyfa (GPL) code
is used.

### Optimizer (docs/21 v1)

`exfa optimize FILE` / RPC `{"method":"optimize","params":OptimizeRequest}` (serve-stdio and the exfa-wasm `rpc`
export). Objective = one metric or a weighted sum (`dps`, `volley`, `ehp`, `tank`, `max_velocity`, `align_time`,
`applied_dps`, `price`, or any JSON pointer into FitStats); constraints = character skills (or `ignore`), meta level /
meta groups, cap stable, metric floors/ceilings, price budget; search over high/mid/low/rig racks, charges and one
drone stack, with `keep`, candidate pools `variations` | `group` | `all_fittable` and include/exclude lists.
Deterministic: candidates are pruned by one-module probes (best by constraints and best by objective, two stages over
variation families), a beam of `limits.beam` builds the fit rig → high → low → mid, then local search (single swaps,
emptying a slot, charges, drones, pair swaps) runs until no improvement or `max_evaluations` / `time_ms`. Skills
missing for the part of the fit the search cannot change (hull, implants, kept modules) are reported as a warning and
do not make results infeasible. Errors `OPT_NO_FEASIBLE` (closest fits returned with their violations),
`OPT_BAD_METRIC`, `OPT_MISSING_PRICE`; `clone: "alpha"` → `UNSUPPORTED` (planned for 1.0). Not yet: dominated-candidate
filtering, several drone stacks, subsystems/T3 modes, the bench optimizer suite.

### Import / export formats (Pyfa parity)

Crate `exfa-formats` (served by `exfa serve-stdio` and the `exfa-formats-wasm` `rpc` export, not by the
engine). RPC `format_export {fit, name, format, options}` with `format` = `eft` | `dna` | `esi` | `xml` | `multibuy` |
`shipstats`, and `format_import {text, format, path?}` with `format` = `auto` | `eft` | `eftcfg` | `dna` |
`dna_alt` | `dna_link` | `esi` | `xml` (`auto` follows Pyfa's detection order and also recognises additions lists
and single mutated items). Against the eve-dogma-bench `formats-suite` (Pyfa-generated round trips): all export
variants 326/326 except shipstats 324/326, all four round-trip imports 326/326, edge files 16/16
(`bench/formats/scorecard.md`).

`--dataset PATH` is accepted (ignored) so command lines written for the reference engine keep working.

## License

LGPL-3.0-or-later (per eve-fit-docs `LICENSING.md`; `license = "LGPL-3.0-or-later"` in `Cargo.toml`). The full
LGPL v3 text is in [`LICENSE`](LICENSE); as the LGPL v3 is a set of additional permissions on top of the GPL v3,
the GPL v3 text is included as [`LICENSE.GPL-3.0`](LICENSE.GPL-3.0) (same layout as eve-dogma-rs).

Provenance: engine semantics derived from eve-dogma-rs; behaviour tables that mirror Pyfa (GPL-3.0) handlers are
described in DESIGN.md "Provenance". Import/export formats are written from public format descriptions and
Pyfa used only as a black-box test oracle (no Pyfa code). EVE data is CCP's (not covered by this licence).
EXFA · 精密装配助理 (Exactitude Fitting Assistant) is an EX-CT project, not affiliated with CCP.
