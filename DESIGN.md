# Variant F: design

## Core idea
The SDE is compiled into Rust code. `build.rs` reads `dataset-3569502.json.gz` (eve-sde-pipeline format v1, path from
`$EVE_DOGMA_DATASET`) and writes `$OUT_DIR/gen.rs` (about 5.4 MB of Rust), which is pulled in with `include!`. At runtime
the engine never parses dataset JSON and never interprets modifier records. Every effect is a match arm of straight-line
calls.

```rust
// generated (shape)
pub fn apply_local(f: &mut Fit, ei: u16, i: usize, p: bool) {
    match ei {
        123 => { f.m_ship_loc(i, a::maxVelocity, a::speedFactor, 6, true); f.m_item(i, a::mass, a::massAddition, 2, false); }
        ...
    }
}
```

### Build-time steps
| what | how |
|---|---|
| attributes | dense tables indexed by attribute id: default, flags (stackable / highIsGood / 2-dp rounding), min/max cap attribute, name |
| types | sorted `TYPE_IDS` + `TypeRec` records. Attribute values are stored as u16 indexes into a pool of unique f64 values (4 141 values), and effects are packed `(effect index << 1) \| default` |
| effects → code | 5 118 local modifiers become `apply_local` arms. The domain/func/op and the stacking-penalty decision are resolved at build time (the flag is a constant `false` for stackable attributes and the bastion hull-resonance exception) |
| projected effects | `apply_projected` arms plus effect metadata (range/falloff/resistance attribute, web/TP/damp/sebo class) |
| skills folded | 512 published skills. Each skill's outbound modifiers (878) are emitted with the **source value precomputed for levels 0..5** (`SKILL_VALS[slot][lvl]`). A build-time mini-evaluator runs the skill's own Item-domain modifiers. Skills therefore never become items at runtime. Skills with overrides or unpublished skills fall back to generic items. The generator panics if any effect could modify skill attributes, which keeps the fold sound |
| warfare buffs | `apply_dbuff` is generated from dbuff collections |
| misc | T3D default modes, fighter default abilities, mutaplasmid tables, can-fit/charge-group attribute lists, a name index for search, and the stacking-penalty table `exp(-k²/7.1289)` |

### Runtime
* `Fit` has an item list, a slot arena for modified attributes, and per-item sorted *dynamic* attribute lists. An attribute
  becomes materialised only when a modifier targets it. Every other attribute reads straight through to the static type
  table, with min/max caps and rounding still applied.
* Indexes: by location (ship/char), by group, and by required skill.
* Lazy memoised evaluation uses stack buffers. Stacking penalties come from the constant table.
* Projections: `collect_projection` evaluates the source side (a module, drone, or whole projected fit) into actions.
  These are modifiers carrying range factor and resistance, remote reps (incl. AAR paste ×3 and mutadaptive spool),
  cap drains (neut/nos, signature-scaled), and cap transfer. They are applied to the target fit.
* Fleet boosts: explicit buffs, local bursts, and `booster_fits`. The strongest value per buff id wins.
* Stats/capsim/RAH follow the reference engine (eve-dogma-rs), incl. sustainable tank and ECM jam chance. Missile
  range follows Pyfa's `missileMaxRangeData`. Fighter self abilities (MWD/AB/evasive) and fighter projected abilities
  (web/point/neut/ECM) have no SDE modifier data; they are hand-compiled match arms with constant attribute ids.
* Output: a small JSON tree (`src/j.rs`, objects = vectors with static keys) written directly to a string with sorted
  keys and 6-decimal rounding — byte-identical to the earlier serde_json `Value` output, ≈ 35 % fewer instructions.
* Modifiers live in one arena per fit (singly linked chains per attribute slot) — no per-attribute `Vec`.
* Capacitor simulator: index min-heap (16-byte entries, full lexicographic tie-break only on equal times) and an
  `exp()` memo; results bit-identical to the reference simulator.
* `batch` runs worker threads (requests are independent, the engine is pure) and writes results in input order,
  flushing whenever output has caught up with input. The WASI build stays single-threaded.
* The bench build (`bench.yaml`) links glibc statically (`+crt-static`, ≈ 0.4 ms faster process start), falling back
  to the default build if that fails.

### Pyfa parity layer
The bench oracle is Pyfa. Where Pyfa's hand-written effect handlers (`eos/effects.py`) or fit code differ from the SDE
modifier data, the generator and engine reproduce Pyfa's behaviour:
* **Specials without modifierInfo** become generated calls (`sp_*`): prop modules, MJD/MJFG, slot/hardpoint modifiers,
  lances / doomsdays / reapers / jump portals / clone bays (ship speed boost + warp scramble status), the HIC bubble
  (WDFG), entosis link, cyno, emergency hull energizer, and incursion system effects.
* **Handler types.** `pyfa_cat` remaps effect categories where Pyfa's handler `type` differs. Entosis and superweapons
  are active. 'offline' handlers apply in every state. Effects without a Pyfa class (`online`, `barrage`, ...) never
  make a module activatable. `PYFA_NO_HANDLER` lists modifier-carrying effects that Pyfa ignores.
* **Penalty groups.** Op 8 marks penalised PostMul of 8 hand-written effects that Pyfa stacks in the "default" group
  together with PostPercent boosts. `AF_OVERLOAD` + `eval_before` reproduce Pyfa's module-order dependence for
  overload bonuses. Non-penalised post multipliers are folded into one product (float parity).
* **Python rounding.** `py_round` gives correctly-rounded, ties-to-even `round(x, n)` for the RAH average, cpu/pg,
  floatUnerr, and the capsim wrap.
* **Fleet buffs.** Warfare buffs from bursts, booster fits, and weather/AoE-cloud beacons go into one pool, strongest
  |value| per id. Weather buffs are unpenalised and buffs 79/90/93–99 also reach drones, as in
  `Fit.__runCommandBoosts`.
* **Doomsday DPS** uses Pyfa's subcycles (`doomsdayDamageDuration / doomsdayDamageCycleTime`).
* **Projections.** Projected TD/GD/RTC/Standup WD use Pyfa's per-skill item filters. Burst projectors run at full
  strength. NPC TD drones apply inside maxRange only.

#### Pyfa parity quirks (effects suite 2378/2378)
Observed behaviour of Pyfa's handlers (eos/effects.py / fit code, GPL-3.0; no Pyfa code used), reproduced because Pyfa
is the oracle. Modifier-level quirks live in codegen `pyfa_mod_quirk`, the rest in engine `sp_*` / `apply_buff`.
None of these is a Pyfa-data vs SDE difference. Pyfa's bundled eve.db was compared with the SDE dataset for all 43
types in the 26 former failures: the only differences (remoteCapacitorImpedance / remoteCapacitorImpedanceBonus absent
in eve.db on siege/triage/industrial cores and some hulls; Proteus maxSubSystems 4 vs SDE 5) do not change any scored
value and are listed for eve's pyfa-data-drift decision, not excluded here.

| # | effect(s) | SDE modifierInfo | Pyfa handler (observed) |
|---|---|---|---|
| Q1 | skillBonusDroneInterfacing, mining (drones) | Drone Interfacing: +% miningAmount | +% miningDroneAmountPercent; the drone's `mining` handler (passive, any drone state) multiplies miningAmount by pct/100 unless pct is 0 / absent |
| Q2 | missileDMGBonus, missileDMGBonusPassive | PreMul / ModAdd of the character's missileDamageMultiplier | multiplies the loaded missiles' em/kinetic/explosive/thermal damage (charges requiring Missile Launcher Operation; BCS also Defender Missiles), penalised except from boosters |
| Q3 | moduleBonusSiegeModule, moduleBonusTriageModule, industrialCoreEffect2 | ModAdd gateScrambleStatus | no gateScrambleStatus line |
| Q4 | moduleBonusTriageModule | -100% drone em/kin/exp/therm damage | -100% drone damageMultiplier (penalised), damage attributes unchanged |
| Q5 | shipConsumptionQuantityBonusIndustrialReconfigurationORECapital1 | modules requiring Capital Industrial Reconfiguration | modules requiring Industrial Reconfiguration (so the Capital Industrial Core is not affected) |
| Q6 | shipBonusDroneShieldHitpointsGF2 | ModAdd drone shieldCapacity | percentage boost |
| Q7 | subsystemBonusGallentePropulsionWarpSpeed | +% warpSpeedMultiplier | +% baseWarpSpeed |
| Q8 | structureHiddenArmorHPMultiplier | also scales armor plating armorHpBonus by structurePowerStateArmorPlatingMultiplier | armorHP x hiddenArmorHPMultiplier only |
| Q9 | shipRoleBonusUpwellHaulersMediumMissileFittingBonus | PreMul cpu/power | multiplyItemAttr (PostMul, unpenalised): changes cpu rounding (2.63, not 2.62) |
| Q10 | moduleBonusBreacherPodDamageControl | none | active: +% ship breacherPodDamageResistance by breacherPodActivatedDamageReceivedPercentage |
| Q11 | warfare buff 2474 (Mining Burst: Enhanced Mining Scanner) | +% ship miningScannerUpgrade | forces miningScannerUpgrade to the buff value |
| Q12 | any effect with duplicated modifierInfo rows (e.g. structureConversionRigBasicBonuses) | rows apply twice | each modifier once |

## Trade-offs
* **Rebuild per dataset.** A new SDE needs a recompile (native release ≈ 35 s, wasm32-wasip1 ≈ 24 s, wasm32-unknown-unknown
  release-small ≈ 12 s). `--dataset` is accepted and ignored. `meta` reports the sha256 of the compiled-in dataset.
* **Binary size vs. startup.** The binary is 4.99 MB native (4.45 MB wasip1, 3.76 MB unknown-unknown size-opt; the import/export formats added ≈ 0.1–0.2 MB), but there
  is no JSON load at all. Cold start + one calc is about 2–3 ms native. The reference engine needs hundreds of ms to
  load its dataset.
* The generated source is large (5.4 MB). Fat LTO + codegen-units=1 keep the output compact but make builds slower.
* Chinese names (`names.zh`, ≈ 330 KB) are compiled in for `search`/`type`. EFT import/export (`eve-fit-formats::eft`, RPC `eft_parse`/`eft_export`) is ported from eve-dogma-rs; mutated types resolve through a build-time mutaplasmid mapping table.
* Import/export formats (crate `eve-fit-formats`, outside the engine; RPC `format_export` / `format_import`): DNA (+ chat link, alt), ESI JSON,
  XML (multi-fit), multibuy, EFT option switches and EFT `.cfg`, additions lists / single mutant, ship-stats text.
  Importers reproduce Pyfa's observable behaviour: fit checks (slots, hardpoints, canFitShip*, capital size, rig size,
  subsystem slot, maxGroupFitted; DNA/EFT-cfg one-over leniency), active-state limits, charge validity, item
  publicity as Pyfa's database sees it (Civilian modules public; abyssal/mutated types not), fighter-tube checks.
  `shipstats` formats unrounded engine stats supplied by the caller (`eve-fit` passes `calc(..).to_value_raw()`,
  the formats wasm takes `params.stats`) with a 3-significant-digit k/M/G formatter, at no spool-up. Slot and
  hardpoint counts after subsystems and fighter squadron sizes come from static data (`eve_fit_formats::fitting`),
  not from the engine.

## WASM
* `wasm32-wasip1`: the same CLI binary. Run it with `wasmtime run eve-fit.wasm calc < req.json`. Precompiling with
  `wasmtime compile` gives about 6.6 ms cold.
* `wasm32-unknown-unknown` (`--lib --profile release-small`): engine `eve-wasm` C-ABI exports `alloc`, `dealloc`,
  `calc(ptr,len)->u64`, `rpc(ptr,len)->u64` (engine methods only). See `examples/node-calc.mjs`. Formats are a
  separate module, `eve-fit-formats-wasm` (`alloc`, `dealloc`, `rpc`), see `examples/node-formats.mjs`.

## Repository note
The lab repo's branches share no history, so `variant-f` is an **orphan branch** that contains only `variant-f/`.

## Provenance
LGPL-3.0-or-later. `src/request.rs` and `src/capsim.rs` are copies of eve-dogma-rs (LGPL-3.0-or-later). `src/engine.rs`
and `src/stats.rs` are derived from eve-dogma-rs's engine/stats semantics (RAH, capsim usage, stat formulas), and their file
headers say so. The code generator (`build.rs`), the folded-skill scheme, the projection system and the data layout are new.
Pyfa (GPL-3.0) was read for behaviour only and no Pyfa code was copied. That covers the missile range formula,
remote-rep diminishing, sensor-booster attributes, and everything in "Pyfa parity layer" above. Some small tables are
facts taken from reading Pyfa handlers: effect-name lists (`PYFA_DEFAULT_GROUP_MUL`, `PYFA_NO_HANDLER`, `pyfa_cat`), the
weather buff scopes, the fighter refuel cycle model, and the EFT drone ordering. They are re-expressed as data in
LGPL code. The import/export formats were written from the public format descriptions and from Pyfa's *outputs*
(eve-dogma-bench `formats-suite` cases, generated by running Pyfa's `service/port` as a black box); no code from
Pyfa's GPL `service/port`, `gui/utils/numberFormatter.py` or `eos` was copied or translated. The Pyfa oracle and the fuzz/sweep tools were used as black-box test oracles and are not part of
`variant-f/`.
