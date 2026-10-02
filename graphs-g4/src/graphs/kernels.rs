//! Kernels: the hand-written parts of the graph catalogue (called from `graphs.json` formulas).
use super::Ctx;
use crate::capsim::{self, Drain};
use crate::data as d;
use crate::request::State;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Caches {
    pub drains: Vec<Drain>,
    pub subwarp: Option<f64>,
    /// capsim history per starting cap (bits): sorted (t s, cap)
    pub cap_hist: Vec<(u64, Vec<(f64, f64)>)>,
    pub extra_series: Vec<(String, Vec<Value>)>,
}
impl Caches {
    pub fn point_clear(&mut self) {}
}

pub fn call(ctx: &mut Ctx, name: &str, a: &[Option<f64>]) -> Result<Option<f64>, String> {
    Ok(match name {
        "subwarp_speed" => Some(subwarp_speed(ctx)),
        "capsim_cap" => match (a.first().copied().flatten(), a.get(1).copied().flatten()) {
            (Some(t), Some(c0)) => capsim_cap(ctx, t, c0),
            _ => None,
        },
        "shield_ehp_mult" => {
            let hp = ctx.stat("defense.hp.shield").unwrap_or(0.0);
            let ehp = ctx.stat("defense.ehp.shield").unwrap_or(0.0);
            Some(if hp > 0.0 { ehp / hp } else { 1.0 })
        }
        "rr_rps" => super::rr::rr(ctx, 0, a.first().copied().flatten(), a.get(1).copied().flatten()),
        "rr_total" => super::rr::rr(ctx, 1, a.first().copied().flatten(), a.get(1).copied().flatten()),
        n if n.starts_with("sum_sources_") => Some(sources(ctx, &n[12..], false)?),
        n if n.starts_with("stack_sources_") => Some(sources(ctx, &n[14..], true)?),
        _ => return Err(format!("unknown kernel '{name}'")),
    })
}

/// Max velocity with speed-changing modules that cannot run in warp set to online and projections switched off.
fn subwarp_speed(ctx: &mut Ctx) -> f64 {
    if let Some(v) = ctx.k.subwarp {
        return v;
    }
    const GROUPS: [&str; 8] = [
        "Propulsion Module",
        "Mass Entanglers",
        "Cloaking Device",
        "Siege Module",
        "Super Weapon",
        "Cynosural Field Generator",
        "Clone Vat Bay",
        "Jump Portal Generator",
    ];
    let mut r = ctx.fit_req.clone();
    for m in r.modules.iter_mut() {
        let g = d::type_index(m.type_id).map(|i| d::ty(i).group).and_then(d::group_name).unwrap_or("");
        if GROUPS.contains(&g) && matches!(m.state, Some(State::Active) | Some(State::Overheated)) {
            m.state = Some(State::Online);
        }
    }
    r.projected.clear();
    let v = crate::calc(&r).to_value_raw().pointer("/navigation/max_velocity").and_then(|v| v.as_f64()).unwrap_or(0.0);
    ctx.k.subwarp = Some(v);
    v
}

/// Capacitor at time t (s) from the simulation history (Pyfa getCapSimData: t_max 3600 s, no repeat optimisation,
/// stagger on, reload = factor_reload): last recorded point <= t advanced by passive regen; None once the
/// simulation has ended and t is past its last point. Without drains: passive regen only.
fn capsim_cap(ctx: &mut Ctx, t: f64, c0: f64) -> Option<f64> {
    let cmax = ctx.ship_attr("capacitorCapacity");
    let tau = ctx.ship_attr("rechargeRate") / 1000.0;
    let regen = |c_start: f64, dt: f64| cmax * (1.0 + (-5.0 * dt / tau).exp() * ((c_start / cmax).sqrt() - 1.0)).powi(2);
    if ctx.k.drains.is_empty() {
        return Some(regen(c0, t));
    }
    let key = c0.to_bits();
    if !ctx.k.cap_hist.iter().any(|(k, _)| *k == key) {
        let mut h = BTreeMap::new();
        let reload = ctx.fit_req.options.factor_reload;
        let drains = ctx.k.drains.clone();
        capsim::simulate_ex(cmax, tau * 1000.0, &drains, if cmax > 0.0 { c0 / cmax } else { 0.0 }, reload, true, 3600.0 * 1000.0, false, Some(&mut h));
        let v: Vec<(f64, f64)> = h.into_iter().map(|(k, c)| (f64::from_bits(k) / 1000.0, c.max(0.0))).collect();
        ctx.k.cap_hist.push((key, v));
    }
    let hist = &ctx.k.cap_hist.iter().find(|(k, _)| *k == key).unwrap().1;
    if hist.is_empty() {
        return Some(regen(c0, t));
    }
    let last_t = hist.last().unwrap().0;
    let before = hist.iter().rev().find(|(ht, _)| *ht <= t);
    match before {
        Some((bt, _)) if *bt == last_t => None,
        Some((bt, bc)) => Some(if *bt == t { *bc } else { regen(*bc, t - bt) }),
        None => Some(regen(c0, t)),
    }
}

/// Pyfa stacking penalty over one group of multipliers (bonuses and penalties separately, strongest first).
pub fn stack_mult(mults: &[f64]) -> f64 {
    let mut val = 1.0;
    let mut up: Vec<f64> = mults.iter().cloned().filter(|m| *m > 1.0).collect();
    let mut down: Vec<f64> = mults.iter().cloned().filter(|m| *m < 1.0).collect();
    for l in [&mut up, &mut down] {
        l.sort_by(|a, b| (b - 1.0).abs().partial_cmp(&(a - 1.0).abs()).unwrap());
        for (i, m) in l.iter().enumerate() {
            val *= 1.0 + (m - 1.0) * (-((i * i) as f64) / 7.1289).exp();
        }
    }
    val
}

/// Pyfa module optimal range / falloff (first non-zero of the range-like / falloff-like attributes).
pub fn module_range(fit: &crate::engine::Fit, i: usize) -> (f64, f64) {
    let g = |n: &str| d::attr_by_name(n).map(|a| fit.get(i, a)).unwrap_or(0.0);
    let mut opt = 0.0;
    for n in ["maxRange", "shieldTransferRange", "powerTransferRange", "energyDestabilizationRange", "empFieldRange", "ecmBurstRange", "warpScrambleRange", "cargoScanRange", "shipScanRange", "surveyScanRange"] {
        opt = g(n);
        if opt != 0.0 {
            break;
        }
    }
    if opt != 0.0 && d::type_name(fit.items[i].ty).to_lowercase().contains("burst projector") {
        opt -= fit.get(fit.ship, d::attr_by_name("radius").unwrap_or(0));
    }
    let mut fo = 0.0;
    for n in ["falloffEffectiveness", "falloff", "shipScanFalloff"] {
        fo = g(n);
        if fo != 0.0 {
            break;
        }
    }
    (opt, fo)
}

pub fn has_effect_name(fit: &crate::engine::Fit, i: usize, names: &[String]) -> Vec<String> {
    fit.items[i].effects().map(|(ei, _)| d::eff_name(ei)).filter(|n| names.iter().any(|x| x == n)).map(String::from).collect()
}

/// Expression environment of one item (a.<attr>, cycle_s, squadron).
pub struct ItemEnv<'a> {
    pub fit: &'a crate::engine::Fit,
    pub i: usize,
    pub cycle_s: f64,
    pub squadron: f64,
}
impl super::expr::Env for ItemEnv<'_> {
    fn var(&mut self, name: &str) -> Result<Option<f64>, String> {
        if let Some(a) = name.strip_prefix("a.") {
            return Ok(Some(d::attr_by_name(a).map(|x| self.fit.get(self.i, x)).unwrap_or(0.0)));
        }
        match name {
            "cycle_s" => Ok(Some(self.cycle_s)),
            "squadron" => Ok(Some(self.squadron)),
            _ => Err(format!("unknown item name '{name}'")),
        }
    }
    fn call(&mut self, name: &str, _a: &[Option<f64>]) -> Result<Option<f64>, String> {
        Err(format!("unknown item kernel '{name}'"))
    }
}

/// EWAR-style source tables: sum (or stacking-penalised product) of strength x range factor at distance x.
fn sources(ctx: &mut Ctx, table: &str, stack: bool) -> Result<f64, String> {
    use crate::engine::Kind;
    let Some(list) = super::spec().tables.get(table) else { return Err(format!("unknown source table {table}")) };
    let dist = ctx.x;
    let resonance = 1.0 - ctx.param_f("resist").unwrap_or(0.0);
    let in_lock = ctx.setting_b("ignore_lock_range", true) || dist <= ctx.stat("targeting.max_range_m").unwrap_or(0.0);
    let in_dcr = ctx.setting_b("ignore_drone_control_range", false) || dist <= ctx.stat("drones.control_range_m").unwrap_or(0.0);
    let factor_reload = ctx.fit_req.options.factor_reload;
    let fit = &ctx.fit;
    let mut sum = 0.0;
    let mut mults = Vec::new();
    for i in 0..fit.items.len() {
        let it = &fit.items[i];
        let (is_mod, is_drone) = (it.kind == Kind::Module, it.kind == Kind::Drone);
        if !(is_mod && it.state >= State::Active) && !(is_drone && it.active_count > 0) {
            continue;
        }
        for src in list {
            if (src.from == "module") != is_mod || (src.from == "drone") != is_drone {
                continue;
            }
            let n = has_effect_name(fit, i, &src.effects).len();
            if n == 0 {
                continue;
            }
            if (src.lock && !in_lock) || (src.dcr && !in_dcr) {
                continue;
            }
            let cyc = fit.avg_cycle_ms(i, factor_reload) / 1000.0;
            let mut env = ItemEnv { fit, i, cycle_s: if cyc > 0.0 { cyc } else { f64::INFINITY }, squadron: it.quantity as f64 };
            if let Some(w) = &src.when {
                if !matches!(super::expr::eval(w, &mut env)?, Some(v) if v != 0.0) {
                    continue;
                }
            }
            let strength = super::expr::eval(&src.strength, &mut env)?.unwrap_or(0.0) * resonance;
            let (opt, fo) = match src.range.as_str() {
                "inf" => (f64::INFINITY, 0.0),
                "doomsday" => {
                    let (o, f) = module_range(fit, i);
                    ((o + d::attr_by_name("doomsdayAOERange").map(|a| fit.get(i, a)).unwrap_or(0.0)).max(0.0), f)
                }
                _ => module_range(fit, i),
            };
            let rf = crate::stats::range_factor(opt, fo, Some(dist), true);
            let copies = if is_drone { it.active_count as usize } else { n };
            for _ in 0..copies {
                if stack {
                    mults.push(1.0 + strength * rf / 100.0);
                } else {
                    sum += strength * rf;
                }
            }
        }
    }
    Ok(if stack { stack_mult(&mults) } else { sum })
}
