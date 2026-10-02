//! Fit text/JSON formats besides EFT: DNA (+ chat link), ESI fitting JSON, EVE client XML, multibuy.
//! Output layout follows the formats as Pyfa writes them (behaviour verified against Pyfa-generated cases in
//! eve-dogma-bench `formats-suite`; no Pyfa code is used here).
use crate::data as d;
use crate::eft;
use crate::engine::infer_slot;
use crate::request::*;

const CAT_CHARGE: u32 = 8;

fn ix(id: u32) -> Option<usize> {
    d::type_index(id)
}
fn name(id: u32) -> String {
    ix(id).map(|i| d::type_name(i).to_string()).unwrap_or_else(|| id.to_string())
}
fn slot_of(m: &ModuleReq) -> Option<Slot> {
    m.slot.or_else(|| ix(m.type_id).and_then(infer_slot))
}
fn base_attr(id: u32, attr: u16) -> Option<f64> {
    ix(id).and_then(|i| d::type_attr(i, attr))
}
fn category(id: u32) -> u32 {
    ix(id).map(|i| d::ty(i).category).unwrap_or(0)
}

/// Charges a module holds: whole number of charge volumes in the module type's (unmodified) capacity, 0 if
/// either is missing; scripts and other zero-count charges count as 1 where the formats list them.
pub fn num_charges(module: u32, charge: u32) -> u64 {
    let (Some(mi), Some(ci)) = (ix(module), ix(charge)) else { return 0 };
    let cap = d::type_capacity(mi);
    let vol = d::type_volume(ci);
    if vol == 0.0 {
        return 0;
    }
    let v = eft::float_unerr(cap / vol);
    if v.is_finite() && v > 0.0 { v as u64 } else { 0 }
}

/// Fighter squadron size as written by the formats: requested size if below the (modified) maximum, else the maximum.
fn fighter_amounts(req: &FitRequest) -> Vec<u64> {
    let fit = crate::engine::Fit::build(req).ok();
    req.fighters
        .iter()
        .enumerate()
        .map(|(fi, f)| {
            let modmax = fit.as_ref().and_then(|ft| {
                let k = (0..ft.items.len()).find(|&k| ft.items[k].kind == crate::engine::Kind::Fighter && ft.items[k].req_index == Some(fi))?;
                Some(ft.get(k, d::a::fighterSquadronMaxSize))
            });
            let maxsq = modmax.or_else(|| base_attr(f.type_id, d::a::fighterSquadronMaxSize)).unwrap_or(0.0);
            match f.quantity {
                Some(q) if q > 0 && (q as f64) < maxsq => q as u64,
                _ => maxsq as u64,
            }
        })
        .collect()
}

/// Insertion-ordered counter.
struct Counter<K: PartialEq + Clone>(Vec<(K, u64)>);
impl<K: PartialEq + Clone> Counter<K> {
    fn new() -> Self {
        Counter(Vec::new())
    }
    fn add(&mut self, k: K, n: u64) {
        match self.0.iter_mut().find(|x| x.0 == k) {
            Some(x) => x.1 += n,
            None => self.0.push((k, n)),
        }
    }
}

fn subsystem_slot(id: u32) -> f64 {
    base_attr(id, d::a::subSystemSlot).unwrap_or(0.0)
}

// ---------------------------------------------------------------------------------------------------------- DNA

/// DNA: `ship:subsystems(by subsystem slot);1:modules;n (first-seen order):drones:fighters:charges(loaded, then
/// cargo charges)::`, optionally wrapped as an in-game chat link `<url=fitting:…>name</url>`.
pub fn dna_export(req: &FitRequest, fit_name: &str, formatting: bool) -> String {
    let mut s = req.ship.type_id.to_string();
    let mut subs: Vec<u32> = Vec::new();
    let mut mods = Counter::new();
    let mut charges = Counter::new();
    for m in &req.modules {
        if slot_of(m) == Some(Slot::Subsystem) {
            subs.push(m.type_id);
            continue;
        }
        mods.add(m.type_id, 1);
        if let Some(c) = m.charge_type_id {
            charges.add(c, num_charges(m.type_id, c).max(1));
        }
    }
    subs.sort_by(|a, b| subsystem_slot(*a).partial_cmp(&subsystem_slot(*b)).unwrap());
    for t in subs {
        s += &format!(":{t};1");
    }
    for (t, n) in &mods.0 {
        s += &format!(":{t};{n}");
    }
    for dr in &req.drones {
        s += &format!(":{};{}", dr.type_id, dr.quantity);
    }
    for (f, n) in req.fighters.iter().zip(fighter_amounts(req)) {
        s += &format!(":{};{}", f.type_id, n);
    }
    for c in &req.cargo {
        if category(c.type_id) == CAT_CHARGE {
            charges.add(c.type_id, c.quantity as u64);
        }
    }
    for (t, n) in &charges.0 {
        s += &format!(":{t};{n}");
    }
    s += "::";
    if formatting {
        format!("<url=fitting:{s}>{fit_name}</url>")
    } else {
        s
    }
}

// ---------------------------------------------------------------------------------------------------------- ESI

const FLAG_CARGO: u64 = 5;
const FLAG_DRONE: u64 = 87;
const FLAG_FIGHTER: u64 = 158;

fn slot_flag_base(s: Slot) -> u64 {
    match s {
        Slot::Low => 11,
        Slot::Mid => 19,
        Slot::High => 27,
        Slot::Rig => 92,
        Slot::Subsystem => 125,
        Slot::Service => 164,
    }
}

/// Python `json.dumps` string escaping (ensure_ascii).
fn py_json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for ch in s.chars() {
        match ch {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            '\u{8}' => o.push_str("\\b"),
            '\u{c}' => o.push_str("\\f"),
            c if (c as u32) < 0x20 || (c as u32) > 0x7e => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    o.push_str(&format!("\\u{:04x}", u));
                }
            }
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// ESI fitting JSON (`POST /characters/{id}/fittings/` body) with Pyfa's key order and `json.dumps` spacing.
/// Err when there would be no items (ESI rejects empty fittings).
pub fn esi_export(req: &FitRequest, fit_name: &str, charges_on: bool, implants_on: bool, boosters_on: bool) -> Result<String, String> {
    let fname = if fit_name.chars().count() > 50 { fit_name.chars().take(47).collect::<String>() + "..." } else { fit_name.to_string() };
    let mut items: Vec<(u64, u64, u32)> = Vec::new();
    let mut next: Vec<(Slot, u64)> = Vec::new();
    let mut charges = Counter::new();
    for m in &req.modules {
        let Some(s) = slot_of(m) else { continue };
        let flag = if s == Slot::Subsystem {
            subsystem_slot(m.type_id) as u64
        } else {
            match next.iter_mut().find(|x| x.0 == s) {
                Some(x) => {
                    let f = x.1;
                    x.1 += 1;
                    f
                }
                None => {
                    let f = slot_flag_base(s);
                    next.push((s, f + 1));
                    f
                }
            }
        };
        items.push((flag, 1, m.type_id));
        if let (Some(c), true) = (m.charge_type_id, charges_on) {
            charges.add(c, num_charges(m.type_id, c).max(1));
        }
    }
    for c in &req.cargo {
        items.push((FLAG_CARGO, c.quantity as u64, c.type_id));
    }
    for (c, n) in &charges.0 {
        items.push((FLAG_CARGO, *n, *c));
    }
    for dr in &req.drones {
        items.push((FLAG_DRONE, dr.quantity as u64, dr.type_id));
    }
    for (f, n) in req.fighters.iter().zip(fighter_amounts(req)) {
        items.push((FLAG_FIGHTER, n, f.type_id));
    }
    if implants_on {
        for &i in &req.implants {
            items.push((FLAG_CARGO, 1, i));
        }
    }
    if boosters_on {
        for b in &req.boosters {
            items.push((FLAG_CARGO, 1, b.type_id));
        }
    }
    if items.is_empty() {
        return Err("Cannot export fitting: module list cannot be empty.".into());
    }
    let it: Vec<String> = items.iter().map(|(f, q, t)| format!("{{\"flag\": {f}, \"quantity\": {q}, \"type_id\": {t}}}")).collect();
    Ok(format!(
        "{{\"name\": {}, \"ship_type_id\": {}, \"description\": \"\", \"items\": [{}]}}",
        py_json_str(&fname),
        req.ship.type_id,
        it.join(", ")
    ))
}

// ---------------------------------------------------------------------------------------------------------- XML

/// XML attribute escaping as Python's minidom writes it.
fn xml_attr(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;").replace('>', "&gt;")
}

fn mutant_attrs(mu: &Mutation) -> String {
    eft::mutator_lines(mu).into_iter().map(|(a, v)| format!("{a} {}", eft::py_float(eft::float_unerr(v)))).collect::<Vec<_>>().join(", ")
}

fn xml_mutation(mu: &Option<Mutation>) -> String {
    match mu {
        Some(mu) => format!(
            " base_type=\"{}\" mutaplasmid=\"{}\" mutated_attrs=\"{}\"",
            xml_attr(&name(mu.base_type_id)),
            xml_attr(&mu.mutaplasmid_type_id.map(name).unwrap_or_default()),
            xml_attr(&mutant_attrs(mu))
        ),
        None => String::new(),
    }
}

/// EVE client fitting XML (`<fittings count=…>`), pretty-printed with tabs like minidom's `toprettyxml()`.
pub fn xml_export(fits: &[(&FitRequest, &str)]) -> String {
    let mut o = format!("<?xml version=\"1.0\" ?>\n<fittings count=\"{}\">\n", fits.len());
    for (req, fit_name) in fits {
        o += &format!("\t<fitting name=\"{}\">\n", xml_attr(fit_name));
        o += "\t\t<description value=\"\"/>\n";
        o += &format!("\t\t<shipType value=\"{}\"/>\n", xml_attr(&name(req.ship.type_id)));
        let mut next: Vec<(Slot, u64)> = Vec::new();
        let mut charges: Counter<String> = Counter::new();
        for m in &req.modules {
            let Some(s) = slot_of(m) else { continue };
            let sid = if s == Slot::Subsystem {
                (subsystem_slot(m.type_id) as i64 - 125) as u64
            } else {
                match next.iter_mut().find(|x| x.0 == s) {
                    Some(x) => {
                        x.1 += 1;
                        x.1 - 1
                    }
                    None => {
                        next.push((s, 1));
                        0
                    }
                }
            };
            let sn = match s {
                Slot::Low => "low",
                Slot::Mid => "med",
                Slot::High => "hi",
                Slot::Rig => "rig",
                Slot::Subsystem => "subsystem",
                Slot::Service => "service",
            };
            o += &format!("\t\t<hardware type=\"{}\" slot=\"{} slot {}\"{}/>\n", xml_attr(&name(m.type_id)), sn, sid, xml_mutation(&m.mutation));
            if let Some(c) = m.charge_type_id {
                charges.add(name(c), num_charges(m.type_id, c).max(1));
            }
        }
        for dr in &req.drones {
            o += &format!("\t\t<hardware qty=\"{}\" slot=\"drone bay\" type=\"{}\"{}/>\n", dr.quantity, xml_attr(&name(dr.type_id)), xml_mutation(&dr.mutation));
        }
        for (f, n) in req.fighters.iter().zip(fighter_amounts(req)) {
            o += &format!("\t\t<hardware qty=\"{}\" slot=\"fighter bay\" type=\"{}\"/>\n", n, xml_attr(&name(f.type_id)));
        }
        for c in &req.cargo {
            charges.add(name(c.type_id), c.quantity as u64);
        }
        for (cn, q) in &charges.0 {
            o += &format!("\t\t<hardware qty=\"{}\" slot=\"cargo\" type=\"{}\"/>\n", q, xml_attr(cn));
        }
        o += "\t</fitting>\n";
    }
    o += "</fittings>\n";
    o
}

// ----------------------------------------------------------------------------------------------------- multibuy

#[derive(Debug, Clone, Copy)]
pub struct MultibuyOpts {
    pub loaded_charges: bool,
    pub cargo: bool,
    pub implants: bool,
    pub boosters: bool,
}

/// Multibuy list: ship name, then every item once with its total count (` xN` when N > 1), sorted by
/// (category name, group name, type name). Mutated modules are left out (they can't be bought).
pub fn multibuy_export(req: &FitRequest, o: &MultibuyOpts) -> String {
    let mut c: Counter<u32> = Counter::new();
    for m in &req.modules {
        if m.mutation.is_some() {
            continue;
        }
        c.add(m.type_id, 1);
        if let (Some(ch), true) = (m.charge_type_id, o.loaded_charges) {
            c.add(ch, num_charges(m.type_id, ch));
        }
    }
    for dr in &req.drones {
        c.add(dr.type_id, dr.quantity as u64);
    }
    for (f, n) in req.fighters.iter().zip(fighter_amounts(req)) {
        c.add(f.type_id, n);
    }
    if o.cargo {
        for x in &req.cargo {
            c.add(x.type_id, x.quantity as u64);
        }
    }
    if o.implants {
        for &i in &req.implants {
            c.add(i, 1);
        }
    }
    if o.boosters {
        for b in &req.boosters {
            c.add(b.type_id, 1);
        }
    }
    let key = |t: u32| {
        let (g, cat) = ix(t).map(|i| (d::ty(i).group, d::ty(i).category)).unwrap_or((0, 0));
        (d::category_name(cat).unwrap_or("").to_string(), d::group_name(g).unwrap_or("").to_string(), name(t))
    };
    let mut v: Vec<((String, String, String), u32, u64)> = c.0.iter().map(|(t, n)| (key(*t), *t, *n)).collect();
    v.sort_by(|a, b| a.0.cmp(&b.0));
    let mut lines = vec![name(req.ship.type_id)];
    for (_, t, n) in v {
        lines.push(if n == 1 { name(t) } else { format!("{} x{}", name(t), n) });
    }
    lines.join("\n")
}
