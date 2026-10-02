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
