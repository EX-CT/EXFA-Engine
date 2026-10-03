//! Remote repairs kernel: per-source rep amounts (shield / armor / hull), either as a steady rate (time unset) or
//! from a cycle-by-cycle time line (time set), multiplied by a per-source application factor at a distance.
use super::cycles::{self, Schedule};
use super::Ctx;
use crate::data as d;
use super::cycles::float_unerr;
use crate::engine::{Fit, Kind};
use crate::request::{Spool, SpoolType, State};
use crate::stats::{range_factor, spoolup};

/// rep amount landing `delay_ms` after cycle start: [shield, armor, hull]
type Amounts = Vec<(f64, [f64; 3])>;

const GROUPS: [(&str, usize); 6] = [
    ("Remote Armor Repairer", 1),
    ("Ancillary Remote Armor Repairer", 1),
    ("Mutadaptive Remote Armor Repairer", 1),
    ("Remote Hull Repairer", 2),
    ("Remote Shield Booster", 0),
    ("Ancillary Remote Shield Booster", 0),
];

fn attr(fit: &Fit, i: usize, n: &str) -> f64 {
    super::cycles::an(n).map(|a| fit.get(i, a)).unwrap_or(0.0)
}
fn has_eff(fit: &Fit, i: usize, n: &str) -> bool {
    fit.items[i].effects().any(|(ei, _)| d::eff_name(ei) == n)
}

struct Src {
    i: usize,
    drone: bool,
    anc: bool,
    anc_armor: bool,
    base: Amounts,
}

fn sources(fit: &Fit) -> Vec<Src> {
    let mut v = Vec::new();
    for i in 0..fit.items.len() {
        let it = &fit.items[i];
        if it.kind == Kind::Module && it.state >= State::Active {
            let g = d::group_name(it.group).unwrap_or("");
            let Some(&(_, ty)) = GROUPS.iter().find(|(n, _)| *n == g) else { continue };
            let mut amt = [0.0; 3];
            amt[ty] = match ty {
                0 => attr(fit, i, "shieldBonus"),
                2 => attr(fit, i, "structureDamageAmount"),
                _ => {
                    let m = if g == "Ancillary Remote Armor Repairer" && it.charge.is_some() {
                        super::cycles::an("chargedArmorDamageMultiplier").filter(|&a| fit.has(i, a)).map(|a| fit.get(i, a)).unwrap_or(1.0)
                    } else {
                        1.0
                    };
                    attr(fit, i, "armorDamageAmount") * m
                }
            };
            if amt.iter().all(|x| *x == 0.0) {
                continue;
            }
            let delay = if ty == 0 { 0.0 } else { fit.raw_cycle_ms(i) };
            let anc_s = has_eff(fit, i, "shipModuleAncillaryRemoteShieldBooster");
            let anc_a = has_eff(fit, i, "shipModuleAncillaryRemoteArmorRepairer");
            v.push(Src { i, drone: false, anc: anc_s || anc_a, anc_armor: anc_a, base: vec![(delay, amt)] });
        } else if it.kind == Kind::Drone && it.active_count > 0 {
            let n = it.active_count as f64;
            let (s, ar, h) = (attr(fit, i, "shieldBonus"), attr(fit, i, "armorDamageAmount"), attr(fit, i, "structureDamageAmount"));
            let mut base = Vec::new();
            if s != 0.0 {
                base.push((0.0, [s * n, 0.0, 0.0]));
            }
            if ar != 0.0 || h != 0.0 {
                base.push((cycles::drone_cycle_ms(fit, i), [0.0, ar * n, h * n]));
            }
            if !base.is_empty() {
                v.push(Src { i, drone: true, anc: false, anc_armor: false, base });
            }
        }
    }
    v
}

fn spool_mult(fit: &Fit, i: usize, spool: Spool) -> f64 {
    1.0 + spoolup(attr(fit, i, "repairMultiplierBonusMax"), attr(fit, i, "repairMultiplierBonusPerCycle"), fit.raw_cycle_ms(i) / 1000.0, spool).0
}

fn schedule(fit: &Fit, s: &Src, anc_reload: bool, owner_reload: bool, time_line: bool) -> Option<Schedule> {
    if s.drone {
        cycles::drone(fit, s.i)
    } else if s.anc {
        cycles::module(fit, s.i, Some(anc_reload), owner_reload)
    } else {
        cycles::module(fit, s.i, if time_line { Some(true) } else { None }, owner_reload)
    }
}

fn application(ctx: &Ctx, s: &Src, dist: Option<f64>) -> f64 {
    let in_lock = match dist {
        None => true,
        Some(x) => ctx.setting_b("ignore_lock_range", true) || x <= ctx.stat("targeting.max_range_m").unwrap_or(0.0),
    };
    let in_dcr = match dist {
        None => true,
        Some(x) => ctx.setting_b("ignore_drone_control_range", false) || x <= ctx.stat("drones.control_range_m").unwrap_or(0.0),
    };
    let v = if s.drone {
        (in_lock && in_dcr) as u8 as f64
    } else if !in_lock {
        0.0
    } else {
        let (o, f) = super::kernels::module_range(&ctx.fit, s.i);
        range_factor(o, f, dist, true)
    };
    float_unerr(v)
}

/// mode 0 = rps, 1 = total repaired; `time` None = steady state (only valid for rps).
pub fn rr(ctx: &Ctx, mode: u8, time: Option<f64>, dist: Option<f64>) -> Option<f64> {
    let fit = &ctx.fit;
    let anc_reload = ctx.param_f("anc_reload").map(|v| v != 0.0).unwrap_or(true);
    let owner_reload = ctx.fit_req.options.factor_reload;
    let srcs = sources(fit);
    let mut total = 0.0;
    match time {
        None => {
            if mode == 1 {
                return None;
            }
            let def = ctx.fit_req.options.default_spool.unwrap_or(Spool { kind: SpoolType::SpoolScale, amount: 1.0 });
            for s in &srcs {
                let Some(sch) = schedule(fit, s, anc_reload, owner_reload, false) else { continue };
                let avg = sch.average_ms();
                if avg == 0.0 {
                    continue;
                }
                let m = if s.drone { 1.0 } else { spool_mult(fit, s.i, fit.items[s.i].spool.unwrap_or(def)) };
                let amt: f64 = s.base.iter().map(|(_, a)| a.iter().sum::<f64>() * m).sum();
                total += amt / (avg / 1000.0) * application(ctx, s, dist);
            }
        }
        Some(t) => {
            let tu = float_unerr(t);
            for s in &srcs {
                let Some(sch) = schedule(fit, s, anc_reload, owner_reload, true) else { continue };
                let shots = if s.drone { 0.0 } else { fit.num_shots(s.i) as f64 };
                let charged_mult = super::cycles::an("chargedArmorDamageMultiplier").filter(|&a| fit.has(s.i, a)).map(|a| fit.get(s.i, a)).unwrap_or(1.0);
                let reduced = s.anc_armor && fit.items[s.i].charge.is_some() && !anc_reload;
                let (mut now, mut nonstop, mut since_reload) = (0.0f64, 0.0f64, 0.0f64);
                // rps: value of the last cycle segment starting at or before t (0 in gaps); total: amounts landed <= t
                let mut rps_at: Option<f64> = None;
                let mut amount = 0.0;
                for (act, inact, is_reload) in sch.iter() {
                    since_reload += 1.0;
                    let m = if s.drone { 1.0 } else { spool_mult(fit, s.i, Spool { kind: SpoolType::Cycles, amount: nonstop }) };
                    let mut cyc_sum = 0.0;
                    for (delay, a) in &s.base {
                        let mut v: f64 = a.iter().sum::<f64>() * m;
                        if reduced && since_reload > shots {
                            v /= charged_mult;
                        }
                        cyc_sum += v;
                        if v > 0.0 && float_unerr(now + delay / 1000.0) <= tu {
                            amount += v;
                        }
                    }
                    let end = now + act / 1000.0;
                    if float_unerr(now) <= tu && cyc_sum > 0.0 {
                        rps_at = Some(if float_unerr(end) <= tu { 0.0 } else { cyc_sum / (act / 1000.0) });
                    }
                    nonstop = if inact > 0.0 { 0.0 } else { nonstop + 1.0 };
                    if is_reload {
                        since_reload = 0.0;
                    }
                    if now > t {
                        break;
                    }
                    now += act / 1000.0 + inact / 1000.0;
                }
                let app = application(ctx, s, dist);
                total += app * if mode == 0 { rps_at.unwrap_or(0.0) } else { amount };
            }
        }
    }
    // target fit (contract 0.2): remote repair impedance, no assistance to ships that disallow it
    if let Ok(Some(t)) = super::kernels::target_ship_attrs(ctx) {
        if t.attr("disallowAssistance") != 0.0 {
            return Some(0.0);
        }
        total *= t.attr("remoteRepairImpedance");
    }
    Some(total)
}
