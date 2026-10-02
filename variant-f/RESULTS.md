# Variant F: results

Bench: EX-CT/eve-dogma-bench **1.8.0** (0969967; 326 cases, 21 051 values, contract revision 1.4.3; expected values come
from the Pyfa oracle). Raw scorecards are in `bench/`: `bench/F` is the official harness (`bench.py --only F`), and
`bench/variant-f-wasm` is the same corpus run on the WASI build.

| run | cases | values | EFT export | ms/fit (latency) | fits/s (batch, corpus ×5) | cold ms (process + calc) |
|---|---|---|---|---|---|---|
| F native, `bench.py --only F` (static glibc, parallel batch) | **326/326** | **21051/21051** | **326/326** | 0.064 | 10 507 | 4.1 |
| F wasm32-wasip1, wasmtime (precompiled .cwasm, single thread) | **326/326** | **21051/21051** | – | 0.214 | 3 731 | 7.5 |
| reference A (eve-dogma-rs, combined scorecard, same box) | 326/326 | 21051/21051 | 326/326 | 0.507 | 1 429 | 147 |

All accuracy groups are at 100 %: application, capacitor, defense, fitting, navigation, offense, tank, targeting.
The box is shared (load average ≈ 6–9 on 8 cores), so perf numbers move by ±30 % between runs. Earlier native runs
measured 0.043–0.081 ms/fit and 9 900–19 000 fits/s.

Bench progression (all 100 %): 1.2: 226 → 1.3: 249 → 1.4: 289 → 1.5: 295 → 1.6: 297 → 1.7: 306 → 1.8: 326 cases.

## Beyond the corpus: Pyfa robustness checks (test tools outside the repo)
* **Fuzzer.** Perturbs corpus cases (skills, states, reload, distances, damage patterns, security, dropped or
  duplicated modules, projected amounts, drone counts, random projected effects) and compares against the Pyfa
  oracle. More than 30 seeds × 60 cases now pass. The few accepted divergences are all Pyfa artefacts in fits that are
  illegal in game: duplicate siege modules, duplicate structure rigs (maxGroupFitted), and one 0.05 cpu difference on a
  dropped-module Tengu.
* **Sweep.** Every published module (active, overheated and offline) and every implant is fitted on an empty
  Hyperion: 4 811 fits, all matching Pyfa. Drones (154), charges (1 002, each in a matching launcher or turret), boosters
  (453), fighters (94 on an Aeon), and every projectable module projected onto an Ishtar at 5 km and 40 km (1 155 each)
  also all match. Every ship hull with a generic 10-module fit gives 421/423. The 2 exceptions (Paladin, Golem agility)
  are a data-version difference: Pyfa's eve.db (client build 3532181) has 10× their agility compared with SDE 3569502. The sweep found the lance/superweapon/cyno/jump-portal handlers, capital
  MJFG, sensor-array warp status, and Pyfa's handler-type rules (no-class effects never activate, 'offline' handlers
  apply in every state), breacher pod damage (Pyfa `pure` damage, strongest applies), the EWAR drone
  cycle time (speed before duration), the target signature that projected neutralisers see (before the 'late' MWD
  bloom; nosferatu run late and see it), and the penalised Bowhead agility bonus.

## Engine cost
Single thread, measured with valgrind instruction counts as a stable metric:
* Rifter: ≈ 0.97 M instructions per calc in `batch` (re-measured at 7f39e9f; the `bench` subcommand measures 1.57 M per iteration, unchanged since bench 1.4.1). Single process: ≈ 1.3 ms, vs `/bin/true` at 0.45 ms.
* The heaviest cases are capacitor simulations with many staggered modules (Vexor/neut cases, 20–30 k sim events,
  ≈ 0.8 ms).

Speed history (Rifter, instructions/calc): 3.0 M (first folded-skills build) → 2.3 M (modifier arena) → 1.55 M (direct
JSON writer) → 0.95 M (attribute fast paths). The capsim heap/compare rewrite took Vexor from 25.7 M to 16.3 M.

## Sizes and builds
| artefact | size | gzip -9 |
|---|---|---|
| native release (x86_64, dynamic) | 4.75 MB | – |
| wasm32-wasip1 CLI | 4.22 MB | 0.99 MB |
| wasm32-unknown-unknown, release-small, C-ABI (`calc`/`rpc`) | 3.65 MB | 0.82 MB |
| wasmtime precompiled `.cwasm` | 6.1 MB | – |

About 2.9 MB of the wasm module is the data section: type/attribute tables (≈ 1 MB), names (en+zh ≈ 0.5 MB), effect
metadata, and mutaplasmid tables. Code is 0.75 MB. Build times: native ≈ 35–50 s, wasip1 ≈ 35 s, unknown-unknown
≈ 12–20 s.
