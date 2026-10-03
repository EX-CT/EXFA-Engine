//! Damage kernel: damage dealers (modules, drones, fighter abilities) with per-type volley / dps, either steady
//! state (time unset) or from a cycle-by-cycle time line (time set), times an application factor per dealer
//! (turret chance to hit, missile explosion formula, smartbomb / bomb / doomsday range rules), against a target
//! profile or target fit, after the source's own webs / target painters are applied to the target.
use super::cycles::{self, Schedule};
use super::Ctx;
use crate::data as d;
use crate::eft::float_unerr;
use crate::engine::{Fit, Kind};
use crate::request::{Spool, SpoolType, State};
use crate::stats::{range_factor, spoolup};
use serde_json::Value;

/// em, thermal, kinetic, explosive, pure (breacher)
pub type D = [f64; 5];

fn attr(fit: &Fit, i: usize, n: &str) -> f64 {
    super::cycles::an(n).map(|a| fit.get(i, a)).unwrap_or(0.0)
}
fn attr_opt(fit: &Fit, i: usize, n: &str) -> Option<f64> {
    super::cycles::an(n).filter(|&a| fit.has(i, a)).map(|a| fit.get(i, a))
}
fn has_eff(fit: &Fit, i: usize, n: &str) -> bool {
    fit.items[i].effects().any(|(ei, _)| d::eff_name(ei) == n)
}
fn group(fit: &Fit, i: usize) -> &'static str {
    d::group_name(fit.items[i].group).unwrap_or("")
}
fn scale(v: &D, k: f64) -> D {
    [v[0] * k, v[1] * k, v[2] * k, v[3] * k, v[4] * k]
}
fn add(a: &mut D, b: &D) {
    for k in 0..5 {
        a[k] += b[k];
    }
}
fn tot(v: &D) -> f64 {
    v.iter().sum()
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum K {
    Turret,
    Missile,
    Vorton,
    Smartbomb,
    Bomb,
    GuidedBomb,
    Doomsday,
    Breacher,
    Other,
    Drone,
    Fighter(&'static str),
}

pub struct Dealer {
    pub k: K,
    pub i: usize,
    pub eid: u32,
    /// volleys landing `delay` ms after cycle start (before spool)
    pub base: Vec<(f64, D)>,
}

fn module_kind(fit: &Fit, i: usize) -> K {
    let g = group(fit, i);
    let charge_breacher = fit.items[i].charge.map(|c| has_eff(fit, c, "dotMissileLaunching")).unwrap_or(false);
    if has_eff(fit, i, "ChainLightning") {
        K::Vorton
    } else if has_eff(fit, i, "turretFitted") {
        K::Turret
    } else if charge_breacher {
        K::Breacher
    } else if has_eff(fit, i, "launcherFitted") || fit.items[i].type_id == 32461 {
        K::Missile
    } else if g == "Smart Bomb" || g == "Structure Area Denial Module" {
        K::Smartbomb
    } else if g == "Missile Launcher Bomb" {
        K::Bomb
    } else if g == "Structure Guided Bomb Launcher" {
        K::GuidedBomb
    } else if g == "Super Weapon" || g == "Structure Doomsday Weapon" {
        K::Doomsday
    } else {
        K::Other
    }
}

const SINGLE_DD: [&str; 5] = ["superWeaponAmarr", "superWeaponCaldari", "superWeaponGallente", "superWeaponMinmatar", "lightningWeapon"];

pub fn dealers(fit: &Fit) -> Vec<Dealer> {
    let mut v = Vec::new();
    for i in 0..fit.items.len() {
        let it = &fit.items[i];
        match it.kind {
            Kind::Module if it.state >= State::Active => {
                let k = module_kind(fit, i);
                if k == K::Breacher {
                    let c = it.charge.unwrap();
                    let n = (attr(fit, c, "dotDuration") / 1000.0).floor();
                    let tick = attr(fit, c, "dotMaxDamagePerTick");
                    let base: Vec<(f64, D)> = (0..n as i64).map(|k| (1000.0 * (1 + k) as f64, [0.0, 0.0, 0.0, 0.0, tick])).collect();
                    if tick > 0.0 {
                        v.push(Dealer { k, i, eid: 0, base });
                    }
                    continue;
                }
                let src = it.charge.unwrap_or(i);
                let mut mult = attr_opt(fit, i, "damageMultiplier").unwrap_or(1.0);
                if k == K::Missile && it.charge.is_some() {
                    mult *= attr(fit, fit.char, "missileDamageMultiplier");
                }
                let vol: D = [
                    attr(fit, src, "emDamage") * mult,
                    attr(fit, src, "thermalDamage") * mult,
                    attr(fit, src, "kineticDamage") * mult,
                    attr(fit, src, "explosiveDamage") * mult,
                    0.0,
                ];
                if tot(&vol) == 0.0 {
                    continue;
                }
                let delay = if SINGLE_DD.iter().any(|e| has_eff(fit, i, e)) {
                    attr(fit, i, "damageDelayDuration")
                } else if ["doomsdayBeamDOT", "doomsdaySlash", "doomsdayConeDOT", "debuffLance"].iter().any(|e| has_eff(fit, i, e)) {
                    attr(fit, i, "doomsdayWarningDuration")
                } else {
                    0.0
                };
                let (dd, dc) = (attr(fit, i, "doomsdayDamageDuration"), attr(fit, i, "doomsdayDamageCycleTime"));
                let sub = if dd != 0.0 && dc != 0.0 && !has_eff(fit, i, "doomsdaySlash") { float_unerr(dd / dc).floor() } else { 1.0 };
                let base = (0..sub as i64).map(|k| (delay + dc * k as f64, vol)).collect();
                v.push(Dealer { k, i, eid: 0, base });
            }
            Kind::Drone if it.active_count > 0 => {
                let n = it.active_count as f64;
                let mult = attr_opt(fit, i, "damageMultiplier").unwrap_or(1.0) * n;
                let vol: D = [attr(fit, i, "emDamage") * mult, attr(fit, i, "thermalDamage") * mult, attr(fit, i, "kineticDamage") * mult, attr(fit, i, "explosiveDamage") * mult, 0.0];
                if tot(&vol) == 0.0 || drone_cycle(fit, i) == 0.0 {
                    continue;
                }
                v.push(Dealer { k: K::Drone, i, eid: 0, base: vec![(0.0, vol)] });
            }
            Kind::Fighter if it.active_count > 0 => {
                for ab in cycles::fighter_abilities(fit, i) {
                    let mattr = format!("{}DamageMultiplier", ab.prefix);
                    let deals = super::cycles::an(&mattr).map(|a| fit.has(i, a)).unwrap_or(false);
                    if !deals || !it.fighter_abilities.contains(&ab.eid) || ab.prefix == "fighterAbilityLaunchBomb" {
                        continue;
                    }
                    let m = it.active_count as f64 * attr_opt(fit, i, &mattr).unwrap_or(1.0);
                    let p = ab.prefix;
                    let vol: D = [
                        attr(fit, i, &format!("{p}DamageEM")) * m,
                        attr(fit, i, &format!("{p}DamageTherm")) * m,
                        attr(fit, i, &format!("{p}DamageKin")) * m,
                        attr(fit, i, &format!("{p}DamageExp")) * m,
                        0.0,
                    ];
                    v.push(Dealer { k: K::Fighter(p), i, eid: ab.eid, base: vec![(0.0, vol)] });
                }
            }
            _ => {}
        }
    }
    v
}

fn drone_cycle(fit: &Fit, i: usize) -> f64 {
    if super::cycles::an("entityMissileTypeID").map(|a| fit.has(i, a)).unwrap_or(false) {
        fit.raw_cycle_ms(i)
    } else {
        cycles::drone_cycle_ms(fit, i)
    }
}

fn spool_mult(fit: &Fit, i: usize, spool: Spool) -> f64 {
    1.0 + spoolup(attr(fit, i, "damageMultiplierBonusMax"), attr(fit, i, "damageMultiplierBonusPerCycle"), fit.raw_cycle_ms(i) / 1000.0, spool).0
}

/// Fighter ability volley (whole squadron) for dps selection, by effect id.
fn fighter_schedules(fit: &Fit, i: usize, dl: &[Dealer], reload: bool) -> Vec<(u32, Schedule)> {
    let abs = cycles::fighter_abilities(fit, i);
    let inf = cycles::fighter_infinite(&abs);
    let rel = cycles::fighter_with_reload(fit, i, &abs, reload);
    let dps_of = |s: &[(u32, Schedule)]| -> f64 {
        s.iter()
            .map(|(eid, sch)| {
                let vol: f64 = dl.iter().filter(|x| x.i == i && x.eid == *eid).map(|x| tot(&x.base[0].1)).sum();
                let avg = sch.average_ms();
                if avg > 0.0 { vol / (avg / 1000.0) } else { 0.0 }
            })
            .sum()
    };
    if dps_of(&inf) >= dps_of(&rel) { inf } else { rel }
}

fn schedule(fit: &Fit, dl: &Dealer, all: &[Dealer], time_line: bool, owner_reload: bool) -> Option<Schedule> {
    match dl.k {
        K::Drone => {
            let c = drone_cycle(fit, dl.i);
            if c == 0.0 { None } else { Some(Schedule { seq: vec![cycles::Cyc { active: c, inactive: 0.0, qty: f64::INFINITY, reload: false }], repeat: 1.0 }) }
        }
        K::Fighter(_) => fighter_schedules(fit, dl.i, all, if time_line { true } else { owner_reload }).into_iter().find(|(e, _)| *e == dl.eid).map(|x| x.1),
        K::Breacher => Some(Schedule { seq: vec![cycles::Cyc { active: 1000.0, inactive: 0.0, qty: f64::INFINITY, reload: false }], repeat: 1.0 }),
        _ => cycles::module(fit, dl.i, if time_line { Some(true) } else { None }, owner_reload),
    }
}

/// Per-dealer (dps, volley, damage-so-far) at `time` (None = steady state; damage None then).
pub fn dealer_values(ctx: &Ctx, dl: &[Dealer], time: Option<f64>) -> Vec<(D, D, D)> {
    let fit = &ctx.fit;
    let owner_reload = ctx.fit_req.options.factor_reload;
    let def = ctx.fit_req.options.default_spool.unwrap_or(Spool { kind: SpoolType::SpoolScale, amount: 1.0 });
    let mut out = Vec::new();
    for x in dl {
        let is_mod = !matches!(x.k, K::Drone | K::Fighter(_));
        let Some(sch) = schedule(fit, x, dl, time.is_some(), owner_reload) else {
            out.push(([0.0; 5], [0.0; 5], [0.0; 5]));
            continue;
        };
        match time {
            None => {
                let m = if is_mod { spool_mult(fit, x.i, fit.items[x.i].spool.unwrap_or(def)) } else { 1.0 };
                let mut sum = [0.0; 5];
                for (_, v) in &x.base {
                    add(&mut sum, &scale(v, m));
                }
                let avg = sch.average_ms();
                let dps = if x.k == K::Breacher {
                    scale(&x.base[0].1, 1.0)
                } else if avg > 0.0 {
                    scale(&sum, 1000.0 / avg)
                } else {
                    [0.0; 5]
                };
                out.push((dps, scale(&x.base[0].1, m), [0.0; 5]));
            }
            Some(t) => {
                let tu = float_unerr(t);
                let (mut now, mut nonstop) = (0.0f64, 0.0f64);
                let mut cur: Option<(D, D)> = None;
                let mut dmg = [0.0; 5];
                let off = if x.k == K::Breacher { 1.0 } else { 0.0 };
                for (act, inact, _) in sch.iter() {
                    let m = if is_mod { spool_mult(fit, x.i, Spool { kind: SpoolType::Cycles, amount: nonstop }) } else { 1.0 };
                    let mut sum = [0.0; 5];
                    let mut best = [0.0; 5];
                    for (delay, v) in &x.base {
                        let vv = scale(v, m);
                        add(&mut sum, &vv);
                        if tot(&vv) > tot(&best) {
                            best = vv;
                        }
                        if tot(&vv) != 0.0 && float_unerr(now + delay / 1000.0) <= tu {
                            add(&mut dmg, &vv);
                        }
                        if x.k == K::Breacher {
                            break;
                        }
                    }
                    let (ts, te) = (now + off, now + act / 1000.0 + off);
                    if tot(&sum) > 0.0 && float_unerr(ts) <= tu {
                        cur = Some(if float_unerr(te) <= tu { ([0.0; 5], [0.0; 5]) } else { (scale(&sum, 1000.0 / act), best) });
                    }
                    nonstop = if inact > 0.0 { 0.0 } else { nonstop + 1.0 };
                    if now > t {
                        break;
                    }
                    now += act / 1000.0 + inact / 1000.0;
                }
                let (dps, vol) = cur.unwrap_or(([0.0; 5], [0.0; 5]));
                out.push((dps, vol, dmg));
            }
        }
    }
    out
}

// ------------------------------------------------------------------ application

pub struct Target {
    pub res: [f64; 4],
    pub vmax: f64,
    pub sig: f64,
    pub radius: f64,
    pub fit: Option<Box<TargetFit>>,
}

pub struct TargetFit {
    pub fit: Fit,
    pub req: crate::request::FitRequest,
}

pub struct Geo {
    pub dist: Option<f64>,
    pub atk_speed: f64,
    pub atk_angle: f64,
    pub tgt_speed: f64,
    pub tgt_angle: f64,
    pub tgt_sig: f64,
}

pub fn turret_mult(cth: f64) -> f64 {
    let wreck = cth.min(0.01);
    let normal = cth - wreck;
    let np = if normal > 0.0 { normal * ((0.01 + cth) / 2.0 + 0.49) } else { 0.0 };
    np + wreck * 3.0
}

#[allow(clippy::too_many_arguments)]
fn cth(atk_speed: f64, atk_angle: f64, atk_r: f64, opt: f64, fo: f64, tracking: f64, osr: f64, dist: Option<f64>, tgt_speed: f64, tgt_angle: f64, tgt_r: f64, sig: f64) -> f64 {
    let ang = match dist {
        None => 0.0,
        Some(dd) => {
            let ctc = atk_r + dd + tgt_r;
            let trans = (atk_speed * atk_angle.to_radians().sin() - tgt_speed * tgt_angle.to_radians().sin()).abs();
            if ctc == 0.0 {
                if trans == 0.0 { 0.0 } else { f64::INFINITY }
            } else {
                trans / ctc
            }
        }
    };
    let rf = range_factor(opt, fo, dist, false);
    let tf = 0.5f64.powf(((ang * osr) / (tracking * sig)).powi(2));
    rf * tf
}

fn missile_factor(er: f64, ev: f64, drf: f64, speed: f64, sig: f64) -> f64 {
    let mut m = 1.0f64;
    if er > 0.0 {
        m = m.min(sig / er);
    }
    if speed > 0.0 {
        m = m.min(((ev * sig) / (er * speed)).powf(drf));
    }
    m
}

fn first_attr(fit: &Fit, i: usize, names: &[&str]) -> f64 {
    for n in names {
        if let Some(v) = attr_opt(fit, i, n) {
            return v;
        }
    }
    0.0
}

pub fn drone_range(fit: &Fit, i: usize) -> (f64, f64) {
    (
        first_attr(fit, i, &["shieldTransferRange", "powerTransferRange", "energyDestabilizationRange", "empFieldRange", "ecmBurstRange", "maxRange"]),
        first_attr(fit, i, &["falloff", "falloffEffectiveness"]),
    )
}

fn module_max_range(fit: &Fit, i: usize) -> Option<f64> {
    let (o, _) = super::kernels::module_range(fit, i);
    if o != 0.0 {
        return Some(o);
    }
    let c = fit.items[i].charge?;
    let (lo, hi, ch) = fit.missile_range_data(c)?;
    Some(lo * (1.0 - ch) + hi * ch)
}

pub struct Ranges {
    pub in_lock: bool,
    pub in_dcr: bool,
    pub drone_mode: String,
}

pub fn application(ctx: &Ctx, x: &Dealer, tgt: &Target, g: &Geo, r: &Ranges) -> f64 {
    let fit = &ctx.fit;
    let i = x.i;
    let atk_r = attr(fit, fit.ship, "radius");
    let dist = g.dist;
    let missile_dist = |c: usize| -> f64 {
        match fit.missile_range_data(c) {
            None => 0.0,
            Some((lo, hi, ch)) => match dist {
                None => 1.0,
                Some(dd) if dd <= lo => 1.0,
                Some(dd) if dd <= hi => ch,
                _ => 0.0,
            },
        }
    };
    let v = match x.k {
        K::Vorton => {
            if !r.in_lock {
                return 0.0; // Pyfa leaves the key unset: no damage
            }
            range_factor(attr(fit, i, "maxRange"), 0.0, dist, true)
                * missile_factor(attr(fit, i, "aoeCloudSize"), attr(fit, i, "aoeVelocity"), attr(fit, i, "aoeDamageReductionFactor"), g.tgt_speed, g.tgt_sig)
        }
        K::Turret => {
            if !r.in_lock {
                0.0
            } else {
                let (o, f) = super::kernels::module_range(fit, i);
                turret_mult(cth(g.atk_speed, g.atk_angle, atk_r, o, f, attr(fit, i, "trackingSpeed"), attr(fit, i, "optimalSigRadius"), dist, g.tgt_speed, g.tgt_angle, tgt.radius, g.tgt_sig))
            }
        }
        K::Missile => {
            let fof = fit.items[i].charge.map(|c| has_eff(fit, c, "fofMissileLaunching")).unwrap_or(false);
            if !(r.in_lock || fof) {
                0.0
            } else if let Some(c) = fit.items[i].charge {
                missile_dist(c) * missile_factor(attr(fit, c, "aoeCloudSize"), attr(fit, c, "aoeVelocity"), attr(fit, c, "aoeDamageReductionFactor"), g.tgt_speed, g.tgt_sig)
            } else {
                0.0
            }
        }
        K::Smartbomb => match module_max_range(fit, i) {
            None => 0.0,
            Some(mr) => {
                if matches!(dist, Some(dd) if dd > mr) { 0.0 } else { 1.0 }
            }
        },
        K::Bomb => {
            let Some(mr) = module_max_range(fit, i) else { return 0.0 };
            let c = fit.items[i].charge.unwrap_or(i);
            let blast = attr(fit, c, "explosionRange");
            if let Some(dd) = dist {
                if dd < (mr - atk_r - tgt.radius - blast).max(0.0) || dd > (mr - atk_r + tgt.radius + blast).max(0.0) {
                    return 0.0;
                }
            }
            let er = attr(fit, c, "aoeCloudSize");
            if er == 0.0 { 1.0 } else { (g.tgt_sig / er).min(1.0) }
        }
        K::GuidedBomb => {
            if !r.in_lock {
                0.0
            } else {
                let Some(mr) = module_max_range(fit, i) else { return 0.0 };
                if matches!(dist, Some(dd) if dd > mr - atk_r) {
                    0.0
                } else {
                    let c = fit.items[i].charge.unwrap_or(i);
                    let er = attr(fit, c, "aoeCloudSize");
                    if er == 0.0 { 1.0 } else { (g.tgt_sig / er).min(1.0) }
                }
            }
        }
        K::Doomsday => {
            let single = SINGLE_DD.iter().any(|e| has_eff(fit, i, e));
            if !r.in_lock && single {
                0.0
            } else {
                let mr = module_max_range(fit, i).unwrap_or(0.0);
                if matches!(dist, Some(dd) if mr != 0.0 && dd > mr) {
                    0.0
                } else if SINGLE_DD[..4].iter().any(|e| has_eff(fit, i, e)) && tgt.fit.as_ref().map(|t| !requires_capital(&t.fit)).unwrap_or(false) {
                    0.0
                } else {
                    let ds = attr(fit, i, "signatureRadius");
                    if ds == 0.0 { 1.0 } else { (g.tgt_sig / ds).min(1.0) }
                }
            }
        }
        K::Breacher => {
            if !r.in_lock {
                0.0
            } else {
                let c = fit.items[i].charge.unwrap();
                let rm = tgt.fit.as_ref().map(|t| attr_opt(&t.fit, t.fit.ship, "breacherPodDamageResistance").unwrap_or(1.0)).unwrap_or(1.0);
                missile_dist(c) * rm
            }
        }
        K::Other => 0.0,
        K::Drone => {
            if !(r.in_lock && r.in_dcr) {
                0.0
            } else {
                let ds = attr(fit, i, "maxVelocity");
                let c = if ds > 1.0 && ((r.drone_mode == "auto" && ds >= g.tgt_speed) || r.drone_mode == "follow_target") {
                    1.0
                } else {
                    let dr = attr(fit, i, "radius");
                    let (o, f) = drone_range(fit, i);
                    cth(g.atk_speed.min(ds), g.atk_angle, dr, o, f, attr(fit, i, "trackingSpeed"), attr(fit, i, "optimalSigRadius"), dist.map(|dd| dd + atk_r - dr), g.tgt_speed, g.tgt_angle, tgt.radius, g.tgt_sig)
                };
                turret_mult(c)
            }
        }
        K::Fighter(p) => {
            if !r.in_lock && p != "fighterAbilityLaunchBomb" {
                0.0
            } else {
                let fs = attr(fit, i, "maxVelocity");
                let rf = if (r.drone_mode == "auto" && fs >= g.tgt_speed) || r.drone_mode == "follow_target" {
                    1.0
                } else {
                    let o = attr(fit, i, &format!("{p}RangeOptimal"));
                    let o = if o != 0.0 { o } else { attr(fit, i, &format!("{p}Range")) };
                    range_factor(o, attr(fit, i, &format!("{p}RangeFalloff")), dist.map(|dd| dd + atk_r - attr(fit, i, "radius")), true)
                };
                let drf = attr_opt(fit, i, &format!("{p}ReductionFactor")).unwrap_or_else(|| attr(fit, i, &format!("{p}DamageReductionFactor")));
                let drs = attr_opt(fit, i, &format!("{p}ReductionSensitivity")).unwrap_or_else(|| attr(fit, i, &format!("{p}DamageReductionSensitivity")));
                let mf = missile_factor(attr(fit, i, &format!("{p}ExplosionRadius")), attr(fit, i, &format!("{p}ExplosionVelocity")), drf.ln() / drs.ln(), g.tgt_speed, g.tgt_sig);
                let mut rm = 1.0;
                if let Some(t) = &tgt.fit {
                    let rid = attr(fit, i, &format!("{p}ResistanceID")) as u32;
                    if rid != 0 {
                        let a = rid as u16;
                        if d::attr_name(a).is_some() {
                            rm = if t.fit.has(t.fit.ship, a) { t.fit.get(t.fit.ship, a) } else { 1.0 };
                        }
                    }
                }
                rf * mf * rm
            }
        }
    };
    float_unerr(v)
}

fn requires_capital(f: &Fit) -> bool {
    let cap = d::type_by_name("Capital Ships");
    match cap {
        Some(c) => f.items[f.ship].req_skills.iter().any(|&s| s == c),
        None => false,
    }
}


// ------------------------------------------------------------------ target + projected

fn res_layer(f: &Fit, pre: &str) -> [f64; 4] {
    let names: [String; 4] = if pre.is_empty() {
        ["emDamageResonance".into(), "thermalDamageResonance".into(), "kineticDamageResonance".into(), "explosiveDamageResonance".into()]
    } else {
        [format!("{pre}EmDamageResonance"), format!("{pre}ThermalDamageResonance"), format!("{pre}KineticDamageResonance"), format!("{pre}ExplosiveDamageResonance")]
    };
    let mut r = [0.0; 4];
    for k in 0..4 {
        r[k] = 1.0 - attr(f, f.ship, &names[k]);
    }
    r
}

fn sget(v: &Value, path: &str) -> f64 {
    let mut x = v;
    for p in path.split('.') {
        match x.get(p) {
            Some(y) => x = y,
            None => return 0.0,
        }
    }
    x.as_f64().unwrap_or(0.0)
}

fn fit_resists(f: &Fit, stats: &Value, mode: &str) -> [f64; 4] {
    let (sh, ar, hu) = (res_layer(f, "shield"), res_layer(f, "armor"), res_layer(f, ""));
    let (hs, ha, hh) = (sget(stats, "defense.hp.shield"), sget(stats, "defense.hp.armor"), sget(stats, "defense.hp.hull"));
    match mode {
        "shield" => sh,
        "armor" => ar,
        "hull" => hu,
        "weighted_average" => {
            let tot = hs + ha + hh;
            let mut r = [0.0; 4];
            for k in 0..4 {
                let e = hs / (1.0 - sh[k]) + ha / (1.0 - ar[k]) + hh / (1.0 - hu[k]);
                r[k] = 1.0 - tot / e;
            }
            r
        }
        _ => {
            let uni = |r: &[f64; 4], hp: f64| hp / (r.iter().map(|x| 0.25 * (1.0 - x)).sum::<f64>());
            let (es, ea, eh) = (uni(&sh, hs), uni(&ar, ha), uni(&hu, hh));
            let te = es + ea + eh;
            let rf = |e: f64, h: f64| if h == 0.0 { 1.0 } else { e / h };
            let (rs, ra, rh) = (rf(es, hs), rf(ea, ha), rf(eh, hh));
            let best = rs.max(ra).max(rh);
            let mut sc = [
                100.0 * (es / te).powf(1.5) + 25.0 * (rs / best).powf(1.5),
                100.0 * (ea / te).powf(1.5) + 25.0 * (ra / best).powf(1.5),
                100.0 * (eh / te).powf(1.5) + 25.0 * (rh / best).powf(1.5),
            ];
            sc[0] += 10000.0 * sget(stats, "defense.tank.raw.shield_repair") * rs / te;
            sc[1] += 10000.0 * sget(stats, "defense.tank.raw.armor_repair") * ra / te;
            sc[2] += 10000.0 * sget(stats, "defense.tank.raw.hull_repair") * rh / te;
            sc[0] += 5000.0 * sget(stats, "defense.tank.raw.passive_shield") * rs / te;
            let m = sc[0].max(sc[1]).max(sc[2]);
            if m == sc[0] {
                sh
            } else if m == sc[1] {
                ar
            } else {
                hu
            }
        }
    }
}

pub fn build_target(ctx: &Ctx) -> Result<Target, String> {
    let t = ctx.req.get("target").cloned().unwrap_or(Value::Null);
    if let Some(fr) = t.get("fit").filter(|v| !v.is_null()) {
        let req: crate::request::FitRequest = serde_json::from_value(fr.clone()).map_err(|e| format!("target fit: {e}"))?;
        let fit = Fit::build(&req).map_err(|e| format!("target fit: {e:?}"))?;
        let stats = crate::calc(&req).to_value_raw();
        let mode = t.get("resist_mode").and_then(|v| v.as_str()).unwrap_or("auto");
        let res = fit_resists(&fit, &stats, mode);
        let s = fit.ship;
        return Ok(Target { res, vmax: attr(&fit, s, "maxVelocity"), sig: attr(&fit, s, "signatureRadius"), radius: attr(&fit, s, "radius"), fit: Some(Box::new(TargetFit { fit, req })) });
    }
    let p = t.get("profile").cloned().unwrap_or(Value::Null);
    let f = |k: &str, def: f64| p.get(k).and_then(|v| v.as_f64()).unwrap_or(def);
    let sig = match p.get("signature_radius") {
        Some(Value::Null) => f64::INFINITY,
        Some(v) => v.as_f64().unwrap_or(f64::INFINITY),
        None => if p.is_null() { f64::INFINITY } else { 125.0 },
    };
    if p.is_null() {
        return Ok(Target { res: [0.0; 4], vmax: 0.0, sig, radius: 0.0, fit: None });
    }
    Ok(Target { res: [f("em", 0.0), f("thermal", 0.0), f("kinetic", 0.0), f("explosive", 0.0)], vmax: f("max_velocity", 0.0), sig, radius: f("radius", 0.0), fit: None })
}

struct Proj {
    boost: f64,
    opt: f64,
    fo: f64,
    speed: f64,
    radius: f64,
}

/// (module webs, module TPs, mobile webs, mobile TPs); mobile = (fighters first, then drones) with their `in_dcr` flag
fn proj_sources(fit: &Fit) -> (Vec<Proj>, Vec<Proj>, Vec<(Proj, bool)>, Vec<(Proj, bool)>) {
    let (mut wm, mut tm, mut wd, mut td) = (vec![], vec![], vec![], vec![]);
    for i in 0..fit.items.len() {
        let it = &fit.items[i];
        match it.kind {
            Kind::Module if it.state >= State::Active => {
                let (o, f) = super::kernels::module_range(fit, i);
                let ddr = (o + attr(fit, i, "doomsdayAOERange")).max(0.0);
                for e in ["remoteWebifierFalloff", "structureModuleEffectStasisWebifier"] {
                    if has_eff(fit, i, e) {
                        wm.push(Proj { boost: attr(fit, i, "speedFactor"), opt: o, fo: f, speed: 0.0, radius: 0.0 });
                    }
                }
                if has_eff(fit, i, "doomsdayAOEWeb") {
                    wm.push(Proj { boost: attr(fit, i, "speedFactor"), opt: ddr, fo: f, speed: 0.0, radius: 0.0 });
                }
                for e in ["remoteTargetPaintFalloff", "structureModuleEffectTargetPainter"] {
                    if has_eff(fit, i, e) {
                        tm.push(Proj { boost: attr(fit, i, "signatureRadiusBonus"), opt: o, fo: f, speed: 0.0, radius: 0.0 });
                    }
                }
                if has_eff(fit, i, "doomsdayAOEPaint") {
                    tm.push(Proj { boost: attr(fit, i, "signatureRadiusBonus"), opt: ddr, fo: f, speed: 0.0, radius: 0.0 });
                }
            }
            Kind::Fighter if it.active_count > 0 => {
                let eid = it.effects().find(|(ei, _)| d::eff_name(*ei) == "fighterAbilityStasisWebifier").map(|(ei, _)| d::EFF_IDS[ei]);
                if let Some(eid) = eid.filter(|e| it.fighter_abilities.contains(e)) {
                    let _ = eid;
                    wd.push((
                        Proj {
                            boost: attr(fit, i, "fighterAbilityStasisWebifierSpeedPenalty") * it.active_count as f64,
                            opt: attr(fit, i, "fighterAbilityStasisWebifierOptimalRange"),
                            fo: attr(fit, i, "fighterAbilityStasisWebifierFalloffRange"),
                            speed: attr(fit, i, "maxVelocity"),
                            radius: attr(fit, i, "radius"),
                        },
                        false,
                    ));
                }
            }
            _ => {}
        }
    }
    for i in 0..fit.items.len() {
        let it = &fit.items[i];
        if it.kind != Kind::Drone || it.active_count == 0 {
            continue;
        }
        let (o, f) = drone_range(fit, i);
        for _ in 0..it.active_count {
            if has_eff(fit, i, "remoteWebifierEntity") {
                wd.push((Proj { boost: attr(fit, i, "speedFactor"), opt: o, fo: f, speed: attr(fit, i, "maxVelocity"), radius: attr(fit, i, "radius") }, true));
            }
            if has_eff(fit, i, "remoteTargetPaintEntity") {
                td.push((Proj { boost: attr(fit, i, "signatureRadiusBonus"), opt: o, fo: f, speed: attr(fit, i, "maxVelocity"), radius: attr(fit, i, "radius") }, true));
            }
        }
    }
    (wm, tm, wd, td)
}

fn has_scram(fit: &Fit) -> Option<f64> {
    let mut r: Option<f64> = None;
    for i in 0..fit.items.len() {
        let it = &fit.items[i];
        if it.kind != Kind::Module || it.state < State::Active {
            continue;
        }
        let reg = (has_eff(fit, i, "warpScrambleBlockMWDWithNPCEffect") || has_eff(fit, i, "structureWarpScrambleBlockMWDWithNPCEffect")) && attr(fit, i, "activationBlockedStrenght") != 0.0;
        let hic = has_eff(fit, i, "warpDisruptSphere") && it.charge.map(|c| has_eff(fit, c, "shipModuleFocusedWarpScramblingScript")).unwrap_or(false);
        if reg || hic {
            r = Some(r.unwrap_or(0.0).max(module_max_range(fit, i).unwrap_or(0.0)));
        }
    }
    r
}

/// Target fit's max velocity / signature with the source fit projected at `dist` (the engine applies the source's
/// webs / painters with their range factors, stacking-penalised with the target's own modifiers) and scrammable
/// modules (MWD / MJD) switched off when `scrammed`.
fn tgt_fit_attrs(ctx: &Ctx, t: &TargetFit, dist: Option<f64>, scrammed: bool) -> (f64, f64) {
    let mut req = t.req.clone();
    if scrammed {
        for m in req.modules.iter_mut() {
            let Some(ix) = d::type_index(m.type_id) else { continue };
            let blocked = d::type_effects(ix).iter().map(|x| d::eff_name((x >> 1) as usize)).any(|e| e == "moduleBonusMicrowarpdrive" || e == "microJumpDrive" || e == "microJumpPortalDrive");
            if blocked && matches!(m.state, Some(State::Active) | Some(State::Overheated)) {
                m.state = Some(State::Online);
            }
        }
    }
    req.projected.push(crate::request::Projected { kind: "fit".into(), module: None, drone: None, fit: Some(Box::new(ctx.fit_req.clone())), fighter: None, amount: 1, distance_m: dist });
    match Fit::build(&req) {
        Ok(f) => (attr(&f, f.ship, "maxVelocity"), attr(&f, f.ship, "signatureRadius")),
        Err(_) => (attr(&t.fit, t.fit.ship, "maxVelocity"), attr(&t.fit, t.fit.ship, "signatureRadius")),
    }
}

pub struct Tackle {
    pub speed: f64,
    pub sig_mult: f64,
}

/// Pyfa getTackledSpeed + getSigRadiusMult semantics.
pub fn tackle(ctx: &Ctx, tgt: &Target, cur_speed: f64, dist: Option<f64>, r: &Ranges) -> Tackle {
    let fit = &ctx.fit;
    let immune = tgt.fit.as_ref().map(|t| attr(&t.fit, t.fit.ship, "disallowOffensiveModifiers") != 0.0).unwrap_or(false);
    if immune {
        return Tackle { speed: cur_speed, sig_mult: 1.0 };
    }
    let (wm, tm, wd, td) = proj_sources(fit);
    let scram = has_scram(fit);
    let scrammed = tgt.fit.is_some() && r.in_lock && scram.is_some() && !matches!(dist, Some(dd) if dd > scram.unwrap());
    if let Some(t) = &tgt.fit {
        if wm.is_empty() && tm.is_empty() && wd.is_empty() && td.is_empty() && !scrammed {
            return Tackle { speed: cur_speed, sig_mult: 1.0 };
        }
        if tgt.vmax == 0.0 {
            return Tackle { speed: 0.0, sig_mult: 1.0 };
        }
        let (v, sg) = tgt_fit_attrs(ctx, t, if r.in_lock { dist } else { Some(f64::MAX) }, scrammed);
        return Tackle { speed: float_unerr(v * cur_speed / tgt.vmax), sig_mult: float_unerr(sg / tgt.sig) };
    }
    let atk_r = attr(fit, fit.ship, "radius");
    let rfd = |p: &Proj| range_factor(p.opt, p.fo, dist.map(|dd| dd + atk_r - p.radius), true);
    let eval = |web: &[f64], tp: &[f64]| -> (f64, f64) {
        (tgt.vmax * super::kernels::stack_mult(web), tgt.sig * super::kernels::stack_mult(tp))
    };
    // speed
    let max0 = tgt.vmax;
    let speed = if max0 == 0.0 {
        0.0
    } else {
        let ratio = cur_speed / max0;
        let mut mults: Vec<f64> = Vec::new();
        if r.in_lock {
            for w in &wm {
                let b = w.boost * range_factor(w.opt, w.fo, dist, true);
                if b != 0.0 {
                    mults.push(1.0 + b / 100.0);
                }
            }
        }
        let mut cur = eval(&mults, &[]).0 * ratio;
        let mut mobile: Vec<&Proj> = wd.iter().filter(|(_, is_drone)| r.in_lock && (!is_drone || r.in_dcr)).map(|(p, _)| p).collect();
        let long: Vec<&Proj> = mobile.iter().copied().filter(|p| dist.map(|dd| dd <= p.opt - atk_r + p.radius).unwrap_or(true)).collect();
        if !long.is_empty() {
            for p in &long {
                mults.push(1.0 + p.boost / 100.0);
            }
            mobile.retain(|p| !long.iter().any(|q| std::ptr::eq(*p, *q)));
            cur = eval(&mults, &[]).0 * ratio;
        }
        while !mobile.is_empty() {
            let fastest = mobile.iter().map(|p| p.speed).fold(f64::MIN, f64::max);
            let batch: Vec<&Proj> = mobile.iter().copied().filter(|p| p.speed == fastest).collect();
            for p in &batch {
                let b = if (r.drone_mode == "auto" && p.speed >= cur) || r.drone_mode == "follow_target" { p.boost } else { p.boost * rfd(p) };
                mults.push(1.0 + b / 100.0);
            }
            mobile.retain(|p| p.speed != fastest);
            cur = eval(&mults, &[]).0 * ratio;
        }
        float_unerr(cur)
    };
    // signature
    let mut mults: Vec<f64> = Vec::new();
    if r.in_lock {
        for p in &tm {
            let b = p.boost * range_factor(p.opt, p.fo, dist, true);
            if b != 0.0 {
                mults.push(1.0 + b / 100.0);
            }
        }
    }
    for (p, is_drone) in &td {
        if !(r.in_lock && (!is_drone || r.in_dcr)) {
            continue;
        }
        let b = if (r.drone_mode == "auto" && p.speed >= speed) || r.drone_mode == "follow_target" { p.boost } else { p.boost * rfd(p) };
        mults.push(1.0 + b / 100.0);
    }
    let init = tgt.sig;
    let modified = eval(&[], &mults).1;
    let sig_mult = if modified.is_infinite() && init.is_infinite() { 1.0 } else { float_unerr(modified / init) };
    Tackle { speed, sig_mult }
}

/// mode: 0 dps, 1 volley, 2 damage. Arguments may be None (not set).
pub fn damage(ctx: &Ctx, mode: u8, time: Option<f64>, dist: Option<f64>, speed_x: Option<f64>, sig_x: Option<f64>) -> Result<Option<f64>, String> {
    if mode == 2 && time.is_none() {
        return Ok(None);
    }
    let tgt = build_target(ctx)?;
    let fit = &ctx.fit;
    let p = |k: &str| ctx.param_f(k);
    let tgt_speed0 = match speed_x {
        Some(v) => v,
        None => p("tgt_speed_mps").unwrap_or_else(|| p("tgt_speed_pct").unwrap_or(100.0) / 100.0 * tgt.vmax),
    };
    let src_v = attr(fit, fit.ship, "maxVelocity");
    let atk_speed = p("atk_speed_mps").unwrap_or_else(|| p("atk_speed_pct").unwrap_or(0.0) / 100.0 * src_v);
    let in_lock = match dist {
        None => true,
        Some(x) => ctx.setting_b("ignore_lock_range", true) || x <= ctx.stat("targeting.max_range_m").unwrap_or(0.0),
    };
    let in_dcr = match dist {
        None => true,
        Some(x) => ctx.setting_b("ignore_drone_control_range", false) || x <= ctx.stat("drones.control_range_m").unwrap_or(0.0),
    };
    let r = Ranges { in_lock, in_dcr, drone_mode: ctx.setting_s("mobile_drone_mode", "auto") };
    let (tgt_speed, sig_mult) = if ctx.setting_b("apply_projected", true) {
        let t = tackle(ctx, &tgt, tgt_speed0, dist, &r);
        (t.speed, t.sig_mult)
    } else {
        (tgt_speed0, 1.0)
    };
    let tgt_sig = sig_x.unwrap_or(tgt.sig) * sig_mult;
    let g = Geo { dist, atk_speed, atk_angle: p("atk_angle_deg").unwrap_or(90.0), tgt_speed, tgt_angle: p("tgt_angle_deg").unwrap_or(90.0), tgt_sig };
    let dl = dealers(fit);
    let vals = dealer_values(ctx, &dl, time);
    let res = if ctx.setting_b("ignore_resists", true) { [0.0; 4] } else { tgt.res };
    let mut total = [0.0; 5];
    for (x, v) in dl.iter().zip(&vals) {
        let app = application(ctx, x, &tgt, &g, &r);
        let y = match mode {
            0 => &v.0,
            1 => &v.1,
            _ => &v.2,
        };
        add(&mut total, &scale(y, app));
    }
    let mut out = total[4];
    for k in 0..4 {
        out += total[k] * (1.0 - res[k]);
    }
    Ok(Some(out))
}
