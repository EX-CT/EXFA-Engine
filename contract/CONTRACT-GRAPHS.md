# eve-dogma graphs contract (round 2, revision 0.3)

> **Released 2026-10-03 CST** (branch `graphs-round2`, tag `graphs-v0.3`). Changes against 0.2 are marked **(0.3)**.
> Round 2 was scored on 0.2 (frozen at bench commit `0397d95`); 0.2 scores are not comparable with 0.3 scores.

Extension of the eve-dogma request/response contract (`CONTRACT.md`, revision 1.4.3) with **Pyfa's graph
subsystem** (`graphs/data/*` in Pyfa). Same style and rules as the base contract: stateless, **one JSON
`GraphRequest` in → one JSON `GraphResult` out**, no clocks, no network, byte-identical output for identical input on
the same dataset, unknown request fields ignored. Expected values come from Pyfa itself
(`oracle/pyfa_graph_oracle.py` runs Pyfa's own getters headless).

Status: draft for review. Nothing here is part of 1.x scoring. Field names may still change before revision 1.0.

## Process interface (CLI / RPC)

| Mode | Command | I/O |
|---|---|---|
| single | `eve-dogma graph [FILE]` | GraphRequest JSON on stdin (or FILE) → GraphResult JSON on stdout. Exit 0, or exit 2 with `{"error":…}` on stdout |
| batch | `eve-dogma graph-batch` | one GraphRequest per line (JSONL) → one GraphResult per line, same order; a failing line yields its `{"error":…}` line |
| rpc | `eve-dogma serve-stdio` | new method `graph`: `{"id","method":"graph","params":<GraphRequest>}` → `{"id","result":<GraphResult>}` |

Variants announce support in `bench.yaml` with any of `graph_cmd`, `graph_batch_cmd`, or (via `rpc_cmd`) the `graph`
method. Errors use the base contract's `{"error":{"code","message","path"}}`; new codes: `UNKNOWN_GRAPH`,
`BAD_AXIS` (x axis or y series not valid for the graph). See "Validation and error codes".

## Design principles

1. **Explicit sample points.** The request lists the x values; the engine evaluates the curve exactly there
   (Pyfa `FitGraph.getPoint` semantics, the "x mark" value). Pyfa's adaptive plot sampling (`getRange`:
   `_baseResolution` + `_extraDepth` bisection, capsim event insertion) is a rendering detail and is **not** part
   of the contract. A UI samples as densely as it likes.
2. **SI units everywhere.** Pyfa's display units (km, AU, Mkg, Gkg⋅m/s, %-of-target) are normalised away:
   distances m, times s, speeds m/s, mass kg, capacitor GJ, HP, momentum kg⋅m/s. The only percentages are
   inputs/axes that Pyfa itself defines relative to the fit (`*_pct`) and the EWAR strength outputs, which Pyfa
   defines as percent.
3. **Each point is independent.** Engines must not let one sample influence another (Pyfa's caches are keyed by
   fit id; the oracle uses fresh caches per point).
4. **Distances are surface-to-surface** (as in the overview, and as Pyfa's inputs). Engines add ship radii where the
   math needs centre-to-centre distance.
5. **`null`** is returned where Pyfa returns `None` or a non-finite number, and for x values outside the graph's
   valid range ("limiters", listed per graph). Never `NaN`/`Infinity` in JSON.

## GraphRequest

```jsonc
{
  "schema_version": 1,
  "graph": "damage",                       // see "Graph types"
  "fit": { /* FitRequest (CONTRACT.md) of the source / attacker */ },
  "target": {                              // graphs `damage`, `application_profile` (profile or fit); `ewar`, `remote_reps` (fit only, 0.2); default = ideal target
    "profile": {"em": 0.0, "thermal": 0.0, "kinetic": 0.0, "explosive": 0.0,   // resist fractions 0..1
                "max_velocity": 250, "signature_radius": 125, "radius": 150,   // m/s, m (null sig = infinite), m
                "hp": null}                                                    // total HP (null = infinite); informational
    // or: "fit": { /* FitRequest */ }, "resist_mode": "auto"   // auto | shield | armor | hull | weighted_average
  },
  "x": {"axis": "distance_m", "values": [0, 1000, 5000]},   // axis valid for the graph; values in the axis unit
  "y": ["dps", "volley"],                  // one or more series valid for the graph
  "params": { /* graph-specific inputs, see below; omitted = Pyfa GUI defaults */ },
  "settings": {                            // Pyfa graph settings (GraphSettings) — defaults shown
    "ignore_resists": true,                // damage/app: ignore target resists (Pyfa default true)
    "apply_projected": true,               // damage/app: apply the source's own webs/TPs (+ scram vs MWD) to the target
    "ignore_lock_range": true,             // weapons/ewar/RR still apply beyond the source's lock range
    "ignore_drone_control_range": false,   // drones apply beyond drone control range
    "mobile_drone_mode": "auto"            // auto | follow_attacker | follow_target
  }
}
```

The x value `null` is not allowed in `x.values`. Parameters that Pyfa allows to be "not set" (e.g. damage `distance_m`,
`time_s`) take `null` = not set.

**Empty `x.values` (0.2):** `"values": []` is valid and returns success: `"x": []` and an empty array `[]` for every
requested y series (application profile: also empty `<y>_charge_type_id`). The request is still fully validated
(graph, axis, y, fit, target, settings), so an invalid request with empty x is still an error.

### Validation and error codes (0.2)

Checked before any sample is evaluated; the first failing rule determines the code. `path` is a JSON pointer-like
path (`graph`, `x.axis`, `y[1]`, `x.values[2]`, `target.resist_mode`, …).

| condition | code |
|---|---|
| `graph` missing / not a string; `fit` missing / not an object; `x` or `x.values` missing; `x.values` not an array; an x value `null`, non-numeric or non-finite; `y` missing, not an array, or **empty** | `BAD_REQUEST` |
| `graph` not one of the graph types below | `UNKNOWN_GRAPH` |
| `x.axis` not valid for the graph, a y not valid for the graph, or an (x, y) pair the graph does not define (e.g. `ecm_burst` `tgt_dps` × `tgt_lock_time_s`) | `BAD_AXIS` |
| enumerated value not recognised: `target.resist_mode`, `settings.mobile_drone_mode`, `params.ammo_quality` | `BAD_REQUEST` |
| unknown type id in the source fit, or in `target.fit` for graphs that use a target (`damage`, `application_profile`, `ewar`, `remote_reps`; other graphs ignore `target`) — base contract rules | `UNKNOWN_TYPE` |

Out-of-range x values are **not** errors: they yield `null` at that point (per-graph valid ranges). Numeric params
outside their range are clamped where stated (`time_s` 0 … 2500, `cap_start_pct` / `shield_start_pct` 0 … 100,
ewar `resist` 0 … 1).

## GraphResult

```jsonc
{
  "graph": "damage",
  "x_axis": "distance_m",
  "x": [0, 1000, 5000],                    // echo of the request values
  "series": {"dps": [58.27, 261.25, 239.54], "volley": [201.2, 465.7, 437.5]},   // one array per requested y, same length as x
  "meta": { /* optional, informational: e.g. resolved target_speed_mps, target resist layer */ }
}
```

Application profile additionally returns `"<y>_charge_type_id": [...]` arrays (the charge Pyfa picks at each
point; informational, see that graph).

## Graph types

Notation: `src` = source fit after a normal `calc` (same FitRequest semantics, including `projected`,
`fleet`, `environment` applied to it). "Pyfa:" names the Pyfa getter class that defines the value.

**(0.3) Module state correction (same rule as the stats contract draft 1.4.5, pending-1.10 `49f7555`; coordinator
ruling 2026-10-03 11:19 CST, which reverses the earlier "keeps the requested value" ruling; principle: align with Pyfa).**
For every module of the source fit, of `target.fit`, and of projected / booster fits:
- `rig` and `subsystem` modules are always `online` unless `offline` is requested (no warning).
- A requested `active` or `overheated` state the module cannot use is **corrected to `online`**, exactly like Pyfa
  (`mod.state = st if mod.isValidState(st) else ONLINE`): `active` needs an activatable effect (effect category
  active/target) and `activationBlocked` ≤ 0; `overheated` additionally needs an overload effect. An `overheated`
  module with no overload effect (Bastion Module, doomsdays) therefore becomes `online`, not `active`.
- The graph is computed with the corrected state. Graph results have no `modules[]` echo; where an engine also returns
  the stats response for the same request (`calc`), that response reports the corrected state in `modules[N].state`.
  The correction is silent — no `warnings[]` entry is emitted (base contract 1.4.6; the 1.4.5 warning text is
  superseded). Warnings are not scored in the graph corpus.
- Expected values come from the unchanged Pyfa graph oracle on the request as written (Pyfa applies the same
  correction). Case: `dmg_dist_vargur_bastion_overheated_state` (Vargur, Bastion Module I requested `overheated` →
  corrected to `online`: no Bastion bonuses, so dps is about half of the Bastion-active value).

### `damage` — Pyfa "Damage Stats" (`fitDamageStats`)

| x axis | unit | valid range | notes |
|---|---|---|---|
| `distance_m` | m | ≥ 0 | surface-to-surface distance |
| `time_s` | s | 0 … 2500 (else `null`) | |
| `tgt_speed_mps` | m/s | ≥ 0 | target's current speed (absolute) |
| `tgt_speed_pct` | % | ≥ 0 | 0.2: % of the target's max velocity (Pyfa `('tgtSpeed', '%')` normaliser: x/100 × target maxVelocity — profile `max_velocity` or the target fit's calculated speed, before the source's webs), then as `tgt_speed_mps` |
| `tgt_sig_m` | m | > 0 (else `null`) | target signature radius before the source's TPs |
| `tgt_sig_pct` | % | > 0 (else `null`) | 0.2: % of the target's signature radius (x/100 × profile `signature_radius` or the target fit's signatureRadius), then as `tgt_sig_m`; `null` at every point for an infinite-signature target (profile `signature_radius: null`) |

| y | unit | definition |
|---|---|---|
| `dps` | HP/s | Σ over damage dealers of dps × application (× (1 − resist) unless `ignore_resists`) |
| `volley` | HP | same with volley (best volley of the current cycle for time axis) |
| `damage` | HP | cumulative damage inflicted from t = 0 to `time_s` (needs `params.time_s` unless x = `time_s`; else `null`) |

| param | default | meaning |
|---|---|---|
| `distance_m` | `null` | when x ≠ distance. `null` = "not set": every weapon hits, no range/tracking penalty from distance (Pyfa: distance None → range factor 1, angular speed 0) |
| `time_s` | `null` | when x ≠ time. `null` = stats-panel dps/volley (spool at `globalDefaultSpoolupPercentage` = 100 %); a number = exact state at that time (spool by cycles, **reloads always included**, Pyfa `getCycleParametersForDps(reloadOverride=True)`) |
| `tgt_speed_mps` / `tgt_speed_pct` | 100 % of target max velocity | target speed (absolute wins over pct) |
| `atk_speed_mps` / `atk_speed_pct` | 0 % | attacker speed |
| `atk_angle_deg`, `tgt_angle_deg` | 90, 90 | movement directions; transversal = \|v_a·sin(a_a) − v_t·sin(a_t)\| |

Definitions (Pyfa `graphs/data/fitDamageStats/calc/application.py`):
- Damage dealers: active modules with `isDealingDamage`, active drones, active fighter abilities that deal damage.
  At `time_s` set, dps/volley/damage per dealer come from the time cache (`cache/time.py`): cycle by cycle from
  t = 0 (module `getCycleParametersForDps(reloadOverride=True)`, volley parameters with
  `SpoolOptions(CYCLES, nonstopCycles)`, breacher pods offset 1 s), value at t = last change point ≤ t (with
  `floatUnerr` comparison).
  **(0.3)** Each cycle contributes a dps/volley segment over its *active* time only. Between two segments, during a
  module's `reactivationDelay` (e.g. bombs) or a reload, dps and volley are **0** (Pyfa inserts a zero point at
  the end of the previous segment). `damage` is cumulative and keeps its value during the gap.
- Turrets / drones: chance to hit = range factor (`calculateRangeFactor`, not restricted to 3× falloff) ×
  `0.5^((ω·optimalSigRadius/(tracking·sig))²)`, ω = transversal / (r_atk + distance + r_tgt); damage multiplier
  from chance to hit with 1 % wrecking hits (×3) and the (0.01 + cth)/2 + 0.49 average (`_calcTurretMult`).
  **(0.3)** A drone hits with cth 1 when its `maxVelocity` > 1 *and* (mode `auto` and drone speed ≥ target speed,
  or mode `follow_target`). Otherwise the drone shoots from the attacker's centre: distance = d + r_attacker −
  r_drone, its own radius, speed min(attacker speed, drone speed). Every sentry drone (speed ≤ 1) does this in every
  mode, including `follow_target`.
- Missiles: distance factor from `missileMaxRangeData` (lower range → 1, between lower and higher → higher chance,
  beyond → 0) × `min(1, sig/eR, (eV·sig/(eR·v))^drf)`; FoF ignores lock range. Vorton: range factor (no falloff)
  × missile formula with the module's aoe attributes. Smartbombs: 1 inside range, else 0. Bombs / guided bombs /
  doomsdays as in Pyfa (doomsday: `min(1, sig/signatureRadius)`; titan single-target DDs 0 vs sub-capital target
  fits).
- **(0.3) Breacher pods:** application = (1 if d ≤ lowerRange, `higherChance` if d ≤ higherRange, else 0) from the
  module's `missileMaxRangeData`, × the target fit's `breacherPodDamageResistance` (1 for profiles). Lock range is
  required. The factor multiplies the per-tick value min(absolute, relative · target HP).
- **(0.3) Fighter abilities:**
  - A fighter follows when mode is `auto` and fighter speed ≥ target speed, or mode is `follow_target`. Unlike
    drones there is no minimum speed. A following fighter has range factor 1.
  - Otherwise the range factor is `calculateRangeFactor(<prefix>RangeOptimal or <prefix>Range, <prefix>RangeFalloff,
    d + r_attacker − r_fighter)`. The fighter sits at the attacker's centre.
  - The range factor is multiplied by the missile factor (ability explosion radius/velocity, aggregated drf) and by
    the target fit's `<prefix>ResistanceID` attribute.
  - `fighterAbilityLaunchBomb` needs no lock and has no range check: bomb factor only.
- Application multipliers are rounded with `floatUnerr` before use.
- `apply_projected`: the source's own active webs and TPs (modules with range factor; drones/fighters as mobile
  sources) slow / paint the target, stacking-penalised together with the target fit's own modifiers (Pyfa
  `getTackledSpeed`, `getSigRadiusMult`, `getModifiedItemAttrExtended`); a source scram in range also cancels the
  target fit's MWD/MJD. Target speed keeps its ratio to the target's max speed. Targets with
  `disallowOffensiveModifiers` are not affected.
- Resists (`ignore_resists: false`): profile resists, or for a target fit the layer chosen by `resist_mode`
  (`auto` = Pyfa `_getAutoResists` scoring of EHP, resist factor, active tank and regen; `weighted_average` = HP-weighted).

### `application_profile` — Pyfa "Application Profile" (`fitApplicationProfile`, newer Pyfa)

x: `distance_m` (≥ 0). y: `dps`, `volley` — the best value over every charge the fit's dominant weapon group can
load (Pyfa `getValidChargesForModule`, quality tier `params.ammo_quality`: `t1` | `navy` | `all`, default `all`,
defined below),
with the same application math and target parameters as `damage` (settings `ignore_resists` /
`apply_projected` map to Pyfa's `ammoOptimal*` settings). Returns `<y>_charge_type_id` per point:
**informational only** — Pyfa breaks exact DPS ties between equal-stat faction charges (e.g. Dark Blood vs True
Sansha) by set iteration order, so the id is not well defined. Params: `tgt_speed_*`, `atk_speed_*`, angles as for
`damage`.

**(0.3) Quality tiers** (Pyfa `filterChargesByQuality`; cumulative):
- `t1`: charges with metaGroup 1 (Tech I) or no metaGroup.
- `navy`: `t1` + metaGroup 2 (Tech II) + metaGroup 4 (faction) charges whose name starts with `Imperial Navy `,
  `Republic Fleet `, `Caldari Navy ` or `Federation Navy `. For charges whose name ends in ` XL`, the prefixes are
  `Sansha `, `Arch Angel ` or `Shadow ` instead. So Republic Fleet XL, Blood XL and every lower-tier pirate sub-capital
  charge are **not** `navy`.
- `all`: every valid charge.
- If no candidate charge has a metaGroup, the tier is ignored (all candidates).
- dataset-3569502 carries no metaGroupID per type. Derive it as meta level 5 → 2, a variation parent → 4, else 1;
  for every turret and missile charge this matches Pyfa's eve.db.

**(0.3) Informational, not scored: Pyfa's sampling shortcuts.** Pyfa builds a projected cache: the target's speed and
signature after the source's webs, TPs and scram, at d = 0, s, 2s, … ≤ R, linearly interpolated in between.
- s = `getSampleStep(R)` = max(100, ⌈R / 300 / 100⌉ · 100) m.
- R = the largest, over the dominant weapon group, of int(optimal × longest-range charge multiplier + 3.1 × falloff)
  for turrets, or of the longest missile effective range for launchers.
- Pyfa's ammo-transition scan uses the same step, plus a 10 m bisection.

Near a web/TP/scram range edge, and within a few metres of a charge crossover ("ammo-switch hysteresis"), Pyfa's
value therefore depends on s. The contract does **not** require reproducing this:
- The scored value at a point is the exact application at d: projected effects evaluated at d, best charge at d.
- The corpus only samples points where the two coincide. Checked with `graphs/draft-0.3/tools/edge_check.py`: Pyfa
  re-run with s/4 and s/10 must agree with the default within tolerance.
- Engines may implement either behaviour.

### `ewar` — Pyfa "Electronic Warfare Stats" (`fitEwarStats`)

x: `distance_m` (≥ 0). Param `resist` (clamped to 0..1, default 0; target's resistance to the EWAR type). Optional
`target.fit` (0.2, see "Target fits for ewar / remote_reps"); a target profile is ignored.
Sources: active modules (incl. burst projectors `doomsdayAOE*`, range = maxRange + doomsdayAOERange, no lock
needed), active drones (need lock + drone control range), active fighter abilities (need lock).

| y | unit | definition |
|---|---|---|
| `neut_gj_s` | GJ/s | Σ energyNeutralizerAmount / avg cycle s × (1 − resist) × rf (nosferatu only with `nosOverride`) |
| `web_pct` | % | speed reduction `(1 − m)·100`, m = stacking-penalised product (`calculateMultiplier`) of `1 + speedFactor·(1 − resist)·rf/100` |
| `ecm_strength` | points | Σ max(racial jam strengths) × (1 − resist) × rf |
| `damp_lock_range_pct` | % | lock range reduction `(1 − m)·100` from `maxTargetRangeBonus` |
| `td_optimal_pct` | % | turret optimal reduction `(1 − m)·100` from `maxRangeBonus` |
| `gd_range_pct` | % | missile flight range reduction `(1 − m_vel·m_time)·100` from `missileVelocityBonus` and `explosionDelayBonus` (guidance disruptors) |
| `tp_sig_pct` | % | signature increase `(m − 1)·100` from `signatureRadiusBonus` |

rf = range factor. Range factor = `calculateRangeFactor` (restricted: 0 beyond optimal + 3 × falloff). Drones/fighters: infinite range.

### `remote_reps` — Pyfa "Remote Repairs" (`fitRemoteReps`)

| x | unit | valid |
|---|---|---|
| `distance_m` | m | ≥ 0 |
| `time_s` | s | 0 … 2500 |

y: `rps` (HP/s, shield+armor+hull; energy transfers excluded), `total` (HP repaired from t = 0; needs
`params.time_s` unless x = time). Params: `distance_m`, `time_s` (as for `damage`), `anc_reload` (default true:
ancillary remote reps include their reload; Pyfa checkbox "Reload ancillary RRs"). Range factor
`calculateRangeFactor(optimal, falloff, d)`; drones 1 inside drone control range. Optional `target.fit` (0.2, below).

### Target fits for ewar / remote_reps (0.2, derived — Pyfa's graphs have no target here)

Pyfa's `fitEwarStats` and `fitRemoteReps` graphs take no target, so these values are **derived**, not read from a
Pyfa graph. The rules mirror how Pyfa applies the same modules when one fit is projected onto another
(`eos/effects.py` handlers and `ModifiedAttributeDict.getResistance`). `T` = the target fit after a normal `calc`.
- **ewar:** when `target.fit` is given and `params.resist` is absent, each y uses
  `resist = clamp(1 − T.ship[attr], 0, 1)` (an attribute value of 0 or missing counts as 1, as in Pyfa's `resist or 1`):
  `neut_gj_s` energyWarfareResistance (2045), `web_pct` stasisWebifierResistance (2115), `ecm_strength` ECMResistance
  (2253), `damp_lock_range_pct` sensorDampenerResistance (2112), `td_optimal_pct` and `gd_range_pct`
  weaponDisruptionResistance (2113), `tp_sig_pct` targetPainterResistance (2114). These are the effects'
  `resistanceID` / the modules' `remoteResistanceID`. An explicit `params.resist` overrides the target fit. If
  `T.ship.disallowOffensiveModifiers` is set, every y except `neut_gj_s` is 0 (Pyfa's ewar handlers return early;
  neutralizers do not).
- **remote_reps:** `rps` and `total` are multiplied by `T.ship.remoteRepairImpedance` (2116, the `resistanceID` of the
  remote shield/armor/hull repair effects; e.g. Bastion and structures 1e-5 … 1e-6). They are 0 if
  `T.ship.disallowAssistance` is set (Pyfa's RR handlers return early). Pyfa's own projected-fit stats skip the
  impedance (its RR handlers do not pass the effect to `getResistance`). The contract follows the SDE.
- The oracle builds `T` with Pyfa and reads these attributes (`oracle/pyfa_graph_oracle.py`, `target_ship_attr`).
  Everything else is Pyfa's graph getter.

### `capacitor` — Pyfa "Capacitor" (`fitCapacitor`)

| x | unit | valid |
|---|---|---|
| `time_s` | s | 0 … 3600 |
| `cap_pct` | % of capacitorCapacity | 0 … 100 |

y: `cap_gj` (GJ), `cap_regen_gj_s` (GJ/s, passive recharge at that cap level). Params: `cap_start_pct`
(default 100), `use_capsim` (default true).
- `time_s` + `cap_gj` with `use_capsim`: Pyfa's capacitor simulator (`Fit.getCapSimData(startingCap)`: stagger on,
  reload = `options.factor_reload`, t_max 3600 s, no repeat optimisation); the value at t is the last simulated
  point ≤ t advanced by the regen formula; `null` once the simulation has ended (cap ran out) and t is past its last
  point. Without drains or `use_capsim: false`: regen only,
  `C·(1 + e^(−5t/τ)·(√(C0/C) − 1))²`, τ = rechargeRate/1000.
- `cap_regen_gj_s` = `10·C/τ·(√(c/C) − c/C)` at the cap level c of the point (regen-only curve on the time axis).

### `shield_regen` — Pyfa "Shield Regeneration" (`fitShieldRegen`)

x: `time_s` (≥ 0) or `shield_pct` (0 … 100 of shieldCapacity). y: `shield_hp`, `shield_regen_hp_s`. Params:
`shield_start_pct` (default 0), `effective` (default false: when true both y are EHP using the fit's
`damage_pattern`, Pyfa `effectivify(ship, v, "shield")`). Same formulas as capacitor with shieldCapacity /
shieldRechargeRate.

### `mobility` — Pyfa "Mobility" (`fitMobility`)

x: `time_s` (≥ 0, from standstill, full throttle). y:
`speed_mps` = v·(1 − e^(−t·10⁶/(agility·mass))); `distance_m` (integral of speed); `momentum_kg_mps` = speed·mass;
`bump_speed_mps` = 2·v_s·m_s/(m_s + m_t) (masses in Mkg); `bump_distance_m` = bump speed · m_t(Mkg) · tgt_inertia.
Params `tgt_mass_kg` (default 1.3e9), `tgt_inertia` (default 0.015). v = ship maxVelocity with all active
modules.

### `warp_time` — Pyfa "Warp Time" (`fitWarpTime`)

x: `distance_m` (0 … maxWarpDistance(AU)·149597870700, else `null`). y: `time_s` — Pyfa `calculate_time_in_warp`
(EVE University model): k_accel = warp speed (AU/s), k_decel = min(warp/3, 2), dropout speed = min(subwarp/2,
100), cruise only if distance > 1 AU + decel distance. **Subwarp speed** is the ship's max velocity recomputed with
propulsion / cloak / siege / doomsday / cyno / jump-portal modules set to online and every projected effect on the
fit switched off (Pyfa `SubwarpSpeedCache`). 0 m → 0 s.

### `lock_time` — Pyfa "Lock Time" (`fitLockTime`)

x: `tgt_sig_m` (≥ 1, else `null`). y: `time_s` = `min(40000 / scanResolution / asinh(sig)², 1800)` (if
scanResolution ≤ 0: scanSpeed/1000).

### `ecm_burst` — Pyfa "ECM Burst + Scanres Damps" (`fitEcmBurstScanresDamps`, hidden/experimental; 0.2)

Scenario: the source fit ECM-bursts every 30 s and the enemy re-locks after each burst.

| x | unit | valid | y allowed |
|---|---|---|---|
| `tgt_scan_res_mm` | mm | ≥ 1 (Pyfa limiter; else `null`) | `src_damage`, `tgt_lock_time_s`, `tgt_lock_uptime_s` |
| `tgt_dps` | HP/s | > 0 (else `null`) | `src_damage` only (other y → `BAD_AXIS`) |

| param | default | meaning |
|---|---|---|
| `tgt_scan_res_mm` | 700 | enemy scan resolution when x ≠ scan res (< 1 → `null` points) |
| `tgt_dps` | 200 | enemy dps against the source when x ≠ dps (≤ 0 → `src_damage` `null`) |
| `uptime_adj_s` | 1 | seconds subtracted from each lock uptime (reaction time) |
| `uptime_amount_limit` | 3 | max number of 30 s burst cycles counted (`int()` truncation) |
| `apply_damps` | true | apply the source's scan-resolution damps to the enemy scan res |
| `apply_drones` | true | count the source's drone + fighter dps |

- Damp multiplier `m` (when `apply_damps`): `calculateMultiplier` over one stacking group of `1 + scanResolutionBonus/100`
  for every active module with `remoteSensorDampFalloff`, `structureModuleEffectRemoteSensorDampener` or
  `doomsdayAOEDamp`, and `amountActive` copies for active drones with `remoteSensorDampEntity`. Range is ignored. Pyfa `Fit.getDampMultScanRes`.
- `lock(sr) = min(40000 / (sr·m) / asinh(sig)², 1800)`, with sig = the source ship's signatureRadius.
  `tgt_lock_time_s = lock(x)`, `tgt_lock_uptime_s = max(0, 30 − lock(x))`.
- `src_damage` (HP the source deals before it dies): with `L = lock(scan res)`, `up = max(0, 30 − L − uptime_adj_s)`,
  `down = 30 − up`, `rem = ehp`; repeat `int(uptime_amount_limit)` times: `alive = down + min(up, rem / tgt_dps)`;
  `rem −= up · tgt_dps`; `dmg += alive · weapon_dps + max(0, alive − 3) · drone_dps`; stop once `rem ≤ 0`.
  ehp = Σ layers of the source's EHP under its `damage_pattern` (default uniform), the stats-panel value.
  weapon_dps = the stats-panel module dps (`Fit.getWeaponDps().total`, default spool), drone_dps = drones + fighters
  (`getDroneDps().total`), or 0 without `apply_drones`.

## Scoring (bench, branch `graphs-round2`)

**(0.3)** The 0.3 corpus is `graphs/cases` + `graphs/expected` (192 cases, 2694 values), scored with the unchanged
`graphs/run_graphs.py`. The 0.2 corpus is the same files at bench commit `0397d95`.


`graphs/run_graphs.py` compares every sample value with the corpus tolerance
(|got − want| ≤ max(1e-3, 1e-4·|want|), or both `null`), grouped by graph type; `*_charge_type_id` series are
reported separately and not scored. 0.2: an empty expected series scores one value (the response must have
`"x": []` and that series `[]`). An error case (`graphs/expected/err_*.json`, `"expect_error": CODE`) scores one value
in group `errors`, correct when the response is `{"error":{"code":CODE,…}}`. Corpus: `graphs/cases/*.json` (GraphRequests, fits embedded) and
`graphs/expected/*.json` (Pyfa values).

## Changelog

- 0.3 (released 2026-10-03, tag `graphs-v0.3`): wording fixes from G2's probe findings (graphs/pending.md items 1–4, 6),
  following eve's rulings:
  - sentry drones never follow;
  - breacher range chance × breacherPodDamageResistance on the per-tick value;
  - dps/volley 0 between active cycle segments (reactivation delay, reload);
  - non-following fighters at the attacker's centre;
  - quality-tier definition including XL.
  Pyfa's application-profile grid step and ammo-switch hysteresis are documented as informational and not scored;
  sample points are kept off web/TP edges. Module state correction: an impossible `active` / `overheated` state is
  corrected to `online` (as in Pyfa and stats contract draft 1.4.5). Corpus: 192 cases (172 value + 20 error), 2694 values.
- 0.2 (2026-10-03): empty `x.values` → success with empty `x` and empty series. New "Validation and error codes"
  section (empty `y` → `BAD_REQUEST`; enum values validated). New graph `ecm_burst` (Pyfa's hidden ECM burst graph).
  Damage x axes `tgt_speed_pct` and `tgt_sig_pct`; `tgt_sig_m` ≤ 0 → `null`. `target.fit` for `ewar` (resistance
  attributes, `disallowOffensiveModifiers`) and `remote_reps` (`remoteRepairImpedance`, `disallowAssistance`), derived
  from the effect handlers because Pyfa's graphs have no target there. Corpus: 158 cases + 20 error cases.
- 0.1 (2026-10-03): first draft; 9 graph types (Pyfa's 9 public graphs), explicit sampling, SI units.
