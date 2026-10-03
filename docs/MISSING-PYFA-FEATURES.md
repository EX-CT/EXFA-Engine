# Pyfa features that variant F does not have (engine, CLI, WASM interface)

Compared: Pyfa `1d9f72b71` (2026-10-01, `/workspace/exct-eve/ref/pyfa`, read for behaviour only, GPL: no code taken)
against F at `variant-f-features` (= `af1c04b` + void-bomb cap drain + breacher `pure` + mutated-suite fixes + the
graphs-g4 graph layer). Correctness evidence: eve3's coverage run `eve-dogma-bench` `results/F-coverage/` (d9c309d)
for `bc84e2b` / `af1c04b` / `6e2ebf1`, re-run here with the same suites on `variant-f-features` (section G).

F's surface today:
- **CLI:** `calc`, `batch`, `serve-stdio`, `eft`, `search`, `type`, `meta`, `bench`, `graph`.
- **RPC:** `calc`, `eft_parse`, `eft_export`, `format_export`, `format_import`, `search`, `type`, `meta`, `graph`.
- **WASM:** `src/wasm.rs` exports only `alloc`/`dealloc`/`calc`/`rpc` (C ABI). wasip1 runs the same CLI.

Evidence key:
- "F: rg 0" means `rg -i <term> src/` finds nothing relevant in F.
- "F: error …" is the actual response of `serve-stdio`.

## A. Fit statistics Pyfa shows that F doesn't compute

| # | feature | Pyfa evidence | F evidence | size |
|---|---|---|---|---|
| A1 | ~~**Mining yield**~~ **done** (calc output top-level `mining` {modules_m3_s, drones_m3_s, total_m3_s, modules_drain_m3_s, drones_drain_m3_s}, contract draft 1.10 stats-ext) | `eos/saveddata/fit.py:374 minerYield`, `droneYield` (l.142); `gui/builtinStatsViews/miningyieldViewFull.py` | done: bench v1.10.0 ext/ mining 12/12 (incl. mining+fleet.buffs); module yield = miningAmount/avg cycle (active only), crit adds yield×miningCritChance×miningCritBonusYield, drain = yield×(1+clamp(miningWasteProbability/100)×miningWastedVolumeMultiplier); drone stacks count `quantity` once any drone of the stack is active (Pyfa behaviour) | S |
| A2 | **Outgoing remote repair / cap transfer** (the "Remote reps" panel: shield/armor/hull/cap outgoing per s, with spool) | `eos/saveddata/fit.py:1560 getRemoteReps(spoolOptions)`, `gui/builtinStatsViews/outgoingViewFull.py` | F only models *incoming* RR (`stats.rs:581`). There's no outgoing key in the output. | S |
| A3 | **Bombing panel** (bombs needed to kill a target per bomb type / damage pattern) | `gui/builtinStatsViews/bombingViewFull.py:33` | F: `rg -i bomb src/stats.rs` only matches smartbomb DPS (l.283, 421) | S |
| A4 | **Drone EHP / drone regen columns** | `gui/builtinViewColumns/droneEhp.py`, `droneRegen.py` | F's drone rows have dps/volley/range/speed/sig only (no hp/ehp) | S |
| A5 | **Price** (ship + fit + per-module market price, Jita/ESI/evemarketer sources) | `service/price.py:70 fetchPrices`, `service/marketSources/`, `gui/builtinStatsViews/priceViewFull.py` | F: `{"method":"price"}` → `UNKNOWN_METHOD`. F is offline by design (needs a network source or a price input). | M |
| A6 | **"Affected by" / modifier sources per attribute** (item-stats Affected-by tab, skill affectors menu) | `gui/builtinContextMenus/skillAffectors.py`, `gui/builtinItemStatsViews/` | `options.sources` is parsed (`request.rs:224`) but never read: `rg "\.sources" src/` gives 0 hits. `include_attributes` returns values only. | M |
| A7 | **Heat / overheat damage** (module heat column, burnout estimate) | `gui/builtinViewColumns/heat.py` | F applies overheated *bonuses* (state `overheated`) but has no heat-damage model | M |

## B. Graphs

| # | feature | Pyfa evidence | F evidence | size |
|---|---|---|---|---|
| B1 | ~~All 10 Pyfa graphs~~ **now in mainline-candidate**: merged from graphs-g4 `f73ff1c` into `variant-f-features` (`5cb32f1`); graphs 0.2 178/178, charge ids 120/120, native + wasm. Remaining gap: only the contract-0.2 parameter space is verified (Pyfa's GUI also offers per-graph "extra" toggles / multiple source fits and targets in one plot, which F's `graph` request takes one at a time). | `graphs/data/fit*` (10 dirs), `graphs/gui/` | F-coverage `graphs-0.2__af1c04b`: 0/178 (`UNKNOWN_METHOD graph`, no layer on variant-f) → 178/178 after merge | S (multi-source plots) |

## C. Profiles, presets and libraries (Pyfa keeps them; F takes everything inline)

| # | feature | Pyfa evidence | F evidence | size |
|---|---|---|---|---|
| C1 | **Character / skill profiles** (All 0, All 5, saved characters, skill import/export, ESI character skills) | `service/character.py:217 importCharacter`, `:223 all0`, `:230 all5`; `eos/saveddata/character.py`, `ssocharacter.py` | F: per-request `character.skills {default_level, levels}` only; no named profiles | S (presets) / L (ESI) |
| C2 | **Implant sets** (saved sets, apply to fit; the precalculated faction sets) | `eos/saveddata/implantSet.py:25`, `service/implantSet.py`, `service/precalcImplantSet.py` | F: `implants: [type_id]` only; `rg -i implant_set src/` 0 hits | S |
| C3 | **Built-in damage-pattern presets** (NPC factions, ammo patterns, "ammo → damage pattern") | `eos/saveddata/damagePattern.py:37 BUILTINS`, `gui/builtinContextMenus/ammoToDmgPattern.py` | F: `damage_pattern {em,…}` numbers only | S |
| C4 | **Built-in target-profile presets** (~195 lines of NPC/ship profiles) | `eos/saveddata/targetProfile.py:192 getBuiltinList` | F: `target_profile {…}` numbers only | S |
| C5 | **Fit storage / management** (saved fits DB, fit browser, backup/export all, fit notes, tags) | `service/fit.py`, `eos/saveddata/fit.py` (DB-mapped), `gui/builtinAdditionPanes/notesView.py` | F is stateless by design (request → stats) | L (UI/app layer, not engine) |

## D. Import / export and integrations

| # | feature | Pyfa evidence | F evidence | size |
|---|---|---|---|---|
| D1 | **EFS export** (the Eve Fitting Stats JSON) | `service/port/efs.py` | `format_export {"format":"efs"}` → `{"error":{"code":"UNSUPPORTED_FORMAT","message":"efs"}}` | M |
| D2 | **Mutated-module text** (`muta.py`, the "copy mutated module" export) | `service/port/muta.py`, `gui/builtinContextMenus/moduleMutatedExport.py` | F reads mutations inside EFT/ESI, but has no standalone muta export or import | S |
| D3 | **ESI fittings** (SSO login, fetch / upload / delete in-game fittings) | `gui/esiFittings.py`, `service/esi.py`, `service/esiAccess.py` | none (no network) | L |
| D4 | **Killmail import**: *not a Pyfa feature either* (Pyfa has no zKill/killmail importer in `service/port/`) | `service/port/port.py` import list: EFT, EFT cfg, DNA, DNA alt, ESI, XML, multibuy (export) | n/a; F already covers all Pyfa import paths except EFS (export-only in Pyfa anyway) | – |
| D5 | **Clipboard multi-format auto-detect for files and folders** (`importFitsFromFile(s)`, threaded) | `service/port/port.py:100–193` | F `format_import` auto-detects one text. There's no file or folder import (CLI reads one file or stdin). | S |

## E. Data access / search

| # | feature | Pyfa evidence | F evidence | size |
|---|---|---|---|---|
| E1 | **Search jargon / aliases** ("mwd", "lse", "ab", …) | `service/jargon/` (defaults.yaml, jargon.py) | F `search` is name exact > prefix > substring only | S |
| E2 | **Market browser tree and meta variations** (market groups, "show variations", meta-level switch) | `gui/builtinMarketBrowser/`, `gui/builtinContextMenus/itemVariationChange.py`, `service/market.py` | F: `search` + `type` only; no market-group tree or variation lookup over RPC/WASM | M |
| E3 | **Localisation** (Pyfa's UI + item names in many languages) | `locale/` | F: English + Chinese names in `search` only | S–M |

## F. Interface gaps (CLI / WASM)

| # | gap | evidence | size |
|---|---|---|---|
| F1 | WASM C-ABI exports only `calc` and `rpc`. There's no typed JS binding (wasm-bindgen / TS types) and no streaming or batch call. | `src/wasm.rs` (36 lines, 4 exports) | S |
| F2 | No multi-fit context: compare fits, fleet of fits, "command fit" chosen by *saved fit name*. Booster/projected fits must be inlined as full requests. | `request.rs` `booster_fits: Vec<FitRequest>` | S–M |
| F3 | No incremental API: every change re-computes from scratch. Pyfa keeps a live fit and recalculates. Fine for F's latency (~0.08 ms/fit single-thread), but a UI would want `set_module`/`undo`-style editing. | design | M |

## Already covered by F (not missing, for the avoidance of doubt)

The following are covered and verified by the suites (section G: bench 1.9.0 331/331, cap 150/150, mutated 93/93, formats 100 %, graphs 178/178):
- Capacitor sim: stable %, depletes time, reload/stagger options, injectors, incoming neuts/nos/transfers.
- Tank: raw / effective / sustained, RAH sim, damage pattern EHP.
- Offence: weapons / drones / fighters, spool, reload factor, vs target profile.
- Fleet boosts and booster fits; projected modules / drones / fits.
- Environment effects: wormhole, abyssal, incursion.
- Structures, T3D modes, mutaplasmids, booster side effects, fighter abilities, pilot/system security.
- Validation, including missing skills.
- Formats: EFT, DNA, ESI, XML, multibuy, shipstats, EFT cfg import.

- Graphs (all 10 Pyfa graphs, contract 0.2), since the graphs-g4 merge (B1).
- Breacher pods (`pure` damage, contract 1.4.4), void / lockbreaker bombs as projected cap drain / ECM, Pyfa's
  implant / booster slot rule, effect 2791 override, EFT `[Mutated]` notation export + import (mutated-suite 2ac7c00).

## G. Coverage-suite evidence (eve3 `results/F-coverage/`, d9c309d, plus re-run on variant-f-features)

| suite (pinned) | `bc84e2b` | `af1c04b` (before) | `variant-f-features` `5cb32f1` (after, native + wasm) |
|---|---|---|---|
| bench v1.9.0 (d2edf98), 331 cases | 330/331 | 330/331 | **331/331**, 22046/22046 values |
| cap-suite d80cc38, 150 | 147/150 | 147/150 | **150/150** |
| mutated-suite 2ac7c00, stats 93 | 86/93 | 86/93 | **93/93** |
| mutated-suite EFT export / import | 85/93 / 96/99 | 85/93 / 96/99 | **93/93 / 99/99** |
| formats-suite 7c716e7, 4779 scored | 4773/4779 | 4779/4779 | 4779/4779 |
| graphs-round2 84f7c2e, contract 0.2, 178 | 0/178 | 0/178 | **178/178** (charge ids 120/120) |
| pending-1.10 3193689: e_fz + 200 fuzz (native) | 8/8; 198/200 | 8/8; 198/200 | 8/8; 198/200 |
| round-1 corpus (bench 1.8.0) batch output | – | sha256 214f6192… | byte-identical to af1c04b; EFT 326/326 |

What is still not matched, with evidence:
- **pending-1.10 fuzz `lf10_123_28659`, `lf10_197_28659`** (Paladin `align_time_s`: F 10.29 s, Pyfa oracle 102.93 s): Pyfa's
  eve.db 3532181 has agility 0.858 vs SDE 3569502 0.0858 (docs/15 class (a)), i.e. oracle data drift, not a
  missing feature. Effectively 200/200.
- **Module-state handling** follows Pyfa (F corrects an impossible state and warns); `warp_scramble_status` follows
  Pyfa. Both confirmed by eve, so they're not gaps.

No suite has a crash or timeout at any ref.

Suggested order (smallest first; each S is roughly a day or less with bench cases from the Pyfa oracle):
A1 mining, A2 outgoing RR, C1–C4 presets, A4 drone EHP, A3 bombing, D1 EFS, E1 jargon, then A6 (sources).
