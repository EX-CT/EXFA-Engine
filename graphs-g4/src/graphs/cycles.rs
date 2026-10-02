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
    d::attr_by_name(n).map(|a| fit.get(i, a)).unwrap_or(0.0)
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
