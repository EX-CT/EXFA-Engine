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
./target/release/eve-dogma-f batch < requests.jsonl > responses.jsonl     # one FitRequest per line
./target/release/eve-dogma-f serve-stdio                                   # JSONL RPC: calc | search | type | meta
./target/release/eve-dogma-f meta | search QUERY | type ID|NAME | bench FILE -n N
```

`--dataset PATH` is accepted (ignored) so command lines written for the reference engine keep working.

License: LGPL-3.0-or-later (engine semantics derived from eve-dogma-rs, see DESIGN.md "Provenance").
