# Variant J — C++20 high-performance EVE dogma engine

A C++20 implementation of the EX-CT FitRequest → FitStats contract (`docs/contract.md` of eve-dogma-rs,
`CONTRACT.md` in eve-dogma-bench). It is an algorithmic port of eve-dogma-rs (Variant A), built for minimal
single-fit latency and high multi-core batch throughput. Its output is byte-identical to the reference engine
on the whole bench corpus (only `meta.engine` differs).

## Build

Requirements: C++20 compiler (g++ ≥ 13 or clang ≥ 17), CMake ≥ 3.20, Ninja, simdjson (≥ 3), libdeflate.
On Debian/Ubuntu: `apt install g++ cmake ninja-build libsimdjson-dev libdeflate-dev`.

```bash
cd variant-j
cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release   # -DEVEJ_NATIVE=ON for -march=native, -DEVEJ_LTO=OFF to disable LTO
ninja -C build
```

## Test

```bash
EVE_DOGMA_DATASET=/path/to/dataset.json.gz ctest --test-dir build --output-on-failure
```

123 CTest tests (`-DEVEJ_TESTS=OFF` to skip building them): 7 unit tests (`test/unit_test.cpp`: number formatting
fast path vs generic path, serde_json number semantics, Rust u32 parsing, capsim compact vs general event layout,
capsim and range-factor basics) and 116 golden regression tests (`test/golden/`): 100 structured random fits
(`tools/randfit_ref.py 41`), 15 malformed/edge requests through `calc`, and one 60-line `serve-stdio` session
(calc, eft_export, eft_parse round trip, search, type, unknown method). When recorded, every stored output was checked
byte-identical to eve-dogma-rs (engine name aside), except the BAD_REQUEST message wording of 4 malformed requests
(see "Known contract differences"; codes and paths match). Wider parity checks against the reference
live in `tools/` (compare_ref.py, fuzz_ref.py, randfit_ref.py).

## Run

```bash
D=/workspace/exct-eve/data/dataset-3569502.json.gz
./build/eve-dogma-j calc  --dataset $D < request.json          # one FitRequest (stdin or FILE) -> FitStats
./build/eve-dogma-j batch --dataset $D < requests.jsonl        # JSONL in -> JSONL out, same order, multithreaded
./build/eve-dogma-j batch --dataset $D --threads 1 < req.jsonl # single-threaded batch
./build/eve-dogma-j serve-stdio --dataset $D                   # JSONL {"cmd":"calc"|"meta"|"type"|"search",...}
./build/eve-dogma-j meta|type ID|search TEXT --dataset $D
./build/eve-dogma-j bench FILE -n 1000 --dataset $D            # in-process latency of one request
./build/eve-dogma-j build-cache --dataset $D                   # pre-build the binary dataset image
```

The first run converts the gzipped JSON dataset to a flat binary image and caches it at
`~/.cache/eve-dogma-j/<name>-<sha>.bin` (override with `--cache PATH` or `$EVE_DOGMA_J_CACHE`; `--no-cache`
disables it). Later runs mmap the image in under 1 ms.

Exit code: 0 on success, 2 if the response is an error object (`calc`), 1 on usage or dataset errors.

## Bench

`bench.yaml` is the eve-dogma-bench manifest. Results are in `results/`:

* `results/bench/`: `bench.py --only J` scorecard (bench version and machine load are in `RUN.txt`)
* `results/compare_ref.txt`: byte/tolerance comparison against the eve-dogma-rs binary (`tools/compare_ref.py`)

Bench 1.8.0 (326 cases), shared 8-core box (J measured 2026-10-03 08:34 CST at load 9.6; A measured 2026-10-03 08:36 CST at load 6.1):

| | J (this) | A (eve-dogma-rs) |
|---|---|---|
| cases / values vs Pyfa | 326/326, 21 051/21 051 | 326/326, 21 051/21 051 |
| latency, one fit (bench ms/calc) | 0.052 ms | 0.143 ms |
| batch throughput | 15 002 fits/s | 4 283 fits/s |
| cold start (one process per case, median) | 2 ms | 10 ms |
| EFT export vs Pyfa (informational) | 326/326 | 326/326 |
| byte-identical output to A (c3822c1) | 326/326 calc cases, all RPC methods | – |

Pinned to one CPU (`taskset -c 3`, the round-1 evaluate.py latency method; batch then uses one worker), the bench's
rifter request costs ≈ 0.088 ms/calc on this loaded box (≈ 0.82 M instructions per calc under callgrind).

Timings on this shared box swing by ±50 % with the load from other agents (the bench takes one run per metric).
Best J run so far: 0.034 ms/fit, 20 067 fits/s, 2 ms cold (dca13b9, bench 1.5.0, load 8.7).

`EVEJ_TIMING=1` prints a phase breakdown (dataset open, ids, read, calc, write) to stderr.

## Known contract differences vs eve-dogma-rs

These are accepted by the coordinator. None of them affect calc outputs.

* `BAD_REQUEST` messages: the error **codes** and paths match the reference, but the message text is J's own
  wording rather than serde's.
* Duplicate type names in `type_by_name`-style lookups (EFT parsing, `type`) resolve to the smallest published
  type id, else the smallest id. Since eve-dogma-rs 7e24406 the reference uses the same rule (all 213 duplicate
  names give identical `type` responses), so this is no longer a difference.
* `meta.engine` is `eve-dogma-j 0.1.0`.
* Structs sent as JSON arrays (serde's sequence form) are accepted since a203c98. Requests are normalised on the
  error path only, so the fast path is unchanged. `tools/arrconv_ref.py` checks this against the reference.

## License

SPDX-License-Identifier: LGPL-3.0-or-later. Full texts: [LICENSE](LICENSE) (LGPL-3.0) and
[LICENSE.GPL-3.0](LICENSE.GPL-3.0) (GPL-3.0, which the LGPL-3.0 incorporates by reference).
The engine is a port of eve-dogma-rs's algorithms, which are LGPL-3.0-or-later. Variant J is therefore
distributed under **LGPL-3.0-or-later** as well (see DESIGN.md, "Provenance"). No Pyfa (GPL) code is included.
Pyfa served only as a black-box test oracle, through the bench. EVE Online data © CCP hf.
