# Variant F — build-time code generation (Rust → native + WASM)

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
./target/release/eve-dogma-f serve-stdio                                   # JSONL RPC: calc | search | type | meta | eft_parse | eft_export
./target/release/eve-dogma-f meta | search QUERY [--limit N --kinds k,..] | type ID|NAME | eft ... | bench FILE -n N
```

### WASM

```bash
rustup target add wasm32-wasip1 wasm32-unknown-unknown
cargo build --release --target wasm32-wasip1                 # CLI as WASI module
wasmtime run target/wasm32-wasip1/release/eve-dogma-f.wasm calc < request.json
cargo build --lib --profile release-small --target wasm32-unknown-unknown   # 3.65 MB (0.82 MB gzip), C-ABI exports
node examples/node-calc.mjs target/wasm32-unknown-unknown/release-small/eve_dogma_f.wasm < request.json
```

### Bench

`bench.yaml` is the eve-dogma-bench manifest. Bench 1.8.0: **326/326 cases, 21 051/21 051 values, EFT export 326/326**,
0.064 ms/fit, 10 500 fits/s batch, 4 ms cold (see RESULTS.md, `bench/`).

Branch note: `variant-f` is an orphan branch (the lab branches share no history) and holds only `variant-f/`.

`--dataset PATH` is accepted (ignored) so command lines written for the reference engine keep working.

## License

LGPL-3.0-or-later (per eve-fit-docs `LICENSING.md`; `license = "LGPL-3.0-or-later"` in `Cargo.toml`). The full
LGPL v3 text is in [`LICENSE`](LICENSE); as the LGPL v3 is a set of additional permissions on top of the GPL v3,
the GPL v3 text is included as [`LICENSE.GPL-3.0`](LICENSE.GPL-3.0) (same layout as eve-dogma-rs).

Provenance: engine semantics derived from eve-dogma-rs; behaviour tables that mirror Pyfa (GPL-3.0) handlers are
described in DESIGN.md "Provenance". Import/export formats are written from public format descriptions and
Pyfa used only as a black-box test oracle (no Pyfa code). EVE data is CCP's (not covered by this licence).
