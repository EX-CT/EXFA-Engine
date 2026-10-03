# Graphs, scheme G4 — declarative graph spec

Round 2 of the EXCT engine bake-off: Pyfa's graph subsystem (contract: eve-dogma-bench branch `graphs-round2`,
`graphs/CONTRACT-GRAPHS.md` rev 0.1, extended to rev 0.2 — ecm_burst, damage `tgt_speed_pct`/`tgt_sig_pct`,
target fits for ewar/remote_reps, validation codes, param clamping; plan: eve-fit-docs `docs/10-round-2-graphs-plan.md`). This branch
(`graphs-g4`, directory `graphs-g4/`) is variant F's engine plus a graph layer; `variant-f` itself is untouched.

## Shape

```
graphs.json            catalogue (compiled in via include_str!): constants, graphs, EWAR source tables
src/graphs/expr.rs     expression parser/evaluator (numbers, names, + - * / ^, comparisons, && ||, ?:, calls,
                       builtins exp ln sqrt abs asinh sin cos rad floor min max null isnull coalesce ...)
src/graphs/mod.rs      GraphRequest -> Ctx (source Fit + raw stats + params/settings) -> per x: limiter, defs, series
src/graphs/kernels.rs  kernel registry + capsim history, sub-warp speed, EWAR source tables, stacking, module range
src/graphs/cycles.rs   cycle schedules (active / reactivation delay / reload; fighter refuel plans)
src/graphs/rr.rs       remote repairs: steady rate or time line, range factor per source
src/graphs/dmg.rs      damage dealers, steady / time-line dps-volley-damage, application (turret chance to hit,
                       missiles, vorton, smartbombs, bombs, doomsdays, breachers, drones, fighters), target
                       profiles and target fits (resist modes), source webs / painters / scram vs MWD
src/graphs/app.rs      application profile: valid charges per weapon type, quality tiers, distance-band choice
```

A graph is data: e.g. `shield_regen` is

```json
"defs":   {"S": "ship.shieldCapacity", "tau": "ship.shieldRechargeRate / 1000",
           "s0": "p.shield_start_pct * S / 100", "eff": "p.effective ? shield_ehp_mult() : 1",
           "sh_t": "S * (1 + exp(-5 * x / tau) * (sqrt(s0 / S) - 1)) ^ 2", "sh_p": "x * S / 100"},
"series": {"shield_hp": {"by_axis": {"time_s": "eff * sh_t", "shield_pct": "eff * sh_p"}}, ...}
```

and EWAR strengths are rows in `source_tables` (`from` module/drone/fighter, effect names, strength expression
over the item's attributes, range model, lock / drone-control-range flags). Graphs whose behaviour is a simulation
(capacitor, damage, remote reps, application profile) call kernels from their formulas
(`dmg_dps(p.time_s, x, null(), null())`, `rr_total(x, p.distance_m)`, `app_dps(x)`).

Each sample point is evaluated independently (contract rule 3); x values outside an axis' `valid` expression and
non-finite results give `null`.

Interfaces: CLI `graph`, `graph-batch`, `graph-specs`; RPC methods `graph` / `graph_specs` (serve-stdio and the
WASM `rpc` export). `bench.yaml` gains `graph_batch_cmd`.

## Engine changes vs variant F

- capsim: `simulate_ex(..., history)` records the cap level at every event (capacitor graph);
- stats: crate-visible helpers (raw cycle, shots, charges, average cycle, missile range data);
- data: `type_count` / `type_id_at` (charge enumeration by group).

Round-1 behaviour is unchanged (same stats corpus score).

## Provenance

Behaviour is implemented from the contract text and from Pyfa's documented graph behaviour, verified against the
Pyfa-generated expected values (oracle). Pyfa is GPL-3.0; none of its code is copied here. Data lists that are facts
about the game (e.g. navy ammo name prefixes, charge groups) are restated, not copied code. License of this
variant: LGPL-3.0-or-later (see LICENSE).
