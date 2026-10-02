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
./target/release/eve-dogma-f serve-stdio                                   # JSONL RPC: calc | search | type | meta
./target/release/eve-dogma-f meta | search QUERY | type ID|NAME | bench FILE -n N
```

### WASM

```bash
rustup target add wasm32-wasip1 wasm32-unknown-unknown
cargo build --release --target wasm32-wasip1                 # CLI as WASI module
wasmtime run target/wasm32-wasip1/release/eve-dogma-f.wasm calc < request.json
cargo build --lib --profile release-small --target wasm32-unknown-unknown   # 2.9 MB, C-ABI exports
node examples/node-calc.mjs target/wasm32-unknown-unknown/release-small/eve_dogma_f.wasm < request.json
```

### Bench

`bench.yaml` is the eve-dogma-bench manifest. Bench 1.4.0: **289/289 cases, 18 591/18 591 values** (see RESULTS.md, `bench/`).

Branch note: `variant-f` is an orphan branch (the lab branches share no history) and holds only `variant-f/`.

`--dataset PATH` is accepted (ignored) so command lines written for the reference engine keep working.

License: LGPL-3.0-or-later (engine semantics derived from eve-dogma-rs, see DESIGN.md "Provenance").
