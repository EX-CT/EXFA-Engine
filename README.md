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

Bench 1.6.0 (297 cases), shared 8-core box (J measured 2026-10-03 05:47 CST at load 13.0; A measured 2026-10-03 05:42 CST at load 7.9):

| | J (this) | A (eve-dogma-rs) |
|---|---|---|
| cases / values vs Pyfa | 297/297, 19 103/19 103 | 297/297, 19 103/19 103 |
| latency, one fit (bench ms/calc) | 0.039 ms | 0.432 ms |
| batch throughput | 18 082 fits/s | 1 777 fits/s |
| cold start (one process per case, median) | 2 ms | 138 ms |
| EFT export vs Pyfa (informational) | 297/297 | 297/297 |
| byte-identical output to A (0f589b5) | 297/297 calc cases, all RPC methods | – |

`EVEJ_TIMING=1` prints a phase breakdown (dataset open, ids, read, calc, write) to stderr.

## License

The engine is a port of eve-dogma-rs's algorithms, which are LGPL-3.0-or-later. Variant J is therefore
distributed under **LGPL-3.0-or-later** as well (see DESIGN.md, "Provenance"). No Pyfa (GPL) code is included.
Pyfa served only as a black-box test oracle, through the bench. EVE Online data © CCP hf.
