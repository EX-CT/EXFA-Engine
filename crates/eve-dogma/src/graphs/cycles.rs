//! Cycle schedules (behaviour of Pyfa getCycleParameters / iterCycles, written from its documented semantics):
//! a module runs `active` ms, then waits `inactive` ms (reactivation delay or reload).
use crate::data as d;
use crate::engine::Fit;

#[derive(Clone, Copy, Debug)]
pub struct Cyc {
    pub active: f64,
    pub inactive: f64,
    pub qty: f64,
    pub reload: bool,
}

#[derive(Clone, Debug)]
pub struct Schedule {
    /// sequence of cycle groups, repeated `repeat` times (f64::INFINITY = forever)
    pub seq: Vec<Cyc>,
    pub repeat: f64,
}

impl Schedule {
    fn single(c: Cyc) -> Self {
        Schedule { seq: vec![c], repeat: 1.0 }
    }
    pub fn average_ms(&self) -> f64 {
        let (mut t, mut n) = (0.0, 0.0);
        for c in &self.seq {
            if c.qty.is_infinite() {
                return c.active + c.inactive;
            }
            t += (c.active + c.inactive) * c.qty;
            n += c.qty;
        }
        if n > 0.0 { t / n } else { 0.0 }
    }
    /// Iterate cycles as (active ms, inactive ms, inactivity is a reload).
    pub fn iter(&self) -> impl Iterator<Item = (f64, f64, bool)> + '_ {
        let rep = self.repeat;
        (0u64..).take_while(move |r| (*r as f64) < rep).flat_map(move |_| {
            self.seq.iter().flat_map(|c| (0u64..).take_while(move |k| (*k as f64) < c.qty).map(move |_| (c.active, c.inactive, c.reload)))
        })
    }
}

fn attr(fit: &Fit, i: usize, n: &str) -> f64 {
    an(n).map(|a| fit.get(i, a)).unwrap_or(0.0)
}

/// Module schedule. `reload_override` None = owner's factor_reload (forced on for capacitor boosters).
pub fn module(fit: &Fit, i: usize, reload_override: Option<bool>, owner_factor_reload: bool) -> Option<Schedule> {
    let group = d::group_name(fit.items[i].group).unwrap_or("");
    let factor_reload = reload_override.unwrap_or(if group == "Capacitor Booster" { true } else { owner_factor_reload });
    let shots = fit.num_shots(i) as f64;
    let until_reload = if shots == 0.0 { f64::INFINITY } else { shots };
    let active = fit.raw_cycle_ms(i);
    if active == 0.0 {
        return None;
    }
    let forced = attr(fit, i, "moduleReactivationDelay");
    let reload = attr(fit, i, "reloadTime");
    if !factor_reload || until_reload.is_infinite() || forced >= reload {
        let inact_reload = factor_reload && forced >= reload;
        return Some(Schedule::single(Cyc { active, inactive: forced, qty: f64::INFINITY, reload: inact_reload }));
    }
    let early = until_reload - 1.0;
    if early == 0.0 {
        return Some(Schedule::single(Cyc { active, inactive: reload, qty: f64::INFINITY, reload: true }));
    }
    Some(Schedule {
        seq: vec![
            Cyc { active, inactive: forced, qty: early, reload: false },
            Cyc { active, inactive: reload, qty: 1.0, reload: true },
        ],
        repeat: f64::INFINITY,
    })
}

/// Drone schedule (constant cycle, Pyfa Drone.cycleTime: missileLaunchDuration with ammo, else speed/duration).
pub fn drone_cycle_ms(fit: &Fit, i: usize) -> f64 {
    let mut c = 0.0;
    for n in ["speed", "duration", "durationHighisGood"] {
        c = attr(fit, i, n);
        if c != 0.0 {
            break;
        }
    }
    c.max(0.0)
}

pub fn drone(fit: &Fit, i: usize) -> Option<Schedule> {
    let c = drone_cycle_ms(fit, i);
    if c == 0.0 {
        return None;
    }
    Some(Schedule::single(Cyc { active: c, inactive: 0.0, qty: f64::INFINITY, reload: false }))
}

/// Fighter ability (all abilities of the squadron take part in refuel planning, active or not).
#[derive(Clone, Debug)]
pub struct Ability {
    pub eid: u32,
    pub prefix: &'static str,
    pub cycle: f64,
    pub shots: f64,
    pub has_charges: bool,
}

pub const FIGHTER_ABILITIES: [(&str, &str, bool); 8] = [
    ("fighterAbilityMissiles", "fighterAbilityMissiles", true),
    ("fighterAbilityEnergyNeutralizer", "fighterAbilityEnergyNeutralizer", false),
    ("fighterAbilityStasisWebifier", "fighterAbilityStasisWebifier", false),
    ("fighterAbilityWarpDisruption", "fighterAbilityWarpDisruption", false),
    ("fighterAbilityECM", "fighterAbilityECM", false),
    ("fighterAbilityEvasiveManeuvers", "fighterAbilityEvasiveManeuvers", false),
    ("fighterAbilityAttackM", "fighterAbilityAttackMissile", false),
    ("fighterAbilityLaunchBomb", "fighterAbilityLaunchBomb", true),
];

pub fn fighter_abilities(fit: &Fit, i: usize) -> Vec<Ability> {
    let role = attr(fit, i, "fighterSquadronRole") as i64;
    let shots_role = match role {
        2 => 12.0,
        4 => 6.0,
        5 => 3.0,
        _ => 0.0,
    };
    let mut v = Vec::new();
    for (ename, prefix, charges) in FIGHTER_ABILITIES {
        let Some(eid) = fit.items[i].effects().find(|(ei, _)| d::eff_name(*ei) == ename).map(|(ei, _)| d::EFF_IDS[ei]) else { continue };
        v.push(Ability { eid, prefix, cycle: attr(fit, i, &format!("{prefix}Duration")), shots: if charges { shots_role } else { 0.0 }, has_charges: charges });
    }
    v
}

fn fighter_reload(fit: &Fit, i: usize, ab: &Ability, spent: f64) -> f64 {
    let rearm = match attr(fit, i, "fighterSquadronRole") as i64 {
        2 => 4000.0,
        4 => 6000.0,
        5 => 20000.0,
        _ => 0.0,
    };
    attr(fit, i, "fighterRefuelingTime") + if ab.has_charges { rearm * spent.max(ab.shots) } else { 0.0 }
}

/// Per-ability schedules with refuels (Pyfa getCycleParametersPerEffect semantics).
pub fn fighter_with_reload(fit: &Fit, i: usize, abs: &[Ability], factor_reload: bool) -> Vec<(u32, Schedule)> {
    let inf = |a: &Ability| (a.eid, Schedule::single(Cyc { active: a.cycle, inactive: 0.0, qty: f64::INFINITY, reload: false }));
    let valid: Vec<&Ability> = abs.iter().filter(|a| a.cycle > 0.0).collect();
    let limited: Vec<&Ability> = valid.iter().copied().filter(|a| a.shots > 0.0).collect();
    if !factor_reload || limited.is_empty() {
        return valid.iter().map(|a| inf(a)).collect();
    }
    let ml = limited.iter().copied().min_by(|x, y| (x.cycle * x.shots).partial_cmp(&(y.cycle * y.shots)).unwrap()).unwrap();
    let to_refuel = ml.cycle * ml.shots;
    let plan: Vec<(f64, Option<f64>)> = valid
        .iter()
        .map(|a| {
            if a.eid == ml.eid {
                (ml.shots, None)
            } else {
                let full = float_unerr(to_refuel / a.cycle).trunc();
                let extra = float_unerr(to_refuel - full * a.cycle);
                (full, if extra == 0.0 { None } else { Some(extra) })
            }
        })
        .collect();
    let refuel = valid
        .iter()
        .zip(&plan)
        .map(|(a, (n, ex))| fighter_reload(fit, i, a, n + ex.is_some() as u8 as f64))
        .fold(f64::MIN, f64::max);
    valid
        .iter()
        .zip(&plan)
        .map(|(a, (n, ex))| {
            let mut seq = Vec::new();
            match ex {
                Some(extra) => {
                    if *n > 0.0 {
                        seq.push(Cyc { active: a.cycle, inactive: 0.0, qty: *n, reload: false });
                    }
                    seq.push(Cyc { active: *extra, inactive: refuel, qty: 1.0, reload: true });
                }
                None => {
                    if n - 1.0 > 0.0 {
                        seq.push(Cyc { active: a.cycle, inactive: 0.0, qty: n - 1.0, reload: false });
                    }
                    seq.push(Cyc { active: a.cycle, inactive: refuel, qty: 1.0, reload: true });
                }
            }
            (a.eid, Schedule { seq, repeat: f64::INFINITY })
        })
        .collect()
}

pub fn fighter_infinite(abs: &[Ability]) -> Vec<(u32, Schedule)> {
    abs.iter()
        .filter(|a| a.shots == 0.0 && a.cycle > 0.0)
        .map(|a| (a.eid, Schedule::single(Cyc { active: a.cycle, inactive: 0.0, qty: f64::INFINITY, reload: false })))
        .collect()
}

/// Pyfa `floatUnerr` (7 significant digits kept).
pub fn float_unerr(x: f64) -> f64 {
    if x == 0.0 || x.is_infinite() {
        return x;
    }
    let k = 7 - x.abs().log10().ceil() as i32;
    if k >= 0 {
        format!("{:.*}", k as usize, x).parse().unwrap_or(x)
    } else {
        let p = 10f64.powi(-k);
        (x / p).round_ties_even() * p
    }
}

/// Attribute id by name, memoised (the data table lookup is a linear scan).
pub fn an(name: &str) -> Option<u16> {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static M: OnceLock<Mutex<HashMap<String, Option<u16>>>> = OnceLock::new();
    let m = M.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(v) = m.lock().unwrap().get(name) {
        return *v;
    }
    let v = d::attr_by_name(name);
    m.lock().unwrap().insert(name.to_string(), v);
    v
}
