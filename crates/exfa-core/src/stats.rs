//! Fit statistics on top of the evaluated dogma graph (Pyfa-equivalent formulas).
//! Formulas follow the reference engine eve-dogma-rs (LGPL-3.0-or-later), from which this file is derived;
//! all attribute/effect/type references are compile-time constants from the generated tables.
use crate::capsim::{self, Drain};
use crate::data::{self as d, a, e};
use crate::engine::{Fit, Kind};
use crate::request::{FitRequest, Resists, Slot, Spool, SpoolType, State};
use crate::j::{Key, PushKv, J};
use crate::jv;

thread_local! {
    /// capacitor drains of the last `compute_stats` on this thread (graphs: capacitor simulation history)
    pub static LAST_DRAINS: std::cell::RefCell<Vec<crate::capsim::Drain>> = const { std::cell::RefCell::new(Vec::new()) };
}

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
    /// breacher pod damage (untyped, Pyfa DmgTypes.pure): does not stack, the strongest applies
    pure: f64,
}
impl Dmg {
    fn total(&self) -> f64 {
        self.em + self.th + self.ki + self.ex + self.pure
    }
    fn scale(&self, k: f64) -> Dmg {
        Dmg { em: self.em * k, th: self.th * k, ki: self.ki * k, ex: self.ex * k, pure: self.pure * k }
    }
    fn add(&mut self, o: &Dmg) {
        self.em += o.em;
        self.th += o.th;
        self.ki += o.ki;
        self.ex += o.ex;
        self.pure = self.pure.max(o.pure);
    }
    fn vs(&self, r: &Resists) -> f64 {
        self.em * (1.0 - r.em) + self.th * (1.0 - r.thermal) + self.ki * (1.0 - r.kinetic) + self.ex * (1.0 - r.explosive) + self.pure
    }
    fn json(&self) -> J {
        let mut o = jv!({"em": self.em, "thermal": self.th, "kinetic": self.ki, "explosive": self.ex, "total": self.total()});
        if self.pure != 0.0 {
            // contract 1.4.4: optional `pure` key (breacher pods), omitted when 0
            o["pure"] = jv!(self.pure);
        }
        o
    }
}

const DMG: [u16; 4] = [a::emDamage, a::thermalDamage, a::kineticDamage, a::explosiveDamage];


fn sig_radius_now(f: &Fit) -> f64 {
    f.sig_before_late()
}

/// Pyfa's remote-repair diminishing returns for one layer (cycle time truncated to whole seconds, as Pyfa does).
fn applied_rr(rr: &[(u8, f64, f64)], kind: u8) -> f64 {
    let list: Vec<(f64, f64)> = rr.iter().filter(|x| x.0 == kind).map(|x| (x.1, x.2)).collect();
    if list.is_empty() {
        return 0.0;
    }
    let total: f64 = list.iter().map(|(amt, c)| if c.trunc() > 0.0 { amt / c.trunc() } else { 0.0 }).sum();
    let mut applied = 0.0;
    for (amt, c) in list {
        if c.trunc() <= 0.0 || c <= 0.0 {
            continue;
        }
        let rrps = amt / c.trunc();
        let modified = 7000.0 + rrps * 20.0;
        let mult = 1.0 - (((rrps + modified) / (total + modified)) - 1.0).powi(2);
        applied += mult * amt / c;
    }
    applied
}

impl Fit {
    fn has_eff(&self, i: usize, ids: &[u32]) -> bool {
        self.items[i].effects().any(|(ei, _)| ids.contains(&d::EFF_IDS[ei]))
    }

    /// Missile range like Pyfa: acceleration phase, flight-time bonus from ship radius, whole-second
    /// interpolation, FoF limit, centre-to-surface correction.
    pub(crate) fn missile_range(&self, c: usize) -> f64 {
        match self.missile_range_data(c) {
            None => 0.0,
            Some((lo, hi, chance)) => lo * (1.0 - chance) + hi * chance,
        }
    }

    /// (lower range, higher range, chance of the higher one), surface distances.
    pub(crate) fn missile_range_data(&self, c: usize) -> Option<(f64, f64, f64)> {
        let v = self.get(c, a::maxVelocity);
        if v == 0.0 {
            return None;
        }
        let radius = self.get(self.ship, a::radius);
        let flight = float_unerr(self.get(c, a::explosionDelay) / 1000.0 + radius / v);
        let mass = self.get(c, a::mass);
        let agility = self.get(c, a::agility);
        let range = |t: f64| {
            let accel = t.min(mass * agility / 1e6);
            v / 2.0 * accel + v * (t - accel)
        };
        let (lt, ht) = (flight.floor(), flight.ceil());
        let (mut lo, mut hi) = (range(lt), range(ht));
        if self.items[c].has_effect(e::fofMissileLaunching) {
            let lim = self.get(c, a::maxFOFTargetRange);
            if lim != 0.0 {
                lo = lo.min(lim);
                hi = hi.min(lim);
            }
        }
        lo = (lo - radius).max(0.0);
        hi = (hi - radius).max(0.0);
        Some((lo, hi, flight - lt))
    }

    pub(crate) fn raw_cycle_ms(&self, i: usize) -> f64 {
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

    pub(crate) fn num_charges(&self, i: usize) -> u32 {
        let Some(c) = self.items[i].charge else { return 0 };
        let vol = self.get(c, a::volume);
        let cap = self.base(i, a::capacity);
        if vol <= 0.0 { 0 } else { float_unerr(cap / vol).floor() as u32 }
    }

    pub(crate) fn num_shots(&self, i: usize) -> u32 {
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
    /// Pyfa Fighter.getCycleParametersPerEffect (+ FighterAbility numShots/reloadTime): per ability
    /// (effect id, plain cycle ms, average cycle ms incl. refuels, shots per refuel).
    fn fighter_cycles(&self, i: usize, factor_reload: bool) -> Vec<(u32, f64, f64, u32)> {
        const ABIL: [(u32, u16, bool); 8] = [
            (e::fighterAbilityMissiles, a::fighterAbilityMissilesDuration, true),
            (e::fighterAbilityEnergyNeutralizer, a::fighterAbilityEnergyNeutralizerDuration, false),
            (e::fighterAbilityStasisWebifier, a::fighterAbilityStasisWebifierDuration, false),
            (e::fighterAbilityWarpDisruption, a::fighterAbilityWarpDisruptionDuration, false),
            (e::fighterAbilityECM, a::fighterAbilityECMDuration, false),
            (e::fighterAbilityEvasiveManeuvers, a::fighterAbilityEvasiveManeuversDuration, false),
            (e::fighterAbilityAttackM, a::fighterAbilityAttackMissileDuration, false),
            (e::fighterAbilityLaunchBomb, a::fighterAbilityLaunchBombDuration, true),
        ];
        let role = self.get(i, a::fighterSquadronRole) as i64;
        let (shots_role, rearm) = match role {
            2 => (12u32, 4000.0),
            4 => (6, 6000.0),
            5 => (3, 20000.0),
            _ => (0, 0.0),
        };
        let refuel = self.get(i, a::fighterRefuelingTime);
        // (eid, cycle, numShots, hasCharges)
        let mut v: Vec<(u32, f64, u32, bool)> = Vec::new();
        for (eid, dur, charges) in ABIL {
            if self.items[i].has_effect(eid) {
                let c = self.get(i, dur);
                if c > 0.0 {
                    v.push((eid, c, if charges { shots_role } else { 0 }, charges));
                }
            }
        }
        let limited = v.iter().filter(|x| x.2 > 0).min_by(|x, y| (x.1 * x.2 as f64).partial_cmp(&(y.1 * y.2 as f64)).unwrap());
        let Some(&(ml_eid, ml_c, ml_n, _)) = limited.filter(|_| factor_reload) else {
            return v.iter().map(|x| (x.0, x.1, x.1, x.2)).collect();
        };
        let unerr = |x: f64| {
            if x == 0.0 || x.is_infinite() {
                return x;
            }
            let k = 7 - x.abs().log10().ceil() as i32;
            py_round(x, k)
        };
        let dur_to_refuel = ml_c * ml_n as f64;
        let per: Vec<(u32, f64, u32, Option<f64>)> = v
            .iter()
            .map(|x| {
                if x.0 == ml_eid {
                    (x.0, x.1, ml_n, None)
                } else {
                    let full = unerr(dur_to_refuel / x.1) as i64 as u32;
                    let extra = unerr(dur_to_refuel - full as f64 * x.1);
                    (x.0, x.1, full, if extra == 0.0 { None } else { Some(extra) })
                }
            })
            .collect();
        let mut refuel_time = f64::MIN;
        for (k, p) in per.iter().enumerate() {
            let spent = p.2 + p.3.is_some() as u32;
            let spent = spent.max(v[k].2);
            let rt = refuel + if v[k].3 { rearm * spent as f64 } else { 0.0 };
            refuel_time = refuel_time.max(rt);
        }
        per.iter()
            .enumerate()
            .map(|(k, p)| {
                let avg = match p.3 {
                    Some(extra) => (p.1 * p.2 as f64 + extra + refuel_time) / (p.2 + 1) as f64,
                    None => (p.1 * p.2 as f64 + refuel_time) / p.2.max(1) as f64,
                };
                (p.0, p.1, avg, v[k].2)
            })
            .collect()
    }

    pub(crate) fn avg_cycle_ms(&self, i: usize, factor_reload: bool) -> f64 {
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
        if let Some(c) = it.charge.filter(|&c| self.has_eff(c, &[e::dotMissileLaunching])) {
            // breacher pod (Pyfa BreacherInfo): untyped damage per 1 s tick, no damage multipliers
            let dm = Dmg { em: 0.0, th: 0.0, ki: 0.0, ex: 0.0, pure: self.get(c, a::dotMaxDamagePerTick) };
            return (dm, "breacher");
        }
        let dm = Dmg {
            em: self.get(src, DMG[0]) * mult,
            th: self.get(src, DMG[1]) * mult,
            ki: self.get(src, DMG[2]) * mult,
            ex: self.get(src, DMG[3]) * mult,
            pure: 0.0,
        };
        (dm, kind)
    }

    pub fn compute_stats(&self, req: &FitRequest) -> J {
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
        let calib_used: f64 = modules.iter().filter(|&&i| self.items[i].slot == Some(Slot::Rig) && self.items[i].state >= State::Online).map(|&i| self.get(i, a::upgradeCost)).sum();
        let drones: Vec<usize> = (0..self.items.len()).filter(|&i| self.items[i].kind == Kind::Drone).collect();
        let fighters: Vec<usize> = (0..self.items.len()).filter(|&i| self.items[i].kind == Kind::Fighter).collect();
        let bw_used: f64 = drones.iter().map(|&i| g(i, a::droneBandwidthUsed) * self.items[i].active_count as f64).sum();
        let bay_used: f64 = drones.iter().map(|&i| self.get(i, a::volume) * self.items[i].quantity as f64).sum();
        let fbay_used: f64 = fighters.iter().map(|&i| self.get(i, a::volume) * self.items[i].quantity as f64).sum();
        let cargo_used: f64 = req.cargo.iter().map(|c| d::type_index(c.type_id).map(d::type_volume).unwrap_or(0.0) * c.quantity as f64).sum();
        let count_slot = |s: Slot| modules.iter().filter(|&&i| self.items[i].slot == Some(s)).count();
        let turrets_used = modules.iter().filter(|&&i| self.has_eff(i, &[e::turretFitted])).count();
        let launchers_used = modules.iter().filter(|&&i| self.has_eff(i, &[e::launcherFitted])).count();
        let usage = |u: f64, t: f64| jv!({"used": u, "total": t});
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
        let resources = jv!({
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
        let tp_res = Resists { em: tp.em, thermal: tp.thermal, kinetic: tp.kinetic, explosive: tp.explosive, builtin: None };
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
            // Pyfa getVolleyParameters: DOT doomsdays hit every doomsdayDamageCycleTime for doomsdayDamageDuration
            let (dd, dc) = (g(i, a::doomsdayDamageDuration), g(i, a::doomsdayDamageCycleTime));
            let subcycles = if dd != 0.0 && dc != 0.0 && !self.has_eff(i, &[e::doomsdaySlash]) { float_unerr(dd / dc).floor() } else { 1.0 };
            let dps = if base.pure > 0.0 {
                // Pyfa getDps: a breacher's dps is its first tick
                vol_spooled
            } else if cyc > 0.0 {
                vol_spooled.scale(subcycles * 1000.0 / cyc)
            } else {
                Dmg::default()
            };
            w_vol.add(&vol_spooled);
            w_dps.add(&dps);
            let mut w = jv!({
                "module_index": self.items[i].req_index, "type_id": self.items[i].type_id,
                "name": d::type_name(self.items[i].ty), "kind": kind,
                "charge_type_id": self.items[i].charge.map(|c| self.items[c].type_id),
                "volley": vol_spooled.json(), "dps": dps.json(), "cycle_time_ms": cyc,
            });
            if kind == "turret" {
                w["optimal_m"] = jv!(g(i, a::maxRange));
                w["falloff_m"] = jv!(g(i, a::falloff));
                w["tracking"] = jv!(g(i, a::trackingSpeed));
            } else if kind == "missile" {
                if let Some(c) = self.items[i].charge {
                    w["range_m"] = jv!(self.missile_range(c));
                    w["explosion_radius"] = jv!(g(c, a::aoeCloudSize));
                    w["explosion_velocity"] = jv!(g(c, a::aoeVelocity));
                }
            } else if kind == "smartbomb" {
                w["range_m"] = jv!(g(i, a::empFieldRange));
            }
            if sp > 0.0 {
                w["spool_multiplier"] = jv!(1.0 + sp);
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
            let v = Dmg { em: g(i, DMG[0]), th: g(i, DMG[1]), ki: g(i, DMG[2]), ex: g(i, DMG[3]), pure: 0.0 }.scale(mult * n);
            // Pyfa Drone.cycleTime: first non-zero of speed, duration, durationHighisGood (missile drones: as before)
            let cyc = if self.has(i, a::entityMissileTypeID) {
                self.raw_cycle_ms(i)
            } else {
                [a::speed, a::duration, a::durationHighisGood].iter().map(|&x| g(i, x)).find(|&x| x != 0.0).unwrap_or(0.0).max(0.0)
            };
            if v.total() == 0.0 || cyc == 0.0 {
                continue;
            }
            let dps = v.scale(1000.0 / cyc);
            d_vol.add(&v);
            d_dps.add(&dps);
            drone_out.push(jv!({"drone_index": self.items[i].req_index, "type_id": self.items[i].type_id, "name": d::type_name(self.items[i].ty), "count": n, "volley": v.json(), "dps": dps.json(),
                "optimal_m": g(i, a::maxRange), "falloff_m": g(i, a::falloff), "tracking": g(i, a::trackingSpeed),
                "max_velocity": g(i, a::maxVelocity), "signature_radius": g(i, a::signatureRadius)}));
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
            let cyc = self.fighter_cycles(i, factor_reload);
            let mut vols: [(u32, Dmg); 2] = [(0, Dmg::default()), (0, Dmg::default())];
            for (k, (eid, at)) in FIGHTER_ATTACKS.iter().enumerate() {
                let eid = *eid;
                if !self.items[i].has_effect(eid) || !self.items[i].fighter_abilities.contains(&eid) {
                    continue;
                }
                let m = g(i, at[0]);
                let m = if m == 0.0 { 1.0 } else { m };
                let v = Dmg { em: g(i, at[1]), th: g(i, at[2]), ki: g(i, at[3]), ex: g(i, at[4]), pure: 0.0 }.scale(m * n);
                fv.add(&v);
                vols[k] = (eid, v);
            }
            // Pyfa Fighter.getCycleParametersPerEffectOptimizedDps: never-refuel cycling (charge-less abilities only)
            // vs. cycling with refuels, whichever gives more dps.
            let dps_with = |inf: bool| {
                let mut t = Dmg::default();
                for (eid, v) in &vols {
                    if let Some(&(_, c_inf, c_rel, shots)) = cyc.iter().find(|c| c.0 == *eid) {
                        let c = if inf { if shots == 0 { c_inf } else { 0.0 } } else { c_rel };
                        if c > 0.0 {
                            t.add(&v.scale(1000.0 / c));
                        }
                    }
                }
                t
            };
            let d_inf = dps_with(true);
            let d_rel = dps_with(false);
            fd.add(if d_inf.total() >= d_rel.total() { &d_inf } else { &d_rel });
            if fv.total() > 0.0 {
                f_vol.add(&fv);
                f_dps.add(&fd);
                fighter_out.push(jv!({"fighter_index": self.items[i].req_index, "type_id": self.items[i].type_id, "name": d::type_name(self.items[i].ty), "squadron_size": n, "volley": fv.json(), "dps": fd.json(),
                    "max_velocity": g(i, a::maxVelocity), "signature_radius": g(i, a::signatureRadius)}));
            }
        }
        let mut t_vol = w_vol;
        t_vol.add(&d_vol);
        t_vol.add(&f_vol);
        let mut t_dps = w_dps;
        t_dps.add(&d_dps);
        t_dps.add(&f_dps);
        let offense = jv!({
            "weapons": weapons, "drones": drone_out, "fighters": fighter_out,
            "total": {"weapon_dps": w_dps.total(), "weapon_volley": w_vol.total(), "drone_dps": d_dps.total(), "drone_volley": d_vol.total(),
                      "fighter_dps": f_dps.total(), "fighter_volley": f_vol.total(), "dps": t_dps.json(), "volley": t_vol.json()},
            "vs_target_profile": {"dps": t_dps.vs(&tp_res), "volley": t_vol.vs(&tp_res)},
        });

        // ---------------- defense
        let dp = req.damage_pattern.clone().unwrap_or(Resists { em: 25.0, thermal: 25.0, kinetic: 25.0, explosive: 25.0, builtin: None });
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
        let res_json = |r: [f64; 4]| jv!({"em": r[0], "thermal": r[1], "kinetic": r[2], "explosive": r[3]});
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
        // incoming remote repairs (Pyfa: diminishing returns over all RR of one layer)
        if g(ship, a::disallowAssistance) == 0.0 {
            shield_rep += applied_rr(&self.rr, 0);
            armor_rep += applied_rr(&self.rr, 1);
            hull_rep += applied_rr(&self.rr, 2);
        }
        let shield_rr_s = g(ship, a::shieldRechargeRate) / 1000.0;
        let passive = if shield_rr_s > 0.0 { 10.0 / shield_rr_s * 0.5 * 0.5 * hp_s } else { 0.0 };
        let mut defense = jv!({
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
        // overheat burnout estimate (Pyfa heat column behaviour; contract 1.10 stats-ext `modules[].heat`)
        let heat = {
            let mods: &[usize] = &modules;
            let rack = move |i: usize| match self.items[i].slot {
                Some(Slot::Low) => 1usize,
                Some(Slot::Mid) => 2,
                Some(Slot::High) => 3,
                Some(Slot::Rig) => 4,
                _ => 0,
            };
            let hot = move |i: usize| self.items[i].state == State::Overheated;
            let mut absorb = [0.0f64; 5];
            for &i in mods {
                if hot(i) {
                    absorb[rack(i)] += g(i, a::heatAbsorbtionRateModifier);
                }
            }
            let racks = g(ship, a::hiSlots) + g(ship, a::medSlots) + g(ship, a::lowSlots);
            let used = mods.iter().filter(|&&i| (1..=3).contains(&rack(i))).count() as f64;
            let offline = mods.iter().filter(|&&i| (1..=3).contains(&rack(i)) && self.items[i].state == State::Offline).count() as f64;
            let slot_factor = (used - offline) / (racks + g(ship, a::rigSlots));
            let hgm = g(ship, a::heatGenerationMultiplier);
            // position of a module within its rack (request order)
            let pos = move |i: usize| mods.iter().filter(|&&j| rack(j) == rack(i)).position(|&j| j == i).unwrap_or(0) as f64;
            let damage_p = move |i: usize, t: f64| {
                let r = rack(i);
                let att_attr = [0, a::heatAttenuationLow, a::heatAttenuationMed, a::heatAttenuationHi];
                let att = if (1..=3).contains(&r) && self.has(ship, att_attr[r]) { g(ship, att_attr[r]) } else { 0.25 };
                let rack_heat = 1.0 - std::f64::consts::E.powf(-t * hgm * absorb[r]);
                let me = pos(i);
                let mut keep = 1.0f64;
                for &j in mods {
                    if j != i && rack(j) == r && hot(j) {
                        keep *= 1.0 - att.powf((pos(j) - me).abs()) * slot_factor * rack_heat;
                    }
                }
                let own = slot_factor * rack_heat;
                if keep == 1.0 { own } else { 1.0 - keep * (1.0 - own) }
            };
            move |i: usize| -> Option<J> {
                if !hot(i) {
                    return None;
                }
                let (speed, dur) = (g(i, a::speed), g(i, a::duration));
                let step = if speed != 0.0 { speed / 1000.0 } else { dur / 1000.0 };
                let hd = g(i, a::heatDamage);
                if step <= 0.0 || hd <= 0.0 {
                    return None;
                }
                // per-cycle failure probability until it settles (5 decimals) or 600 s
                let mut probs = Vec::new();
                let (mut t, mut last) = (step, 0.0f64);
                while t < 600.0 {
                    let p = damage_p(i, t);
                    probs.push(p);
                    if format!("{p:.5}") == format!("{last:.5}") {
                        break;
                    }
                    t += step;
                    last = p;
                }
                if probs.is_empty() {
                    return None;
                }
                // expected number of cycles until `n` damage events (hp / heatDamage, rounded up)
                let n = (g(i, a::hp) / hd).ceil() as usize;
                let mut st = vec![0.0f64; n + 1];
                st[0] = 1.0;
                let mut expect = 0.0f64;
                let mut k_last = 0usize;
                for (k, &p) in probs.iter().enumerate() {
                    k_last = k;
                    if n >= 1 {
                        expect += (k + 1) as f64 * p * st[n - 1];
                    }
                    for m in (1..n).rev() {
                        st[m] = (1.0 - p) * st[m] + p * st[m - 1];
                    }
                    st[0] *= 1.0 - p;
                }
                let p_end = probs[k_last];
                for m in 0..n {
                    expect += ((k_last + 1) as f64 + (n - m) as f64 * (1.0 / p_end)) * st[m];
                }
                let cycles = expect.floor();
                let cyc_s = if dur != 0.0 { dur / 1000.0 } else { speed / 1000.0 };
                Some(jv!({"burn_cycles": cycles, "burnout_s": cycles * cyc_s}))
            }
        };
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
            let mut row = jv!({"module_index": self.items[i].req_index, "type_id": self.items[i].type_id,
                "name": d::type_name(self.items[i].ty), "slot": self.items[i].slot, "state": self.items[i].state,
                "cpu": g(i, a::cpu), "power": g(i, a::power)});
            if cyc_raw > 0.0 {
                row["cycle_time_ms"] = jv!(cyc_raw);
            }
            if let Some(h) = heat(i) {
                row["heat"] = h;
            }
            if active(i) && cap_need != 0.0 && full > 0.0 {
                // Pyfa forces reload into capacitor boosters' average cycle (module.forceReload)
                let avg = self.avg_cycle_ms(i, factor_reload || is_inj);
                let use_ = if avg > 0.0 { cap_need / (avg / 1000.0) } else { 0.0 };
                if use_ > 0.0 { cap_used += use_ } else { cap_added -= use_ }
                row["cap_use_gj_s"] = jv!(use_);
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
        // projected neutralisers / nosferatu / cap transfers
        let no_assist = g(ship, a::disallowAssistance) != 0.0;
        for x in &self.ext_drains {
            if x.assistance && no_assist {
                continue;
            }
            let mut need = x.amount * self.resist(x.resist);
            if x.sig_res != 0.0 {
                let sig = if x.late { self.get(ship, a::signatureRadius) } else { sig_radius_now(self) };
                need *= (sig / x.sig_res).min(1.0);
            }
            if need == 0.0 || x.cycle_ms <= 0.0 {
                continue;
            }
            let per_s = need / (x.cycle_ms.trunc() / 1000.0);
            if per_s > 0.0 { cap_used += per_s } else { cap_added -= per_s }
            drains.push(Drain { duration: x.cycle_ms.trunc(), cap_need: need, clip_size: 0, reload_ms: 0.0, is_injector: false, disable_stagger: false });
        }
        LAST_DRAINS.with(|c| *c.borrow_mut() = drains.clone());
        let mut capj = jv!({"capacity": cap, "recharge_time_s": rr / 1000.0, "peak_recharge_gj_s": peak,
            "use_gj_s": cap_used, "injected_gj_s": cap_added, "delta_gj_s": peak + cap_added - cap_used});
        if drains.is_empty() {
            capj["stable"] = jv!(true);
            capj["stable_percent"] = jv!(100.0);
        } else {
            let o = &req.options.cap_sim;
            let r = capsim::simulate(cap, rr, &drains, 1.0, o.reload || factor_reload, true, o.max_time_s.unwrap_or(6.0 * 3600.0) * 1000.0);
            let st = (r.stable_low + r.stable_high) / 2.0;
            capj["stable"] = jv!(r.stable && st > 0.0);
            if r.stable && st > 0.0 {
                capj["stable_percent"] = jv!((st * 100.0).min(100.0));
            } else {
                capj["depletes_in_s"] = jv!(r.t_s);
            }
            capj["eve_stable_percent"] = jv!(r.eve_stable * 100.0);
            capj["sim_iterations"] = jv!(r.iterations);
        }

        // ---------------- sustainable tank (Pyfa Fit.sustainableTank; ported from eve-dogma-rs, LGPL): when the
        // capacitor is not stable (or reload is factored), local cap-using repairers only run as far as peak
        // recharge + injected cap allow, most cap-efficient first.
        {
            let stable_now = !matches!(capj["stable"], J::Bool(false));
            let mut sus = [shield_rep, armor_rep, hull_rep];
            if !stable_now || factor_reload {
                let spec = |grp: u32| -> Option<(usize, u16)> {
                    match grp {
                        d::G_SHIELD_BOOSTER | d::G_ANCILLARY_SHIELD_BOOSTER => Some((0, a::shieldBonus)),
                        d::G_ARMOR_REPAIR_UNIT | d::G_ANCILLARY_ARMOR_REPAIRER => Some((1, a::armorDamageAmount)),
                        d::G_HULL_REPAIR_UNIT => Some((2, a::structureDamageAmount)),
                        _ => None,
                    }
                };
                let is_paste = |i: usize| self.items[i].charge.map(|c| self.items[c].type_id == d::T_NANITE_REPAIR_PASTE).unwrap_or(false);
                let charged_mult = |i: usize| {
                    let m = g(i, a::chargedArmorDamageMultiplier);
                    if m == 0.0 { 1.0 } else { m }
                };
                let mut adj = [0.0f64; 3];
                let mut used = cap_used;
                let mut reps: Vec<(usize, usize, u16, f64)> = Vec::new();
                for layer in 0..3 {
                    for &i in &modules {
                        if !active(i) {
                            continue;
                        }
                        let grp = self.items[i].group;
                        let Some((l, attr)) = spec(grp) else { continue };
                        if l != layer {
                            continue;
                        }
                        let cap_need = self.get(i, a::capacitorNeed);
                        let avg = self.avg_cycle_ms(i, factor_reload);
                        let cap_use = if cap_need != 0.0 && avg > 0.0 { cap_need / (avg / 1000.0) } else { 0.0 };
                        let cyc = self.raw_cycle_ms(i);
                        if cyc <= 0.0 {
                            continue;
                        }
                        let amount = g(i, attr);
                        let charge = self.items[i].charge;
                        if cap_use != 0.0 {
                            used -= cap_use;
                            let mult = if is_paste(i) { charged_mult(i) } else { 1.0 };
                            adj[l] -= amount * mult / (cyc / 1000.0);
                            reps.push((i, l, attr, cap_use));
                        } else if grp == d::G_ANCILLARY_SHIELD_BOOSTER {
                            let reload = if factor_reload && charge.is_some() { self.get(i, a::reloadTime) } else { 0.0 };
                            let shots = self.num_shots(i).max(1) as f64;
                            let off = reload / (shots * cyc + reload);
                            adj[l] -= amount * off / (cyc / 1000.0);
                        }
                    }
                }
                let eff = |i: usize, attr: u16| g(i, attr) * charged_mult(i) / self.get(i, a::capacitorNeed);
                reps.sort_by(|x, y| eff(y.0, y.2).partial_cmp(&eff(x.0, x.2)).unwrap_or(std::cmp::Ordering::Equal));
                let total_peak = peak + cap_added;
                for (i, l, attr, cap_use) in reps {
                    if used > total_peak {
                        break;
                    }
                    let charge = self.items[i].charge;
                    let reload = if factor_reload && charge.is_some() { self.get(i, a::reloadTime) } else { 0.0 };
                    let cyc = self.raw_cycle_ms(i);
                    let sustain = ((total_peak - used) / cap_use).min(1.0);
                    let amount = g(i, attr);
                    if charge.is_none() {
                        adj[l] += sustain * amount / (cyc / 1000.0);
                    } else {
                        let mult = if is_paste(i) { charged_mult(i) } else { 1.0 };
                        let shots = self.num_shots(i).max(1) as f64;
                        let on = shots * cyc / (shots * cyc + reload);
                        adj[l] += sustain * amount * on * mult / (cyc / 1000.0);
                    }
                    used += cap_use;
                }
                for l in 0..3 {
                    sus[l] += adj[l];
                }
            }
            defense["tank"]["sustained"] = jv!({"passive_shield": passive, "shield_repair": sus[0], "armor_repair": sus[1], "hull_repair": sus[2]});
            defense["tank"]["sustained_effective"] = jv!({"passive_shield": effectivify(passive, rs), "shield_repair": effectivify(sus[0], rs),
                "armor_repair": effectivify(sus[1], ra), "hull_repair": effectivify(sus[2], rh)});
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
        let navigation = jv!({
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
        // ECM jam chance (Pyfa Fit.jamChance): strengths vs the strongest sensor type (a tie -> no type -> 0)
        let jam = {
            let mut max_s = -1.0f64;
            let mut ty: Option<usize> = None;
            for (k, at) in [a::scanMagnetometricStrength, a::scanLadarStrength, a::scanRadarStrength, a::scanGravimetricStrength].into_iter().enumerate() {
                let v = g(ship, at);
                if v > max_s {
                    max_s = v;
                    ty = Some(k);
                } else if v == max_s {
                    ty = None;
                }
            }
            let mut retain = 1.0f64;
            if let Some(t) = ty {
                for x in &self.ext_ecm {
                    let mut st = x.st[t];
                    if x.resist != 0 {
                        let r = self.get(ship, x.resist);
                        if r != 0.0 {
                            st *= r;
                        }
                    }
                    if max_s > 0.0 {
                        retain *= 1.0 - (st / max_s).min(1.0);
                    }
                }
            }
            (1.0 - retain) * 100.0
        };
        let scan_res = g(ship, a::scanResolution);
        let lt = |s: f64| lock_time(scan_res, s);
        let ship_targets = g(ship, a::maxLockedTargets);
        let char_targets = g(ch, a::maxLockedTargets);
        let targeting = jv!({
            "max_targets": ship_targets.min(char_targets.max(0.0)),
            "max_range_m": g(ship, a::maxTargetRange), "scan_resolution": scan_res,
            "sensor_strength": best.1, "sensor_type": best.0, "jam_chance_percent": jam,
            "probe_size": if best.1 > 0.0 { Some((sig / best.1).max(1.08)) } else { None },
            "lock_time_s": {"sig_25m": lt(25.0), "sig_40m": lt(40.0), "sig_125m": lt(125.0), "sig_400m": lt(400.0), "sig_target_profile": tp.signature_radius.and_then(lt)},
        });

        // per drone / fighter type entry: one drone's hp, ehp (request damage pattern) and peak passive shield
        // recharge (Pyfa drone/fighter hp, ehp, calculateShieldRecharge); contract 1.10 stats-ext
        let unit_hp = |i: usize, key: &'static str| {
            let r = |x: [u16; 4]| -> [f64; 4] { [g(i, x[0]), g(i, x[1]), g(i, x[2]), g(i, x[3])] };
            let (hs, ha, hh) = (g(i, a::shieldCapacity), g(i, a::armorHP), g(i, a::hp));
            let rr_s = g(i, a::shieldRechargeRate) / 1000.0;
            let mut o = jv!({
                 "hp": {"shield": hs, "armor": ha, "hull": hh},
                 "ehp": {"shield": effectivify(hs, r([a::shieldEmDamageResonance, a::shieldThermalDamageResonance, a::shieldKineticDamageResonance, a::shieldExplosiveDamageResonance])),
                         "armor": effectivify(ha, r([a::armorEmDamageResonance, a::armorThermalDamageResonance, a::armorKineticDamageResonance, a::armorExplosiveDamageResonance])),
                         "hull": effectivify(hh, r([a::emDamageResonance, a::thermalDamageResonance, a::kineticDamageResonance, a::explosiveDamageResonance]))},
                 "shield_peak_recharge_hp_s": if rr_s > 0.0 { 10.0 / rr_s * 0.5 * 0.5 * hs } else { 0.0 }});
            if let J::O(m) = &mut o {
                m.push_kv(key.into(), jv!(self.items[i].req_index));
            }
            o
        };
        let fighters_j = jv!({"items": J::A(fighters.iter().map(|&i| unit_hp(i, "fighter_index")).collect())});
        let drones_j = jv!({
            "items": J::A(drones.iter().map(|&i| unit_hp(i, "drone_index")).collect()),
            "active": drones.iter().map(|&i| self.items[i].active_count).sum::<u32>(),
            "max_active": g(ch, a::maxActiveDrones),
            "control_range_m": g(ch, a::droneControlDistance),
        });

        // ---------------- mining (Pyfa Fit.calculatemining; contract draft 1.10 stats-ext `mining`)
        let mining = {
            let waste = |i: usize, yps: f64| {
                let ch = (g(i, a::miningWasteProbability) / 100.0).clamp(0.0, 1.0);
                yps * (1.0 + ch * g(i, a::miningWastedVolumeMultiplier))
            };
            let (mut m_y, mut m_d) = (0.0f64, 0.0f64);
            for &i in &modules {
                if !active(i) {
                    continue;
                }
                let amount = g(i, a::miningAmount);
                let cyc = self.avg_cycle_ms(i, factor_reload);
                let yps = if amount != 0.0 && cyc > 0.0 { amount / (cyc / 1000.0) } else { 0.0 };
                m_d += waste(i, yps);
                m_y += yps + yps * g(i, a::miningCritChance) * g(i, a::miningCritBonusYield);
            }
            let (mut d_y, mut d_d) = (0.0f64, 0.0f64);
            for &i in &drones {
                // Pyfa: a mining drone stack yields `amount` (whole stack) once any drone of it is active
                if self.items[i].active_count == 0 || !self.has(i, a::miningAmount) {
                    continue;
                }
                let cyc = if self.items[i].charge.is_some() {
                    g(i, a::missileLaunchDuration)
                } else {
                    [a::speed, a::duration, a::durationHighisGood].iter().map(|&x| g(i, x)).find(|&x| x != 0.0).unwrap_or(0.0).max(0.0)
                };
                let yps = if cyc > 0.0 { g(i, a::miningAmount) * self.items[i].quantity as f64 / (cyc / 1000.0) } else { 0.0 };
                d_y += yps;
                d_d += waste(i, yps);
            }
            jv!({"modules_m3_s": m_y, "drones_m3_s": d_y, "total_m3_s": m_y + d_y, "modules_drain_m3_s": m_d, "drones_drain_m3_s": d_d})
        };

        // ---------------- outgoing remote reps / cap transfer (Pyfa Fit.getRemoteReps; contract 1.10 stats-ext `outgoing`)
        let outgoing = {
            let rr_mods: Vec<(usize, u8)> = modules
                .iter()
                .filter(|&&i| active(i))
                .filter_map(|&i| {
                    let k = match d::group_name(self.items[i].group)? {
                        "Remote Shield Booster" | "Ancillary Remote Shield Booster" => 0u8,
                        "Remote Armor Repairer" | "Ancillary Remote Armor Repairer" | "Mutadaptive Remote Armor Repairer" => 1,
                        "Remote Hull Repairer" => 2,
                        "Remote Capacitor Transmitter" => 3,
                        _ => return None,
                    };
                    Some((i, k))
                })
                .collect();
            // per module / drone stack base amounts (shield, armor, hull, cap per cycle) and average cycle
            let drone_rr: [f64; 4] = {
                let mut t = [0.0f64; 4];
                for &i in &drones {
                    let n = self.items[i].active_count as f64;
                    if n <= 0.0 {
                        continue;
                    }
                    let cyc = if self.items[i].charge.is_some() {
                        g(i, a::missileLaunchDuration)
                    } else {
                        [a::speed, a::duration, a::durationHighisGood].iter().map(|&x| g(i, x)).find(|&x| x != 0.0).unwrap_or(0.0)
                    };
                    if cyc == 0.0 {
                        continue;
                    }
                    let f = 1000.0 / cyc;
                    t[0] += g(i, a::shieldBonus) * n * f;
                    t[1] += g(i, a::armorDamageAmount) * n * f;
                    t[2] += g(i, a::structureDamageAmount) * n * f;
                }
                t
            };
            let calc = |forced: Option<Spool>| {
                let mut t = drone_rr;
                for &(i, k) in &rr_mods {
                    let cyc = self.avg_cycle_ms(i, factor_reload);
                    if cyc == 0.0 {
                        continue;
                    }
                    let amount = match k {
                        0 => g(i, a::shieldBonus),
                        1 => {
                            let paste = self.items[i].charge.is_some() && d::group_name(self.items[i].group) == Some("Ancillary Remote Armor Repairer");
                            g(i, a::armorDamageAmount) * if paste { g(i, a::chargedArmorDamageMultiplier) } else { 1.0 }
                        }
                        2 => g(i, a::structureDamageAmount),
                        _ => g(i, a::powerTransferAmount),
                    };
                    let spool = forced.unwrap_or_else(|| self.items[i].spool.unwrap_or(default_spool));
                    let (sp, _, _) = spoolup(g(i, a::repairMultiplierBonusMax), g(i, a::repairMultiplierBonusPerCycle), self.raw_cycle_ms(i) / 1000.0, spool);
                    t[k as usize] += amount * (1.0 + sp) / (cyc / 1000.0);
                }
                jv!({"shield_per_s": t[0], "armor_per_s": t[1], "hull_per_s": t[2], "capacitor_per_s": t[3]})
            };
            jv!({"current": calc(None),
                 "spool_min": calc(Some(Spool { kind: SpoolType::SpoolScale, amount: 0.0 })),
                 "spool_max": calc(Some(Spool { kind: SpoolType::SpoolScale, amount: 1.0 }))})
        };

        // ---------------- bombing (Pyfa bombing panel; contract 1.10 stats-ext `bombing`): bombs to kill this fit
        let bombing = {
            let stat = |t: u32, at: u16| d::type_index(t).and_then(|ix| d::type_attr(ix, at)).unwrap_or(0.0);
            let mut env_mul = 1.0f64;
            for &t in &req.environment.effect_type_ids {
                let n = d::type_name_by_id(t);
                if n.starts_with("Class ") && n.ends_with(" Red Giant Effects") && n.len() == "Class 1 Red Giant Effects".len() {
                    env_mul *= stat(t, a::smartbombDamageMultiplier);
                }
            }
            let sig = g(ship, a::signatureRadius);
            let (hh, ha, hs) = (g(ship, a::hp), g(ship, a::armorHP), g(ship, a::shieldCapacity));
            let mut m = Vec::<(Key, J)>::new();
            for (name, bomb, rh, ra, rs) in [
                ("em", 27920u32, a::emDamageResonance, a::armorEmDamageResonance, a::shieldEmDamageResonance),
                ("thermal", 27916, a::thermalDamageResonance, a::armorThermalDamageResonance, a::shieldThermalDamageResonance),
                ("kinetic", 27912, a::kineticDamageResonance, a::armorKineticDamageResonance, a::shieldKineticDamageResonance),
                ("explosive", 27918, a::explosiveDamageResonance, a::armorExplosiveDamageResonance, a::shieldExplosiveDamageResonance),
            ] {
                let ehp = hh / g(ship, rh) + ha / g(ship, ra) + hs / g(ship, rs);
                let base = stat(bomb, a::emDamage) + stat(bomb, a::thermalDamage) + stat(bomb, a::kineticDamage) + stat(bomb, a::explosiveDamage);
                let bsig = stat(bomb, a::signatureRadius);
                let mut lv = Vec::<(Key, J)>::new();
                for l in 0..=5 {
                    let applied = base * (1.0 + 0.05 * l as f64) * env_mul * (bsig.min(sig) / bsig);
                    lv.push_kv(format!("covert_ops_{l}").into(), jv!((ehp / applied * 10.0).ceil() / 10.0));
                }
                m.push_kv(name.into(), J::O(lv));
            }
            J::O(m)
        };

        let mut out = Vec::<(Key, J)>::new();
        out.push_kv(
            "meta".into(),
            jv!({"schema_version": 1, "engine": concat!("exfa-engine ", env!("CARGO_PKG_VERSION")),
            "sde_build": d::SDE_BUILD, "dataset_sha256": d::DATASET_SHA256}),
        );
        let st = &self.items[ship];
        out.push_kv("ship".into(), jv!({"type_id": st.type_id, "name": d::type_name(st.ty), "group": d::group_name(st.group)}));
        out.push_kv("resources".into(), resources);
        out.push_kv("offense".into(), offense);
        out.push_kv("defense".into(), defense);
        out.push_kv("capacitor".into(), capj);
        out.push_kv("navigation".into(), navigation);
        out.push_kv("targeting".into(), targeting);
        out.push_kv("drones".into(), drones_j);
        out.push_kv("mining".into(), mining);
        out.push_kv("outgoing".into(), outgoing);
        out.push_kv("fighters".into(), fighters_j);
        out.push_kv("bombing".into(), bombing);
        out.push_kv("modules".into(), J::A(module_rows));
        if req.options.validate {
            out.push_kv("violations".into(), J::A(self.validate(
                cpu_used, pg_used, calib_used, bw_used, bay_used, fbay_used, cargo_used,
                [tubes_used as f64, class_used("light"), class_used("support"), class_used("heavy")],
            )));
        }
        out.push_kv(
            "adjustments".into(),
            J::A(self.adjustments.iter().map(|a| jv!({
                "code": a.code, "path": a.path.clone(), "from": a.from.clone(), "to": a.to.clone(), "message": a.message
            })).collect()),
        );
        if !self.warnings.is_empty() {
            out.push_kv("warnings".into(), jv!(self.warnings.clone()));
        }
        if req.options.sources {
            let (src, dep) = self.sources();
            let sl = |v: Vec<String>| J::A(v.into_iter().map(J::Str).collect());
            let sj = src.into_iter().map(|(t, m)| (Key::Owned(t), J::O(m.into_iter().map(|(a, v)| (Key::Owned(a), sl(v))).collect()))).collect();
            out.push_kv("sources".into(), J::O(sj));
            out.push_kv("dependants".into(), J::O(dep.into_iter().map(|(k, v)| (Key::Owned(k), sl(v))).collect()));
        }
        match req.options.include_attributes.as_deref() {
            Some("ship") => {
                out.push_kv("attributes".into(), jv!({"ship": self.dump_attrs(ship)}));
            }
            Some("all") => {
                let mut m = Vec::<(Key, J)>::new();
                m.push_kv("ship".into(), self.dump_attrs(ship));
                m.push_kv("character".into(), self.dump_attrs(ch));
                let mods: Vec<J> = modules
                    .iter()
                    .map(|&i| {
                        jv!({"module_index": self.items[i].req_index, "type_id": self.items[i].type_id, "attributes": self.dump_attrs(i),
                           "charge": self.items[i].charge.map(|c| self.dump_attrs(c))})
                    })
                    .collect();
                m.push_kv("modules".into(), J::A(mods));
                let dr: Vec<J> =
                    drones.iter().map(|&i| jv!({"drone_index": self.items[i].req_index, "attributes": self.dump_attrs(i)})).collect();
                m.push_kv("drones".into(), J::A(dr));
                let fi: Vec<J> =
                    fighters.iter().map(|&i| jv!({"fighter_index": self.items[i].req_index, "attributes": self.dump_attrs(i)})).collect();
                m.push_kv("fighters".into(), J::A(fi));
                out.push_kv("attributes".into(), J::O(m));
            }
            _ => {}
        }
        J::O(out)
    }

    pub fn dump_attrs(&self, i: usize) -> J {
        let mut m = Vec::<(Key, J)>::new();
        for k in self.attr_ids(i) {
            let name: Key = match d::attr_name(k) {
                Some(s) => Key::Borrowed(s),
                None => Key::Owned(k.to_string()),
            };
            m.push_kv(name, jv!(self.get(i, k)));
        }
        J::O(m)
    }

    fn validate(&self, cpu: f64, pg: f64, calib: f64, bw: f64, bay: f64, fbay: f64, cargo: f64, tubes: [f64; 4]) -> Vec<J> {
        let ship = self.ship;
        let g = |i: usize, x: u16| self.get(i, x);
        let mut v = Vec::new();
        let mut push = |code: &'static str, msg: String, idx: Option<usize>| v.push(jv!({"code": code, "message": msg, "module_index": idx}));
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
        if bay > g(ship, a::droneCapacity) + 1e-9 {
            push("DRONE_BAY", format!("Drone bay used {bay:.1} m3 > capacity {:.1}", g(ship, a::droneCapacity)), None);
        }
        if fbay > g(ship, a::fighterCapacity) + 1e-9 {
            push("FIGHTER_BAY", format!("Fighter bay used {fbay:.1} m3 > capacity {:.1}", g(ship, a::fighterCapacity)), None);
        }
        if cargo > g(ship, a::capacity) + 1e-9 {
            push("CARGO_OVERLOAD", format!("Cargo used {cargo:.1} m3 > capacity {:.1}", g(ship, a::capacity)), None);
        }
        for (u, attr, label) in [
            (tubes[0], a::fighterTubes, "fighter"),
            (tubes[1], a::fighterLightSlots, "light fighter"),
            (tubes[2], a::fighterSupportSlots, "support fighter"),
            (tubes[3], a::fighterHeavySlots, "heavy fighter"),
        ] {
            let lim = g(ship, attr);
            if u > lim + 1e-9 {
                push("FIGHTER_TUBES", format!("{label} squads {u} > tubes {lim}"), None);
            }
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
        // totals first: every module of an over-limit group / type is flagged (Pyfa marks all of them)
        for &i in &modules {
            let it = &self.items[i];
            bump(&mut fitted_group, it.group);
            bump(&mut fitted_type, it.type_id);
            if it.state >= State::Online {
                bump(&mut online_group, it.group);
            }
            if it.state >= State::Active {
                bump(&mut active_group, it.group);
            }
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
            } else if !self.is_structure && g(ship, a::isCapitalSize) != 1.0 && g(i, a::volume) >= 4000.0 {
                // capital-size modules (volume >= 4000 m3) only fit capital hulls (Pyfa, GH #1096)
                push("SHIP_RESTRICTION", format!("{name} is a capital-size module; {ship_name} is not a capital ship"), idx);
            }
            if it.slot == Some(Slot::Rig) {
                let rs = ta(a::rigSize).unwrap_or(0.0);
                let srs = g(ship, a::rigSize);
                if rs != 0.0 && rs != srs {
                    push("RIG_SIZE", format!("{name} rig size {rs} != ship rig size {srs}"), idx);
                }
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
            if let Some((lim, n)) = check(a::maxGroupOnline, &online_group, it.group).filter(|_| it.state >= State::Online) {
                push("MAX_GROUP_ONLINE", format!("{name}: {n} online of group, max {lim}"), idx);
            }
            if let Some((lim, n)) = check(a::maxGroupActive, &active_group, it.group).filter(|_| it.state >= State::Active) {
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
        let have: Vec<(u32, f64)> = self.skill_levels.iter().map(|&(s, l)| (s, l.min(5) as f64)).collect();
        // Pyfa checkRequirements: modules (rigs skipped) and their charges, drones, fighters (not their charges),
        // ship, implants, boosters; a missing skill's own missing prerequisites are reported too (recursively).
        let mut missing: Vec<(u32, f64, u32)> = Vec::new();
        fn walk(ty: usize, by: u32, have: &[(u32, f64)], missing: &mut Vec<(u32, f64, u32)>, depth: u32) {
            if depth > 16 {
                return;
            }
            for (sa, la) in SKILL_ATTRS {
                let s = d::type_attr(ty, sa).unwrap_or(0.0) as u32;
                if s == 0 {
                    continue;
                }
                let need = d::type_attr(ty, la).unwrap_or(1.0);
                let lvl = have.binary_search_by_key(&s, |x| x.0).map(|k| have[k].1).unwrap_or(0.0);
                if lvl >= need {
                    continue;
                }
                match missing.iter_mut().find(|m| m.0 == s) {
                    Some(m) if m.1 >= need => continue,
                    Some(m) => m.1 = need,
                    None => missing.push((s, need, by)),
                }
                if let Some(sx) = d::type_index(s) {
                    walk(sx, by, have, missing, depth + 1);
                }
            }
        }
        for it in &self.items {
            let include = match it.kind {
                Kind::Module => it.slot != Some(Slot::Rig),
                Kind::Charge => it.owned && it.parent.map(|p| self.items[p].kind == Kind::Module && self.items[p].slot != Some(Slot::Rig)).unwrap_or(false),
                Kind::Ship | Kind::Drone | Kind::Fighter | Kind::Implant | Kind::Booster => true,
                _ => false,
            };
            if include {
                walk(it.ty, it.type_id, &have, &mut missing, 0);
            }
        }
        for (s, need, by) in missing {
            v.push(jv!({"code": "MISSING_SKILL", "message": format!("{} {} required by {}", d::type_name_by_id(s), need, d::type_name_by_id(by)),
                        "module_index": None::<usize>, "skill_type_id": s, "level": need}));
        }
        v
    }
}

/// (required skill, required level) attribute pairs
const SKILL_ATTRS: [(u16, u16); 6] = [
    (a::requiredSkill1, a::requiredSkill1Level),
    (a::requiredSkill2, a::requiredSkill2Level),
    (a::requiredSkill3, a::requiredSkill3Level),
    (a::requiredSkill4, a::requiredSkill4Level),
    (a::requiredSkill5, a::requiredSkill5Level),
    (a::requiredSkill6, a::requiredSkill6Level),
];

pub use crate::capsim::py_round;
