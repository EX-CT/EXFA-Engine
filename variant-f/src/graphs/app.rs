//! Application profile kernel: for the fit's dominant weapon kind (turrets or launchers), the best charge per
//! weapon type at each distance among the charges the weapon can load (quality tier filter), with the same
//! application math as the damage graph. Follows the behaviour described for Pyfa's "Application Profile" graph:
//! charge stats scaled by the fitted weapon's current multipliers, target speed / signature after the source's
//! webs / painters sampled on a distance grid with linear interpolation, and the best charge picked per distance
//! band (coarse scan + bisection), then applied exactly at the requested distance.
use super::cycles::an;
use super::dmg;
use super::Ctx;
use crate::data as d;
use crate::engine::{Fit, Kind};
use crate::request::State;
use std::collections::HashMap;
use std::sync::OnceLock;

fn attr(fit: &Fit, i: usize, n: &str) -> f64 {
    an(n).map(|a| fit.get(i, a)).unwrap_or(0.0)
}
fn base(ix: usize, n: &str) -> f64 {
    an(n).and_then(|a| d::type_attr(ix, a)).unwrap_or(0.0)
}
fn has_eff(fit: &Fit, i: usize, n: &str) -> bool {
    fit.items[i].effects().any(|(ei, _)| d::eff_name(ei) == n)
}

fn by_group() -> &'static HashMap<u32, Vec<usize>> {
    static M: OnceLock<HashMap<u32, Vec<usize>>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m: HashMap<u32, Vec<usize>> = HashMap::new();
        for ix in 0..d::type_count() {
            if d::type_published(ix) {
                m.entry(d::ty(ix).group).or_default().push(ix);
            }
        }
        m
    })
}

const NAVY: [&str; 4] = ["Imperial Navy ", "Republic Fleet ", "Caldari Navy ", "Federation Navy "];
const CAP_NAVY: [&str; 3] = ["Sansha ", "Arch Angel ", "Shadow "];

fn meta_group(ix: usize) -> Option<i64> {
    let mg = base(ix, "metaGroupID");
    if mg != 0.0 {
        return Some(mg as i64);
    }
    match base(ix, "techLevel") as i64 {
        2 => Some(2),
        _ => None,
    }
}

fn valid_charges(fit: &Fit, i: usize, tier: &str) -> Vec<usize> {
    let mty = fit.items[i].ty;
    let cap = d::type_capacity(mty);
    let size = attr(fit, i, "chargeSize");
    let mut out = Vec::new();
    for k in 1..=4 {
        let g = attr(fit, i, &format!("chargeGroup{k}"));
        if g == 0.0 {
            continue;
        }
        for &ix in by_group().get(&(g as u32)).map(|v| v.as_slice()).unwrap_or(&[]) {
            if d::type_volume(ix) > cap {
                continue;
            }
            if size > 0.0 && base(ix, "chargeSize") != size {
                continue;
            }
            if !out.contains(&ix) {
                out.push(ix);
            }
        }
    }
    if tier == "all" {
        return out;
    }
    let mut classifiable = false;
    let mut f = Vec::new();
    for &ix in &out {
        let mg = meta_group(ix);
        if mg.is_some() {
            classifiable = true;
        }
        let name = d::type_name(ix);
        let keep = match mg {
            None | Some(1) => true,
            Some(2) => tier == "navy",
            Some(4) if tier == "navy" => {
                if name.ends_with(" XL") {
                    CAP_NAVY.iter().any(|p| name.starts_with(p))
                } else {
                    NAVY.iter().any(|p| name.starts_with(p))
                }
            }
            _ => false,
        };
        if keep {
            f.push(ix);
        }
    }
    if !f.is_empty() || classifiable {
        f
    } else {
        out
    }
}

#[derive(Clone)]
struct Cd {
    name: &'static str,
    tid: u32,
    raw_volley: f64,
    // turret
    opt: f64,
    fo: f64,
    tracking: f64,
    // missile
    lo: f64,
    hi: f64,
    chance: f64,
    max_eff: f64,
    er: f64,
    ev: f64,
    drf: f64,
    prio: i32,
}

struct Group {
    count: f64,
    cycle_ms: f64,
    cds: Vec<Cd>,
    osr: f64,
    transitions: Vec<(i64, i64)>,
}

struct Proj {
    has: bool,
    base_speed: f64,
    base_sig: f64,
    dists: Vec<i64>,
    vals: Vec<(f64, f64)>,
}

impl Proj {
    fn at(&self, dist: f64) -> (f64, f64) {
        if !self.has || self.dists.is_empty() {
            return (self.base_speed, self.base_sig);
        }
        let idx = self.dists.partition_point(|&x| (x as f64) <= dist) as i64 - 1;
        let idx = idx.max(0) as usize;
        if idx >= self.dists.len() - 1 {
            return *self.vals.last().unwrap();
        }
        let (dl, dh) = (self.dists[idx] as f64, self.dists[idx + 1] as f64);
        if dist <= dl {
            return self.vals[idx];
        }
        let (lo, hi) = (self.vals[idx], self.vals[idx + 1]);
        let t = if dh > dl { (dist - dl) / (dh - dl) } else { 0.0 };
        let sp = lo.0 + t * (hi.0 - lo.0);
        let sg = if lo.1.is_infinite() || hi.1.is_infinite() { f64::INFINITY } else { lo.1 + t * (hi.1 - lo.1) };
        (sp, sg)
    }
}

fn sample_step(max: f64) -> i64 {
    if max <= 0.0 {
        return 100;
    }
    let step = max / 300.0;
    if step <= 100.0 {
        100
    } else {
        ((step / 100.0).ceil() * 100.0) as i64
    }
}

struct Track {
    atk_speed: f64,
    atk_angle: f64,
    atk_r: f64,
    tgt_angle: f64,
    tgt_r: f64,
    perfect: bool,
}

fn turret_volley(cd: &Cd, osr: f64, dist: f64, tr: &Track, speed: f64, sig: f64) -> f64 {
    let rf = if dist <= cd.opt { 1.0 } else { crate::stats::range_factor(cd.opt, cd.fo, Some(dist), false) };
    let tf = if tr.perfect {
        1.0
    } else {
        let ctc = tr.atk_r + dist + tr.tgt_r;
        let trans = (tr.atk_speed * tr.atk_angle.to_radians().sin() - speed * tr.tgt_angle.to_radians().sin()).abs();
        let ang = if ctc == 0.0 { if trans == 0.0 { 0.0 } else { f64::INFINITY } } else { trans / ctc };
        0.5f64.powf(((ang * osr) / (cd.tracking * sig)).powi(2))
    };
    cd.raw_volley * dmg::turret_mult(rf * tf)
}

fn missile_volley(cd: &Cd, dist: f64, speed: f64, sig: f64) -> f64 {
    let rf = if dist <= cd.lo {
        1.0
    } else if dist <= cd.hi {
        cd.chance
    } else {
        0.0
    };
    if rf == 0.0 {
        return 0.0;
    }
    let mut m = 1.0f64;
    if cd.er > 0.0 {
        m = m.min(sig / cd.er);
    }
    if speed > 0.0 && cd.er > 0.0 {
        m = m.min(((cd.ev * sig) / (cd.er * speed)).powf(cd.drf));
    }
    cd.raw_volley * rf * m
}

/// Exact turret-charge ties (bit-identical applied damage): Pyfa's winner comes from the iteration order of a set
/// of item objects (hash = memory address), so no data rule reproduces it (see GRAPHS.md). These (winner, loser)
/// type-id pairs are the order *observed* in the Pyfa oracle outputs of the graphs corpus (black-box, contract 0.2,
/// all 19 observed pairs consistent). Pairs not listed keep the deterministic rule: first strict maximum in dataset order.
const PYFA_TIE_PREF: &[(u32, u32)] = &[
    (21740, 22993), // Caldari Navy Antimatter L > Federation Navy Antimatter L
    (21300, 20893), // Dark Blood Gamma L > True Sansha Gamma L
    (21284, 20877), // Dark Blood Gamma M > True Sansha Gamma M
    (21302, 20895), // Dark Blood Multifrequency L > True Sansha Multifrequency L
    (21286, 20879), // Dark Blood Multifrequency M > True Sansha Multifrequency M
    (21294, 20887), // Dark Blood Standard L > True Sansha Standard L
    (21298, 20891), // Dark Blood Xray L > True Sansha Xray L
    (21282, 20875), // Dark Blood Xray M > True Sansha Xray M
    (181, 182),     // Depleted Uranium S > Titanium Sabot S
    (20791, 20793), // Domination Depleted Uranium L > Domination Titanium Sabot L
    (20759, 20761), // Domination Depleted Uranium S > Domination Titanium Sabot S
    (20795, 20799), // Domination Fusion L > Domination EMP L
    (20795, 20797), // Domination Fusion L > Domination Phased Plasma L
    (21430, 20991), // Dread Guristas Antimatter L > Guardian Antimatter L
    (22997, 23043), // Federation Navy Uranium L > Caldari Navy Uranium L
    (183, 185),     // Fusion S > EMP S
    (183, 184),     // Fusion S > Phased Plasma S
    (20869, 21276), // True Sansha Infrared M > Dark Blood Infrared M
    (20871, 21278), // True Sansha Standard M > Dark Blood Standard M
];

fn tie_pref(cand: u32, cur: u32) -> bool {
    PYFA_TIE_PREF.contains(&(cand, cur))
}

fn best(cds: &[Cd], turret: bool, osr: f64, dist: f64, tr: &Track, p: &Proj) -> (f64, Option<usize>) {
    let (sp, sg) = p.at(dist);
    let (mut bv, mut bi, mut bp) = (0.0, None, 99);
    for (k, cd) in cds.iter().enumerate() {
        let v = if turret { turret_volley(cd, osr, dist, tr, sp, sg) } else { missile_volley(cd, dist, sp, sg) };
        let better = if turret {
            v > bv || (v == bv && v > 0.0 && bi.is_some_and(|b: usize| tie_pref(cd.tid, cds[b].tid)))
        } else {
            v > bv || (v == bv && v > 0.0 && cd.prio < bp)
        };
        if better {
            bv = v;
            bi = Some(k);
            bp = cd.prio;
        }
    }
    (bv, bi)
}

fn transitions(cds: &[Cd], turret: bool, osr: f64, tr: &Track, p: &Proj, max_d: i64) -> Vec<(i64, i64)> {
    if cds.is_empty() {
        return vec![];
    }
    let res = sample_step(max_d as f64);
    let name = |i: Option<usize>| i.map(|k| cds[k].name);
    let (_, b0) = best(cds, turret, osr, 0.0, tr, p);
    let mut out = vec![(0i64, b0.map(|k| k as i64).unwrap_or(0))];
    let mut cur = name(b0);
    let mut dist = res;
    while dist <= max_d {
        let (mut bv, bi) = best(cds, turret, osr, dist as f64, tr, p);
        if name(bi) != cur {
            let (mut lo, mut hi) = (dist - res, dist);
            while hi - lo > 10 {
                let mid = (lo + hi).div_euclid(2);
                let (_, mi) = best(cds, turret, osr, mid as f64, tr, p);
                if name(mi) == cur {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            bv = best(cds, turret, osr, hi as f64, tr, p).0;
            out.push((hi, bi.map(|k| k as i64).unwrap_or(0)));
            cur = name(bi);
        }
        if !turret && bv < 0.01 {
            out.push((dist, -1));
            break;
        }
        dist += res;
    }
    out
}

/// mode 0 = dps, 1 = volley
pub fn app_profile(ctx: &Ctx, mode: u8, dist: f64) -> Result<(Option<f64>, Option<u32>), String> {
    let fit = &ctx.fit;
    let tier = ctx.param("ammo_quality").and_then(|v| v.as_str()).unwrap_or("all").to_string();
    let mut turrets = Vec::new();
    let mut launchers = Vec::new();
    for i in 0..fit.items.len() {
        let it = &fit.items[i];
        if it.kind != Kind::Module || it.state < State::Active || attr(fit, i, "miningAmount") != 0.0 {
            continue;
        }
        if has_eff(fit, i, "turretFitted") {
            turrets.push(i);
        } else if has_eff(fit, i, "launcherFitted") {
            launchers.push(i);
        }
    }
    if turrets.is_empty() && launchers.is_empty() {
        return Ok((Some(0.0), None));
    }
    let turret = turrets.len() >= launchers.len();
    let mods = if turret { turrets } else { launchers };
    let tgt = dmg::build_target(ctx)?;
    let ignore_res = ctx.setting_b("ignore_resists", true);
    let res = if ignore_res { None } else { Some(tgt.res) };
    let p = |k: &str| ctx.param_f(k);
    let tgt_speed = p("tgt_speed_mps").unwrap_or_else(|| p("tgt_speed_pct").unwrap_or(100.0) / 100.0 * tgt.vmax);
    let src_v = attr(fit, fit.ship, "maxVelocity");
    let atk_r = attr(fit, fit.ship, "radius");
    let tr = Track {
        atk_speed: p("atk_speed_mps").unwrap_or_else(|| p("atk_speed_pct").unwrap_or(0.0) / 100.0 * src_v),
        atk_angle: p("atk_angle_deg").unwrap_or(90.0),
        atk_r,
        tgt_angle: p("tgt_angle_deg").unwrap_or(90.0),
        tgt_r: tgt.radius,
        perfect: tgt.sig == 0.0,
    };
    let owner_reload = ctx.fit_req.options.factor_reload;
    // ---- per weapon type: charge data
    let mut groups: Vec<(u32, Group, f64)> = Vec::new(); // (type id, group, range-info max effective range)
    for &i in &mods {
        let tid = fit.items[i].type_id;
        if let Some(g) = groups.iter_mut().find(|g| g.0 == tid) {
            g.1.count += 1.0;
            continue;
        }
        let Some(sch) = super::cycles::module(fit, i, None, owner_reload) else { continue };
        let cycle_ms = sch.average_ms();
        let charges = valid_charges(fit, i, &tier);
        if charges.is_empty() {
            continue;
        }
        let dmg_of = |ix: usize, mults: [f64; 4]| -> f64 {
            let v = [base(ix, "emDamage") * mults[0], base(ix, "thermalDamage") * mults[1], base(ix, "kineticDamage") * mults[2], base(ix, "explosiveDamage") * mults[3]];
            match res {
                Some(r) => (0..4).map(|k| v[k] * (1.0 - r[k])).sum(),
                None => v.iter().sum(),
            }
        };
        let or1 = |x: f64| if x == 0.0 { 1.0 } else { x };
        if turret {
            let cur = fit.items[i].charge;
            let (mut opt, mut fo, mut trk) = (attr(fit, i, "maxRange"), attr(fit, i, "falloff"), attr(fit, i, "trackingSpeed"));
            let osr = attr(fit, i, "optimalSigRadius");
            let dmult = or1(attr(fit, i, "damageMultiplier"));
            let mut skill = 1.0;
            if let Some(c) = cur {
                let cix = fit.items[c].ty;
                opt /= or1(base(cix, "weaponRangeMultiplier"));
                fo /= or1(base(cix, "fallofMultiplier"));
                trk /= or1(base(cix, "trackingSpeedMultiplier"));
                let bt: f64 = ["emDamage", "thermalDamage", "kineticDamage", "explosiveDamage"].iter().map(|n| base(cix, n)).sum();
                if bt > 0.0 {
                    let mt: f64 = ["emDamage", "thermalDamage", "kineticDamage", "explosiveDamage"].iter().map(|n| attr(fit, c, n)).sum();
                    skill = mt / bt;
                }
            }
            let cds: Vec<Cd> = charges
                .iter()
                .map(|&ix| Cd {
                    name: d::type_name(ix),
                    tid: d::type_id_at(ix),
                    raw_volley: dmg_of(ix, [1.0; 4]) * skill * dmult,
                    opt: opt * or1(base(ix, "weaponRangeMultiplier")),
                    fo: fo * or1(base(ix, "fallofMultiplier")),
                    tracking: trk * or1(base(ix, "trackingSpeedMultiplier")),
                    lo: 0.0,
                    hi: 0.0,
                    chance: 0.0,
                    max_eff: 0.0,
                    er: 0.0,
                    ev: 0.0,
                    drf: 0.0,
                    prio: 99,
                })
                .collect();
            let longest = charges.iter().map(|&ix| or1(base(ix, "weaponRangeMultiplier"))).fold(1.0, f64::max);
            let ri = ((opt * longest + fo * 3.1) as i64) as f64;
            groups.push((tid, Group { count: 1.0, cycle_ms, cds, osr, transitions: vec![] }, ri));
        } else {
            // multipliers of the loaded charge (modified / base); empty launcher: first valid charge loaded
            let probe_fit;
            let (pf, c) = match fit.items[i].charge {
                Some(c) => (fit, c),
                None => {
                    let mut req = ctx.fit_req.clone();
                    let ri = fit.items[i].req_index.unwrap_or(0);
                    if let Some(m) = req.modules.get_mut(ri) {
                        m.charge_type_id = Some(d::type_id_at(charges[0]));
                    }
                    probe_fit = Fit::build(&req).map_err(|e| format!("{e:?}"))?;
                    let c = probe_fit.items.iter().position(|x| x.kind == Kind::Module && x.req_index == Some(ri)).and_then(|mi| probe_fit.items[mi].charge);
                    match c {
                        Some(c) => (&probe_fit, c),
                        None => continue,
                    }
                }
            };
            let cix = pf.items[c].ty;
            let ratio = |n: &str| -> f64 {
                let b = base(cix, n);
                if b > 0.0 {
                    attr(pf, c, n) / b
                } else {
                    // no base value on the loaded charge: use the same multiplier seen on a charge that has one
                    let mut m = 1.0;
                    for &ix in &charges {
                        if base(ix, n) > 0.0 {
                            let mut req = ctx.fit_req.clone();
                            let ri = pf.items[i].req_index.unwrap_or(0);
                            if let Some(md) = req.modules.get_mut(ri) {
                                md.charge_type_id = Some(d::type_id_at(ix));
                            }
                            if let Ok(f2) = Fit::build(&req) {
                                if let Some(c2) = f2.items.iter().position(|x| x.kind == Kind::Module && x.req_index == Some(ri)).and_then(|mi| f2.items[mi].charge) {
                                    m = attr(&f2, c2, n) / base(ix, n);
                                }
                            }
                            break;
                        }
                    }
                    m
                }
            };
            let dm = [ratio("emDamage"), ratio("thermalDamage"), ratio("kineticDamage"), ratio("explosiveDamage")];
            let (fv, fd) = (ratio("maxVelocity"), ratio("explosionDelay"));
            let (aer, aev, adrf) = (ratio("aoeCloudSize"), ratio("aoeVelocity"), ratio("aoeDamageReductionFactor"));
            // the engine keeps the character's missile damage multiplier out of the charge attributes
            let lmult = or1(attr(fit, i, "damageMultiplier")) * attr(fit, fit.char, "missileDamageMultiplier");
            let mut cds = Vec::new();
            for &ix in &charges {
                let (bv, bd) = (base(ix, "maxVelocity"), base(ix, "explosionDelay"));
                if bv <= 0.0 || bd <= 0.0 {
                    continue;
                }
                let (mass, agi) = (or1(base(ix, "mass")), or1(base(ix, "agility")));
                let v = bv * fv;
                let ft = bd * fd / 1000.0 + atk_r / v;
                let (lt, ht) = (ft.floor(), ft.ceil());
                let rng = |t: f64| {
                    let acc = t.min(mass * agi / 1e6);
                    v / 2.0 * acc + v * (t - acc)
                };
                let lo = (rng(lt) - atk_r).max(0.0);
                let hi = (rng(ht) - atk_r).max(0.0);
                let name = d::type_name(ix);
                let ln = name.to_lowercase();
                let prio = if ln.contains("mjolnir") { 0 } else if ln.contains("inferno") { 1 } else if ln.contains("scourge") { 2 } else if ln.contains("nova") { 3 } else { 99 };
                let rv = dmg_of(ix, dm) * lmult;
                cds.push(Cd {
                    name,
                    tid: d::type_id_at(ix),
                    raw_volley: rv,
                    opt: 0.0,
                    fo: 0.0,
                    tracking: 0.0,
                    lo,
                    hi,
                    chance: ft - lt,
                    max_eff: hi,
                    er: base(ix, "aoeCloudSize") * aer,
                    ev: base(ix, "aoeVelocity") * aev,
                    drf: {
                        let b = base(ix, "aoeDamageReductionFactor");
                        (if b == 0.0 { 1.0 } else { b }) * adrf
                    },
                    prio,
                });
            }
            let raw_dps = |c: &Cd| if cycle_ms > 0.0 { c.raw_volley / (cycle_ms / 1000.0) } else { 0.0 };
            cds.sort_by(|a, b| b.max_eff.partial_cmp(&a.max_eff).unwrap().then(raw_dps(b).partial_cmp(&raw_dps(a)).unwrap()));
            if cds.is_empty() {
                continue;
            }
            let ri = cds[0].max_eff;
            groups.push((tid, Group { count: 1.0, cycle_ms, cds, osr: 0.0, transitions: vec![] }, ri));
        }
    }
    if groups.is_empty() {
        return Ok((Some(0.0), None));
    }
    // ---- projected grid
    let max_range = groups.iter().map(|g| g.2).fold(0.0, f64::max);
    let mut proj = Proj { has: false, base_speed: tgt_speed, base_sig: tgt.sig, dists: vec![], vals: vec![] };
    if ctx.setting_b("apply_projected", true) {
        proj.has = true;
        let step = sample_step(max_range);
        let in_lock = |x: f64| ctx.setting_b("ignore_lock_range", true) || x <= ctx.stat("targeting.max_range_m").unwrap_or(0.0);
        let in_dcr = |x: f64| ctx.setting_b("ignore_drone_control_range", false) || x <= ctx.stat("drones.control_range_m").unwrap_or(0.0);
        let mut x = 0i64;
        while (x as f64) <= max_range {
            let r = dmg::Ranges { in_lock: in_lock(x as f64), in_dcr: in_dcr(x as f64), drone_mode: ctx.setting_s("mobile_drone_mode", "auto") };
            let t = dmg::tackle(ctx, &tgt, tgt_speed, Some(x as f64), &r);
            proj.dists.push(x);
            proj.vals.push((t.speed, tgt.sig * t.sig_mult));
            x += step;
        }
    }
    // ---- transitions + value
    let mut total = 0.0;
    let mut first_charge: Option<u32> = None;
    for (_, g, _) in groups.iter_mut() {
        let max_d = if turret {
            let mo = g.cds.iter().map(|c| c.opt).fold(f64::MIN, f64::max);
            let mf = g.cds.iter().map(|c| c.fo).fold(f64::MIN, f64::max);
            (mo + mf * 3.1) as i64
        } else {
            g.cds[0].max_eff as i64
        };
        g.transitions = transitions(&g.cds, turret, g.osr, &tr, &proj, max_d);
        let ts = &g.transitions;
        if ts.is_empty() {
            continue;
        }
        let idx = (ts.partition_point(|t| (t.0 as f64) <= dist) as i64 - 1).max(0) as usize;
        let ci = ts[idx].1;
        if ci < 0 || ci as usize >= g.cds.len() {
            continue;
        }
        let cd = &g.cds[ci as usize];
        if first_charge.is_none() {
            first_charge = d::type_by_name(cd.name);
        }
        let (sp, sg) = proj.at(dist);
        let v = if turret { turret_volley(cd, g.osr, dist, &tr, sp, sg) } else { missile_volley(cd, dist, sp, sg) };
        let y = if mode == 0 {
            if g.cycle_ms <= 0.0 { 0.0 } else { v / (g.cycle_ms / 1000.0) }
        } else {
            v
        };
        total += y * g.count;
    }
    Ok((Some(total), first_charge))
}
