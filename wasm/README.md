# Variant J in the browser (WebAssembly)

The same engine sources compile to wasm32 with Emscripten. simdjson and libdeflate are fetched and built from
source for wasm (CMake `FetchContent`, pinned to the versions the native build uses).

```bash
source /path/to/emsdk/emsdk_env.sh            # Emscripten ≥ 3.1 (tested with 6.0.11)
cd variant-j
emcmake cmake -S . -B build-wasm -G Ninja -DCMAKE_BUILD_TYPE=Release
ninja -C build-wasm                            # -> build-wasm/evej.mjs + evej.wasm (~0.8 MB)
EVE_DOGMA_DATASET=/path/dataset.json.gz ctest --test-dir build-wasm   # golden set in Node (116/116)
python3 -m http.server                         # then open http://localhost:8000/wasm/index.html
```

API (ES module `createEvej()`, C functions via `cwrap`; returned strings are valid until the next call):

| function | |
|---|---|
| `evej_open(path)` | load the dataset `.json.gz` previously written with `M.FS.writeFile(path, bytes)`; returns `""` or an error |
| `evej_calc(json)` | FitRequest JSON → FitStats JSON (same bytes as `eve-dogma-j calc`) |
| `evej_rpc(line)` | one `serve-stdio` line (`calc`, `eft_parse`, `eft_export`, `search`, `type`, `meta`) → response line |

Output is byte-identical to the native build: the 326 bench 1.8.0 requests and the golden set give identical
bytes. Measured in Node 20 on the shared box: dataset load ≈ 0.2 s, ≈ 0.6 ms/fit (single thread, no SIMD
kernels; the native build is about 15× faster). `test/wasm_golden.mjs` is the Node test driver (run by CTest above); `wasm/index.html` is a minimal page
(its file-picker flow has not been tested in an automated browser run).
