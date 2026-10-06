//! Item lookups behind the ext/rpc methods (bench 1.11 "lookups"; Pyfa service layer behaviour):
//! `item.variations`, `item.compare`, `market.group`, `market.search`, `implant_sets.list`,
//! `character.import_evemon`, `names.resolve`, and the item-stats fields of `type`.
//!
//! Pyfa-derived *data* (search jargon, renamed-item conversions; GPL-3.0) is never compiled in: it is loaded at run
//! time from the pipeline's `presets-pyfa-*.json` (`--pyfa-data FILE`, `EXFA_PYFA_DATA`, or RPC
//! `pyfa_data_load {path}`). Without it, search runs without jargon and names resolve without conversions.
use crate::data as d;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::{OnceLock, RwLock};

const CAT_SKILL: u32 = 16;
const CAT_IMPLANT: u32 = 20;
const GROUP_BOOSTER: u32 = 303;
/// Pyfa Market.ROOT_MARKET_GROUPS
const ROOT_MARKET_GROUPS: [u32; 10] = [9, 1111, 157, 11, 1112, 24, 404, 2202, 2203, 2456];
/// Pyfa Market.ITEMS_FORCEDMARKETGROUP
const FORCED_MARKET_GROUP: [(&str, u32); 9] = [
    ("Advanced Cerebral Accelerator", 2487),
    ("Civilian Hobgoblin", 837),
    ("Civilian Light Missile Launcher", 640),
    ("Civilian Scourge Light Missile", 920),
    ("Civilian Small Remote Armor Repairer", 1059),
    ("Civilian Small Remote Shield Booster", 603),
    ("Prototype Cerebral Accelerator", 2487),
    ("Prototype Iris Probe Launcher", 712),
    ("Standard Cerebral Accelerator", 2487),
];
/// Pyfa Market.ITEMS_FORCEPUBLISHED (all False)
const FORCE_UNPUBLISHED: [&str; 15] = [
    "Data Subverter I",
    "QA Cross Protocol Analyzer",
    "QA Damage Module",
    "QA ECCM",
    "QA Immunity Module",
    "QA Multiship Module - 10 Players",
    "QA Multiship Module - 20 Players",
    "QA Multiship Module - 40 Players",
    "QA Multiship Module - 5 Players",
    "QA Remote Armor Repair System - 5 Players",
    "QA Shield Transporter - 5 Players",
    "Goru's Shuttle",
    "Guristas Shuttle",
    "Mobile Decoy Unit",
    "Tournament Micro Jump Unit",
];
/// Pyfa Market.ITEMS_FORCEDMETAGROUP: item -> parent
const FORCED_META_PARENT: [(&str, &str); 5] = [
    ("'Habitat' Miner I", "Miner I"),
    ("'Wild' Miner I", "Miner I"),
    ("Khanid Navy Torpedo Launcher", "Torpedo Launcher I"),
    ("Dread Guristas Standup Variable Spectrum ECM", "Standup Variable Spectrum ECM I"),
    ("Dark Blood Standup Heavy Energy Neutralizer", "Standup Heavy Energy Neutralizer I"),
];
const SEARCH_CATEGORIES: [&str; 9] = ["Drone", "Module", "Subsystem", "Charge", "Implant", "Deployable", "Fighter", "Structure", "Structure Module"];
const SEARCH_GROUPS: [&str; 7] = [
    "Ice Product",
    "Cargo Container",
    "Secure Cargo Container",
    "Audit Log Secure Container",
    "Freight Container",
    "Jump Filaments",
    "Triglavian Space Filaments",
];
const FIT_GROUPS: [&str; 3] = ["Citadel", "Engineering Complex", "Refinery"];
/// requiredSkillN / requiredSkillNLevel attribute pairs
const REQ_SKILL_ATTRS: [(u16, u16); 6] = [(182, 277), (183, 278), (184, 279), (1285, 1286), (1289, 1287), (1290, 1288)];

fn name(ix: usize) -> &'static str {
    d::type_name(ix)
}
fn group(ix: usize) -> u32 {
    d::ty(ix).group
}
fn category(ix: usize) -> u32 {
    d::ty(ix).category
}
fn n_types() -> usize {
    d::type_count()
}

/// Pyfa db_update `processEveTypes` publication rules (the first pass).
fn published_stage1(ix: usize) -> bool {
    let n = name(ix);
    let id = d::type_id_at(ix);
    let after = |prefix: &str, needle: &str, min_gap: usize| {
        n.strip_prefix(prefix).and_then(|r| r.find(needle)).is_some_and(|p| p >= min_gap)
    };
    if (n.starts_with("Civilian") && !n.contains("Shuttle"))
        || n == "Capsule"
        || group(ix) == 4033
        || matches!(id, 82941 | 87164 | 87177)
        || after("AIR ", "Booster", 1)
    {
        return true;
    }
    let mining_blitz = n
        .strip_prefix("Mining Blitz ")
        .and_then(|r| r.find(" Booster Dose ").map(|p| (p, r.len() - p - " Booster Dose ".len())))
        .is_some_and(|(p, rest)| p >= 1 && rest >= 1);
    let filament = n.ends_with(" Filament")
        && !["'Needlejack'", "'Devana'", "'Pochven'", "'Extraction'", "'Krai Veles'", "'Krai Perun'", "'Krai Svarog'"].iter().any(|x| n.contains(x));
    if n.starts_with("Limited Synth ") || n.starts_with("Expired ") || after("Grand Prix ", "Booster", 0) || mining_blitz || filament {
        return false;
    }
    d::type_published(ix)
}

/// The type is in Pyfa's item table (db_update keeps published rows and a few special groups / ids).
fn in_pyfa_db(ix: usize) -> bool {
    published_stage1(ix)
        || matches!(group(ix), 1306 | 1882 | 1975 | 1971 | 1983)
        || matches!(d::type_id_at(ix), 41548..=41551 | 92609 | 95625)
}

/// Pyfa's `published` flag after db_update's second pass (abyssal / mutated / placeholder / Drifter weapons
/// unpublished) and Market.getPublicityByItem's forced list.
pub fn pyfa_published(ix: usize) -> bool {
    let n = name(ix);
    if FORCE_UNPUBLISHED.contains(&n) {
        return false;
    }
    let l = n.to_lowercase();
    let hidden = (l.contains("abyssal") || l.contains("mutated") || l.find("placeholder").is_some_and(|p| p >= 1) || ["Lux Kontos", "Lux Xiphos", "Lux Ballistra", "Lux Kopis"].contains(&n))
        && !matches!(n, "Abyssal Ore Processing" | "Mutated Drone Specialization")
        && !n.contains("Asteroid Mining Crystal");
    if hidden {
        return false;
    }
    published_stage1(ix)
}

fn by_exact_name() -> &'static HashMap<&'static str, usize> {
    static M: OnceLock<HashMap<&'static str, usize>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        for ix in 0..n_types() {
            if in_pyfa_db(ix) {
                m.entry(name(ix)).or_insert(ix);
            }
        }
        m
    })
}

fn id_of(ix: usize) -> u32 {
    d::type_id_at(ix)
}

fn parent_of(ix: usize) -> usize {
    if let Some((_, p)) = FORCED_META_PARENT.iter().find(|x| x.0 == name(ix)) {
        if let Some(&pix) = by_exact_name().get(p) {
            return pix;
        }
    }
    d::type_variation_parent(ix).and_then(d::type_index).unwrap_or(ix)
}

fn attr_named(ix: usize, n: &str) -> Option<f64> {
    d::attr_by_name(n).and_then(|a| d::type_attr(ix, a))
}

/// Pyfa Market.getVariationsByItems([item]).
pub fn variations_ix(ix: usize) -> Vec<usize> {
    let n = name(ix);
    let mut limiter: BTreeSet<String> = BTreeSet::new();
    if category(ix) == CAT_IMPLANT && group(ix) != GROUP_BOOSTER {
        let mut rm: Vec<String> = ["Low-Grade ", "Low-grade ", "Mid-Grade ", "Mid-grade ", "High-Grade ", "High-grade ", "Limited ", " - Advanced", " - Basic", " - Elite", " - Improved", " - Standard"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        for p in ["-6", "-7", "-8", "-9", "-10"] {
            for i in 0..50 {
                rm.push(format!("{p}{i:02}"));
            }
        }
        for r in rm {
            if n.contains(&r) {
                limiter.insert(n.replace(&r, ""));
            }
        }
    }
    let p = parent_of(ix);
    let mut out: BTreeSet<usize> = BTreeSet::new();
    out.insert(p);
    for (it, par) in FORCED_META_PARENT {
        if par == name(p) {
            if let Some(&i) = by_exact_name().get(it) {
                out.insert(i);
            }
        }
    }
    let pid = id_of(p);
    let mut vl: Vec<usize> = (0..n_types()).filter(|&i| in_pyfa_db(i) && d::type_variation_parent(i) == Some(pid)).collect();
    if vl.is_empty() && matches!(category(p), 18 | 87 | 20) {
        let g = group(p);
        vl = (0..n_types()).filter(|&i| in_pyfa_db(i) && group(i) == g).collect();
    }
    if !limiter.is_empty() {
        // Pyfa keeps only the last limiter's trim (loop variable leak; set order there, sorted order here)
        let mut trimmed = Vec::new();
        for l in &limiter {
            trimmed = vl.iter().copied().filter(|&v| name(v).contains(l.as_str())).collect();
        }
        if !trimmed.is_empty() {
            vl = trimmed;
        }
    }
    if group(ix) == GROUP_BOOSTER {
        let slot = attr_named(ix, "boosterness");
        let mg = d::type_market_group(ix);
        vl.retain(|&v| attr_named(v, "boosterness") == slot && (d::type_market_group(v) == mg || d::type_market_group(v).is_none()));
    }
    out.extend(vl);
    out.into_iter().collect()
}

fn type_ix(p: &Value, key: &str) -> Result<usize, Value> {
    let id = p.get(key).and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())));
    id.and_then(|i| d::type_index(i as u32))
        .ok_or_else(|| json!({"error": {"code": "UNKNOWN_TYPE", "message": format!("{key}: {:?}", p.get(key))}}))
}

pub fn item_variations(p: &Value) -> Value {
    match type_ix(p, "type_id") {
        Ok(ix) => json!({"type_ids": variations_ix(ix).into_iter().map(id_of).collect::<Vec<_>>()}),
        Err(e) => e,
    }
}

/// `item.compare {type_id, attributes}`: base attribute values of every variation (Pyfa item compare window).
pub fn item_compare(p: &Value) -> Value {
    let ix = match type_ix(p, "type_id") {
        Ok(ix) => ix,
        Err(e) => return e,
    };
    let names: Vec<&str> = p.get("attributes").and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
    let items: Vec<Value> = variations_ix(ix)
        .into_iter()
        .map(|v| {
            let mut a = Map::new();
            for n in &names {
                if let Some(x) = base_attr(v, n) {
                    a.insert(n.to_string(), json!(x));
                }
            }
            json!({"type_id": id_of(v), "attributes": a})
        })
        .collect();
    json!({"items": items})
}

/// Base attribute by name as Pyfa's Item.attributes has it (mass / capacity / volume / radius included).
fn base_attr(ix: usize, n: &str) -> Option<f64> {
    match n {
        "mass" => Some(d::type_mass(ix)),
        "capacity" => Some(d::type_capacity(ix)),
        "volume" => Some(d::type_volume(ix)),
        "radius" => Some(d::type_radius(id_of(ix))),
        _ => attr_named(ix, n),
    }
}

/// `market.group {market_group_id|null}` -> `{groups, items}` (Pyfa getMarketRoot / getMarketGroupChildren /
/// getItemsByMarketGroup(vars_=False)).
pub fn market_group(p: &Value) -> Value {
    let g = match p.get("market_group_id") {
        None | Some(Value::Null) => {
            let mut r = ROOT_MARKET_GROUPS.to_vec();
            r.sort();
            return json!({"groups": r, "items": []});
        }
        Some(x) => match x.as_u64().or_else(|| x.as_str().and_then(|s| s.parse().ok())) {
            Some(g) => g as u32,
            None => return json!({"error": {"code": "BAD_REQUEST", "message": "market_group_id must be an integer or null"}}),
        },
    };
    if d::market_group_ids().binary_search(&g).is_err() {
        return json!({"error": {"code": "UNKNOWN_MARKET_GROUP", "message": g.to_string()}});
    }
    let groups: Vec<u32> = d::market_group_ids().iter().copied().filter(|&c| d::market_group_parent(c) == Some(g)).collect();
    let mut items: BTreeSet<u32> = (0..n_types()).filter(|&i| in_pyfa_db(i) && d::type_market_group(i) == Some(g)).map(id_of).collect();
    for (n, mg) in FORCED_MARKET_GROUP {
        if mg == g {
            if let Some(&i) = by_exact_name().get(n) {
                items.insert(id_of(i));
            }
        }
    }
    json!({"groups": groups, "items": items.into_iter().collect::<Vec<_>>()})
}

// ------------------------------------------------------------------------------------------- Pyfa data (runtime)

#[derive(Default)]
struct PyfaData {
    /// lowercased key -> patterns
    jargon: HashMap<String, Vec<String>>,
    /// old name -> current name
    conversions: HashMap<String, String>,
    /// builtin damage patterns: raw name -> amounts [em, thermal, kinetic, explosive]
    damage_patterns: HashMap<String, [f64; 4]>,
    /// builtin target profiles: raw name -> resists [em, thermal, kinetic, explosive], sig, velocity, radius
    target_profiles: HashMap<String, ([f64; 4], Option<f64>, Option<f64>, Option<f64>)>,
    source: Option<String>,
}

fn pyfa_data() -> &'static RwLock<PyfaData> {
    static P: OnceLock<RwLock<PyfaData>> = OnceLock::new();
    P.get_or_init(|| {
        let mut pd = PyfaData::default();
        if let Ok(path) = std::env::var("EXFA_PYFA_DATA") {
            if let Ok(v) = read_pyfa_file(&path) {
                pd = v;
            }
        }
        RwLock::new(pd)
    })
}

fn read_pyfa_file(path: &str) -> Result<PyfaData, String> {
    let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
    let raw = if b.starts_with(&[0x1f, 0x8b]) {
        let mut s = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&b[..]), &mut s).map_err(|e| format!("{path}: gzip: {e}"))?;
        s
    } else {
        b
    };
    let v: Value = serde_json::from_slice(&raw).map_err(|e| format!("{path}: {e}"))?;
    let mut pd = pyfa_from_value(&v)?;
    pd.source = Some(path.to_string());
    Ok(pd)
}

fn pyfa_from_value(v: &Value) -> Result<PyfaData, String> {
    let mut pd = PyfaData::default();
    if let Some(items) = v.pointer("/jargon/items").and_then(|x| x.as_object()) {
        for (k, pats) in items {
            if k.is_empty() {
                continue;
            }
            let pats: Vec<String> = pats.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
            pd.jargon.insert(k.to_lowercase(), pats);
        }
    }
    // renamed items: `conversions.items` {old: new} (eve-sde-pipeline presets-pyfa, from Pyfa service/conversions)
    if let Some(items) = v.pointer("/conversions/items").and_then(|x| x.as_object()) {
        for (old, new) in items {
            if let Some(n) = new.as_str() {
                pd.conversions.insert(old.clone(), n.to_string());
            }
        }
    }
    let f = |x: &Value, k: &str| x.get(k).and_then(|v| v.as_f64());
    // builtin damage patterns / target profiles (eve-sde-pipeline presets-pyfa, from Pyfa DamagePattern /
    // TargetProfile getBuiltinList): patterns carry Pyfa's raw amounts, profiles resist fractions
    for it in v.pointer("/damage_patterns/items").and_then(|x| x.as_array()).into_iter().flatten() {
        let (Some(name), Some(a)) = (it.get("name").and_then(|x| x.as_str()), it.get("amounts")) else { continue };
        pd.damage_patterns.insert(name.to_string(), [f(a, "em").unwrap_or(0.0), f(a, "thermal").unwrap_or(0.0), f(a, "kinetic").unwrap_or(0.0), f(a, "explosive").unwrap_or(0.0)]);
    }
    for it in v.pointer("/target_profiles/items").and_then(|x| x.as_array()).into_iter().flatten() {
        let Some(name) = it.get("name").and_then(|x| x.as_str()) else { continue };
        let r = [f(it, "em").unwrap_or(0.0), f(it, "thermal").unwrap_or(0.0), f(it, "kinetic").unwrap_or(0.0), f(it, "explosive").unwrap_or(0.0)];
        pd.target_profiles.insert(name.to_string(), (r, f(it, "signature_radius"), f(it, "max_velocity"), f(it, "radius")));
    }
    if pd.jargon.is_empty() && pd.conversions.is_empty() && pd.damage_patterns.is_empty() && pd.target_profiles.is_empty() {
        return Err("no jargon, conversions, damage_patterns or target_profiles items in the Pyfa data file".into());
    }
    Ok(pd)
}

/// Load Pyfa data (CLI `--pyfa-data FILE`).
pub fn load_pyfa_data(path: &str) -> Result<(), String> {
    let pd = read_pyfa_file(path)?;
    *pyfa_data().write().unwrap() = pd;
    Ok(())
}

/// RPC `pyfa_data_load {path}` / `{data}` / `{clear: true}`.
pub fn pyfa_data_load(p: &Value) -> Value {
    let r = if p.get("clear").and_then(|c| c.as_bool()) == Some(true) {
        Ok(PyfaData::default())
    } else if let Some(path) = p.get("path").and_then(|x| x.as_str()) {
        read_pyfa_file(path)
    } else if let Some(v) = p.get("data") {
        pyfa_from_value(v)
    } else {
        Err("pyfa_data_load needs path, data or clear".into())
    };
    match r {
        Ok(pd) => {
            let out = json!({"ok": true, "jargon": pd.jargon.len(), "conversions": pd.conversions.len(), "damage_patterns": pd.damage_patterns.len(), "target_profiles": pd.target_profiles.len()});
            *pyfa_data().write().unwrap() = pd;
            out
        }
        Err(e) => json!({"error": {"code": "BAD_PYFA_DATA", "message": e}}),
    }
}

pub fn pyfa_data_status() -> Value {
    let pd = pyfa_data().read().unwrap();
    json!({"loaded": pd.source.is_some() || !pd.jargon.is_empty() || !pd.damage_patterns.is_empty(), "source": pd.source, "jargon": pd.jargon.len(), "conversions": pd.conversions.len(), "damage_patterns": pd.damage_patterns.len(), "target_profiles": pd.target_profiles.len(), "builtin_aliases": builtin_aliases().len()})
}

/// Resolve `damage_pattern.builtin` / `target_profile.builtin` (bench draft 1.11) against the runtime Pyfa data.
/// Returns None when the request names no builtin; Err((path, message)) for an unknown name.
pub fn resolve_builtins(req: &crate::FitRequest) -> Option<Result<crate::FitRequest, (String, String)>> {
    let dpn = req.damage_pattern.as_ref().and_then(|d| d.builtin.clone());
    let tpn = req.target_profile.as_ref().and_then(|t| t.builtin.clone());
    if dpn.is_none() && tpn.is_none() {
        return None;
    }
    let pd = pyfa_data().read().unwrap();
    let hint = if pd.damage_patterns.is_empty() && pd.target_profiles.is_empty() { " (no Pyfa data loaded: --pyfa-data / EXFA_PYFA_DATA / pyfa_data_load)" } else { "" };
    let mut r = req.clone();
    if let Some(n) = dpn {
        let Some(a) = pd.damage_patterns.get(&n) else { return Some(Err(("damage_pattern/builtin".into(), format!("unknown builtin damage pattern '{n}'{hint}")))) };
        r.damage_pattern = Some(crate::request::Resists { em: a[0], thermal: a[1], kinetic: a[2], explosive: a[3], builtin: None });
    }
    if let Some(n) = tpn {
        let Some((a, sig, vel, rad)) = pd.target_profiles.get(&n) else { return Some(Err(("target_profile/builtin".into(), format!("unknown builtin target profile '{n}'{hint}")))) };
        let t = r.target_profile.get_or_insert_with(Default::default);
        t.em = a[0];
        t.thermal = a[1];
        t.kinetic = a[2];
        t.explosive = a[3];
        t.signature_radius = *sig;
        t.max_velocity = *vel;
        t.radius = *rad;
        t.builtin = None;
    }
    Some(Ok(r))
}

// ---------------------------------------------------------------------------------------------------- search

/// Pyfa SearchWorkerThread._prepareRequestNormal: re.escape, whitespace un-escaped, `*` / `?` wildcards.
fn prep_normal(q: &str) -> Vec<String> {
    let mut s = String::new();
    for c in q.chars() {
        if "()[]{}?*+-|^$\\.&~#".contains(c) {
            s.push('\\');
        }
        s.push(c);
    }
    let s = s.replace("\\*", "\\w*").replace("\\?", "\\w?");
    s.split_whitespace().map(String::from).collect()
}

/// Pyfa SearchWorkerThread._prepareRequestRegex (falls back to the normal form on unbalanced braces).
fn prep_regex(q: &str) -> Vec<String> {
    let (mut round, mut square, mut esc) = (0i32, 0i32, false);
    let mut toks = Vec::new();
    let mut cur = String::new();
    for c in q.chars() {
        let this_esc = esc;
        esc = false;
        if this_esc {
            cur.push(c);
        } else if c == '\\' {
            cur.push(c);
            esc = true;
        } else if c == '[' {
            cur.push(c);
            square += 1;
        } else if c == ']' {
            cur.push(c);
            square -= 1;
        } else if c == '(' && square == 0 {
            cur.push(c);
            round += 1;
        } else if c == ')' && square == 0 {
            cur.push(c);
            round -= 1;
        } else if c.is_whitespace() && round == 0 && square == 0 {
            if !cur.is_empty() {
                toks.push(std::mem::take(&mut cur));
            }
        } else {
            cur.push(c);
        }
        if !(0..=1).contains(&square) || round < 0 {
            return prep_normal(q);
        }
    }
    if !cur.is_empty() {
        toks.push(cur);
    }
    toks
}

fn is_cjk(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x3040..=0x30ff | 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xac00..=0xd7af | 0xf900..=0xfaff))
}

fn search_filter(ix: usize, f: &str) -> bool {
    let cat = d::category_name(category(ix)).unwrap_or("");
    let grp = d::group_name(group(ix)).unwrap_or("");
    let market = SEARCH_CATEGORIES.contains(&cat) || SEARCH_GROUPS.contains(&grp);
    match f {
        "market" => market,
        "implants" => cat == "Implant",
        "fit" => cat == "Ship" || FIT_GROUPS.contains(&grp),
        _ => true,
    }
}

/// Built-in EX-CT search aliases (dataset `search_aliases`, embedded by codegen as ready patterns in
/// `data::SEARCH_ALIASES`): alias -> regex over English type names. License-clean (LGPL), so unlike the
/// Pyfa jargon table it ships compiled into every build — including the browser wasm.
fn builtin_aliases() -> &'static HashMap<String, String> {
    static M: OnceLock<HashMap<String, String>> = OnceLock::new();
    M.get_or_init(|| d::SEARCH_ALIASES.iter().map(|(a, e)| (a.to_lowercase(), e.to_string())).collect())
}

/// `market.search {query, filter}` -> `{type_ids}` (Pyfa SearchWorkerThread.processSearches, jargon applied when
/// Pyfa data is loaded). The EX-CT alias table (`data::SEARCH_ALIASES`) is always on: patterns from both
/// sources are OR-ed per query token.
pub fn market_search(p: &Value) -> Value {
    let q = p.get("query").and_then(|x| x.as_str()).unwrap_or("");
    let filter = p.get("filter").and_then(|x| x.as_str()).unwrap_or("market");
    let mut toks = match q.trim().to_lowercase().strip_prefix("re:") {
        Some(_) => prep_regex(&q.trim_start()[3..]),
        None => prep_normal(q),
    };
    {
        let pd = pyfa_data().read().unwrap();
        let builtins = builtin_aliases();
        for t in toks.iter_mut() {
            let tl = t.to_lowercase();
            let mut pats: Vec<&str> = Vec::new();
            if let Some(r) = pd.jargon.get(&tl) {
                pats.extend(r.iter().map(|s| s.as_str()));
            }
            if let Some(r) = builtins.get(&tl) {
                pats.push(r.as_str());
            }
            if !pats.is_empty() {
                *t = format!("({})", pats.join("|"));
            }
        }
    }
    let joined = toks.join(" ");
    let long_enough = if is_cjk(&joined) { joined.chars().count() >= 1 } else { joined.chars().count() >= 3 };
    if !long_enough {
        return json!({"type_ids": []});
    }
    // Python `re.search(token, name, re.IGNORECASE)`; a token that does not compile matches nothing
    let res: Vec<Option<crate::pyre::Regex>> = toks.iter().map(|t| crate::pyre::Regex::new(t).ok()).collect();
    let filters: Vec<&str> = match filter {
        "market" => vec!["market"],
        "implants" => vec!["implants"],
        "everything" => vec!["fit", "market"],
        _ => vec!["all"],
    };
    let mut found: BTreeSet<u32> = BTreeSet::new();
    for f in filters {
        let mut n = 0;
        for ix in 0..n_types() {
            if !in_pyfa_db(ix) || !search_filter(ix, f) {
                continue;
            }
            let nm = name(ix);
            if res.iter().all(|r| r.as_ref().is_some_and(|r| r.search(nm))) {
                // SQL LIMIT 100 before the publicity filter
                n += 1;
                if pyfa_published(ix) {
                    found.insert(id_of(ix));
                }
                if n == 100 {
                    break;
                }
            }
        }
    }
    json!({"type_ids": found.into_iter().collect::<Vec<_>>()})
}

// ------------------------------------------------------------------------------------------------ implant sets

/// `implant_sets.list` -> `{sets: {set: {grade: [type_ids]}}}` (Pyfa db_update processImplantSets).
pub fn implant_sets(_p: &Value) -> Value {
    let mut sets: BTreeMap<(String, String), BTreeSet<u32>> = BTreeMap::new();
    for ix in 0..n_types() {
        if !published_stage1(ix) || !matches!(group(ix), 300 | 1730) {
            continue;
        }
        let n = name(ix);
        let w: Vec<&str> = n.split(' ').collect();
        let grade_ok = |g: &str| {
            let l = g.to_lowercase();
            l == "high-grade" || l == "mid-grade" || l == "low-grade"
        };
        let letter = |s: &str| ["alpha", "beta", "gamma", "delta", "epsilon", "omega"].iter().any(|g| s.to_lowercase().starts_with(g));
        if w.len() >= 3 && grade_ok(w[0]) && !w[1].is_empty() && w[1].chars().all(|c| c.is_alphanumeric() || c == '_') && letter(w[2]) {
            sets.entry((w[1].to_string(), w[0].to_string())).or_default().insert(id_of(ix));
        }
        if let Some(r) = n.strip_prefix("Genolution Core Augmentation CA-") {
            if r.starts_with(|c: char| c.is_ascii_digit()) {
                sets.entry(("Genolution".into(), String::new())).or_default().insert(id_of(ix));
            }
        }
    }
    let mut out: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    for ((set, grade), ids) in sets {
        if ids.len() < 2 {
            continue;
        }
        out.entry(set).or_default().insert(grade, json!(ids.into_iter().collect::<Vec<_>>()));
    }
    json!({"sets": out})
}

// ------------------------------------------------------------------------------------------------ EVEMon import

fn xml_unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Minimal XML scan: (tag, attributes, text up to the next tag) for every start tag.
fn xml_elements(x: &str) -> Vec<(String, Vec<(String, String)>, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    let b = x.as_bytes();
    while let Some(o) = x[i..].find('<') {
        let s = i + o;
        let Some(e) = x[s..].find('>').map(|e| s + e) else { break };
        let inner = &x[s + 1..e];
        i = e + 1;
        if inner.starts_with('/') || inner.starts_with('?') || inner.starts_with('!') {
            continue;
        }
        let inner = inner.trim_end_matches('/');
        let (tag, rest) = inner.split_at(inner.find(char::is_whitespace).unwrap_or(inner.len()));
        let mut attrs = Vec::new();
        let mut r = rest;
        while let Some(eq) = r.find('=') {
            let k = r[..eq].trim().to_string();
            let after = r[eq + 1..].trim_start();
            let Some(q) = after.chars().next().filter(|c| *c == '"' || *c == '\'') else { break };
            let Some(end) = after[1..].find(q) else { break };
            attrs.push((k, xml_unescape(&after[1..1 + end])));
            r = &after[end + 2..];
        }
        let text_end = x[i..].find('<').map(|t| i + t).unwrap_or(b.len());
        out.push((tag.to_string(), attrs, xml_unescape(&x[i..text_end])));
    }
    out
}

/// `character.import_evemon {xml}` -> `{name, security_status, skills}` (Pyfa CharacterImportThread).
pub fn import_evemon(p: &Value) -> Value {
    let bad = |m: &str| json!({"error": {"code": "INVALID_CHARACTER_XML", "message": m}});
    let x = p.get("xml").and_then(|x| x.as_str()).unwrap_or("");
    let els = xml_elements(x);
    match els.first() {
        Some((t, _, _)) if t == "SerializableCCPCharacter" || t == "SerializableUriCharacter" => {}
        _ => return bad("root element must be SerializableCCPCharacter or SerializableUriCharacter"),
    }
    let Some(nm) = els.iter().find(|e| e.0 == "name").map(|e| e.2.clone()) else { return bad("no <name>") };
    let Some(sec) = els.iter().find(|e| e.0 == "securityStatus").map(|e| e.2.trim().to_string()) else { return bad("no <securityStatus>") };
    let sec: f64 = if sec.is_empty() { 0.0 } else { match sec.parse() { Ok(v) => v, Err(_) => return bad("bad securityStatus") } };
    let mut skills = BTreeMap::new();
    for (_, a, _) in els.iter().filter(|e| e.0 == "skill") {
        let get = |k: &str| a.iter().find(|x| x.0 == k).and_then(|x| x.1.trim().parse::<i64>().ok());
        let (Some(id), Some(lv)) = (get("typeID"), get("level")) else { return bad("skill without typeID / level") };
        let is_skill = u32::try_from(id).ok().and_then(d::type_index).is_some_and(|ix| category(ix) == CAT_SKILL && in_pyfa_db(ix));
        if is_skill && (0..=5).contains(&lv) {
            skills.insert(id.to_string(), json!(lv));
        }
    }
    json!({"name": format!("{nm} (EVEMon)"), "security_status": sec, "skills": skills})
}

// ------------------------------------------------------------------------------------------------ names

/// `names.resolve {names}` -> `{resolved: {name: type_id|null}}` (Pyfa Market.getItem(str): renamed-item
/// conversions, then the exact name in Pyfa's item table).
pub fn names_resolve(p: &Value) -> Value {
    let pd = pyfa_data().read().unwrap();
    let mut out = Map::new();
    for n in p.get("names").and_then(|x| x.as_array()).into_iter().flatten().filter_map(|x| x.as_str()) {
        let cur = pd.conversions.get(n).map(|s| s.as_str()).unwrap_or(n);
        out.insert(n.to_string(), by_exact_name().get(cur).map(|&ix| json!(id_of(ix))).unwrap_or(Value::Null));
    }
    json!({"resolved": out})
}

// ------------------------------------------------------------------------------------------------ type fields

/// English description (dataset `descriptions`, pipeline revisions that carry it; embedded gzip, decoded once).
pub fn description(id: u32) -> Option<&'static str> {
    static M: OnceLock<HashMap<u32, String>> = OnceLock::new();
    let m = M.get_or_init(|| {
        let mut raw = Vec::new();
        if d::DESCRIPTIONS_GZ.is_empty() || std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(d::DESCRIPTIONS_GZ), &mut raw).is_err() {
            return HashMap::new();
        }
        let mut m = HashMap::new();
        let mut i = 0;
        while i + 8 <= raw.len() {
            let id = u32::from_le_bytes(raw[i..i + 4].try_into().unwrap());
            let n = u32::from_le_bytes(raw[i + 4..i + 8].try_into().unwrap()) as usize;
            i += 8;
            if let Some(t) = raw.get(i..i + n).and_then(|b| std::str::from_utf8(b).ok()) {
                m.insert(id, t.to_string());
            }
            i += n;
        }
        m
    });
    m.get(&id).map(|s| s.as_str())
}

/// Pyfa item-stats fields added to `type`: `required_skills` {skill id: level}, `traits_html`, `description`,
/// `radius`; mass / capacity / volume / radius also inside `attributes` (Pyfa Item.attributes has them).
pub fn type_extra(ix: usize, out: &mut Map<String, Value>) {
    let id = id_of(ix);
    let mut req = Map::new();
    for (s, l) in REQ_SKILL_ATTRS {
        if let Some(sk) = d::type_attr(ix, s).filter(|v| *v != 0.0) {
            req.insert((sk as u32).to_string(), json!(d::type_attr(ix, l).unwrap_or(0.0) as i64));
        }
    }
    out.insert("required_skills".into(), Value::Object(req));
    out.insert("traits_html".into(), d::type_traits_html(id).map(|s| json!(s)).unwrap_or(Value::Null));
    // Pyfa's text uses CRLF line breaks and no trailing whitespace
    out.insert("description".into(), description(id).map(|s| json!(s.replace("\r\n", "\n").replace('\n', "\r\n").trim())).unwrap_or(Value::Null));
    out.insert("radius".into(), json!(d::type_radius(id)));
    if let Some(Value::Object(a)) = out.get_mut("attributes") {
        a.insert("mass".into(), json!(d::type_mass(ix)));
        a.insert("capacity".into(), json!(d::type_capacity(ix)));
        a.insert("volume".into(), json!(d::type_volume(ix)));
        a.insert("radius".into(), json!(d::type_radius(id)));
    }
}
