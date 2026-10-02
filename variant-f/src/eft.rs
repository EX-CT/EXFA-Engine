//! Minimal EFT text import/export. Adapted from eve-dogma-rs `src/eft.rs` (LGPL-3.0-or-later; written clean-room
//! there from the public EVE fitting text format), using this crate's compiled data tables.
use crate::data as d;
use crate::engine::infer_slot;
use crate::request::*;
use std::collections::HashMap;

const CAT_IMPLANT: u32 = 20;
const CAT_DRONE: u32 = 18;
const CAT_FIGHTER: u32 = 87;
const CAT_CHARGE: u32 = 8;
const GROUP_T3D_MODE: u32 = 1306;
const ATTR_BOOSTERNESS: u16 = 1087;

fn type_ix(id: u32) -> Option<usize> {
    d::type_index(id)
}

/// strip a trailing " [N]" mutation reference
fn mut_ref(line: &str) -> (&str, Option<u32>) {
    let l = line.trim_end();
    if l.ends_with(']') {
        if let Some(p) = l.rfind(" [") {
            if let Ok(n) = l[p + 2..l.len() - 1].parse::<u32>() {
                return (l[..p].trim_end(), Some(n));
            }
        }
    }
    (l, None)
}

/// Parse the trailing mutation blocks:  "[N] Base Name" / "  Mutaplasmid Name" / "  attr value, attr value"
fn parse_mutations(text: &str) -> Result<(HashMap<u32, Mutation>, usize), String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = HashMap::new();
    let is_head = |l: &str| {
        let t = l.trim();
        t.starts_with('[') && t.find(']').map(|e| t[1..e].parse::<u32>().is_ok()).unwrap_or(false)
    };
    let first = lines.iter().position(|l| is_head(l)).unwrap_or(lines.len());
    let mut i = first;
    while i < lines.len() {
        let t = lines[i].trim();
        if !is_head(t) {
            i += 1;
            continue;
        }
        let e = t.find(']').unwrap();
        let n: u32 = t[1..e].parse().unwrap();
        let base_name = t[e + 1..].trim();
        let base = d::type_by_name(base_name).ok_or(format!("unknown mutated base '{base_name}'"))?;
        let mut m = Mutation { base_type_id: base, mutaplasmid_type_id: None, attributes: Default::default() };
        i += 1;
        while i < lines.len() && !is_head(lines[i]) {
            let l = lines[i].trim();
            i += 1;
            if l.is_empty() {
                continue;
            }
            if m.mutaplasmid_type_id.is_none() {
                m.mutaplasmid_type_id = Some(d::type_by_name(l).ok_or(format!("unknown mutaplasmid '{l}'"))?);
                continue;
            }
            for kv in l.split(',') {
                let kv = kv.trim();
                if let Some((k, v)) = kv.rsplit_once(' ') {
                    if let Some(aid) = d::attr_by_name(k.trim()) {
                        if let Ok(v) = v.trim().parse::<f64>() {
                            m.attributes.insert(aid.to_string(), v);
                        }
                    }
                }
            }
        }
        out.insert(n, m);
    }
    Ok((out, first))
}

fn mutated_type(m: &Mutation) -> u32 {
    m.mutaplasmid_type_id.and_then(|mu| d::muta_output(mu, m.base_type_id)).unwrap_or(m.base_type_id)
}

pub fn parse(text: &str) -> Result<FitRequest, String> {
    let (muts, first_mut_line) = parse_mutations(text)?;
    let body: Vec<&str> = text.lines().take(first_mut_line).collect();
    let mut lines = body.iter().map(|l| l.trim()).filter(|l| !l.is_empty());
    let header = lines.next().ok_or("empty EFT")?;
    let h = header.trim_start_matches('[').trim_end_matches(']');
    let ship_name = h.split(',').next().unwrap_or("").trim();
    let ship = d::type_by_name(ship_name).ok_or(format!("unknown ship '{ship_name}'"))?;
    // serde defaults (validate = true, ...) for everything not in the text
    let mut req: FitRequest = serde_json::from_str(&format!("{{\"schema_version\":1,\"ship\":{{\"type_id\":{ship}}}}}")).map_err(|e| e.to_string())?;
    req.options.validate = true;
    for line in lines {
        if line.starts_with("[Empty") {
            continue;
        }
        let (line, offline) = match line.strip_suffix("/OFFLINE").or_else(|| line.strip_suffix("/offline")) {
            Some(l) => (l.trim(), true),
            None => (line, false),
        };
        let (line, mref) = mut_ref(line);
        let mutation = match mref {
            Some(n) => Some(muts.get(&n).cloned().ok_or(format!("mutation [{n}] not defined"))?),
            None => None,
        };
        // "Name xN" => drone / fighter / cargo
        if let Some(pos) = line.rfind(" x") {
            if let Ok(n) = line[pos + 2..].trim().parse::<u32>() {
                let name = line[..pos].trim();
                let Some(mut tid) = d::type_by_name(name) else { return Err(format!("unknown item '{name}'")) };
                if let Some(m) = &mutation {
                    tid = mutated_type(m);
                }
                let ix = type_ix(tid).ok_or(format!("unknown item '{name}'"))?;
                match d::ty(ix).category {
                    CAT_DRONE => req.drones.push(DroneReq { type_id: tid, quantity: n, active: Some(n), mutation: mutation.clone() }),
                    CAT_FIGHTER => req.fighters.push(FighterReq { type_id: tid, quantity: Some(n), active: true, abilities: None }),
                    _ => req.cargo.push(CargoReq { type_id: tid, quantity: n }),
                }
                continue;
            }
        }
        let mut parts = line.splitn(2, ',');
        let name = parts.next().unwrap().trim();
        let charge = parts.next().map(|s| s.trim());
        let Some(mut tid) = d::type_by_name(name) else { return Err(format!("unknown item '{name}'")) };
        if let Some(m) = &mutation {
            tid = mutated_type(m);
        }
        let ix = type_ix(tid).ok_or(format!("unknown item '{name}'"))?;
        let t = d::ty(ix);
        match t.category {
            CAT_IMPLANT => {
                if d::type_attr(ix, ATTR_BOOSTERNESS).is_some() {
                    req.boosters.push(BoosterReq { type_id: tid, side_effects: vec![] })
                } else {
                    req.implants.push(tid)
                }
            }
            CAT_DRONE => req.drones.push(DroneReq { type_id: tid, quantity: 1, active: Some(1), mutation: mutation.clone() }),
            CAT_CHARGE => req.cargo.push(CargoReq { type_id: tid, quantity: 1 }),
            _ => {
                if t.group == GROUP_T3D_MODE {
                    req.ship.mode_type_id = Some(tid);
                    continue;
                }
                let slot = infer_slot(ix);
                let charge_type_id = match charge {
                    Some(c) => Some(d::type_by_name(c).ok_or(format!("unknown charge '{c}'"))?),
                    None => None,
                };
                let active_capable = d::type_effects(ix).iter().any(|&x| d::EFF_META[(x >> 1) as usize].cat == 1)
                    || d::type_attr(ix, 6).map(|v| v != 0.0).unwrap_or(false);
                let state = if offline {
                    State::Offline
                } else if active_capable && !matches!(slot, Some(Slot::Rig) | Some(Slot::Subsystem)) {
                    State::Active
                } else {
                    State::Online
                };
                req.modules.push(ModuleReq { type_id: tid, slot, state: Some(state), charge_type_id, mutation: mutation.clone(), spool: None });
            }
        }
    }
    Ok(req)
}

/// Python `repr(float)` (shortest round-trip, always with a fractional part or exponent).
pub(crate) fn py_float(x: f64) -> String {
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x.is_nan() {
        return "nan".into();
    }
    let a = x.abs();
    if a != 0.0 && !(1e-4..1e16).contains(&a) {
        let s = format!("{x:e}");
        let (m, e) = s.split_once('e').unwrap();
        let e: i32 = e.parse().unwrap();
        return format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs());
    }
    let s = format!("{x}");
    if s.contains('.') { s } else { s + ".0" }
}

/// Pyfa `floatUnerr` (7 significant digits kept).
pub(crate) fn float_unerr(x: f64) -> f64 {
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

/// Pyfa mutator values: every mutaplasmid attribute, request value (or base value) clamped to the roll range.
pub(crate) fn mutator_lines(m: &Mutation) -> Vec<(String, f64)> {
    let req_val = |aid: u16| m.attributes.iter().find(|(k, _)| k.parse::<u16>().ok() == Some(aid) || d::attr_by_name(k) == Some(aid)).map(|x| *x.1);
    let base_ix = type_ix(m.base_type_id);
    let mut out: Vec<(String, f64)> = Vec::new();
    match m.mutaplasmid_type_id.and_then(d::muta_attrs) {
        Some(attrs) => {
            for &(aid, lo, hi) in attrs {
                let base = base_ix.and_then(|ix| d::type_attr(ix, aid)).unwrap_or(0.0);
                let val = req_val(aid).unwrap_or(base);
                let v = if base == 0.0 {
                    0.0
                } else {
                    let (lo, hi) = (round3(lo), round3(hi));
                    let r = val / base;
                    if lo <= r && r <= hi {
                        val
                    } else {
                        let (a, b) = (lo * base, hi * base);
                        val.max(a.min(b)).min(a.max(b))
                    }
                };
                out.push((d::attr_name(aid).map(|s| s.to_string()).unwrap_or(aid.to_string()), v));
            }
        }
        None => {
            for (k, v) in &m.attributes {
                let an = k.parse::<u16>().ok().and_then(d::attr_name).map(|x| x.to_string()).unwrap_or(k.clone());
                out.push((an, *v));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn round3(x: f64) -> f64 {
    format!("{x:.3}").parse().unwrap_or(x)
}

/// EFT export, byte-compatible with Pyfa `exportEft` (all options on) after the GUI's `fill()`: empty slots are
/// written as `[Empty X slot]` up to the ship's (modified) slot counts.
pub fn export(req: &FitRequest, name: &str) -> String {
    export_opts(req, name, &EftOpts::default())
}

/// Pyfa `PortEftOptions` switches (all on = `export`).
#[derive(Debug, Clone, Copy)]
pub struct EftOpts {
    pub implants: bool,
    pub mutations: bool,
    pub loaded_charges: bool,
    pub boosters: bool,
    pub cargo: bool,
}

impl Default for EftOpts {
    fn default() -> Self {
        EftOpts { implants: true, mutations: true, loaded_charges: true, boosters: true, cargo: true }
    }
}

/// EFT export with Pyfa's option switches: charges, implants, boosters, cargo sections and the mutation block
/// (mutations off: mutated items are written under their base type name without a `[n]` reference).
pub fn export_opts(req: &FitRequest, name: &str, o: &EftOpts) -> String {
    let n = |id: u32| type_ix(id).map(|ix| d::type_name(ix).to_string()).unwrap_or_else(|| id.to_string());
    let header = format!("[{}, {}]", n(req.ship.type_id), name);
    let fit = crate::engine::Fit::build(req).ok();
    let ship_attr = |attr: u16| fit.as_ref().map(|f| f.get(f.ship, attr)).unwrap_or(0.0);
    let mut muts: Vec<Mutation> = Vec::new();
    let mut sections: Vec<String> = Vec::new();
    // modules
    let mut racks: Vec<String> = Vec::new();
    for (slot, attr, label) in [
        (Slot::Low, d::a::lowSlots, "Low"),
        (Slot::Mid, d::a::medSlots, "Med"),
        (Slot::High, d::a::hiSlots, "High"),
        (Slot::Rig, d::a::rigSlots, "Rig"),
        (Slot::Subsystem, d::a::maxSubSystems, "Subsystem"),
        (Slot::Service, d::a::serviceSlots, "Service"),
    ] {
        let mut lines: Vec<String> = Vec::new();
        for m in req.modules.iter().filter(|m| m.slot.or_else(|| type_ix(m.type_id).and_then(infer_slot)) == Some(slot)) {
            let mut l = match &m.mutation {
                Some(mu) => n(mu.base_type_id),
                None => n(m.type_id),
            };
            let off = if m.state == Some(State::Offline) { " /offline" } else { "" };
            if let Some(c) = m.charge_type_id.filter(|_| o.loaded_charges) {
                l += &format!(", {}", n(c));
            }
            l += off;
            if let Some(mu) = m.mutation.as_ref().filter(|_| o.mutations) {
                muts.push(mu.clone());
                l += &format!(" [{}]", muts.len());
            }
            lines.push(l);
        }
        let total = ship_attr(attr) as i64;
        for _ in (lines.len() as i64)..total {
            lines.push(format!("[Empty {label} slot]"));
        }
        if !lines.is_empty() {
            racks.push(lines.join("\n"));
        }
    }
    if !racks.is_empty() {
        sections.push(racks.join("\n\n"));
    }
    // drones, fighters
    let mut minion: Vec<String> = Vec::new();
    let mut drones: Vec<&DroneReq> = req.drones.iter().collect();
    let dname = |dr: &DroneReq| match &dr.mutation {
        Some(mu) => n(mu.base_type_id),
        None => n(dr.type_id),
    };
    let dfull = |dr: &DroneReq| match &dr.mutation {
        Some(mu) => {
            let muta = mu.mutaplasmid_type_id.map(n).unwrap_or_default();
            let short = muta.split(' ').next().unwrap_or("").to_string();
            format!("{short} {}", n(mu.base_type_id))
        }
        None => n(dr.type_id),
    };
    drones.sort_by(|a, b| {
        let ka = (d::drone_eft_rank(a.mutation.as_ref().map(|m| m.base_type_id).unwrap_or(a.type_id)), a.mutation.is_some(), dfull(a));
        let kb = (d::drone_eft_rank(b.mutation.as_ref().map(|m| m.base_type_id).unwrap_or(b.type_id)), b.mutation.is_some(), dfull(b));
        ka.cmp(&kb)
    });
    let mut dl: Vec<String> = Vec::new();
    for dr in drones {
        let mut l = format!("{} x{}", dname(dr), dr.quantity);
        if let Some(mu) = dr.mutation.as_ref().filter(|_| o.mutations) {
            muts.push(mu.clone());
            l += &format!(" [{}]", muts.len());
        }
        dl.push(l);
    }
    if !dl.is_empty() {
        minion.push(dl.join("\n"));
    }
    const FIGHTER_ORDER: [&str; 6] =
        ["Light Fighter", "Structure Light Fighter", "Heavy Fighter", "Structure Heavy Fighter", "Support Fighter", "Structure Support Fighter"];
    let mod_sq = |i: usize| -> Option<f64> {
        let f = fit.as_ref()?;
        let k = (0..f.items.len()).find(|&k| f.items[k].kind == crate::engine::Kind::Fighter && f.items[k].req_index == Some(i))?;
        Some(f.get(k, d::a::fighterSquadronMaxSize))
    };
    let mut fighters: Vec<(usize, String, String)> = req
        .fighters
        .iter()
        .enumerate()
        .map(|(fi, f)| {
            let ix = type_ix(f.type_id);
            let g = ix.and_then(|ix| d::group_name(d::TYPES[ix].group)).unwrap_or("");
            let rank = FIGHTER_ORDER.iter().position(|x| *x == g).unwrap_or(99);
            // Pyfa Fighter.amount: a quantity >= squadron max size (or none) means "full squadron"
            let maxsq = mod_sq(fi).or_else(|| ix.and_then(|ix| d::type_attr(ix, d::a::fighterSquadronMaxSize))).unwrap_or(0.0);
            let qty = match f.quantity {
                Some(q) if q > 0 && (q as f64) < maxsq => q as i64,
                _ => maxsq as i64,
            };
            (rank, n(f.type_id), format!("{} x{}", n(f.type_id), qty))
        })
        .collect();
    fighters.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));
    if !fighters.is_empty() {
        minion.push(fighters.iter().map(|f| f.2.clone()).collect::<Vec<_>>().join("\n"));
    }
    if !minion.is_empty() {
        sections.push(minion.join("\n\n"));
    }
    // implants, boosters (sorted by slot, stable)
    let slot_of = |id: u32, attr: u16| type_ix(id).and_then(|ix| d::type_attr(ix, attr)).unwrap_or(0.0);
    let mut chr: Vec<String> = Vec::new();
    let mut imps: Vec<u32> = req.implants.clone();
    imps.sort_by(|a, b| slot_of(*a, d::a::implantness).partial_cmp(&slot_of(*b, d::a::implantness)).unwrap());
    if !imps.is_empty() && o.implants {
        chr.push(imps.iter().map(|&i| n(i)).collect::<Vec<_>>().join("\n"));
    }
    let mut boos: Vec<u32> = req.boosters.iter().map(|b| b.type_id).collect();
    boos.sort_by(|a, b| slot_of(*a, ATTR_BOOSTERNESS).partial_cmp(&slot_of(*b, ATTR_BOOSTERNESS)).unwrap());
    if !boos.is_empty() && o.boosters {
        chr.push(boos.iter().map(|&i| n(i)).collect::<Vec<_>>().join("\n"));
    }
    if !chr.is_empty() {
        sections.push(chr.join("\n\n"));
    }
    // cargo sorted by (category name, group name, type name)
    let mut cargo: Vec<((String, String, String), String)> = req
        .cargo
        .iter()
        .map(|c| {
            let ix = type_ix(c.type_id);
            let g = ix.map(|ix| d::TYPES[ix].group).unwrap_or(0);
            let cat = ix.map(|ix| d::TYPES[ix].category).unwrap_or(0);
            let key = (d::category_name(cat).unwrap_or("").to_string(), d::group_name(g).unwrap_or("").to_string(), n(c.type_id));
            (key, format!("{} x{}", n(c.type_id), c.quantity))
        })
        .collect();
    cargo.sort_by(|a, b| a.0.cmp(&b.0));
    if !cargo.is_empty() && o.cargo {
        sections.push(cargo.into_iter().map(|c| c.1).collect::<Vec<_>>().join("\n"));
    }
    // mutated items
    if !muts.is_empty() {
        let mut ml: Vec<String> = Vec::new();
        for (k, m) in muts.iter().enumerate() {
            let mut t = format!("[{}] {}", k + 1, n(m.base_type_id));
            t += &format!("\n  {}", m.mutaplasmid_type_id.map(n).unwrap_or_default());
            let kv: Vec<String> = mutator_lines(m).into_iter().map(|(a, v)| format!("{a} {}", py_float(float_unerr(v)))).collect();
            t += &format!("\n  {}", kv.join(", "));
            ml.push(t);
        }
        sections.push(ml.join("\n"));
    }
    format!("{header}\n\n{}", sections.join("\n\n\n"))
}
