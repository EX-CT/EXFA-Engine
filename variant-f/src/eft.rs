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

pub fn export(req: &FitRequest, name: &str) -> String {
    let n = |id: u32| type_ix(id).map(|ix| d::type_name(ix).to_string()).unwrap_or_else(|| id.to_string());
    let mut out = format!("[{}, {}]\n", n(req.ship.type_id), name);
    let mut muts: Vec<Mutation> = Vec::new();
    let mut tag = |m: &Option<Mutation>| -> String {
        match m {
            Some(m) => {
                muts.push(m.clone());
                format!(" [{}]", muts.len())
            }
            None => String::new(),
        }
    };
    for slot in [Slot::Low, Slot::Mid, Slot::High, Slot::Rig, Slot::Subsystem, Slot::Service] {
        let mut any = false;
        for m in req.modules.iter().filter(|m| m.slot.or_else(|| type_ix(m.type_id).and_then(infer_slot)) == Some(slot)) {
            any = true;
            match &m.mutation {
                Some(mu) => out += &n(mu.base_type_id),
                None => out += &n(m.type_id),
            }
            if let Some(c) = m.charge_type_id {
                out += &format!(", {}", n(c));
            }
            if m.state == Some(State::Offline) {
                out += " /OFFLINE";
            }
            out += &tag(&m.mutation);
            out += "\n";
        }
        if any {
            out += "\n";
        }
    }
    for dr in &req.drones {
        let nm = dr.mutation.as_ref().map(|m| m.base_type_id).unwrap_or(dr.type_id);
        out += &format!("{} x{}{}\n", n(nm), dr.quantity, tag(&dr.mutation));
    }
    for f in &req.fighters {
        out += &format!("{} x{}\n", n(f.type_id), f.quantity.unwrap_or(1));
    }
    if !req.implants.is_empty() || !req.boosters.is_empty() {
        out += "\n";
        for i in &req.implants {
            out += &format!("{}\n", n(*i));
        }
        for b in &req.boosters {
            out += &format!("{}\n", n(b.type_id));
        }
    }
    if !req.cargo.is_empty() {
        out += "\n";
        for c in &req.cargo {
            out += &format!("{} x{}\n", n(c.type_id), c.quantity);
        }
    }
    drop(tag);
    if !muts.is_empty() {
        out += "\n";
        for (k, m) in muts.iter().enumerate() {
            out += &format!("[{}] {}\n", k + 1, n(m.base_type_id));
            if let Some(p) = m.mutaplasmid_type_id {
                out += &format!("  {}\n", n(p));
            }
            let kv: Vec<String> = m
                .attributes
                .iter()
                .map(|(a, v)| {
                    let an = a.parse::<u16>().ok().and_then(d::attr_name).map(|x| x.to_string()).unwrap_or(a.clone());
                    format!("{an} {v}")
                })
                .collect();
            if !kv.is_empty() {
                out += &format!("  {}\n", kv.join(", "));
            }
        }
    }
    out
}
