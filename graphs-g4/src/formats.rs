//! Fit text/JSON formats besides EFT: DNA (+ chat link), ESI fitting JSON, EVE client XML, multibuy.
//! Output layout follows the formats as Pyfa writes them (behaviour verified against Pyfa-generated cases in
//! eve-dogma-bench `formats-suite`; no Pyfa code is used here).
use crate::data as d;
use crate::eft;
use crate::engine::infer_slot;
use crate::request::*;
use serde_json::Value;

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

// ======================================================================================================= imports
//
// Importers reproduce what Pyfa builds from each format (verified against Pyfa-generated round-trip cases):
// modules get the highest state Pyfa gives on import, subsystems are fitted first, then every other module is
// kept only if it still fits (free slot, hardpoint, ship restriction, rig size, max-group, capital size), drones
// arrive with 0 active, and charges that the format carries as cargo stay in cargo.

const CAT_MODULE: u32 = 7;
const CAT_SUBSYSTEM: u32 = 32;
const CAT_STRUCTURE_MODULE: u32 = 66;
const CAT_SHIP: u32 = 6;
const CAT_STRUCTURE: u32 = 65;
const CAT_DRONE: u32 = 18;
const CAT_FIGHTER: u32 = 87;
const CAT_IMPLANT: u32 = 20;

/// Effects for which an imported module stays online instead of active (cloaks, MJD, cyno, bastion-like toggles, ...).
const ONLINE_ONLY_EFFECTS: &[&str] = &[
    "moduleBonusAssaultDamageControl", "moduleBonusIndustrialInvulnerability", "microJumpDrive", "microJumpPortalDrive",
    "emergencyHullEnergizer", "cynosuralGeneration", "jumpPortalGeneration", "jumpPortalGenerationBO",
    "cloneJumpAccepting", "cloakingWarpSafe", "cloakingPrototype", "cloaking", "massEntanglerEffect5",
    "electronicAttributeModifyOnline", "targetPassively", "cargoScan", "shipScan", "surveyScan",
    "targetSpectrumBreakerBonus", "interdictionNullifierBonus", "warpCoreStabilizerActive", "industrialItemCompression",
];

fn effect_names(t: u32) -> Vec<&'static str> {
    ix(t).map(|i| d::type_effects(i).iter().map(|&x| d::eff_name((x >> 1) as usize)).collect()).unwrap_or_default()
}

fn can_be_active(t: u32) -> bool {
    let Some(i) = ix(t) else { return false };
    d::type_effects(i).iter().any(|&x| matches!(d::EFF_META[(x >> 1) as usize].cat, 1 | 2))
        && d::type_attr(i, d::a::activationBlocked).unwrap_or(0.0) <= 0.0
}

/// State an imported module gets: active when it can be activated, unless it is one of the online-only kinds.
fn import_state(t: u32) -> State {
    if can_be_active(t) {
        let effs = effect_names(t);
        if effs.iter().any(|e| ONLINE_ONLY_EFFECTS.contains(e)) {
            State::Online
        } else {
            State::Active
        }
    } else {
        State::Online
    }
}

/// Item publicity as Pyfa's item database sees it (behavioural rule, checked against the oracle's data; no Pyfa
/// code): SDE `published`, but Civilian modules/charges (not shuttles), the Capsule, effect beacons (group 4033)
/// and the Metenox drill are public; Limited Synth / Expired / Grand Prix / Mining Blitz boosters are not; and
/// anything named *abyssal* / *mutated* (mutated output types, except two skills and mining crystals) plus the
/// Drifter "Lux" weapons is not.
fn published(t: u32) -> bool {
    let Some(i) = ix(t) else { return false };
    let n = d::type_name(i);
    let l = n.to_lowercase();
    let mut p = d::type_published(i);
    if (n.starts_with("Civilian") && !n.contains("Shuttle")) || n == "Capsule" || d::ty(i).group == 4033 || t == 82941 {
        p = true;
    } else if n.starts_with("Limited Synth ") || n.starts_with("Expired ") || (n.starts_with("Grand Prix ") && n.contains("Booster"))
        || (n.starts_with("Mining Blitz ") && n.contains(" Booster Dose "))
    {
        p = false;
    }
    if (l.contains("abyssal") || l.contains("mutated") || l.contains("_placeholder") || matches!(n, "Lux Kontos" | "Lux Xiphos" | "Lux Ballistra" | "Lux Kopis"))
        && !matches!(n, "Abyssal Ore Processing" | "Mutated Drone Specialization")
        && !n.contains("Asteroid Mining Crystal")
    {
        p = false;
    }
    p
}

/// A fighter fits when the hull has a tube of its class (light/support/heavy, standup variants).
fn fighter_fits(ship: u32, fighter: u32) -> bool {
    for (class, tubes) in [
        ("fighterSquadronIsLight", "fighterLightSlots"),
        ("fighterSquadronIsSupport", "fighterSupportSlots"),
        ("fighterSquadronIsHeavy", "fighterHeavySlots"),
        ("fighterSquadronIsStandupLight", "fighterStandupLightSlots"),
        ("fighterSquadronIsStandupSupport", "fighterStandupSupportSlots"),
        ("fighterSquadronIsStandupHeavy", "fighterStandupHeavySlots"),
    ] {
        let Some(ca) = d::attr_by_name(class) else { continue };
        if base_attr(fighter, ca).unwrap_or(0.0) != 0.0 {
            return d::attr_by_name(tubes).and_then(|ta| base_attr(ship, ta)).unwrap_or(0.0) > 0.0;
        }
    }
    false
}

fn is_module_cat(t: u32) -> bool {
    matches!(category(t), CAT_MODULE | CAT_SUBSYSTEM | CAT_STRUCTURE_MODULE)
}

/// Module type as fitted (mutated result type when a mutation is given).
fn fitted_type(base: u32, mu: &Option<Mutation>) -> u32 {
    match mu {
        Some(m) => m.mutaplasmid_type_id.and_then(|x| d::muta_output(x, base)).unwrap_or(base),
        None => base,
    }
}

/// Complete mutation (every mutaplasmid attribute; given values clamped to the roll range, others at base).
fn full_mutation(base: u32, muta: u32, given: &[(u16, f64)]) -> Mutation {
    let mut m = Mutation { base_type_id: base, mutaplasmid_type_id: Some(muta), attributes: Default::default() };
    for (a, v) in given {
        m.attributes.insert(a.to_string(), *v);
    }
    let mut out = Mutation { base_type_id: base, mutaplasmid_type_id: Some(muta), attributes: Default::default() };
    for (an, v) in eft::mutator_lines(&m) {
        if let Some(aid) = d::attr_by_name(&an) {
            out.attributes.insert(aid.to_string(), v);
        }
    }
    out
}

fn parse_mutant_attrs(line: &str) -> Vec<(u16, f64)> {
    let mut v = Vec::new();
    for pair in line.split(',') {
        let p: Vec<&str> = pair.trim().split(' ').collect();
        if p.len() != 2 {
            continue;
        }
        let (Some(aid), Ok(val)) = (d::attr_by_name(p[0].trim()), p[1].parse::<f64>()) else { continue };
        v.push((aid, val));
    }
    v
}

/// Ship-side fitting limits after subsystems (modified values).
struct Limits {
    slots: Vec<(Slot, i64)>,
    turrets: i64,
    launchers: i64,
    rig_size: f64,
    capital: bool,
    structure: bool,
    ship_group: u32,
    ship: u32,
}

fn limits(ship: u32, subs: &[ModuleReq]) -> Limits {
    let mut req: FitRequest = serde_json::from_str(&format!("{{\"schema_version\":1,\"ship\":{{\"type_id\":{ship}}}}}")).unwrap();
    req.modules = subs.to_vec();
    let fit = crate::engine::Fit::build(&req).ok();
    let g = |a: u16| fit.as_ref().map(|f| f.get(f.ship, a)).or_else(|| base_attr(ship, a)).unwrap_or(0.0);
    Limits {
        slots: vec![
            (Slot::Low, g(d::a::lowSlots) as i64),
            (Slot::Mid, g(d::a::medSlots) as i64),
            (Slot::High, g(d::a::hiSlots) as i64),
            (Slot::Rig, g(d::a::rigSlots) as i64),
            (Slot::Subsystem, g(d::a::maxSubSystems) as i64),
            (Slot::Service, g(d::a::serviceSlots) as i64),
        ],
        turrets: g(d::a::turretSlotsLeft) as i64,
        launchers: g(d::a::launcherSlotsLeft) as i64,
        rig_size: g(d::a::rigSize),
        capital: g(d::a::isCapitalSize) == 1.0,
        structure: category(ship) == CAT_STRUCTURE,
        ship_group: ix(ship).map(|i| d::ty(i).group).unwrap_or(0),
        ship,
    }
}

fn hardpoint(t: u32) -> u8 {
    let e = effect_names(t);
    if e.contains(&"turretFitted") {
        1
    } else if e.contains(&"launcherFitted") {
        2
    } else {
        0
    }
}

/// Ship restrictions of a module type (fitsToShipType / canFitShipType* / canFitShipGroup*) and structure-ness.
fn can_fit_ship(t: u32, l: &Limits) -> bool {
    let Some(i) = ix(t) else { return false };
    let (mut types, mut groups) = (Vec::new(), Vec::new());
    for &aid in d::type_attr_ids(i) {
        let n = d::attr_name(aid).unwrap_or("");
        let v = d::type_attr(i, aid).unwrap_or(0.0) as u32;
        if aid == d::a::fitsToShipType || n.starts_with("canFitShipType") {
            types.push(v);
        } else if n.starts_with("canFitShipGroup") {
            groups.push(v);
        }
    }
    if (!types.is_empty() || !groups.is_empty()) && !groups.contains(&l.ship_group) && !types.contains(&l.ship) {
        return false;
    }
    (category(t) == CAT_STRUCTURE_MODULE) == l.structure
}

/// Would `m` still fit next to `fitted` (Pyfa `Module.fits`)? `lenient` reproduces the DNA importer, which
/// attaches modules to the fit before checking and so lets one extra module per slot type / hardpoint through.
fn fits(m: &ModuleReq, fitted: &[ModuleReq], l: &Limits) -> bool {
    fits_x(m, fitted, l, false)
}

fn fits_x(m: &ModuleReq, fitted: &[ModuleReq], l: &Limits, lenient: bool) -> bool {
    let Some(slot) = slot_of(m) else { return false };
    let total = l.slots.iter().find(|x| x.0 == slot).map(|x| x.1).unwrap_or(0);
    let used = fitted.iter().filter(|f| slot_of(f) == Some(slot)).count() as i64;
    let extra = if lenient { 1 } else { 0 };
    if total - used <= -extra {
        return false;
    }
    if !can_fit_ship(m.type_id, l) {
        return false;
    }
    if !l.structure && !l.capital && base_attr(m.type_id, 161).unwrap_or(0.0) >= 4000.0 {
        return false;
    }
    if slot == Slot::Subsystem && fitted.iter().any(|f| subsystem_slot(f.type_id) == subsystem_slot(m.type_id)) {
        return false;
    }
    if slot == Slot::Rig && base_attr(m.type_id, d::a::rigSize).unwrap_or(0.0) != l.rig_size {
        return false;
    }
    let base_for_group = m.mutation.as_ref().map(|x| x.base_type_id).unwrap_or(m.type_id);
    if let Some(maxg) = base_attr(base_for_group, d::a::maxGroupFitted).filter(|v| *v > 0.0) {
        let g = ix(m.type_id).map(|i| d::ty(i).group);
        let n = fitted.iter().filter(|f| ix(f.type_id).map(|i| d::ty(i).group) == g).count() as f64;
        if n >= maxg {
            return false;
        }
    }
    match hardpoint(m.type_id) {
        1 => fitted.iter().filter(|f| hardpoint(f.type_id) == 1).count() as i64 + 1 <= l.turrets + extra,
        2 => fitted.iter().filter(|f| hardpoint(f.type_id) == 2).count() as i64 + 1 <= l.launchers + extra,
        _ => true,
    }
}

/// Fit assembly shared by the DNA / ESI / XML importers: subsystems first (each checked as it comes), then the
/// remaining modules in order, each kept only if it fits.
fn assemble(ship: u32, subs: Vec<ModuleReq>, mods: Vec<ModuleReq>, lenient: bool) -> Vec<ModuleReq> {
    let l0 = limits(ship, &[]);
    let mut fitted: Vec<ModuleReq> = Vec::new();
    for s in subs {
        if fits(&s, &fitted, &l0) {
            fitted.push(s);
        }
    }
    let l = limits(ship, &fitted);
    for m in mods {
        if fits_x(&m, &fitted, &l, lenient) {
            fitted.push(m);
        }
    }
    fitted
}

fn new_req(ship: u32) -> FitRequest {
    let mut r: FitRequest = serde_json::from_str(&format!("{{\"schema_version\":1,\"ship\":{{\"type_id\":{ship}}}}}")).unwrap();
    r.ship.mode_type_id = d::default_mode(ship);
    r
}

fn module_req(t: u32, mutation: Option<Mutation>) -> ModuleReq {
    ModuleReq { type_id: t, slot: ix(t).and_then(infer_slot), state: Some(import_state(t)), charge_type_id: None, mutation, spool: None }
}

/// An imported fit: the request plus Pyfa's fit name / notes.
pub struct Imported {
    pub name: String,
    pub notes: Option<String>,
    pub req: FitRequest,
}

fn is_hull(t: u32) -> bool {
    matches!(category(t), CAT_SHIP | CAT_STRUCTURE)
}

// ---- DNA

/// DNA import (`sep` ';' for `ship:id;n:…::`, '*' for the `DNA:ship:id*n` form). The fit name is the link text
/// when given, else "<Ship> - DNA Imported". Charges go to cargo, every module is added `n` times.
pub fn dna_import(text: &str, fit_name: Option<&str>, alt: bool) -> Result<Imported, String> {
    // start at the first number that is a ship or structure
    let mut start = None;
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i].is_ascii_digit() {
            let j = (i..b.len()).find(|&k| !b[k].is_ascii_digit()).unwrap_or(b.len());
            if let Ok(id) = text[i..j].parse::<u32>() {
                if is_hull(id) {
                    start = Some(text.find(&text[i..j]).unwrap());
                    break;
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    let mut s = &text[start.ok_or("no ship in DNA")?..];
    let sep = if alt { '*' } else { ';' };
    if !alt {
        let e = s.find("::").ok_or("DNA without '::'")?;
        s = &s[..e + 2];
    }
    let info: Vec<&str> = s.split(':').collect();
    let ship: u32 = info[0].trim().parse().map_err(|_| "bad ship id")?;
    if !is_hull(ship) {
        return Err("not a ship".into());
    }
    let mut req = new_req(ship);
    let name = fit_name.map(|x| x.to_string()).unwrap_or_else(|| format!("{} - DNA Imported", name(ship)));
    let (mut subs, mut mods) = (Vec::new(), Vec::new());
    for it in &info[1..] {
        if it.is_empty() {
            continue;
        }
        let (id, amount) = match it.split_once(sep) {
            Some((a, n)) => (a.trim().parse::<u32>().map_err(|_| format!("bad id {a}"))?, n.trim().parse::<u32>().map_err(|_| format!("bad amount {n}"))?),
            None => (it.trim().parse::<u32>().map_err(|_| format!("bad id {it}"))?, 1),
        };
        if ix(id).is_none() {
            return Err(format!("unknown type {id}"));
        }
        // a mutated output type without its base/mutaplasmid is not a valid item: drones abort the import, modules are skipped
        let muta_out = d::is_muta_output(id);
        match category(id) {
            CAT_DRONE if muta_out => return Err("Passed item is not a Drone".into()),
            CAT_DRONE => req.drones.push(DroneReq { type_id: id, quantity: amount, active: Some(0), mutation: None }),
            CAT_FIGHTER => {
                if fighter_fits(req.ship.type_id, id) {
                    req.fighters.push(FighterReq { type_id: id, quantity: Some(amount), active: true, abilities: None })
                }
            }
            CAT_CHARGE => req.cargo.push(CargoReq { type_id: id, quantity: amount }),
            _ => {
                if muta_out || !is_module_cat(id) || ix(id).and_then(infer_slot).is_none() {
                    continue;
                }
                for _ in 0..amount {
                    let m = module_req(id, None);
                    if category(id) == CAT_SUBSYSTEM {
                        subs.push(m);
                    } else {
                        mods.push(m);
                    }
                }
            }
        }
    }
    req.modules = assemble(ship, subs, mods, true);
    Ok(Imported { name, notes: None, req })
}

// ---- ESI

/// Minimal JSON helpers for the ESI body (serde_json is used; this just pulls the fields Pyfa reads).
pub fn esi_import(text: &str) -> Result<Imported, String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let ship = v.get("ship_type_id").and_then(|x| x.as_u64()).ok_or("no ship_type_id")? as u32;
    if !is_hull(ship) {
        return Err("not a ship".into());
    }
    let mut req = new_req(ship);
    let mut items: Vec<(i64, u32, u32)> = v
        .get("items")
        .and_then(|x| x.as_array())
        .ok_or("no items")?
        .iter()
        .map(|it| (it["flag"].as_i64().unwrap_or(0), it["type_id"].as_u64().unwrap_or(0) as u32, it["quantity"].as_u64().unwrap_or(1) as u32))
        .collect();
    items.sort_by_key(|x| x.0);
    let (mut subs, mut mods) = (Vec::new(), Vec::new());
    for (flag, t, q) in items {
        if !published(t) {
            continue;
        }
        match flag as u64 {
            FLAG_DRONE => {
                if category(t) == CAT_DRONE {
                    req.drones.push(DroneReq { type_id: t, quantity: q, active: Some(0), mutation: None })
                }
            }
            FLAG_CARGO => req.cargo.push(CargoReq { type_id: t, quantity: q }),
            FLAG_FIGHTER => {
                if category(t) == CAT_FIGHTER {
                    // ESI quantity is ignored: a full squadron
                    let q = base_attr(t, d::a::fighterSquadronMaxSize).map(|v| v as u32);
                    req.fighters.push(FighterReq { type_id: t, quantity: q, active: true, abilities: None })
                }
            }
            _ => {
                if !is_module_cat(t) || ix(t).and_then(infer_slot).is_none() {
                    continue;
                }
                let m = module_req(t, None);
                if category(t) == CAT_SUBSYSTEM {
                    subs.push(m);
                } else {
                    mods.push(m);
                }
            }
        }
    }
    req.modules = assemble(ship, subs, mods, false);
    Ok(Imported {
        name: v.get("name").and_then(|x| x.as_str()).unwrap_or("").to_string(),
        notes: v.get("description").and_then(|x| x.as_str()).map(|s| s.to_string()),
        req,
    })
}

// ---- XML

fn xml_unescape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        o.push_str(&rest[..p]);
        let r = &rest[p..];
        let Some(e) = r.find(';') else {
            o.push_str(r);
            rest = "";
            break;
        };
        let ent = &r[1..e];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse::<u32>().ok().and_then(char::from_u32),
            _ => None,
        };
        match rep {
            Some(c) => {
                o.push(c);
                rest = &r[e + 1..];
            }
            None => {
                o.push('&');
                rest = &r[1..];
            }
        }
    }
    o.push_str(rest);
    o
}

/// Attributes of the start tag beginning at `tag` (text after `<name`).
fn xml_attrs(tag: &str) -> Vec<(String, String)> {
    let mut v = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i] != b'>' {
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] == b'>' || b[i] == b'/' {
            break;
        }
        let ks = i;
        while i < b.len() && b[i] != b'=' && !(b[i] as char).is_whitespace() && b[i] != b'>' {
            i += 1;
        }
        let key = tag[ks..i].to_string();
        while i < b.len() && (b[i] as char).is_whitespace() {
            i += 1;
        }
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && (b[i] as char).is_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                let ve = (vs..b.len()).find(|&k| b[k] == q).unwrap_or(b.len());
                v.push((key, xml_unescape(&tag[vs..ve])));
                i = ve + 1;
                continue;
            }
        }
        v.push((key, String::new()));
    }
    v
}

fn attr<'a>(a: &'a [(String, String)], k: &str) -> &'a str {
    a.iter().find(|x| x.0 == k).map(|x| x.1.as_str()).unwrap_or("")
}

/// Start tags `<name ...>` in `s`, with their attribute lists.
fn xml_tags<'a>(s: &'a str, name: &str) -> Vec<(usize, Vec<(String, String)>)> {
    let pat = format!("<{name}");
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(p) = s[from..].find(&pat) {
        let at = from + p;
        let after = at + pat.len();
        let next = s[after..].chars().next().unwrap_or('>');
        if next.is_whitespace() || next == '>' || next == '/' {
            let end = s[after..].find('>').map(|e| after + e).unwrap_or(s.len());
            out.push((at, xml_attrs(&s[after..end])));
        }
        from = after;
    }
    out
}

/// EVE client XML: every `<fitting>` becomes a fit; hardware by `base_type` (or `type`) name; drone and fighter
/// bays, cargo by `slot="cargo"`; mutated hardware keeps its mutaplasmid and attributes.
pub fn xml_import(text: &str) -> Result<Vec<Imported>, String> {
    let fittings = xml_tags(text, "fitting");
    let mut out = Vec::new();
    for (k, (pos, fa)) in fittings.iter().enumerate() {
        let end = fittings.get(k + 1).map(|x| x.0).unwrap_or(text.len());
        let body = &text[*pos..end];
        let ship_name = xml_tags(body, "shipType").first().map(|x| attr(&x.1, "value").to_string()).unwrap_or_default();
        let Some(ship) = d::type_by_name(&ship_name).filter(|t| is_hull(*t)) else { continue };
        let mut req = new_req(ship);
        let notes = xml_tags(body, "description").first().map(|x| attr(&x.1, "value").to_string());
        let (mut subs, mut mods) = (Vec::new(), Vec::new());
        for (_, ha) in xml_tags(body, "hardware") {
            let nm = if !attr(&ha, "base_type").is_empty() { attr(&ha, "base_type") } else { attr(&ha, "type") };
            let Some(t) = d::type_by_name(nm) else { continue };
            if !published(t) {
                continue;
            }
            let muta = Some(attr(&ha, "mutaplasmid")).filter(|s| !s.is_empty()).and_then(d::type_by_name).filter(|m| published(*m));
            let mattrs = parse_mutant_attrs(attr(&ha, "mutated_attrs"));
            let mutation = muta.filter(|m| d::muta_output(*m, t).is_some()).map(|m| full_mutation(t, m, &mattrs));
            let qty = attr(&ha, "qty").trim().parse::<u32>().unwrap_or(0);
            match category(t) {
                CAT_DRONE => {
                    let tt = fitted_type(t, &mutation);
                    req.drones.push(DroneReq { type_id: tt, quantity: qty, active: Some(0), mutation })
                }
                CAT_FIGHTER => req.fighters.push(FighterReq { type_id: t, quantity: Some(qty), active: true, abilities: None }),
                _ if attr(&ha, "slot").to_lowercase() == "cargo" => req.cargo.push(CargoReq { type_id: t, quantity: qty }),
                _ => {
                    if !is_module_cat(t) || ix(t).and_then(infer_slot).is_none() {
                        continue;
                    }
                    let tt = fitted_type(t, &mutation);
                    let m = module_req(tt, mutation);
                    if category(t) == CAT_SUBSYSTEM {
                        subs.push(m);
                    } else {
                        mods.push(m);
                    }
                }
            }
        }
        req.modules = assemble(ship, subs, mods, false);
        out.push(Imported { name: xml_unescape(attr(fa, "name")), notes, req });
    }
    Ok(out)
}

// ---- EFT (Pyfa import semantics)

enum Spec {
    /// module / implant / charge line: type, charge, offline, mutation ref
    Regular(u32, Option<u32>, bool, Option<u32>),
    /// "Name xN": type, amount, mutation ref
    Multi(u32, u32, Option<u32>),
}

fn spec_is_module(s: &Option<Spec>) -> bool {
    matches!(s, Some(Spec::Regular(t, ..)) if is_module_cat(*t))
}
fn spec_is_implant(s: &Option<Spec>) -> bool {
    matches!(s, Some(Spec::Regular(t, ..)) if category(*t) == CAT_IMPLANT
        && (base_attr(*t, d::a::implantness).is_some() || base_attr(*t, d::a::boosterness).is_some()))
}
fn spec_multi_cat(s: &Option<Spec>, cat: u32) -> bool {
    matches!(s, Some(Spec::Multi(t, ..)) if category(*t) == cat)
}

/// A published type by exact (trimmed) name; unknown or unpublished names are stubs, like Pyfa.
fn fetch(name: &str) -> Option<u32> {
    d::type_by_name(name).filter(|t| published(*t))
}

fn valid_charge(module: u32, charge: u32) -> bool {
    let (Some(mi), Some(ci)) = (ix(module), ix(charge)) else { return false };
    if d::type_volume(ci) > d::type_capacity(mi) {
        return false;
    }
    let size = d::type_attr(mi, d::a::chargeSize).unwrap_or(0.0);
    if size > 0.0 && d::type_attr(ci, d::a::chargeSize) != Some(size) {
        return false;
    }
    // Pyfa checks chargeGroup0..chargeGroup4 (so the 5th charge group is never consulted)
    let cg = d::ty(ci).group as f64;
    ["chargeGroup1", "chargeGroup2", "chargeGroup3", "chargeGroup4"]
        .iter()
        .filter_map(|n| d::attr_by_name(n).and_then(|a| d::type_attr(mi, a)))
        .any(|g| g != 0.0 && g == cg)
}

/// EFT import as Pyfa does it: sections classify lines (module racks with `[Empty]` stubs, implants/boosters,
/// drone bay, fighter bay, cargo), unknown items become stubs, drones of one type merge, cargo merges, invalid
/// charges are dropped, `/offline` sets offline, other modules get the import state, T3D mode lines are not read,
/// and modules are kept only while they fit (subsystems first).
pub fn eft_import(text: &str) -> Result<Imported, String> {
    let mut lines: Vec<String> = text.lines().map(|l| l.trim().to_string()).collect();
    while lines.first().map(|l| l.is_empty()).unwrap_or(false) {
        lines.remove(0);
    }
    while lines.last().map(|l| l.is_empty()).unwrap_or(false) {
        lines.pop();
    }
    if lines.is_empty() {
        return Err("empty".into());
    }
    let header = lines.remove(0);
    let h = header.strip_prefix('[').and_then(|x| x.strip_suffix(']')).ok_or("corrupted fit header")?;
    let (ship_name, fit_name) = h.split_once(',').ok_or("corrupted fit header")?;
    let ship = fetch(ship_name.trim()).filter(|t| is_hull(*t)).ok_or("unknown ship")?;
    let fit_name = fit_name.trim().to_string();
    // mutation blocks: "[n] Base" + following non-blank lines
    let mut muts: Vec<(u32, Vec<String>)> = Vec::new();
    let mut consumed = vec![false; lines.len()];
    let mut cur: Option<(u32, Vec<String>)> = None;
    for (i, l) in lines.iter().enumerate() {
        let head = l.strip_prefix('[').and_then(|r| r.find(']').and_then(|e| r[..e].parse::<u32>().ok().map(|n| (n, r[e + 1..].to_string()))));
        if let Some((n, tail)) = head {
            if let Some(c) = cur.take() {
                muts.push(c);
            }
            cur = Some((n, vec![tail]));
            consumed[i] = true;
        } else if l.is_empty() {
            if let Some(c) = cur.take() {
                muts.push(c);
            }
        } else if let Some(c) = cur.as_mut() {
            c.1.push(l.clone());
            consumed[i] = true;
        }
    }
    if let Some(c) = cur.take() {
        muts.push(c);
    }
    let lines: Vec<String> = lines.into_iter().enumerate().filter(|(i, _)| !consumed[*i]).map(|x| x.1).collect();
    let mut mut_map: Vec<(u32, u32, Vec<(u16, f64)>)> = Vec::new(); // ref, mutaplasmid, attrs
    for (n, ls) in muts {
        if ls.len() >= 3 {
            if let Some(mu) = fetch(ls[1].trim()) {
                mut_map.push((n, mu, parse_mutant_attrs(&ls[2])));
            }
        }
    }
    // sections
    let mut sections: Vec<Vec<Option<Spec>>> = Vec::new();
    let mut sec: Vec<Option<Spec>> = Vec::new();
    let stub = |l: &str| l.starts_with('[') && l.ends_with(']') && l.len() > 2;
    for l in lines.iter().chain(std::iter::once(&String::new())) {
        if l.is_empty() {
            if !sec.is_empty() {
                while matches!(sec.last(), Some(None)) {
                    sec.pop();
                }
                sections.push(std::mem::take(&mut sec));
            }
            continue;
        }
        if stub(l) {
            sec.push(None);
            continue;
        }
        let (body, mref) = match l.rfind('[') {
            Some(p) if l.ends_with(']') && l[p + 1..l.len() - 1].parse::<u32>().is_ok() => (l[..p].trim_end(), l[p + 1..l.len() - 1].parse::<u32>().ok()),
            _ => (l.as_str(), None),
        };
        // "Name xN"
        if let Some(p) = body.rfind(" x") {
            if let Ok(n) = body[p + 2..].parse::<u32>() {
                let nm = &body[..p];
                if !nm.contains([',', '/', '[', ']']) {
                    sec.push(fetch(nm).map(|t| Spec::Multi(t, n, mref)));
                    continue;
                }
            }
        }
        let (body, offline) = match body.strip_suffix("/OFFLINE").or_else(|| body.strip_suffix("/offline")) {
            Some(b) => (b.trim_end(), true),
            None => (body, false),
        };
        let (nm, ch) = match body.split_once(',') {
            Some((a, b)) => (a.trim(), Some(b.trim())),
            None => (body.trim(), None),
        };
        if nm.contains(['/', '[', ']']) || ch.map(|c| c.contains([',', '/', '[', ']'])).unwrap_or(false) {
            continue;
        }
        let charge = ch.and_then(fetch).filter(|c| category(*c) == CAT_CHARGE);
        sec.push(fetch(nm).map(|t| Spec::Regular(t, charge, offline, mref)));
    }
    let has_drone_bay = sections.iter().any(|s| !s.is_empty() && s.iter().all(|x| spec_multi_cat(x, CAT_DRONE)));
    let has_fighter_bay = sections.iter().any(|s| !s.is_empty() && s.iter().all(|x| spec_multi_cat(x, CAT_FIGHTER)));
    let mut req = new_req(ship);
    let mut racks: Vec<(Slot, Vec<Option<ModuleReq>>)> =
        [Slot::High, Slot::Mid, Slot::Low, Slot::Rig, Slot::Subsystem, Slot::Service].iter().map(|s| (*s, Vec::new())).collect();
    let mutation_for = |t: u32, r: Option<u32>| -> Option<Mutation> {
        let (_, mu, at) = mut_map.iter().find(|x| Some(x.0) == r)?;
        d::muta_output(*mu, t)?;
        Some(full_mutation(t, *mu, at))
    };
    let make_module = |s: &Spec| -> Option<ModuleReq> {
        let Spec::Regular(t, charge, offline, r) = s else { return None };
        let mutation = mutation_for(*t, *r);
        let tt = fitted_type(*t, &mutation);
        ix(tt).and_then(infer_slot)?;
        let mut m = module_req(tt, mutation);
        if let Some(c) = charge.filter(|c| valid_charge(tt, *c)) {
            m.charge_type_id = Some(c);
        }
        if *offline {
            m.state = Some(State::Offline);
        }
        Some(m)
    };
    let mut cargo: Vec<(u32, u32)> = Vec::new();
    let add_cargo = |t: u32, n: u32, cargo: &mut Vec<(u32, u32)>| match cargo.iter_mut().find(|x| x.0 == t) {
        Some(x) => x.1 += n,
        None => cargo.push((t, n)),
    };
    let add_drone = |t: u32, n: u32, r: Option<u32>, req: &mut FitRequest| {
        let mutation = mutation_for(t, r);
        if mutation.is_some() {
            let tt = fitted_type(t, &mutation);
            req.drones.push(DroneReq { type_id: tt, quantity: n, active: Some(0), mutation });
        } else {
            match req.drones.iter_mut().find(|x| x.type_id == t && x.mutation.is_none()) {
                Some(x) => x.quantity += n,
                None => req.drones.push(DroneReq { type_id: t, quantity: n, active: Some(0), mutation: None }),
            }
        }
    };
    let add_implant = |t: u32, req: &mut FitRequest| {
        if base_attr(t, d::a::implantness).is_some() {
            req.implants.push(t)
        } else if base_attr(t, d::a::boosterness).is_some() {
            req.boosters.push(BoosterReq { type_id: t, side_effects: vec![] })
        }
    };
    for s in &sections {
        let all = |f: &dyn Fn(&Option<Spec>) -> bool| !s.is_empty() && s.iter().all(f);
        if s.iter().all(|x| x.is_none() || spec_is_module(x)) {
            let mods: Vec<Option<ModuleReq>> = s.iter().map(|x| x.as_ref().and_then(make_module)).collect();
            let mut mods = mods;
            while matches!(mods.last(), Some(None)) {
                mods.pop();
            }
            let slots: std::collections::BTreeSet<u8> = mods.iter().flatten().filter_map(|m| m.slot.map(|x| x as u8)).collect();
            if slots.len() == 1 {
                let sl = mods.iter().flatten().next().unwrap().slot.unwrap();
                racks.iter_mut().find(|r| r.0 == sl).unwrap().1.extend(mods);
            } else {
                for m in mods.into_iter().flatten() {
                    let sl = m.slot.unwrap();
                    racks.iter_mut().find(|r| r.0 == sl).unwrap().1.push(Some(m));
                }
            }
        } else if all(&spec_is_implant) {
            for x in s.iter().flatten() {
                if let Spec::Regular(t, ..) = x {
                    add_implant(*t, &mut req);
                }
            }
        } else if all(&|x| spec_multi_cat(x, CAT_DRONE)) {
            for x in s.iter().flatten() {
                if let Spec::Multi(t, n, r) = x {
                    add_drone(*t, *n, *r, &mut req);
                }
            }
        } else if all(&|x| spec_multi_cat(x, CAT_FIGHTER)) {
            for x in s.iter().flatten() {
                if let Spec::Multi(t, n, _) = x {
                    req.fighters.push(FighterReq { type_id: *t, quantity: Some(*n), active: true, abilities: None });
                }
            }
        } else if all(&|x| matches!(x, Some(Spec::Multi(..)))) {
            for x in s.iter().flatten() {
                if let Spec::Multi(t, n, _) = x {
                    add_cargo(*t, *n, &mut cargo);
                }
            }
        } else {
            for x in s.iter() {
                match x {
                    None => {}
                    Some(sp @ Spec::Regular(t, ..)) => {
                        if is_module_cat(*t) {
                            if let Some(m) = make_module(sp) {
                                let sl = m.slot.unwrap();
                                racks.iter_mut().find(|r| r.0 == sl).unwrap().1.push(Some(m));
                            }
                        } else if spec_is_implant(x) {
                            add_implant(*t, &mut req);
                        }
                    }
                    Some(Spec::Multi(t, n, r)) => {
                        let c = category(*t);
                        if c == CAT_DRONE && !has_drone_bay {
                            add_drone(*t, *n, *r, &mut req);
                        } else if c == CAT_FIGHTER && !has_fighter_bay {
                            req.fighters.push(FighterReq { type_id: *t, quantity: Some(*n), active: true, abilities: None });
                        } else {
                            add_cargo(*t, *n, &mut cargo);
                        }
                    }
                }
            }
        }
    }
    // subsystems first, then rigs, services, high, med, low; each kept only while it fits
    let l0 = limits(ship, &[]);
    let mut fitted: Vec<ModuleReq> = Vec::new();
    for m in racks.iter().find(|r| r.0 == Slot::Subsystem).unwrap().1.iter().flatten() {
        if fits(m, &fitted, &l0) {
            fitted.push(m.clone());
        }
    }
    let l = limits(ship, &fitted);
    for sl in [Slot::Rig, Slot::Service, Slot::High, Slot::Mid, Slot::Low] {
        for m in racks.iter().find(|r| r.0 == sl).unwrap().1.iter().flatten() {
            if fits(m, &fitted, &l) {
                fitted.push(m.clone());
            }
        }
    }
    req.modules = fitted;
    req.cargo = cargo.into_iter().map(|(t, n)| CargoReq { type_id: t, quantity: n }).collect();
    Ok(Imported { name: fit_name, notes: None, req })
}

/// Format detection in Pyfa's order: XML, ESI JSON, EFT config (with a file path), EFT, DNA, DNA chat link, `DNA:`.
pub fn detect(text: &str, path: Option<&str>) -> Option<&'static str> {
    let first = text.lines().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
    if first.starts_with("<?xml") && first.contains("version=\"1.0\"") {
        return Some("xml");
    }
    if first.starts_with('{') {
        return Some("esi");
    }
    if first.starts_with('[') && first.contains(']') && path.map(|p| p.ends_with(".cfg")).unwrap_or(false) {
        return Some("eftcfg");
    }
    if first.starts_with('[') && first[1..].find(']').map(|e| first[1..1 + e].contains(',')).unwrap_or(false) {
        return Some("eft");
    }
    let dna_start = first.bytes().next().map(|c| c.is_ascii_digit()).unwrap_or(false) && first.contains("::");
    if dna_start {
        return Some("dna");
    }
    if first.contains("<url=fitting:") {
        return Some("dna_link");
    }
    if first.contains("DNA:") {
        return Some("dna_alt");
    }
    None
}

/// `<url=fitting:DNA>name</url>` -> (DNA, name)
pub fn dna_link(text: &str) -> Option<(String, String)> {
    let p = text.find("<url=fitting:")?;
    let r = &text[p + 13..];
    let e = r.find('>')?;
    let dna = r[..e].to_string();
    let r2 = &r[e + 1..];
    let e2 = r2.find("</url>")?;
    Some((dna, r2[..e2].to_string()))
}

/// EFT config file (`<Ship>.cfg`, ship = file stem): `[name]` starts a fit; `Drones_Active|Inactive=Name,N`,
/// `Implant_*=`, `Booster_*=`, `Cargohold=Name,N`, `Description=` (`|` = newline), else `Module,Charge` lines.
pub fn eftcfg_import(text: &str, ship_name: &str) -> Result<Vec<Imported>, String> {
    let ship = d::type_by_name(ship_name).filter(|t| is_hull(*t)).ok_or("unknown ship")?;
    let lines: Vec<&str> = text.lines().collect();
    let starts: Vec<usize> = lines.iter().enumerate().filter(|(_, l)| l.starts_with('[') && l.ends_with(']')).map(|x| x.0).collect();
    let mut out = Vec::new();
    for (k, &st) in starts.iter().enumerate() {
        let end = starts.get(k + 1).copied().unwrap_or(lines.len());
        let name = lines[st][1..lines[st].len() - 1].to_string();
        let mut req = new_req(ship);
        let mut notes = None;
        let (mut subs, mut mods) = (Vec::new(), Vec::new());
        let num = |s: &str| -> (String, u32) {
            match s.rsplit_once(',') {
                Some((a, n)) if n.trim().parse::<u32>().is_ok() => (a.to_string(), n.trim().parse().unwrap()),
                _ => (s.to_string(), 1),
            }
        };
        for l in &lines[st + 1..end] {
            if l.is_empty() {
                continue;
            }
            let kv = l.split_once('=');
            match kv {
                Some((k, v)) if k.starts_with("Drones_") => {
                    let (n, amount) = num(v);
                    let Some(t) = d::type_by_name(&n) else { continue };
                    match category(t) {
                        CAT_DRONE => req.drones.push(DroneReq { type_id: t, quantity: amount, active: Some(if k == "Drones_Active" { amount } else { 0 }), mutation: None }),
                        CAT_FIGHTER => req.fighters.push(FighterReq { type_id: t, quantity: Some(amount), active: true, abilities: None }),
                        _ => {}
                    }
                }
                Some((k, v)) if k.starts_with("Implant_") || k.starts_with("Booster_") => {
                    let Some(t) = d::type_by_name(v).filter(|t| category(*t) == CAT_IMPLANT) else { continue };
                    if k.starts_with("Implant_") {
                        req.implants.push(t)
                    } else {
                        req.boosters.push(BoosterReq { type_id: t, side_effects: vec![] })
                    }
                }
                Some(("Cargohold", v)) => {
                    let (n, amount) = num(v);
                    if let Some(t) = d::type_by_name(&n) {
                        req.cargo.push(CargoReq { type_id: t, quantity: amount });
                    }
                }
                Some(("Description", v)) => notes = Some(v.replace('|', "\n")),
                _ => {
                    let (mn, cn) = match l.rsplit_once(',') {
                        Some((a, b)) => (a, Some(b)),
                        None => (*l, None),
                    };
                    let Some(t) = d::type_by_name(mn) else { continue };
                    if !is_module_cat(t) || ix(t).and_then(infer_slot).is_none() {
                        continue;
                    }
                    let mut m = module_req(t, None);
                    if category(t) == CAT_SUBSYSTEM {
                        subs.push(m);
                    } else {
                        m.charge_type_id = cn.and_then(d::type_by_name).filter(|c| category(*c) == CAT_CHARGE);
                        mods.push(m);
                    }
                }
            }
        }
        req.modules = assemble(ship, subs, mods, true);
        out.push(Imported { name, notes, req });
    }
    Ok(out)
}

/// Non-fit clipboard payloads (Pyfa's activeFit-only branch of auto-detection): a single mutated item
/// (`Base` / `Mutaplasmid` / `attr value, ...`) -> `FittingItem`; or an additions list (`Name xN` per line) ->
/// `AdditionsDrones|Fighters|Implants|Boosters|Cargo`. Returns (kind, items as (type_id, amount, mutation)).
pub fn items_import(text: &str) -> Option<(&'static str, Vec<(u32, u32, Option<Mutation>)>)> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).collect();
    if lines.is_empty() {
        return None;
    }
    // single mutant
    if let Some(base) = d::type_by_name(lines[0]) {
        let muta = lines.get(1).and_then(|l| d::type_by_name(l)).filter(|m| d::muta_output(*m, base).is_some());
        let mutation = muta.map(|mu| full_mutation(base, mu, &lines.get(2).map(|l| parse_mutant_attrs(l)).unwrap_or_default()));
        return Some(("FittingItem", vec![(base, 1, mutation)]));
    }
    let parse = |l: &str| -> Option<(u32, u32, bool)> {
        let (l, has_ref) = match l.rfind('[') {
            Some(p) if l.ends_with(']') && l[p + 1..l.len() - 1].bytes().all(|c| c.is_ascii_digit()) => (l[..p].trim_end(), true),
            _ => (l, false),
        };
        let (name, n, has_x) = match l.rsplit_once(" x") {
            Some((a, n)) if !n.is_empty() && n.bytes().all(|c| c.is_ascii_digit()) => (a, n.parse().ok()?, true),
            _ => (l, 1, false),
        };
        d::type_by_name(name).map(|t| (t, n, has_x || has_ref))
    };
    let items: Vec<(u32, u32, bool)> = lines.iter().filter_map(|l| parse(l)).collect();
    if items.is_empty() {
        return None;
    }
    let all_x = items.len() == lines.len() && items.iter().all(|x| x.2);
    let none_x = items.iter().all(|x| !x.2);
    let boost = |t: u32| ix(t).map(|i| d::type_attr(i, 1087).is_some()).unwrap_or(false);
    let kind = if all_x && items.iter().all(|x| category(x.0) == CAT_DRONE) {
        "AdditionsDrones"
    } else if all_x && items.iter().all(|x| category(x.0) == CAT_FIGHTER) {
        "AdditionsFighters"
    } else if none_x && items.iter().all(|x| category(x.0) == CAT_IMPLANT && !boost(x.0)) {
        "AdditionsImplants"
    } else if none_x && items.iter().all(|x| category(x.0) == CAT_IMPLANT && boost(x.0)) {
        "AdditionsBoosters"
    } else if all_x {
        "AdditionsCargo"
    } else {
        return None;
    };
    Some((kind, items.into_iter().map(|(t, n, _)| (t, n, None)).collect()))
}

// ---------------------------------------------------------------------------------------------------------------
// Ship stats clipboard text (Pyfa "Copy stats"): written from the observed output format (no Pyfa code).

/// Round to `prec` significant digits (never into the integer part), like Pyfa's display rounding.
fn round_sig(v: f64, prec: i32) -> f64 {
    if v.trunc() == v {
        return v;
    }
    let digits = (prec - v.abs().log10().floor() as i32 - 1).max(0) as usize;
    format!("{:.*}", digits, v).parse().unwrap_or(v)
}

fn num_text(v: f64) -> String {
    if v.trunc() == v && v.abs() < 1e16 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

/// Short human amount: 3 significant digits with k/M/G suffix up to 10^`highest`.
pub fn amount_text(val: f64, prec: i32, highest: i32) -> String {
    if val == f64::INFINITY {
        return "\u{221e}".into();
    }
    let (mut m, mut suffix) = (val, "");
    let sfx = |k: i32| match k {
        3 => "k",
        6 => "M",
        _ => "G",
    };
    if val.abs() > 1.0 && highest >= 3 {
        for key in [9, 6, 3] {
            if val.abs() >= 10f64.powi(key) && key <= highest {
                m = val / 10f64.powi(key);
                suffix = sfx(key);
                if key != 9 && key + 3 <= highest && round_sig(m, prec) >= 1000.0 {
                    m /= 1000.0;
                    suffix = sfx(key + 3);
                }
                break;
            }
        }
    }
    format!("{}{}", num_text(round_sig(m, prec)), suffix)
}

fn g(v: &Value, path: &[&str]) -> f64 {
    let mut x = v;
    for p in path {
        x = &x[*p];
    }
    x.as_f64().unwrap_or(0.0)
}

const LAYERS: [&str; 3] = ["shield", "armor", "hull"];
const DTYPES: [&str; 4] = ["em", "thermal", "kinetic", "explosive"];

/// Remote repair output per layer (HP/s) of active remote repairers and logistics drones.
fn remote_reps(req: &FitRequest, st: &Value) -> [f64; 3] {
    let mut rr = [0.0; 3];
    let attrs = &st["attributes"];
    let rows = st["modules"].as_array().cloned().unwrap_or_default();
    for (mi, m) in req.modules.iter().enumerate() {
        let Some(row) = rows.iter().find(|r| r["module_index"].as_u64() == Some(mi as u64)) else { continue };
        if !matches!(row["state"].as_str(), Some("active") | Some("overheated")) {
            continue;
        }
        let Some(a) = attrs["modules"].as_array().and_then(|v| v.iter().find(|r| r["module_index"].as_u64() == Some(mi as u64))) else { continue };
        let a = &a["attributes"];
        let at = |n: &str| a[n].as_f64().unwrap_or(0.0);
        let cycle = row["cycle_time_ms"].as_f64().unwrap_or(0.0);
        if cycle <= 0.0 {
            continue;
        }
        let grp = ix(m.type_id).map(|i| d::ty(i).group).unwrap_or(0);
        let spool = 1.0; // no spool-up (Pyfa's default for the stats copy)
        let (layer, amount) = match grp {
            41 | 1697 => (0, at("shieldBonus")),
            325 | 2018 => (1, at("armorDamageAmount")),
            1698 => (1, at("armorDamageAmount") * if m.charge_type_id.is_some() { a["chargedArmorDamageMultiplier"].as_f64().unwrap_or(1.0) } else { 1.0 }),
            585 => (2, at("structureDamageAmount")),
            _ => continue,
        };
        rr[layer] += amount * spool / (cycle / 1000.0);
    }
    for (di, dr) in req.drones.iter().enumerate() {
        let n = dr.active.unwrap_or(0).min(dr.quantity) as f64;
        if n <= 0.0 {
            continue;
        }
        let Some(a) = attrs["drones"].as_array().and_then(|v| v.iter().find(|r| r["drone_index"].as_u64() == Some(di as u64))) else { continue };
        let a = &a["attributes"];
        let at = |k: &str| a[k].as_f64().unwrap_or(0.0);
        let cycle = at("duration");
        if cycle <= 0.0 {
            continue;
        }
        rr[0] += at("shieldBonus") * n / (cycle / 1000.0);
        rr[1] += at("armorDamageAmount") * n / (cycle / 1000.0);
        rr[2] += at("structureDamageAmount") * n / (cycle / 1000.0);
    }
    rr
}

/// `st` = calc output of `req` with `include_attributes: "all"`.
pub fn shipstats_export(req: &FitRequest, name: &str, st: &Value) -> String {
    let ship_name = ix(req.ship.type_id).map(d::type_name).unwrap_or("");
    let mut sections: Vec<String> = Vec::new();
    // firepower
    let tot = &st["offense"]["total"];
    let fp = [
        g(tot, &["dps", "total"]),
        g(tot, &["weapon_dps"]),
        g(tot, &["drone_dps"]) + g(tot, &["fighter_dps"]),
        g(tot, &["volley", "total"]),
    ];
    if fp.iter().sum::<f64>() != 0.0 {
        let s: Vec<String> = fp.iter().map(|v| amount_text(*v, 3, 0)).collect();
        sections.push(format!("DPS: {} (Weapon: {}, Drone: {}, Volley: {})\n", s[0], s[1], s[2], s[3]));
    }
    // tank
    let def = &st["defense"];
    let mut ehp: Vec<f64> = LAYERS.iter().map(|l| g(def, &["ehp", l])).collect();
    ehp.push(ehp.iter().sum());
    let vs: Vec<f64> = DTYPES.iter().map(|t| LAYERS.iter().map(|l| g(def, &["hp", l]) / g(def, &["resonance", l, t])).sum()).collect();
    let mut t = format!(
        "EHP: {} (Em: {}, Th: {}, Kin: {}, Exp: {})\n",
        amount_text(ehp[3], 3, 9),
        amount_text(vs[0], 3, 9),
        amount_text(vs[1], 3, 9),
        amount_text(vs[2], 3, 9),
        amount_text(vs[3], 3, 9)
    );
    for (i, l) in LAYERS.iter().enumerate() {
        let r: Vec<String> = DTYPES.iter().map(|d| format!("{:.0}%", (1.0 - g(def, &["resonance", l, d])) * 100.0)).collect();
        let cap = ["Shield", "Armor", "Hull"][i];
        t += &format!("{}: {} (Em: {}, Th: {}, Kin: {}, Exp: {})\n", cap, amount_text(ehp[i], 3, 9), r[0], r[1], r[2], r[3]);
    }
    sections.push(t);
    // repairs
    let tank = &def["tank"];
    let key = ["shield_repair", "armor_repair", "hull_repair"];
    let mut selfr: Vec<f64> = key.iter().map(|k| g(tank, &["effective", k])).collect();
    let mut sust: Vec<f64> = key.iter().map(|k| g(tank, &["sustained_effective", k])).collect();
    let mut remote: Vec<f64> = remote_reps(req, st).to_vec();
    let mut regen = vec![g(tank, &["sustained_effective", "passive_shield"]), 0.0, 0.0];
    let mult: f64 = req
        .modules
        .iter()
        .filter_map(|m| base_attr(m.type_id, d::attr_by_name("shieldRechargeRateMultiplier").unwrap_or(u16::MAX)))
        .product();
    if mult >= 0.9 {
        regen[0] = 0.0;
    }
    let mut total: Vec<f64> = (0..3).map(|i| selfr[i] + remote[i] + regen[i]).collect();
    for v in [&mut selfr, &mut sust, &mut remote, &mut regen, &mut total] {
        let s = v.iter().sum();
        v.push(s);
    }
    let mut rt = String::new();
    if total.iter().sum::<f64>() > 0.0 {
        let (ts, tr, tg) = (selfr[3], remote[3], regen[3]);
        let mut single: Option<(&Vec<f64>, &str)> = None;
        if tr == 0.0 && tg == 0.0 {
            single = Some((&selfr, "Self"));
        }
        if ts == 0.0 && tg == 0.0 {
            single = Some((&remote, "Remote"));
        }
        if ts == 0.0 && tr == 0.0 {
            single = Some((&regen, "Regen"));
        }
        let single = single.filter(|(v, _)| v[..3].iter().filter(|x| **x > 0.0).count() == 1);
        if let Some((v, kind)) = single {
            let i = v[..3].iter().position(|x| *x > 0.0).unwrap();
            if kind == "Regen" {
                rt += &format!("Shield regeneration: {} EHP/s", amount_text(v[i], 3, 9));
            } else {
                rt += &format!("{} {} repair: {} EHP/s", kind, LAYERS[i], amount_text(v[i], 3, 9));
            }
            if kind == "Self" && sust[i] != v[i] {
                rt += &format!(" (Sustained: {} EHP/s)", amount_text(sust[i], 3, 9));
            }
            rt += "\n";
        } else {
            let fmt = |v: &Vec<f64>, blank0: bool| -> Vec<String> {
                v.iter().map(|x| if blank0 && *x == 0.0 { String::new() } else { amount_text(*x, 3, 9) }).collect()
            };
            let cols_all = [
                (fmt(&total, false), "TOTAL"),
                (fmt(&selfr, false), "SELF"),
                (fmt(&sust, false), "SUST"),
                (fmt(&remote, false), "REMOTE"),
                (fmt(&regen, true), "REGEN"),
            ];
            let show = [ts > 0.0, sust != selfr, tr > 0.0, tg > 0.0];
            let n_show = show.iter().filter(|x| **x).count();
            let mut header = "REPS    ".to_string();
            let mut lines: Vec<String> = ["Shield", "Armor", "Hull", "Total"].iter().map(|l| format!("{:<8}", l)).collect();
            for (ci, (vals, nm)) in cols_all.iter().enumerate() {
                let on = if ci == 0 { n_show > 1 } else { show[ci - 1] };
                if on {
                    header += &format!("{:>7} ", nm);
                    for (li, l) in lines.iter_mut().enumerate() {
                        *l += &format!("{:>7} ", vals[li]);
                    }
                }
            }
            rt += &header;
            rt += "\n";
            for (li, l) in lines.iter().enumerate() {
                if total[li] + selfr[li] + sust[li] + remote[li] + regen[li] > 0.0 {
                    rt += l;
                    rt += "\n";
                }
            }
        }
    }
    if !rt.is_empty() {
        sections.push(rt);
    }
    // misc
    let cap = &st["capacitor"];
    let mut m = format!("Speed: {} m/s\n", amount_text(g(st, &["navigation", "max_velocity"]), 3, 0));
    m += &format!("Signature: {} m\n", amount_text(g(st, &["navigation", "signature_radius"]), 3, 9));
    m += &format!("Capacitor: {} GJ", amount_text(g(cap, &["capacity"]), 3, 9));
    if cap["stable"].as_bool().unwrap_or(true) {
        m += &format!(" (Stable at {:.0}%)", g(cap, &["stable_percent"]));
    } else {
        let s = g(cap, &["depletes_in_s"]);
        if s <= 60.0 {
            m += &format!(" (Lasts {}s)", s.trunc() as i64);
        } else {
            m += &format!(" (Lasts {}m{}s)", (s / 60.0).floor() as i64, (s % 60.0).trunc() as i64);
        }
    }
    m += "\n";
    m += &format!("Targeting range: {} km\n", amount_text(g(st, &["targeting", "max_range_m"]) / 1000.0, 3, 0));
    m += &format!("Scan resolution: {:.0} mm\n", g(st, &["targeting", "scan_resolution"]));
    m += &format!("Sensor strength: {}\n", amount_text(g(st, &["targeting", "sensor_strength"]), 3, 0));
    sections.push(m);
    format!("{} ({})\n\n{}", name, ship_name, sections.join("\n"))
}
