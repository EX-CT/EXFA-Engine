//! Fit statistics on top of the evaluated dogma graph (Pyfa-equivalent formulas).
//! Formulas follow the reference engine eve-dogma-rs (LGPL-3.0-or-later), from which this file is derived;
//! all attribute/effect/type references are compile-time constants from the generated tables.
use crate::capsim::{self, Drain};
use crate::data::{self as d, a, e};
use crate::engine::{Fit, Kind};
use crate::request::{FitRequest, Resists, Slot, Spool, SpoolType, State};
use serde_json::{json, Map, Value};

pub fn range_factor(optimal: f64, falloff: f64, distance: Option<f64>, restricted: bool) -> f64 {
    let Some(dist) = distance else { return 1.0 };
    if falloff > 0.0 {
        if restricted && dist > optimal + 3.0 * falloff {
            return 0.0;
        }
        0.5f64.powf(((dist - optimal).max(0.0) / falloff).powi(2))
    } else if dist <= optimal {
        1.0
    } else {
        0.0
    }
}

pub fn lock_time(scan_res: f64, sig: f64) -> Option<f64> {
    if scan_res <= 0.0 || sig <= 0.0 {
        return None;
    }
    Some((40000.0 / scan_res / sig.asinh().powi(2)).min(1800.0))
}

fn float_unerr(v: f64) -> f64 {
    (v * 1e9).round() / 1e9
}

/// Pyfa spool-up semantics -> (value, cycles, time)
pub fn spoolup(max: f64, step: f64, cycle_s: f64, spool: Spool) -> (f64, f64, f64) {
    if max == 0.0 || step == 0.0 {
        return (0.0, 0.0, 0.0);
    }
    let cycles = match spool.kind {
        SpoolType::SpoolScale => float_unerr(max * spool.amount / step).ceil(),
        SpoolType::CycleScale => (spool.amount * float_unerr(max / step).ceil()).round(),
        SpoolType::Time => float_unerr(spool.amount / cycle_s).floor().min(float_unerr(max / step).ceil()),
        SpoolType::Cycles => spool.amount.floor().min(float_unerr(max / step).ceil()),
    };
    let v = (cycles * step).min(max);
    (v, cycles, cycles * cycle_s)
}

#[derive(Default, Clone, Copy)]
struct Dmg {
    em: f64,
    th: f64,
    ki: f64,
    ex: f64,
}
impl Dmg {
    fn total(&self) -> f64 {
        self.em + self.th + self.ki + self.ex
    }
    fn scale(&self, k: f64) -> Dmg {
        Dmg { em: self.em * k, th: self.th * k, ki: self.ki * k, ex: self.ex * k }
    }
    fn add(&mut self, o: &Dmg) {
        self.em += o.em;
        self.th += o.th;
        self.ki += o.ki;
        self.ex += o.ex;
    }
    fn vs(&self, r: &Resists) -> f64 {
        self.em * (1.0 - r.em) + self.th * (1.0 - r.thermal) + self.ki * (1.0 - r.kinetic) + self.ex * (1.0 - r.explosive)
    }
    fn json(&self) -> Value {
        json!({"em": self.em, "thermal": self.th, "kinetic": self.ki, "explosive": self.ex, "total": self.total()})
    }
}

const DMG: [u16; 4] = [a::emDamage, a::thermalDamage, a::kineticDamage, a::explosiveDamage];

fn round6(v: f64) -> f64 {
    if v.is_finite() { (v * 1e6).round() / 1e6 } else { v }
}

/// Recursively round floats for stable, readable output.
fn tidy(v: Value) -> Value {
    match v {
        Value::Number(n) => {
            if n.is_f64() {
                if let Some(f) = n.as_f64() {
                    return json!(round6(f));
                }
            }
            Value::Number(n)
        }
        Value::Array(x) => Value::Array(x.into_iter().map(tidy).collect()),
        Value::Object(o) => Value::Object(o.into_iter().map(|(k, v)| (k, tidy(v))).collect()),
        x => x,
    }
}

impl Fit {
    fn has_eff(&self, i: usize, ids: &[u32]) -> bool {
        self.items[i].effects().any(|(ei, _)| ids.contains(&d::EFF_IDS[ei]))
    }

    fn raw_cycle_ms(&self, i: usize) -> f64 {
        let mut v: f64 = self.get(i, a::speed).max(self.get(i, a::duration));
        for x in [
            a::durationHighisGood,
            a::durationSensorDampeningBurstProjector,
            a::durationTargetIlluminationBurstProjector,
            a::durationECMJammerBurstProjector,
            a::durationWeaponDisruptionBurstProjector,
        ] {
            v = v.max(self.get(i, x));
        }
        v
    }

    fn num_charges(&self, i: usize) -> u32 {
        let Some(c) = self.items[i].charge else { return 0 };
        let vol = self.get(c, a::volume);
        let cap = self.base(i, a::capacity);
        if vol <= 0.0 { 0 } else { float_unerr(cap / vol).floor() as u32 }
    }

    fn num_shots(&self, i: usize) -> u32 {
        let Some(c) = self.items[i].charge else { return 0 };
        let n = self.num_charges(i);
        if n > 0 && self.has(i, a::chargeRate) {
            let r = self.get(i, a::chargeRate);
            return if r > 0.0 { (n as f64 / r).floor() as u32 } else { 0 };
        }
        if n > 0 && self.has(c, a::crystalsGetDamaged) {
            if self.get(c, a::crystalsGetDamaged) == 1.0 {
                let hp = self.get(c, a::hp);
                let chance = self.get(c, a::crystalVolatilityChance);
                let dmg = self.get(c, a::crystalVolatilityDamage);
                if dmg * chance > 0.0 {
                    return ((n as f64 * hp) / (dmg * chance)).floor() as u32;
                }
            }
            return 0;
        }
        0
    }

    /// Average cycle time in ms (Pyfa getCycleParameters(...).averageTime)
    fn avg_cycle_ms(&self, i: usize, factor_reload: bool) -> f64 {
        let active = self.raw_cycle_ms(i);
        if active == 0.0 {
            return 0.0;
        }
        let inactive = self.get(i, a::moduleReactivationDelay);
        let shots = self.num_shots(i);
        let reload = self.get(i, a::reloadTime);
        if !factor_reload || shots == 0 || inactive >= reload {
            return active + inactive;
        }
        let early = shots as f64 - 1.0;
        ((active + inactive) * early + (active + reload)) / shots as f64
    }

    fn module_volley(&self, i: usize) -> (Dmg, &'static str) {
        let it = &self.items[i];
        let kind = if self.has_eff(i, &[e::turretFitted]) {
            "turret"
        } else if self.has_eff(i, &[e::launcherFitted]) {
            "missile"
        } else if self.has_eff(i, &[e::empWave]) {
            "smartbomb"
        } else if self.has_eff(i, &[e::ChainLightning]) {
            "vorton"
        } else {
            "other"
        };
        let src = it.charge.unwrap_or(i);
        let mut mult = if self.has(i, a::damageMultiplier) { self.get(i, a::damageMultiplier) } else { 1.0 };
        if kind == "missile" && it.charge.is_some() {
            mult *= self.get(self.char, a::missileDamageMultiplier);
        }
        let dm = Dmg {
            em: self.get(src, DMG[0]) * mult,
            th: self.get(src, DMG[1]) * mult,
            ki: self.get(src, DMG[2]) * mult,
            ex: self.get(src, DMG[3]) * mult,
        };
        (dm, kind)
    }

    pub fn compute_stats(&self, req: &FitRequest) -> Value {
        let ship = self.ship;
        let ch = self.char;
        let g = |i: usize, x: u16| self.get(i, x);
        let factor_reload = req.options.factor_reload;
        let modules: Vec<usize> = (0..self.items.len()).filter(|&i| self.items[i].kind == Kind::Module).collect();
        let online = |i: usize| self.items[i].state >= State::Online;
        let active = |i: usize| self.items[i].state >= State::Active;

        // ---------------- resources
        let sum = |attr: u16, f: &dyn Fn(usize) -> bool| -> f64 { modules.iter().filter(|&&i| f(i)).map(|&i| self.get(i, attr)).sum() };
        let cpu_used = sum(a::cpu, &online);
        let pg_used = sum(a::power, &online);
        let calib_used: f64 = modules.iter().filter(|&&i| self.items[i].slot == Some(Slot::Rig)).map(|&i| self.get(i, a::upgradeCost)).sum();
        let drones: Vec<usize> = (0..self.items.len()).filter(|&i| self.items[i].kind == Kind::Drone).collect();
        let fighters: Vec<usize> = (0..self.items.len()).filter(|&i| self.items[i].kind == Kind::Fighter).collect();
        let bw_used: f64 = drones.iter().map(|&i| g(i, a::droneBandwidthUsed) * self.items[i].active_count as f64).sum();
        let bay_used: f64 = drones.iter().map(|&i| self.get(i, a::volume) * self.items[i].quantity as f64).sum();
        let fbay_used: f64 = fighters.iter().map(|&i| self.get(i, a::volume) * self.items[i].quantity as f64).sum();
        let cargo_used: f64 = req.cargo.iter().map(|c| d::type_index(c.type_id).map(d::type_volume).unwrap_or(0.0) * c.quantity as f64).sum();
        let count_slot = |s: Slot| modules.iter().filter(|&&i| self.items[i].slot == Some(s)).count();
        let turrets_used = modules.iter().filter(|&&i| self.has_eff(i, &[e::turretFitted])).count();
        let launchers_used = modules.iter().filter(|&&i| self.has_eff(i, &[e::launcherFitted])).count();
        let usage = |u: f64, t: f64| json!({"used": u, "total": t});
        let fighter_class = |i: usize| -> &'static str {
            if g(i, a::fighterSquadronIsHeavy) > 0.0 {
                "heavy"
            } else if g(i, a::fighterSquadronIsSupport) > 0.0 {
                "support"
            } else {
                "light"
            }
        };
        let tubes_used = fighters.iter().filter(|&&i| self.items[i].active_count > 0).count();
        let class_used = |c: &str| fighters.iter().filter(|&&i| self.items[i].active_count > 0 && fighter_class(i) == c).count() as f64;
        let resources = json!({
            "cpu": usage(cpu_used, g(ship, a::cpuOutput)),
            "power": usage(pg_used, g(ship, a::powerOutput)),
            "calibration": usage(calib_used, g(ship, a::upgradeCapacity)),
            "drone_bandwidth": usage(bw_used, g(ship, a::droneBandwidth)),
            "drone_bay": usage(bay_used, g(ship, a::droneCapacity)),
            "fighter_bay": usage(fbay_used, g(ship, a::fighterCapacity)),
            "cargo": usage(cargo_used, g(ship, a::capacity)),
            "slots": {
                "high": usage(count_slot(Slot::High) as f64, g(ship, a::hiSlots)),
                "mid": usage(count_slot(Slot::Mid) as f64, g(ship, a::medSlots)),
                "low": usage(count_slot(Slot::Low) as f64, g(ship, a::lowSlots)),
                "rig": usage(count_slot(Slot::Rig) as f64, g(ship, a::rigSlots)),
                "subsystem": usage(count_slot(Slot::Subsystem) as f64, g(ship, a::maxSubSystems)),
                "service": usage(count_slot(Slot::Service) as f64, g(ship, a::serviceSlots)),
            },
            "hardpoints": {
                "turret": usage(turrets_used as f64, g(ship, a::turretSlotsLeft)),
                "launcher": usage(launchers_used as f64, g(ship, a::launcherSlotsLeft)),
            },
            "fighter_tubes": {
                "total": usage(tubes_used as f64, g(ship, a::fighterTubes)),
                "light": usage(class_used("light"), g(ship, a::fighterLightSlots)),
                "support": usage(class_used("support"), g(ship, a::fighterSupportSlots)),
                "heavy": usage(class_used("heavy"), g(ship, a::fighterHeavySlots)),
            },
        });

        // ---------------- offense
        let tp = req.target_profile.clone().unwrap_or_default();
        let tp_res = Resists { em: tp.em, thermal: tp.thermal, kinetic: tp.kinetic, explosive: tp.explosive };
        let default_spool = req.options.default_spool.unwrap_or(Spool { kind: SpoolType::SpoolScale, amount: 1.0 });
        let mut weapons = Vec::new();
        let mut w_vol = Dmg::default();
        let mut w_dps = Dmg::default();
        for &i in &modules {
            if !active(i) {
                continue;
            }
            let (base, kind) = self.module_volley(i);
            if base.total() == 0.0 {
                continue;
            }
            let cyc = self.avg_cycle_ms(i, factor_reload);
            let raw = self.raw_cycle_ms(i);
            let spool = self.items[i].spool.unwrap_or(default_spool);
            let (sp, _, _) = spoolup(g(i, a::damageMultiplierBonusMax), g(i, a::damageMultiplierBonusPerCycle), raw / 1000.0, spool);
            let vol_spooled = base.scale(1.0 + sp);
            let dps = if cyc > 0.0 { vol_spooled.scale(1000.0 / cyc) } else { Dmg::default() };
            w_vol.add(&vol_spooled);
            w_dps.add(&dps);
            let mut w = json!({
                "module_index": self.items[i].req_index, "type_id": self.items[i].type_id,
                "name": d::type_name(self.items[i].ty), "kind": kind,
                "charge_type_id": self.items[i].charge.map(|c| self.items[c].type_id),
                "volley": vol_spooled.json(), "dps": dps.json(), "cycle_time_ms": cyc,
            });
            if kind == "turret" {
                w["optimal_m"] = json!(g(i, a::maxRange));
                w["falloff_m"] = json!(g(i, a::falloff));
                w["tracking"] = json!(g(i, a::trackingSpeed));
            } else if kind == "missile" {
                if let Some(c) = self.items[i].charge {
                    w["range_m"] = json!(g(c, a::maxVelocity) * g(c, a::explosionDelay) / 1000.0);
                    w["explosion_radius"] = json!(g(c, a::aoeCloudSize));
                    w["explosion_velocity"] = json!(g(c, a::aoeVelocity));
                }
            } else if kind == "smartbomb" {
                w["range_m"] = json!(g(i, a::empFieldRange));
            }
            if sp > 0.0 {
                w["spool_multiplier"] = json!(1.0 + sp);
                w["volley_unspooled"] = base.json();
            }
            weapons.push(w);
        }
        let mut d_vol = Dmg::default();
        let mut d_dps = Dmg::default();
        let mut drone_out = Vec::new();
        for &i in &drones {
            let n = self.items[i].active_count as f64;
            if n == 0.0 {
                continue;
            }
            let mult = if self.has(i, a::damageMultiplier) { self.get(i, a::damageMultiplier) } else { 1.0 };
            let v = Dmg { em: g(i, DMG[0]), th: g(i, DMG[1]), ki: g(i, DMG[2]), ex: g(i, DMG[3]) }.scale(mult * n);
            let cyc = self.raw_cycle_ms(i);
            if v.total() == 0.0 || cyc == 0.0 {
                continue;
            }
            let dps = v.scale(1000.0 / cyc);
            d_vol.add(&v);
            d_dps.add(&dps);
            drone_out.push(json!({"drone_index": self.items[i].req_index, "type_id": self.items[i].type_id, "name": d::type_name(self.items[i].ty), "count": n, "volley": v.json(), "dps": dps.json()}));
        }
        let mut f_vol = Dmg::default();
        let mut f_dps = Dmg::default();
        let mut fighter_out = Vec::new();
        const FIGHTER_ATTACKS: [(u32, [u16; 6]); 2] = [
            (
                e::fighterAbilityAttackM,
                [
                    a::fighterAbilityAttackMissileDamageMultiplier,
                    a::fighterAbilityAttackMissileDamageEM,
                    a::fighterAbilityAttackMissileDamageTherm,
                    a::fighterAbilityAttackMissileDamageKin,
                    a::fighterAbilityAttackMissileDamageExp,
                    a::fighterAbilityAttackMissileDuration,
                ],
            ),
            (
                e::fighterAbilityMissiles,
                [
                    a::fighterAbilityMissilesDamageMultiplier,
                    a::fighterAbilityMissilesDamageEM,
                    a::fighterAbilityMissilesDamageTherm,
                    a::fighterAbilityMissilesDamageKin,
                    a::fighterAbilityMissilesDamageExp,
                    a::fighterAbilityMissilesDuration,
                ],
            ),
        ];
        for &i in &fighters {
            let n = self.items[i].active_count as f64;
            if n == 0.0 {
                continue;
            }
            let mut fv = Dmg::default();
            let mut fd = Dmg::default();
            for (eid, at) in FIGHTER_ATTACKS {
                if !self.items[i].has_effect(eid) || !self.items[i].fighter_abilities.contains(&eid) {
                    continue;
                }
                let m = g(i, at[0]);
                let m = if m == 0.0 { 1.0 } else { m };
                let v = Dmg { em: g(i, at[1]), th: g(i, at[2]), ki: g(i, at[3]), ex: g(i, at[4]) }.scale(m * n);
                let dur = g(i, at[5]);
                fv.add(&v);
                if dur > 0.0 {
                    fd.add(&v.scale(1000.0 / dur));
                }
            }
            if fv.total() > 0.0 {
                f_vol.add(&fv);
                f_dps.add(&fd);
                fighter_out.push(json!({"fighter_index": self.items[i].req_index, "type_id": self.items[i].type_id, "name": d::type_name(self.items[i].ty), "squadron_size": n, "volley": fv.json(), "dps": fd.json()}));
            }
        }
        let mut t_vol = w_vol;
        t_vol.add(&d_vol);
        t_vol.add(&f_vol);
        let mut t_dps = w_dps;
        t_dps.add(&d_dps);
        t_dps.add(&f_dps);
        let offense = json!({
            "weapons": weapons, "drones": drone_out, "fighters": fighter_out,
            "total": {"weapon_dps": w_dps.total(), "weapon_volley": w_vol.total(), "drone_dps": d_dps.total(), "drone_volley": d_vol.total(),
                      "fighter_dps": f_dps.total(), "fighter_volley": f_vol.total(), "dps": t_dps.json(), "volley": t_vol.json()},
            "vs_target_profile": {"dps": t_dps.vs(&tp_res), "volley": t_vol.vs(&tp_res)},
        });

        // ---------------- defense
        let dp = req.damage_pattern.unwrap_or(Resists { em: 25.0, thermal: 25.0, kinetic: 25.0, explosive: 25.0 });
        let dp_tot = (dp.em + dp.thermal + dp.kinetic + dp.explosive).max(1e-12);
        let res4 = |x: [u16; 4]| -> [f64; 4] { [g(ship, x[0]), g(ship, x[1]), g(ship, x[2]), g(ship, x[3])] };
        let effectivify = |amount: f64, r: [f64; 4]| {
            let div = (dp.em * r[0] + dp.thermal * r[1] + dp.kinetic * r[2] + dp.explosive * r[3]) / dp_tot;
            if div == 0.0 { amount } else { amount / div }
        };
        let rs = res4([a::shieldEmDamageResonance, a::shieldThermalDamageResonance, a::shieldKineticDamageResonance, a::shieldExplosiveDamageResonance]);
        let ra = res4([a::armorEmDamageResonance, a::armorThermalDamageResonance, a::armorKineticDamageResonance, a::armorExplosiveDamageResonance]);
        let rh = res4([a::emDamageResonance, a::thermalDamageResonance, a::kineticDamageResonance, a::explosiveDamageResonance]);
        let hp_s = g(ship, a::shieldCapacity);
        let hp_a = g(ship, a::armorHP);
        let hp_h = g(ship, a::hp);
        let (e_s, e_a, e_h) = (effectivify(hp_s, rs), effectivify(hp_a, ra), effectivify(hp_h, rh));
        let res_json = |r: [f64; 4]| json!({"em": r[0], "thermal": r[1], "kinetic": r[2], "explosive": r[3]});
        let mut shield_rep = 0.0;
        let mut armor_rep = 0.0;
        let mut hull_rep = 0.0;
        for &i in &modules {
            if !active(i) {
                continue;
            }
            let dur = g(i, a::duration) / 1000.0;
            if dur <= 0.0 {
                continue;
            }
            if self.has_eff(i, &[e::shieldBoosting, e::fueledShieldBoosting]) {
                shield_rep += g(i, a::shieldBonus) / dur;
            }
            if self.has_eff(i, &[e::armorRepair]) {
                armor_rep += g(i, a::armorDamageAmount) / dur;
            }
            if self.has_eff(i, &[e::fueledArmorRepair]) {
                let paste = self.items[i].charge.map(|c| self.items[c].type_id == d::T_NANITE_REPAIR_PASTE).unwrap_or(false);
                armor_rep += g(i, a::armorDamageAmount) * if paste { 3.0 } else { 1.0 } / dur;
            }
            if self.has_eff(i, &[e::structureRepair]) {
                hull_rep += g(i, a::structureDamageAmount) / dur;
            }
        }
        let shield_rr_s = g(ship, a::shieldRechargeRate) / 1000.0;
        let passive = if shield_rr_s > 0.0 { 10.0 / shield_rr_s * 0.5 * 0.5 * hp_s } else { 0.0 };
        let defense = json!({
            "hp": {"shield": hp_s, "armor": hp_a, "hull": hp_h, "total": hp_s + hp_a + hp_h},
            "resonance": {"shield": res_json(rs), "armor": res_json(ra), "hull": res_json(rh)},
            "ehp": {"shield": e_s, "armor": e_a, "hull": e_h, "total": e_s + e_a + e_h},
            "damage_pattern": {"em": dp.em, "thermal": dp.thermal, "kinetic": dp.kinetic, "explosive": dp.explosive},
            "tank": {
                "raw": {"passive_shield": passive, "shield_repair": shield_rep, "armor_repair": armor_rep, "hull_repair": hull_rep},
                "effective": {"passive_shield": effectivify(passive, rs), "shield_repair": effectivify(shield_rep, rs),
                              "armor_repair": effectivify(armor_rep, ra), "hull_repair": effectivify(hull_rep, rh)},
            },
        });

        // ---------------- capacitor
        let cap = g(ship, a::capacitorCapacity);
        let rr = g(ship, a::rechargeRate);
        let peak = if rr > 0.0 { 10.0 / (rr / 1000.0) * 0.5 * 0.5 * cap } else { 0.0 };
        let mut drains = Vec::new();
        let mut cap_used = 0.0;
        let mut cap_added = 0.0;
        let mut module_rows = Vec::new();
        for &i in &modules {
            let mut cap_need = g(i, a::capacitorNeed);
            let is_inj = self.items[i].group == d::G_CAPACITOR_BOOSTER;
            if is_inj {
                cap_need = -self.items[i].charge.map(|c| g(c, a::capacitorBonus)).unwrap_or(0.0);
            }
            if self.has_eff(i, &[e::energyNosferatuFalloff]) && !req.options.nos_no_target_cap {
                cap_need = -g(i, a::powerTransferAmount);
            }
            let cyc_raw = self.raw_cycle_ms(i);
            let full = cyc_raw + g(i, a::moduleReactivationDelay);
            let mut row = json!({"module_index": self.items[i].req_index, "type_id": self.items[i].type_id,
                "name": d::type_name(self.items[i].ty), "slot": self.items[i].slot, "state": self.items[i].state,
                "cpu": g(i, a::cpu), "power": g(i, a::power)});
            if cyc_raw > 0.0 {
                row["cycle_time_ms"] = json!(cyc_raw);
            }
            if active(i) && cap_need != 0.0 && full > 0.0 {
                let avg = self.avg_cycle_ms(i, factor_reload);
                let use_ = if avg > 0.0 { cap_need / (avg / 1000.0) } else { 0.0 };
                if use_ > 0.0 { cap_used += use_ } else { cap_added -= use_ }
                row["cap_use_gj_s"] = json!(use_);
                drains.push(Drain {
                    duration: full.trunc(),
                    cap_need,
                    clip_size: self.num_shots(i),
                    reload_ms: g(i, a::reloadTime),
                    is_injector: is_inj,
                    disable_stagger: self.has_eff(i, &[e::turretFitted]),
                });
            }
            module_rows.push(row);
        }
        let mut capj = json!({"capacity": cap, "recharge_time_s": rr / 1000.0, "peak_recharge_gj_s": peak,
            "use_gj_s": cap_used, "injected_gj_s": cap_added, "delta_gj_s": peak + cap_added - cap_used});
        if drains.is_empty() {
            capj["stable"] = json!(true);
            capj["stable_percent"] = json!(100.0);
        } else {
            let o = &req.options.cap_sim;
            let r = capsim::simulate(cap, rr, &drains, 1.0, o.reload || factor_reload, true, o.max_time_s.unwrap_or(6.0 * 3600.0) * 1000.0);
            let st = (r.stable_low + r.stable_high) / 2.0;
            capj["stable"] = json!(r.stable && st > 0.0);
            if r.stable && st > 0.0 {
                capj["stable_percent"] = json!((st * 100.0).min(100.0));
            } else {
                capj["depletes_in_s"] = json!(r.t_s);
            }
            capj["eve_stable_percent"] = json!(r.eve_stable * 100.0);
            capj["sim_iterations"] = json!(r.iterations);
        }

        // ---------------- navigation
        let maxv = g(ship, a::maxVelocity);
        let limit = g(ship, a::speedLimit);
        let max_speed = if limit > 0.0 && maxv > limit { limit } else { maxv };
        let mass = g(ship, a::mass);
        let agility = g(ship, a::agility);
        let base_warp = {
            let v = g(ship, a::baseWarpSpeed);
            if v == 0.0 { 1.0 } else { v }
        };
        let warp_mult = {
            let v = g(ship, a::warpSpeedMultiplier);
            if v == 0.0 { 1.0 } else { v }
        };
        let warp_need = g(ship, a::warpCapacitorNeed);
        let sig = g(ship, a::signatureRadius);
        let navigation = json!({
            "max_velocity": max_speed, "align_time_s": -(0.25f64.ln()) * agility * mass / 1e6, "mass": mass, "agility": agility,
            "signature_radius": sig, "warp_speed_au_s": base_warp * warp_mult,
            "max_warp_distance_au": if warp_need > 0.0 && mass > 0.0 { cap / (mass * warp_need) } else { 0.0 },
            "warp_scramble_status": g(ship, a::warpScrambleStatus),
        });

        // ---------------- targeting
        let strengths = [
            ("radar", a::scanRadarStrength),
            ("ladar", a::scanLadarStrength),
            ("magnetometric", a::scanMagnetometricStrength),
            ("gravimetric", a::scanGravimetricStrength),
        ];
        let mut best = ("none", 0.0f64);
        for (n, at) in strengths {
            let v = g(ship, at);
            if v > best.1 {
                best = (n, v);
            }
        }
        let scan_res = g(ship, a::scanResolution);
        let lt = |s: f64| lock_time(scan_res, s);
        let ship_targets = g(ship, a::maxLockedTargets);
        let char_targets = g(ch, a::maxLockedTargets);
        let targeting = json!({
            "max_targets": ship_targets.min(char_targets.max(0.0)),
            "max_range_m": g(ship, a::maxTargetRange), "scan_resolution": scan_res,
            "sensor_strength": best.1, "sensor_type": best.0,
            "probe_size": if best.1 > 0.0 { Some((sig / best.1).max(1.08)) } else { None },
            "lock_time_s": {"sig_25m": lt(25.0), "sig_40m": lt(40.0), "sig_125m": lt(125.0), "sig_400m": lt(400.0), "sig_target_profile": tp.signature_radius.and_then(lt)},
        });

        let drones_j = json!({
            "active": drones.iter().map(|&i| self.items[i].active_count).sum::<u32>(),
            "max_active": g(ch, a::maxActiveDrones),
            "control_range_m": g(ch, a::droneControlDistance),
        });

        let mut out = Map::new();
        out.insert(
            "meta".into(),
            json!({"schema_version": 1, "engine": concat!("eve-dogma-f ", env!("CARGO_PKG_VERSION")),
            "sde_build": d::SDE_BUILD, "dataset_sha256": d::DATASET_SHA256}),
        );
        let st = &self.items[ship];
        out.insert("ship".into(), json!({"type_id": st.type_id, "name": d::type_name(st.ty), "group": d::group_name(st.group)}));
        out.insert("resources".into(), resources);
        out.insert("offense".into(), offense);
        out.insert("defense".into(), defense);
        out.insert("capacitor".into(), capj);
        out.insert("navigation".into(), navigation);
        out.insert("targeting".into(), targeting);
        out.insert("drones".into(), drones_j);
        out.insert("modules".into(), Value::Array(module_rows));
        if req.options.validate {
            out.insert("violations".into(), Value::Array(self.validate(cpu_used, pg_used, calib_used, bw_used)));
        }
        if !self.warnings.is_empty() {
            out.insert("warnings".into(), json!(self.warnings));
        }
        match req.options.include_attributes.as_deref() {
            Some("ship") => {
                out.insert("attributes".into(), json!({"ship": self.dump_attrs(ship)}));
            }
            Some("all") => {
                let mut m = Map::new();
                m.insert("ship".into(), self.dump_attrs(ship));
                m.insert("character".into(), self.dump_attrs(ch));
                let mods: Vec<Value> = modules
                    .iter()
                    .map(|&i| {
                        json!({"module_index": self.items[i].req_index, "type_id": self.items[i].type_id, "attributes": self.dump_attrs(i),
                           "charge": self.items[i].charge.map(|c| self.dump_attrs(c))})
                    })
                    .collect();
                m.insert("modules".into(), Value::Array(mods));
                let dr: Vec<Value> =
                    drones.iter().map(|&i| json!({"drone_index": self.items[i].req_index, "attributes": self.dump_attrs(i)})).collect();
                m.insert("drones".into(), Value::Array(dr));
                out.insert("attributes".into(), Value::Object(m));
            }
            _ => {}
        }
        tidy(Value::Object(out))
    }

    pub fn dump_attrs(&self, i: usize) -> Value {
        let mut m = Map::new();
        for k in self.attr_ids(i) {
            let name = d::attr_name(k).map(|s| s.to_string()).unwrap_or_else(|| k.to_string());
            m.insert(name, json!(self.get(i, k)));
        }
        Value::Object(m)
    }

    fn validate(&self, cpu: f64, pg: f64, calib: f64, bw: f64) -> Vec<Value> {
        let ship = self.ship;
        let g = |i: usize, x: u16| self.get(i, x);
        let mut v = Vec::new();
        let mut push = |code: &str, msg: String, idx: Option<usize>| v.push(json!({"code": code, "message": msg, "module_index": idx}));
        if cpu > g(ship, a::cpuOutput) + 1e-9 {
            push("CPU_OVERLOAD", format!("CPU used {cpu:.2} > output {:.2}", g(ship, a::cpuOutput)), None);
        }
        if pg > g(ship, a::powerOutput) + 1e-9 {
            push("POWER_OVERLOAD", format!("Powergrid used {pg:.2} > output {:.2}", g(ship, a::powerOutput)), None);
        }
        if calib > g(ship, a::upgradeCapacity) + 1e-9 {
            push("CALIBRATION_OVERLOAD", format!("Calibration used {calib} > {}", g(ship, a::upgradeCapacity)), None);
        }
        if bw > g(ship, a::droneBandwidth) + 1e-9 {
            push("DRONE_BANDWIDTH", format!("Drone bandwidth used {bw} > {}", g(ship, a::droneBandwidth)), None);
        }
        let modules: Vec<usize> = (0..self.items.len()).filter(|&i| self.items[i].kind == Kind::Module).collect();
        for (slot, attr) in [
            (Slot::High, a::hiSlots),
            (Slot::Mid, a::medSlots),
            (Slot::Low, a::lowSlots),
            (Slot::Rig, a::rigSlots),
            (Slot::Subsystem, a::maxSubSystems),
            (Slot::Service, a::serviceSlots),
        ] {
            let used = modules.iter().filter(|&&i| self.items[i].slot == Some(slot)).count() as f64;
            if used > g(ship, attr) {
                push("SLOTS_EXCEEDED", format!("{slot:?} slots used {used} > {}", g(ship, attr)), None);
            }
        }
        let t = modules.iter().filter(|&&i| self.has_eff(i, &[e::turretFitted])).count() as f64;
        if t > g(ship, a::turretSlotsLeft) {
            push("TURRET_HARDPOINTS", format!("turrets {t} > hardpoints {}", g(ship, a::turretSlotsLeft)), None);
        }
        let l = modules.iter().filter(|&&i| self.has_eff(i, &[e::launcherFitted])).count() as f64;
        if l > g(ship, a::launcherSlotsLeft) {
            push("LAUNCHER_HARDPOINTS", format!("launchers {l} > hardpoints {}", g(ship, a::launcherSlotsLeft)), None);
        }
        let ship_it = &self.items[ship];
        let ship_name = d::type_name(ship_it.ty);
        let mut fitted_group: Vec<(u32, u32)> = Vec::new();
        let mut fitted_type: Vec<(u32, u32)> = Vec::new();
        let mut active_group: Vec<(u32, u32)> = Vec::new();
        let mut online_group: Vec<(u32, u32)> = Vec::new();
        fn bump(m: &mut Vec<(u32, u32)>, k: u32) {
            match m.iter_mut().find(|x| x.0 == k) {
                Some(x) => x.1 += 1,
                None => m.push((k, 1)),
            }
        }
        fn count(m: &[(u32, u32)], k: u32) -> u32 {
            m.iter().find(|x| x.0 == k).map(|x| x.1).unwrap_or(0)
        }
        for &i in &modules {
            let it = &self.items[i];
            let idx = it.req_index;
            let name = d::type_name(it.ty);
            let ta = |x: u16| d::type_attr(it.ty, x);
            if it.slot.is_none() {
                push("NOT_FITTABLE", format!("{name} is not a fittable module"), idx);
            }
            let gr: Vec<u32> = d::CAN_FIT_GROUP_ATTRS.iter().filter_map(|x| ta(*x)).map(|v| v as u32).filter(|v| *v != 0).collect();
            let ty: Vec<u32> = d::CAN_FIT_TYPE_ATTRS.iter().filter_map(|x| ta(*x)).map(|v| v as u32).filter(|v| *v != 0).collect();
            if (!gr.is_empty() || !ty.is_empty()) && !gr.contains(&ship_it.group) && !ty.contains(&ship_it.type_id) {
                push("SHIP_RESTRICTION", format!("{name} cannot be fitted to {ship_name}"), idx);
            }
            if it.slot == Some(Slot::Rig) {
                let rs = ta(a::rigSize).unwrap_or(0.0);
                let srs = g(ship, a::rigSize);
                if rs != 0.0 && rs != srs {
                    push("RIG_SIZE", format!("{name} rig size {rs} != ship rig size {srs}"), idx);
                }
            }
            bump(&mut fitted_group, it.group);
            bump(&mut fitted_type, it.type_id);
            if it.state >= State::Online {
                bump(&mut online_group, it.group);
            }
            if it.state >= State::Active {
                bump(&mut active_group, it.group);
            }
            let check = |attr: u16, m: &[(u32, u32)], key: u32| -> Option<(f64, u32)> {
                let lim = ta(attr)?;
                let n = count(m, key);
                if lim > 0.0 && n as f64 > lim { Some((lim, n)) } else { None }
            };
            if let Some((lim, n)) = check(a::maxGroupFitted, &fitted_group, it.group) {
                push("MAX_GROUP_FITTED", format!("{name}: {n} fitted of group, max {lim}"), idx);
            }
            if let Some((lim, n)) = check(a::maxTypeFitted, &fitted_type, it.type_id) {
                push("MAX_TYPE_FITTED", format!("{name}: {n} fitted, max {lim}"), idx);
            }
            if let Some((lim, n)) = check(a::maxGroupOnline, &online_group, it.group) {
                push("MAX_GROUP_ONLINE", format!("{name}: {n} online of group, max {lim}"), idx);
            }
            if let Some((lim, n)) = check(a::maxGroupActive, &active_group, it.group) {
                push("MAX_GROUP_ACTIVE", format!("{name}: {n} active of group, max {lim}"), idx);
            }
            if let Some(c) = it.charge {
                let cit = &self.items[c];
                let cname = d::type_name(cit.ty);
                let cg: Vec<u32> = d::CHARGE_GROUP_ATTRS.iter().filter_map(|x| ta(*x)).map(|v| v as u32).filter(|v| *v != 0).collect();
                if !cg.contains(&cit.group) {
                    push("CHARGE_GROUP", format!("{cname} cannot be loaded into {name}"), idx);
                }
                if let (Some(x), Some(y)) = (ta(a::chargeSize), d::type_attr(cit.ty, a::chargeSize)) {
                    if x != y {
                        push("CHARGE_SIZE", format!("{cname} size {y} != launcher size {x}"), idx);
                    }
                }
                let (cv, mc) = (d::type_volume(cit.ty), d::type_capacity(it.ty));
                if cv > mc && mc > 0.0 {
                    push("CHARGE_CAPACITY", format!("{cname} does not fit into {name}"), idx);
                }
            }
        }
        // skills
        let mut have: Vec<(u32, f64)> = Vec::new();
        for (i, it) in self.items.iter().enumerate() {
            if it.kind == Kind::Skill {
                have.push((it.type_id, self.base(i, crate::engine::ATTR_SKILL_LEVEL)));
            }
        }
        have.sort_by_key(|x| x.0);
        const SKILL_ATTRS: [(u16, u16); 6] = [
            (a::requiredSkill1, a::requiredSkill1Level),
            (a::requiredSkill2, a::requiredSkill2Level),
            (a::requiredSkill3, a::requiredSkill3Level),
            (a::requiredSkill4, a::requiredSkill4Level),
            (a::requiredSkill5, a::requiredSkill5Level),
            (a::requiredSkill6, a::requiredSkill6Level),
        ];
        let mut missing: Vec<(u32, f64, u32)> = Vec::new();
        for it in &self.items {
            if !matches!(it.kind, Kind::Ship | Kind::Module | Kind::Charge | Kind::Drone | Kind::Fighter | Kind::Implant | Kind::Booster) {
                continue;
            }
            for (sa, la) in SKILL_ATTRS {
                let s = d::type_attr(it.ty, sa).unwrap_or(0.0) as u32;
                if s == 0 {
                    continue;
                }
                let need = d::type_attr(it.ty, la).unwrap_or(1.0);
                let lvl = have.binary_search_by_key(&s, |x| x.0).map(|k| have[k].1).unwrap_or(0.0);
                if lvl < need && !missing.iter().any(|m| m.0 == s && m.1 >= need) {
                    missing.push((s, need, it.type_id));
                }
            }
        }
        for (s, need, by) in missing {
            push("MISSING_SKILL", format!("{} {} required by {}", d::type_name_by_id(s), need, d::type_name_by_id(by)), None);
        }
        v
    }
}
