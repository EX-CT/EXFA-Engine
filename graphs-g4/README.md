# Variant F — build-time code generation (Rust → native + WASM)

> **Branch `graphs-g4`:** variant F plus round-2 graphs, scheme **G4 (declarative graph spec)** — see
> [Graphs (G4)](#graphs-g4-declarative-graph-spec) below and [GRAPHS.md](GRAPHS.md).

EVE Online dogma engine for the EXCT contract (`eve-dogma-rs/docs/contract.md`, v1): one JSON `FitRequest` on
stdin → one JSON `FitStats` on stdout, stateless and deterministic.

The SDE dataset (`dataset-3569502.json.gz`, eve-sde-pipeline format v1) is **compiled into the binary**: `build.rs`
turns every effect's modifier list into straight-line Rust code and every type/attribute/group into static tables.
The runtime never loads or parses dataset JSON. See [DESIGN.md](DESIGN.md).

## Build & run

```bash
# dataset: $EVE_DOGMA_DATASET, default ../../data/dataset-3569502.json.gz (EXCT box layout)
export EVE_DOGMA_DATASET=/workspace/exct-eve/data/dataset-3569502.json.gz
cargo build --release
./target/release/eve-dogma-f calc < request.json > response.json
./target/release/eve-dogma-f batch < requests.jsonl > responses.jsonl     # one FitRequest per line, parallel, ordered
# EVE_DOGMA_THREADS=N limits batch worker threads (default: all cores)
./target/release/eve-dogma-f serve-stdio                                   # JSONL RPC: calc | search | type | meta | eft_parse | eft_export | format_export | format_import
./target/release/eve-dogma-f meta | search QUERY [--limit N --kinds k,..] | type ID|NAME | eft ... | bench FILE -n N
```

### WASM

```bash
rustup target add wasm32-wasip1 wasm32-unknown-unknown
cargo build --release --target wasm32-wasip1                 # CLI as WASI module
wasmtime run target/wasm32-wasip1/release/eve-dogma-f.wasm calc < request.json
cargo build --lib --profile release-small --target wasm32-unknown-unknown   # 3.76 MB (0.86 MB gzip), C-ABI exports
node examples/node-calc.mjs target/wasm32-unknown-unknown/release-small/eve_dogma_f.wasm < request.json
```

### Bench

`bench.yaml` is the eve-dogma-bench manifest. Bench 1.8.0: **326/326 cases, 21 051/21 051 values, EFT export 326/326**,
0.064 ms/fit, 10 500 fits/s batch, 4 ms cold (see RESULTS.md, `bench/`).

### Graphs (G4, declarative graph spec)

Round-2 graph contract (eve-dogma-bench `graphs-round2`, `graphs/CONTRACT-GRAPHS.md`): all 9 Pyfa graph types.

```bash
./target/release/eve-dogma-f graph < graph_request.json        # one GraphRequest -> GraphResult
./target/release/eve-dogma-f graph-batch < requests.jsonl       # JSONL, parallel, ordered
./target/release/eve-dogma-f graph-specs                        # the catalogue (graphs.json)
# RPC (serve-stdio and the WASM `rpc` export): {"method":"graph","params":GraphRequest}, {"method":"graph_specs"}
```

The catalogue `graphs.json` (compiled in) declares per graph its axes + validity limiters, params with defaults
and one formula per (series, axis); formulas are expression trees over engine observables (`ship.<attr>`,
`stat.<path>`, `p.<param>`, `s.<setting>`, `x`) and named kernels (capacitor simulation history, sub-warp speed,
EWAR source tables, remote-rep and damage time lines, application, application profile). Score against the
111-case / 1 843-value suite: **111/111 cases, 1 843/1 843 values, native and WASM (wasip1)** —
`bench/graphs/scorecard.md`. Behaviour follows the contract and Pyfa's graph outputs as oracle; no Pyfa (GPL) code
is used.

### Import / export formats (Pyfa parity)

RPC `format_export {fit, name, format, options}` with `format` = `eft` | `dna` | `esi` | `xml` | `multibuy` |
`shipstats`, and `format_import {text, format, path?}` with `format` = `auto` | `eft` | `eftcfg` | `dna` |
`dna_alt` | `dna_link` | `esi` | `xml` (`auto` follows Pyfa's detection order and also recognises additions lists
and single mutated items). Against the eve-dogma-bench `formats-suite` (Pyfa-generated round trips): all export
variants 326/326 except shipstats 324/326, all four round-trip imports 326/326, edge files 16/16
(`bench/formats/scorecard.md`).

Branch note: `variant-f` is an orphan branch (the lab branches share no history) and holds only `variant-f/`.

`--dataset PATH` is accepted (ignored) so command lines written for the reference engine keep working.

## License

LGPL-3.0-or-later (per eve-fit-docs `LICENSING.md`; `license = "LGPL-3.0-or-later"` in `Cargo.toml`). The full
LGPL v3 text is in [`LICENSE`](LICENSE); as the LGPL v3 is a set of additional permissions on top of the GPL v3,
the GPL v3 text is included as [`LICENSE.GPL-3.0`](LICENSE.GPL-3.0) (same layout as eve-dogma-rs).

Provenance: engine semantics derived from eve-dogma-rs; behaviour tables that mirror Pyfa (GPL-3.0) handlers are
described in DESIGN.md "Provenance". Import/export formats are written from public format descriptions and
Pyfa used only as a black-box test oracle (no Pyfa code). EVE data is CCP's (not covered by this licence).
