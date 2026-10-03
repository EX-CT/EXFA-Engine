//! eve-dogma-codegen: build-time code generator (formerly variant F `build.rs`).
//!
//! Reads the EXCT dataset (`dataset-<build>.json.gz`, eve-sde-pipeline format v1) and emits two files:
//! - `tables.rs` for `eve-sde`: static tables (types, attributes, groups, names, mutaplasmids, …);
//! - `effects.rs` for `eve-dogma`: every SDE effect's modifier list compiled into straight-line Rust
//!   (`apply_local`, `apply_projected`, `apply_skill`, `apply_dbuff`).
//!
//! The runtime never parses or interprets dataset JSON.
//!
//! Dataset path: `$EVE_DOGMA_DATASET`, else `../../../data/dataset-3569502-r5.json.gz` relative to the crate dir (EXCT box layout).
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::io::Read;

const DEFAULT_DATASET: &str = "../../../data/dataset-3569502-r5.json.gz";
const REQ_SKILL_ATTRS: [u32; 6] = [182, 183, 184, 1285, 1289, 1290];
const HULL_RESONANCES: [u32; 4] = [113, 111, 109, 110];
const SKILL_EFFECT: u32 = 132;

fn ident_ok(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch.is_ascii_alphabetic() || ch == '_') && c.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn lit(v: f64) -> String {
    if v.is_nan() {
        "f64::NAN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "f64::INFINITY".into() } else { "f64::NEG_INFINITY".into() }
    } else {
        let s = format!("{v:?}");
        s
    }
}

/// Effects whose penalised PostMul modifiers Pyfa (GPL-3.0, eos/effects.py, behaviour only) applies via
/// multiplyItemAttr(..., stackingPenalties=True) in the "default" penalty group, i.e. stacked together with
/// PostPercent boosts (e.g. remote sensor dampeners vs a Warp Core Stabilizer's scanResolutionMultiplier).
/// SDE effects with modifierInfo for which Pyfa (eos/effects.py) has no handler class: Pyfa applies nothing for them
/// (fitting-relevant module/charge/implant/ship effects only; skill effects are handled by the skill folding;
/// security-status ship bonuses are applied by Pyfa's fit code and stay).
const PYFA_NO_HANDLER: [&str; 16] = [
    "skillNaniteInterfacingRepairTime2",
    // see PYFA_SKILL_NOOP (skill items with attribute overrides go through apply_local)
    "skillBonusSupportFightersShield",
    "shadowBarrageDmgMultiplierWithDamageMultiplierPostPercentBarrageDmgMutator",
    "shadowBarrageFalloffWithFalloffPostPercentBarrageFalloffMutator",
    "ammoInfluenceEntityFlyRange",
    "scriptWarpDisruptionFieldGeneratorSetScriptCapacitorNeedHidden",
    "maxRangeHiddenPreAssignmentWarpScrambleRange",
    "shipModuleFocusedWarpDisruptionScript",
    "shipModuleFocusedWarpScramblingScript",
    "leadershipCpuBonus",
    "shipBonusEwWeaponDisruptionStrengthRookie",
    "smugglingModifier",
    "setBonusSerpentis2",
    "moduleBonusIndustrialInvulnerability",
    "cloneRespawnBay",
    "online",
];
/// How Pyfa's hand-written handler for an effect differs from one SDE modifier (observed behaviour of
/// eos/effects.py, GPL-3.0; no Pyfa code used). Each case is listed in DESIGN.md "Pyfa parity quirks".
enum Quirk {
    None,
    /// the handler does not apply this modifier
    Skip,
    /// the handler modifies another attribute
    Target(u32),
    /// the handler uses another operator
    Op(i64),
    /// the handler multiplies (multiplyItemAttr, no stacking penalty) instead of the SDE's PreMul
    PostMulUnpenalised,
    /// the handler filters on another required skill
    Skill(u32),
}
fn pyfa_mod_quirk(effect: &str, modified: u32, aid: &dyn Fn(&str) -> u32) -> Quirk {
    let is = |n: &str| modified == aid(n);
    match effect {
        // Q1 Drone Interfacing: Pyfa boosts the drones' miningDroneAmountPercent (which the drone's own
        // `mining` effect then multiplies into miningAmount), not miningAmount directly
        "skillBonusDroneInterfacing" if is("miningAmount") => Quirk::Target(aid("miningDroneAmountPercent")),
        // Q3 siege / triage / capital industrial core handlers have no gateScrambleStatus line
        "moduleBonusSiegeModule" | "moduleBonusTriageModule" | "industrialCoreEffect2" if is("gateScrambleStatus") => Quirk::Skip,
        // Q4 triage: drones lose damage through damageMultiplier (-100%), the four damage attributes stay
        "moduleBonusTriageModule" if is("kineticDamage") => Quirk::Target(aid("damageMultiplier")),
        "moduleBonusTriageModule" if is("emDamage") || is("thermalDamage") || is("explosiveDamage") => Quirk::Skip,
        // Q5 Rorqual consumption bonus filters on Industrial Reconfiguration (58956), not Capital
        // Industrial Reconfiguration (28585), so it never reaches the Capital Industrial Core
        "shipConsumptionQuantityBonusIndustrialReconfigurationORECapital1" => Quirk::Skill(58956),
        // Q6 Ishkur drone shield bonus is a percentage boost (SDE: ModAdd)
        "shipBonusDroneShieldHitpointsGF2" if is("shieldCapacity") => Quirk::Op(6),
        // Q7 Proteus Hyperspatial Optimization boosts baseWarpSpeed (SDE: warpSpeedMultiplier)
        "subsystemBonusGallentePropulsionWarpSpeed" if is("warpSpeedMultiplier") => Quirk::Target(aid("baseWarpSpeed")),
        // Q8 structure hidden armor multiplier: no power-state scaling of armor plating bonuses
        "structureHiddenArmorHPMultiplier" if is("armorHpBonus") => Quirk::Skip,
        // Q9 Squall-class fitting bonus multiplies after the other modifiers (affects cpu rounding)
        "shipRoleBonusUpwellHaulersMediumMissileFittingBonus" => Quirk::PostMulUnpenalised,
        _ => Quirk::None,
    }
}

/// effect category as Pyfa sees it (handler `type`), where it differs from the SDE category
fn pyfa_cat(name: &str, cat: u32) -> u32 {
    match name {
        "entosisLink" | "superWeaponAmarr" | "superWeaponCaldari" | "superWeaponGallente" | "superWeaponMinmatar" => 1,
        // 'offline' handlers apply in every module state
        "cloakingScanResolutionMultiplier" | "modifyMaxVelocityOfShipPassive" | "disruptionLanceDisallowCloaking" => 7,
        // no handler class: never makes a module activatable
        "online" | "barrage" | "moduleBonusIndustrialInvulnerability" | "cloneRespawnBay" => 0,
        _ => cat,
    }
}
/// Skill effects whose Pyfa handler reads an attribute the skill does not have, so Pyfa applies nothing:
/// skillBonusSupportFightersShield boosts by the skill's `shieldBonus` (absent) instead of the SDE's
/// shieldCapacityBonus, i.e. Support Fighters gives no fighter shield bonus in Pyfa.
/// skillNaniteInterfacingRepairTime2 has no Pyfa handler class at all.
const PYFA_SKILL_NOOP: [&str; 2] = ["skillBonusSupportFightersShield", "skillNaniteInterfacingRepairTime2"];
const PYFA_DEFAULT_GROUP_MUL: [&str; 8] = [
    "fighterAbilityEvasiveManeuvers",
    "industrialCoreEffect2",
    "modifyMaxVelocityOfShipPassive",
    "moduleBonusIntegratedSensorArray",
    "moduleBonusTriageModule",
    "scanResolutionMultiplierOnline",
    "shipCapitalAgilityBonus",
    "systemAgility",
];

fn u(v: &Value) -> Option<u32> {
    v.as_u64().map(|x| x as u32).or_else(|| v.as_f64().map(|x| x as u32))
}

/// SHA-256 as lowercase hex (also used by eve-dogma at runtime for pack / snapshot hashes).
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    let bitlen = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bitlen.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            v = [t1.wrapping_add(s0.wrapping_add(maj)), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// String blob with offsets: returns (blob, offsets[n+1])
fn blob(names: &[String]) -> (String, Vec<u32>) {
    let mut s = String::new();
    let mut off = vec![0u32];
    for n in names {
        s.push_str(n);
        off.push(s.len() as u32);
    }
    (s, off)
}

fn arr<T: std::fmt::Display>(out: &mut String, name: &str, ty: &str, v: &[T]) {
    write!(out, "pub static {name}: [{ty}; {}] = [", v.len()).unwrap();
    for (i, x) in v.iter().enumerate() {
        if i % 32 == 0 {
            out.push('\n');
        }
        write!(out, "{x},").unwrap();
    }
    out.push_str("];\n");
}

struct Eff {
    id: u32,
    name: String,
    cat: u8,
    usage_chance: bool,
    range: u32,
    falloff: u32,
    resist: u32,
    mods: Vec<[i64; 6]>,
}

/// Generate `$OUT_DIR/tables.rs` (static data only; called from `crates/eve-sde/build.rs`).
pub fn run_tables() {
    let (tables, _) = split(&generate());
    write_out("tables.rs", tables);
}

/// Generate `$OUT_DIR/effects.rs` (compiled effect/skill/buff code over the engine's `Fit`; called from
/// `crates/eve-dogma/build.rs`). The tables it refers to come from `eve-sde`.
pub fn run_effects() {
    let (_, code) = split(&generate());
    write_out("effects.rs", code);
}

fn write_out(name: &str, s: String) {
    let dst = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join(name);
    std::fs::write(&dst, s).unwrap();
}

/// Split the generated source into (tables, code): every top-level `pub fn` (with its doc/attribute lines) is code.
fn split(src: &str) -> (String, String) {
    let (mut tables, mut code, mut pending) = (String::new(), String::new(), String::new());
    let mut in_fn = false;
    for line in src.lines() {
        if in_fn {
            code.push_str(line);
            code.push('\n');
            if line == "}" {
                in_fn = false;
            }
        } else if line.starts_with("///") || line.starts_with("#[") {
            pending.push_str(line);
            pending.push('\n');
        } else if line.starts_with("pub fn ") {
            code.push_str(&pending);
            pending.clear();
            code.push_str(line);
            code.push('\n');
            in_fn = !line.ends_with('}');
        } else {
            tables.push_str(&pending);
            pending.clear();
            tables.push_str(line);
            tables.push('\n');
        }
    }
    tables.push_str(&pending);
    (tables, code)
}

fn generate() -> String {
    let path = std::env::var("EVE_DOGMA_DATASET").unwrap_or_else(|_| DEFAULT_DATASET.to_string());
    println!("cargo:rerun-if-env-changed=EVE_DOGMA_DATASET");
    println!("cargo:rerun-if-changed={path}");
    println!("cargo:rerun-if-changed=build.rs");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read dataset {path}: {e} (set EVE_DOGMA_DATASET)"));
    let json: Vec<u8> = if bytes.len() > 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut d = flate2::read::GzDecoder::new(&bytes[..]);
        let mut o = Vec::new();
        d.read_to_end(&mut o).expect("gunzip dataset");
        o
    } else {
        bytes
    };
    let sha = sha256_hex(&json);
    let d: Value = serde_json::from_slice(&json).expect("dataset json");
    assert_eq!(d["format"], "exct-eve-dataset", "unsupported dataset format");
    assert_eq!(d["format_version"], 1, "unsupported dataset format_version");
    let mut out = String::with_capacity(8 << 20);
    out.push_str("// @generated by build.rs from the EXCT dataset. Do not edit.\n");
    writeln!(out, "pub const SDE_BUILD: u64 = {};", d["sde"]["build"].as_u64().unwrap_or(0)).unwrap();
    writeln!(out, "pub const SDE_RELEASE_DATE: &str = {:?};", d["sde"]["release_date"].as_str().unwrap_or("")).unwrap();
    writeln!(out, "pub const DATASET_SHA256: &str = {sha:?};").unwrap();
    writeln!(out, "pub const DATASET_REVISION: u64 = {};", d["dataset_revision"].as_u64().unwrap_or(1)).unwrap();

    // ------------------------------------------------------------ attributes (indexed by raw id)
    let attrs = d["attributes"].as_object().unwrap();
    let amax = attrs.keys().map(|k| k.parse::<u32>().unwrap()).max().unwrap() as usize;
    assert!(amax < 65535);
    let mut a_def = vec![0f64; amax + 1];
    let mut a_flags = vec![0u8; amax + 1];
    let mut a_min = vec![0u16; amax + 1];
    let mut a_max = vec![0u16; amax + 1];
    let mut a_names = vec![String::new(); amax + 1];
    let mut attr_by_name: HashMap<String, u32> = HashMap::new();
    let mut name_count: HashMap<String, u32> = HashMap::new();
    for (k, a) in attrs {
        let id: usize = k.parse().unwrap();
        let name = a["name"].as_str().unwrap_or("").to_string();
        *name_count.entry(name.clone()).or_default() += 1;
        attr_by_name.insert(name.clone(), id as u32);
        a_def[id] = a["default"].as_f64().unwrap_or(0.0);
        let stackable = a["stackable"].as_bool().unwrap_or(true);
        let hig = a["high_is_good"].as_bool().unwrap_or(true);
        let round2 = matches!(name.as_str(), "cpu" | "power" | "cpuOutput" | "powerOutput");
        let overload = name.starts_with("overload");
        a_flags[id] = 1 | (stackable as u8) << 1 | (hig as u8) << 2 | (round2 as u8) << 3 | (overload as u8) << 4;
        a_min[id] = a["min_attr"].as_u64().map(|x| x as u16).unwrap_or(0);
        a_max[id] = a["max_attr"].as_u64().map(|x| x as u16).unwrap_or(0);
        a_names[id] = name;
    }
    let stackable = |id: u32| -> bool { (id as usize) > amax || a_flags[id as usize] & 3 != 1 };
    writeln!(out, "pub const ATTR_COUNT: usize = {};", amax + 1).unwrap();
    writeln!(out, "pub const ATTR_N: usize = {};", d["attributes"].as_object().unwrap().len()).unwrap();
    arr(&mut out, "ATTR_DEFAULT", "f64", &a_def.iter().map(|v| lit(*v)).collect::<Vec<_>>());
    arr(&mut out, "ATTR_FLAGS", "u8", &a_flags);
    arr(&mut out, "ATTR_MIN", "u16", &a_min);
    arr(&mut out, "ATTR_MAX", "u16", &a_max);
    let (ab, aoff) = blob(&a_names);
    writeln!(out, "pub static ATTR_NAMES: &str = {ab:?};").unwrap();
    arr(&mut out, "ATTR_NAME_OFF", "u32", &aoff);
    out.push_str("#[allow(non_upper_case_globals, dead_code)]\npub mod a {\n");
    let mut sorted_attr: Vec<(&String, &u32)> = attr_by_name.iter().collect();
    sorted_attr.sort();
    for (n, id) in &sorted_attr {
        if ident_ok(n) && name_count[*n] == 1 {
            writeln!(out, "pub const {n}: u16 = {id};").unwrap();
        }
    }
    out.push_str("}\n");
    let aid = |n: &str| -> u32 { *attr_by_name.get(n).unwrap_or_else(|| panic!("attribute {n} missing")) };

    // ------------------------------------------------------------ groups
    let groups = d["groups"].as_object().unwrap();
    let mut gl: Vec<(u32, u32, String)> =
        groups.iter().map(|(k, g)| (k.parse().unwrap(), u(&g["category"]).unwrap_or(0), g["name"].as_str().unwrap_or("").to_string())).collect();
    gl.sort();
    arr(&mut out, "GROUP_IDS", "u32", &gl.iter().map(|g| g.0).collect::<Vec<_>>());
    let (gb, goff) = blob(&gl.iter().map(|g| g.2.clone()).collect::<Vec<_>>());
    writeln!(out, "pub static GROUP_NAMES: &str = {gb:?};").unwrap();
    arr(&mut out, "GROUP_NAME_OFF", "u32", &goff);
    let group_by_name = |n: &str| gl.iter().find(|g| g.2 == n).map(|g| g.0).unwrap_or_else(|| panic!("group {n}"));
    for (c, n) in [
        ("G_CAPACITOR_BOOSTER", "Capacitor Booster"),
        ("G_SHIELD_BOOSTER", "Shield Booster"),
        ("G_ANCILLARY_SHIELD_BOOSTER", "Ancillary Shield Booster"),
        ("G_ARMOR_REPAIR_UNIT", "Armor Repair Unit"),
        ("G_ANCILLARY_ARMOR_REPAIRER", "Ancillary Armor Repairer"),
        ("G_HULL_REPAIR_UNIT", "Hull Repair Unit"),
    ] {
        writeln!(out, "pub const {c}: u32 = {};", group_by_name(n)).unwrap();
    }

    // ------------------------------------------------------------ effects
    let effects = d["effects"].as_object().unwrap();
    let mut el: Vec<Eff> = effects
        .iter()
        .map(|(k, e)| Eff {
            id: k.parse().unwrap(),
            name: e["name"].as_str().unwrap_or("").to_string(),
            cat: e["category"].as_u64().unwrap_or(0) as u8,
            usage_chance: !e["fitting_usage_chance_attr"].is_null(),
            range: u(&e["range_attr"]).unwrap_or(0),
            falloff: u(&e["falloff_attr"]).unwrap_or(0),
            resist: u(&e["resistance_attr"]).unwrap_or(0),
            mods: e["mods"]
                .as_array()
                .map(|v| {
                    v.iter()
                        .map(|m| {
                            let m = m.as_array().unwrap();
                            let g = |i: usize| m[i].as_i64().unwrap_or(0);
                            [g(0), g(1), g(2), g(3), g(4), g(5)]
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect();
    el.sort_by_key(|e| e.id);
    let eidx: HashMap<u32, usize> = el.iter().enumerate().map(|(i, e)| (e.id, i)).collect();
    let mut eff_by_name: HashMap<String, u32> = HashMap::new();
    let mut en_count: HashMap<String, u32> = HashMap::new();
    for e in &el {
        eff_by_name.insert(e.name.clone(), e.id);
        *en_count.entry(e.name.clone()).or_default() += 1;
    }
    let eid = |n: &str| -> u32 { *eff_by_name.get(n).unwrap_or_else(|| panic!("effect {n} missing")) };
    out.push_str("#[allow(non_upper_case_globals, dead_code)]\npub mod e {\n");
    let mut en_sorted: Vec<_> = eff_by_name.iter().collect();
    en_sorted.sort();
    for (n, id) in en_sorted {
        if ident_ok(n) && en_count[n] == 1 {
            writeln!(out, "pub const {n}: u32 = {id};").unwrap();
        }
    }
    out.push_str("}\n");
    writeln!(out, "pub const EFFECT_COUNT: usize = {};", el.len()).unwrap();
    arr(&mut out, "EFF_IDS", "u32", &el.iter().map(|e| e.id).collect::<Vec<_>>());
    let (eb, eoff) = blob(&el.iter().map(|e| e.name.clone()).collect::<Vec<_>>());
    writeln!(out, "pub static EFF_NAMES: &str = {eb:?};").unwrap();
    arr(&mut out, "EFF_NAME_OFF", "u32", &eoff);
    // meta
    let mut metas = Vec::new();
    for e in &el {
        let all_item = e.mods.iter().all(|m| m[1] == 0);
        let fighter_ab = e.name.starts_with("fighterAbility");
        let flags = (e.usage_chance as u8) | (all_item as u8) << 1 | ((!e.mods.is_empty()) as u8) << 2 | (fighter_ab as u8) << 3;
        let n = e.name.as_str();
        let proj = if n.starts_with("remoteWebifier") || n == "structureModuleEffectStasisWebifier" {
            1
        } else if n.starts_with("remoteTargetPaint") || n == "structureModuleEffectTargetPainter" {
            2
        } else if n.starts_with("remoteSensorDamp") || n == "structureModuleEffectRemoteSensorDampener" {
            3
        } else if n.starts_with("remoteSensorBoost") {
            4
        } else {
            0
        };
        metas.push(format!(
            "EffMeta{{cat:{},flags:{flags},range:{},falloff:{},resist:{},proj:{proj}}}",
            pyfa_cat(&e.name, e.cat as u32),
            e.range,
            e.falloff,
            e.resist
        ));
    }
    arr(&mut out, "EFF_META", "EffMeta", &metas);

    // ---- compiled local modifiers: one match arm per effect, modifiers as straight-line calls
    let specials: HashMap<u32, &str> = [
        (eid("moduleBonusAfterburner"), "f.sp_prop(i, false, p);"),
        (eid("moduleBonusMicrowarpdrive"), "f.sp_prop(i, true, p);"),
        (eid("microJumpDrive"), "f.sp_mjd(i);"),
        (eid("slotModifier"), "f.sp_slot(i, p);"),
        (eid("hardPointModifierEffect"), "f.sp_hardpoint(i, p);"),
        // Pyfa hand-written handlers (eos/effects.py, GPL-3): no modifierInfo in the SDE
        (eid("doomsdayBeamDOT"), "f.sp_lance(i, p);"),
        (eid("doomsdaySlash"), "f.sp_lance(i, p);"),
        (eid("doomsdayConeDOT"), "f.sp_lance(i, p);"),
        (eid("doomsdayHOG"), "f.sp_lance(i, p);"),
        (eid("superWeaponAmarr"), "f.sp_lance(i, p);"),
        (eid("superWeaponCaldari"), "f.sp_lance(i, p);"),
        (eid("superWeaponGallente"), "f.sp_lance(i, p);"),
        (eid("superWeaponMinmatar"), "f.sp_lance(i, p);"),
        (eid("jumpPortalGeneration"), "f.sp_lance(i, p);"),
        (eid("jumpPortalGenerationBO"), "f.sp_lance(i, p);"),
        (eid("cloneJumpAccepting"), "f.sp_lance(i, p);"),
        (eid("cynosuralGeneration"), "f.sp_cyno(i);"),
        (eid("OffensiveDefensiveReduction"), "f.sp_incursion(i);"),
        (eid("freighterAgilityBonus2O2"), "f.sp_freighter_agility(i);"),
        (eid("microJumpPortalDriveCapital"), "f.sp_mjfg(i, p);"),
        (eid("debuffLance"), "f.sp_lance(i, p);"),
        (eid("warpDisruptSphere"), "f.sp_wdfg(i);"),
        (eid("entosisLink"), "f.sp_entosis(i, p);"),
        (eid("microJumpPortalDrive"), "f.sp_mjfg(i, p);"),
        (eid("emergencyHullEnergizer"), "f.sp_ehe(i, p);"),
        (eid("jumpPortalPassengerBonusModAddSkill"), "f.sp_conduit_passengers(i);"),
        (eid("subsystemBonusBlackOpsJumpPassenger"), "f.sp_force_from_charge(i, &[a::isBlackOpsJumpPortalPassenger, a::isBlackOpsJumpConduitPassenger]);"),
        (eid("modifyJumpConduitPassengerRequired"), "f.sp_force_from_charge(i, &[a::jumpConduitPassengerRequiredAttributeID]);"),
        // Pyfa quirks (observed behaviour of eos/effects.py handlers; see DESIGN.md "Pyfa parity quirks")
        (eid("mining"), "f.sp_mining_drone(i);"),
        (eid("missileDMGBonus"), "f.sp_missile_charge_damage(i, true, true);"),
        (eid("missileDMGBonusPassive"), "f.sp_missile_charge_damage(i, false, p);"),
        (eid("moduleBonusBreacherPodDamageControl"), "f.sp_breacher_pod_dc(i);"),
    ]
    .into_iter()
    .collect();
    let bastion = eid("moduleBonusBastionModule");
    out.push_str("/// Apply effect `ei` (dense index) of item `i`. `p` = source is not exempt from stacking penalties.\n");
    out.push_str("#[allow(unused_variables, clippy::all)]\npub fn apply_local(f: &mut Fit, ei: u16, i: usize, p: bool) {\n    match ei {\n");
    let mut n_local_mods = 0usize;
    for (ix, e) in el.iter().enumerate() {
        if e.id == SKILL_EFFECT {
            continue;
        }
        if let Some(code) = specials.get(&e.id) {
            writeln!(out, "        {ix} => {{ {code} }} // {}", e.name).unwrap();
            continue;
        }
        if PYFA_NO_HANDLER.contains(&e.name.as_str()) {
            continue;
        }
        // Pyfa's sensor array handlers do not touch warpScrambleStatus
        let skip_attr = if e.name == "moduleBonusNetworkedSensorArray" || e.name == "moduleBonusIntegratedSensorArray" { 104 } else { u32::MAX };
        let mut body = String::new();
        let mut need_self_skill = false;
        for m in &e.mods {
            let (func, dom, modified, modifying, op, mut extra) = (m[0], m[1], m[2] as u32, m[3] as u32, m[4], m[5] as u32);
            // Pyfa effect 2791 handler filters charges on Missile Launcher Operation (3319), not the SDE's
            // Acceleration Control (3452), so the Exile / Mindflood side effect raises missile explosion radius
            if e.name == "boosterMissileExplosionCloudPenaltyFixed" && extra == 3452 {
                extra = 3319;
            }
            if func >= 5 || op == 9 || dom == 5 || dom == 6 || modified == skip_attr {
                continue;
            }
            // Pyfa writes each modifier once: exact duplicates in the SDE modifierInfo apply once
            if e.mods.iter().position(|x| x == m).map(|k| !std::ptr::eq(&e.mods[k], m)).unwrap_or(false) {
                continue;
            }
            let (mut modified, mut op) = (modified, op);
            let mut force_unpen = false;
            match pyfa_mod_quirk(&e.name, modified, &aid) {
                Quirk::None => {}
                Quirk::Skip => continue,
                Quirk::Target(t) => modified = t,
                Quirk::Op(o) => op = o,
                Quirk::PostMulUnpenalised => {
                    op = 4;
                    force_unpen = true;
                }
                Quirk::Skill(sk) => extra = sk,
            }
            let pen = if force_unpen || stackable(modified) || (e.id == bastion && HULL_RESONANCES.contains(&modified)) { "false" } else { "p" };
            let skill = if extra == 0 && (func == 3 || func == 4) {
                need_self_skill = true;
                "st".to_string()
            } else {
                extra.to_string()
            };
            // Pyfa stacks these effects' penalised PostMul with PostPercent boosts ("default" penalty group)
            let op = if op == 4 && pen == "p" && PYFA_DEFAULT_GROUP_MUL.contains(&e.name.as_str()) { 8 } else { op };
            let args = format!("{modified}, {op}, i, {modifying}, {pen}");
            let call = match (dom, func) {
                (0, 0) => format!("f.m_item(i, {args});"),
                (0, _) => continue,
                (3, _) => format!("f.m_other({args});"),
                (1, 0) | (4, 0) => format!("f.m_item(f.ship, {args});"),
                (1, 1) | (4, 1) => format!("f.m_ship_loc({args});"),
                (1, 2) | (4, 2) => format!("f.m_ship_group({extra}, {args});"),
                (1, 3) | (4, 3) => format!("f.m_ship_skill({skill}, {args});"),
                (1, 4) | (4, 4) => format!("f.m_owner_skill({skill}, {args});"),
                (2, 0) => format!("f.m_item(f.char, {args});"),
                (2, 1) => format!("f.m_char_loc({args});"),
                (2, 2) => format!("f.m_char_group({extra}, {args});"),
                (2, 3) | (2, 4) => format!("f.m_char_skill({skill}, {args});"),
                _ => continue,
            };
            n_local_mods += 1;
            if dom == 4 {
                writeln!(body, "            if f.is_structure {{ {call} }}").unwrap();
            } else {
                writeln!(body, "            {call}").unwrap();
            }
        }
        if body.is_empty() {
            continue;
        }
        writeln!(out, "        {ix} => {{ // {} ({})", e.name, e.id).unwrap();
        if need_self_skill {
            out.push_str("            let st = f.items[i].type_id;\n");
        }
        out.push_str(&body);
        out.push_str("        }\n");
    }
    out.push_str("        _ => {}\n    }\n}\n");
    writeln!(out, "pub const COMPILED_LOCAL_MODIFIERS: usize = {n_local_mods};").unwrap();

    // ---- compiled projected modifiers (effects applied TO this fit by projected modules/drones)
    out.push_str("/// Projected effect `ei`: calls `p(target_attr, source_attr, op)`; returns false if the effect has no modifierInfo.\n");
    out.push_str("#[allow(clippy::all)]\npub fn apply_projected(ei: u16, p: &mut dyn FnMut(u16, u16, i8)) -> bool {\n    match ei {\n");
    for (ix, e) in el.iter().enumerate() {
        if e.mods.is_empty() || (e.cat != 2 && e.cat != 3) {
            continue;
        }
        let mut body = String::new();
        for m in &e.mods {
            if matches!(m[1], 5 | 6 | 1) && m[0] == 0 {
                write!(body, " p({}, {}, {});", m[2], m[3], m[4]).unwrap();
            }
        }
        writeln!(out, "        {ix} => {{{body} true }} // {}", e.name).unwrap();
    }
    out.push_str("        _ => false,\n    }\n}\n");

    // ------------------------------------------------------------ types
    let types = d["types"].as_object().unwrap();
    let mut tl: Vec<(u32, &Value)> = types.iter().map(|(k, t)| (k.parse().unwrap(), t)).collect();
    tl.sort_by_key(|t| t.0);
    let mut vals: Vec<u64> = Vec::new();
    let mut vidx: HashMap<u64, u16> = HashMap::new();
    let mut pool = |v: f64| -> u16 {
        let b = v.to_bits();
        *vidx.entry(b).or_insert_with(|| {
            vals.push(b);
            (vals.len() - 1) as u16
        })
    };
    let sec_attrs = [aid("hiSecModifier"), aid("lowSecModifier"), aid("nullSecModifier")];
    let (mut ta_id, mut ta_val, mut te, mut trs) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut recs = Vec::new();
    let mut names = Vec::new();
    let mut skills = Vec::new();
    for (id, t) in &tl {
        let name = t["name"].as_str().unwrap_or("").to_string();
        let mut av: BTreeMap<u32, f64> = t["attrs"]
            .as_object()
            .map(|o| o.iter().map(|(k, v)| (k.parse().unwrap(), v.as_f64().unwrap_or(0.0))).collect())
            .unwrap_or_default();
        let f = |k: &str| t[k].as_f64().unwrap_or(0.0);
        // type-level fields are authoritative (same rule as the reference engine)
        for (a, v) in [(4u32, f("mass")), (38, f("capacity")), (161, f("volume")), (162, f("radius"))] {
            if v != 0.0 || !av.contains_key(&a) {
                av.insert(a, v);
            }
        }
        let a0 = ta_id.len();
        for (a, v) in &av {
            ta_id.push(*a as u16);
            ta_val.push(pool(*v));
        }
        let e0 = te.len();
        if let Some(es) = t["effects"].as_array() {
            for e in es {
                let e = e.as_array().unwrap();
                let (x, def) = (u(&e[0]).unwrap(), e[1].as_u64().unwrap_or(0) != 0);
                if let Some(ix) = eidx.get(&x) {
                    te.push(((*ix as u32) << 1 | def as u32) as u16);
                }
            }
        }
        let r0 = trs.len();
        for a in REQ_SKILL_ATTRS {
            if let Some(v) = av.get(&a) {
                if *v as u32 != 0 {
                    trs.push(*v as u32);
                }
            }
        }
        let cat = u(&t["category"]).unwrap_or(0);
        let published = t["published"].as_bool().unwrap_or(false);
        let has_sec = sec_attrs.iter().any(|a| av.contains_key(a));
        if cat == 16 && published {
            skills.push(*id);
        }
        let flags = published as u8 | (has_sec as u8) << 1;
        recs.push(format!(
            "TypeRec{{group:{},category:{cat},flags:{flags},attrs:{a0},n_attrs:{},effs:{e0},n_effs:{},req:{r0},n_req:{},mass:{},volume:{},capacity:{}}}",
            u(&t["group"]).unwrap_or(0),
            av.len(),
            te.len() - e0,
            trs.len() - r0,
            pool(f("mass")),
            pool(f("volume")),
            pool(f("capacity")),
        ));
        names.push(name);
    }
    assert!(vals.len() < 65535);
    writeln!(out, "pub const TYPE_COUNT: usize = {};", tl.len()).unwrap();
    arr(&mut out, "TYPE_IDS", "u32", &tl.iter().map(|t| t.0).collect::<Vec<_>>());
    arr(&mut out, "TYPES", "TypeRec", &recs);
    arr(&mut out, "TA_ID", "u16", &ta_id);
    arr(&mut out, "TA_VAL", "u16", &ta_val);
    arr(&mut out, "TE", "u16", &te);
    arr(&mut out, "TRS", "u32", &trs);
    arr(&mut out, "VALS", "f64", &vals.iter().map(|b| lit(f64::from_bits(*b))).collect::<Vec<_>>());
    let (nb, noff) = blob(&names);
    writeln!(out, "pub static TYPE_NAMES: &str = {nb:?};").unwrap();
    arr(&mut out, "TYPE_NAME_OFF", "u32", &noff);
    // Chinese names (dataset `names.zh`; "" = none) and meta levels (-1 = none) for search / type helpers
    let zh = &d["names"]["zh"];
    let zh_names: Vec<String> = tl.iter().map(|(id, _)| zh[id.to_string()].as_str().unwrap_or("").to_string()).collect();
    let (zb, zoff) = blob(&zh_names);
    writeln!(out, "pub static TYPE_NAMES_ZH: &str = {zb:?};").unwrap();
    arr(&mut out, "TYPE_NAME_ZH_OFF", "u32", &zoff);
    arr(&mut out, "TYPE_META_LEVEL", "i16", &tl.iter().map(|(_, t)| t["meta_level"].as_i64().unwrap_or(-1)).collect::<Vec<_>>());
    // optimizer candidate sets: SDE meta group (0 = none) and variation parent (0 = none)
    arr(&mut out, "TYPE_META_GROUP", "u16", &tl.iter().map(|(_, t)| t["meta_group"].as_u64().unwrap_or(0)).collect::<Vec<_>>());
    arr(&mut out, "TYPE_VARIATION_PARENT", "u32", &tl.iter().map(|(_, t)| t["variation_parent"].as_u64().unwrap_or(0)).collect::<Vec<_>>());
    // EFT export (Pyfa exportDrones DRONE_ORDER): drone market group -> sort rank; a type without its own market
    // group uses its variation parent's (Pyfa Market.getMarketGroupByItem parentcheck).
    {
        const MG_RANK: [(u32, u8); 22] = [
            (837, 0), (1531, 0), (3881, 1), (838, 2), (1532, 2), (3882, 3), (359, 4), (839, 4), (3883, 5), (911, 6), (1533, 6),
            (843, 7), (1586, 7), (841, 8), (1029, 8), (842, 9), (1030, 9), (158, 10), (358, 10), (1643, 11), (1646, 11),
            (0, 99),
        ];
        let rank = |mg: Option<u32>| mg.and_then(|m| MG_RANK.iter().find(|x| x.0 == m).map(|x| x.1));
        let mut dro: Vec<String> = Vec::new();
        for (id, t) in &tl {
            if u(&t["category"]) != Some(18) {
                continue;
            }
            let mut r = rank(u(&t["market_group"]));
            if r.is_none() && t["market_group"].is_null() {
                if let Some(p) = u(&t["variation_parent"]) {
                    r = rank(types.get(&p.to_string()).and_then(|pt| u(&pt["market_group"])));
                }
            }
            dro.push(format!("({id}, {})", r.unwrap_or(99)));
        }
        arr(&mut out, "DRONE_EFT_RANK", "(u32, u8)", &dro);
        let cats = d["categories"].as_object().unwrap();
        let mut cl: Vec<(u32, String)> = cats.iter().map(|(k, c)| (k.parse().unwrap(), c["name"].as_str().unwrap_or("").to_string())).collect();
        cl.sort();
        arr(&mut out, "CAT_IDS", "u32", &cl.iter().map(|c| c.0).collect::<Vec<_>>());
        let (cb, coff) = blob(&cl.iter().map(|c| c.1.clone()).collect::<Vec<_>>());
        writeln!(out, "pub static CAT_NAMES: &str = {cb:?};").unwrap();
        arr(&mut out, "CAT_NAME_OFF", "u32", &coff);
    }
    skills.sort();
    arr(&mut out, "PUBLISHED_SKILLS", "u32", &skills);

    // ---- folded skills: a published skill's attributes are only ever modified by the skill's own Item-domain
    // modifiers (verified below), so every value a skill exports is a pure function of its level (0..5).
    // We evaluate those at build time and compile each skill's outbound modifiers with constant sources.
    {
        let skill_groups: BTreeSet<u32> = gl.iter().filter(|g| g.1 == 16).map(|g| g.0).collect();
        for e in &el {
            for m in &e.mods {
                if m[1] == 2 && (m[0] == 1 || (m[0] == 2 && skill_groups.contains(&(m[5] as u32)))) {
                    panic!("effect {} modifies skill attributes (char location); skill folding assumption broken", e.name);
                }
            }
        }
        let structure_ok: Vec<u32> = [
            "targetingMaxTargetBonusModAddMaxLockedTargetsLocationChar",
            "skillStructureMissileDamageBonus",
            "skillStructureElectronicSystemsCapNeedBonus",
            "skillStructureEngineeringSystemsCapNeedBonus",
            "skillStructureDoomsdayDurationBonus",
        ]
        .iter()
        .map(|n| eid(n))
        .collect();
        let mut vals_tab: Vec<String> = Vec::new();
        let mut code = String::new();
        code.push_str("/// Outbound modifiers of published skill #`k` (index into PUBLISHED_SKILLS) at level `l`, sources folded to constants.\n");
        code.push_str("#[allow(unused_variables, clippy::all)]\npub fn apply_skill(f: &mut Fit, k: usize, l: usize) {\n    match k {\n");
        let mut n_folded = 0usize;
        for (k, sid) in skills.iter().enumerate() {
            let t = &types[&sid.to_string()];
            let mut base: BTreeMap<u32, f64> = t["attrs"]
                .as_object()
                .map(|o| o.iter().map(|(k, v)| (k.parse().unwrap(), v.as_f64().unwrap_or(0.0))).collect())
                .unwrap_or_default();
            let f = |k: &str| t[k].as_f64().unwrap_or(0.0);
            for (a, v) in [(4u32, f("mass")), (38, f("capacity")), (161, f("volume")), (162, f("radius"))] {
                if v != 0.0 || !base.contains_key(&a) {
                    base.insert(a, v);
                }
            }
            // applicable effects (skills are always "online": categories 0, 4, 7)
            let effs: Vec<&Eff> = t["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|e| eidx.get(&u(&e[0]).unwrap()).map(|ix| &el[*ix]))
                .filter(|e| e.id != SKILL_EFFECT && matches!(e.cat, 0 | 4 | 7))
                .collect();
            let own: Vec<(u32, i64, u32)> = effs
                .iter()
                .flat_map(|e| e.mods.iter())
                .filter(|m| m[0] == 0 && m[1] == 0 && m[4] != 9)
                .map(|m| (m[2] as u32, m[4], m[3] as u32))
                .collect();
            let mut body = String::new();
            let mut src_slot: BTreeMap<u32, usize> = BTreeMap::new();
            for e in &effs {
                if PYFA_SKILL_NOOP.contains(&e.name.as_str()) {
                    continue;
                }
                let mut eb = String::new();
                for m in &e.mods {
                    let (func, dom, modified, modifying, op, extra) = (m[0], m[1], m[2] as u32, m[3] as u32, m[4], m[5] as u32);
                    if func >= 5 || op == 9 || dom == 5 || dom == 6 || dom == 0 || dom == 3 {
                        continue;
                    }
                    let modified = match pyfa_mod_quirk(&e.name, modified, &aid) {
                        Quirk::Target(t) => t,
                        Quirk::None => modified,
                        _ => panic!("unsupported Pyfa quirk kind for skill effect {}", e.name),
                    };
                    let slot = *src_slot.entry(modifying).or_insert_with(|| {
                        let row: Vec<String> = (0..6).map(|l| lit(skill_eval(&base, &own, modifying, l as f64, &a_def, &a_flags, &a_min, &a_max, amax, &mut Vec::new()))).collect();
                        vals_tab.push(format!("[{}]", row.join(",")));
                        vals_tab.len() - 1
                    });
                    let skill = if extra == 0 && (func == 3 || func == 4) { *sid } else { extra };
                    let args = format!("{modified}, {op}, SKILL_VALS[{slot}][l]");
                    let call = match (dom, func) {
                        (1, 0) | (4, 0) => format!("f.c_item(f.ship, {args});"),
                        (1, 1) | (4, 1) => format!("f.c_ship_loc({args});"),
                        (1, 2) | (4, 2) => format!("f.c_ship_group({extra}, {args});"),
                        (1, 3) | (4, 3) => format!("f.c_ship_skill({skill}, {args});"),
                        (1, 4) | (4, 4) => format!("f.c_owner_skill({skill}, {args});"),
                        (2, 0) => format!("f.c_item(f.char, {args});"),
                        (2, 2) => format!("f.c_char_group({extra}, {args});"),
                        (2, 3) | (2, 4) => format!("f.c_char_skill({skill}, {args});"),
                        _ => panic!("unexpected skill modifier {m:?} in {}", e.name),
                    };
                    n_folded += 1;
                    if dom == 4 {
                        write!(eb, " if f.is_structure {{ {call} }}").unwrap();
                    } else {
                        write!(eb, " {call}").unwrap();
                    }
                }
                if eb.is_empty() {
                    continue;
                }
                if structure_ok.contains(&e.id) {
                    writeln!(body, "           {eb} // {}", e.name).unwrap();
                } else {
                    writeln!(body, "            if !f.is_structure {{{eb} }} // {}", e.name).unwrap();
                }
            }
            if !body.is_empty() {
                writeln!(code, "        {k} => {{ // {}\n{body}        }}", names[tl.binary_search_by_key(sid, |x| x.0).unwrap()]).unwrap();
            }
        }
        code.push_str("        _ => {}\n    }\n}\n");
        out.push_str(&code);
        arr(&mut out, "SKILL_VALS", "[f64; 6]", &vals_tab);
        writeln!(out, "pub const FOLDED_SKILL_MODIFIERS: usize = {n_folded};").unwrap();
    }
    // name -> type index (lowercase), published types win, then lowest id
    let mut by_name: BTreeMap<String, (bool, u32, usize)> = BTreeMap::new();
    for (ix, (id, t)) in tl.iter().enumerate() {
        let k = names[ix].trim().to_lowercase();
        let p = t["published"].as_bool().unwrap_or(false);
        let cand = (p, *id, ix);
        match by_name.get(&k) {
            Some((bp, bid, _)) if (*bp && !p) || (*bp == p && *bid < *id) => {}
            _ => {
                by_name.insert(k, cand);
            }
        }
    }
    arr(&mut out, "NAME_INDEX", "u16", &by_name.values().map(|v| v.2 as u16).collect::<Vec<_>>());
    let tid_by_name = |n: &str| -> u32 { by_name.get(&n.to_lowercase()).map(|v| v.1).unwrap_or_else(|| panic!("type {n}")) };
    writeln!(out, "pub const T_NANITE_REPAIR_PASTE: u32 = {};", tid_by_name("Nanite Repair Paste")).unwrap();
    writeln!(out, "pub const T_CHARACTER: u32 = 1373;").unwrap();

    // T3D default modes (lowest mode type id whose name starts with the ship name) — resolved at build time
    let modes: Vec<(u32, String)> =
        tl.iter().enumerate().filter(|(_, (_, t))| u(&t["group"]) == Some(1306)).map(|(ix, (id, _))| (*id, names[ix].to_lowercase())).collect();
    let mut ship_modes = Vec::new();
    for (ix, (id, t)) in tl.iter().enumerate() {
        if u(&t["category"]) != Some(6) {
            continue;
        }
        let sn = names[ix].to_lowercase();
        if let Some(m) = modes.iter().filter(|(_, n)| n.starts_with(&sn)).map(|(m, _)| *m).min() {
            ship_modes.push(format!("({id},{m})"));
        }
    }
    arr(&mut out, "SHIP_DEFAULT_MODE", "(u32, u32)", &ship_modes);

    // market groups (prices, docs/23 §5): per-type market group (0 = none) and, when the dataset has the
    // `market_groups` section (pipeline r2+), the parent of every market group (0 = root)
    arr(&mut out, "TYPE_MARKET_GROUP", "u32", &tl.iter().map(|(_, t)| u(&t["market_group"]).unwrap_or(0)).collect::<Vec<_>>());
    let mut mgl: Vec<(u32, u32)> = d["market_groups"]
        .as_object()
        .map(|o| o.iter().map(|(k, g)| (k.parse().unwrap(), u(&g["parent"]).unwrap_or(0))).collect())
        .unwrap_or_default();
    mgl.sort();
    writeln!(out, "pub const HAS_MARKET_GROUP_TREE: bool = {};", d["market_groups"].is_object()).unwrap();
    arr(&mut out, "MARKET_GROUP_IDS", "u32", &mgl.iter().map(|g| g.0).collect::<Vec<_>>());
    arr(&mut out, "MARKET_GROUP_PARENT", "u32", &mgl.iter().map(|g| g.1).collect::<Vec<_>>());

    // lookups (ext/rpc, Pyfa item stats): radius (0 = absent in the SDE; Pyfa shows 1.0), the Traits tab rendered
    // like Pyfa's `traits.display`, and the English description when the dataset carries one (`descriptions`)
    {
        let mut rad: Vec<String> = Vec::new();
        for (id, t) in &tl {
            if let Some(r) = t["radius"].as_f64().filter(|r| *r != 0.0) {
                rad.push(format!("({id}, {r:?})"));
            }
        }
        arr(&mut out, "TYPE_RADIUS", "(u32, f64)", &rad);
        // Alpha clone skill caps (Pyfa alphaCloneID 1 = dataset clone_grades["1"]; bench draft 1.11 alpha_*)
        let mut alpha: Vec<(u32, u64)> = d["clone_grades"]["1"]["skills"]
            .as_object()
            .map(|m| m.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_u64()?))).collect())
            .unwrap_or_default();
        alpha.sort();
        writeln!(out, "pub const HAS_ALPHA_CLONE: bool = {};", !alpha.is_empty()).unwrap();
        arr(&mut out, "ALPHA_CLONE_SKILLS", "(u32, u8)", &alpha.iter().map(|(a, b)| format!("({a}, {b})")).collect::<Vec<_>>());
        let units = &d["units"];
        let tname = |id: &str| types.get(id).and_then(|t| t["name"].as_str()).unwrap_or(id).to_string();
        let strip = |s: &str| {
            let mut o = String::new();
            let mut tag = false;
            for c in s.chars() {
                match c {
                    '<' => tag = true,
                    '>' if tag => tag = false,
                    _ if !tag => o.push(c),
                    _ => {}
                }
            }
            o
        };
        let line = |b: &Value| {
            let text = strip(b["text"].as_str().unwrap_or(""));
            match b["bonus"].as_f64() {
                None => format!("\u{2022} {text}"),
                Some(x) => {
                    let mut n = format!("{x:.6}");
                    while n.contains('.') && (n.ends_with('0') || n.ends_with('.')) {
                        n.pop();
                    }
                    let unit = u(&b["unit"]).and_then(|k| units[k.to_string()]["display"].as_str()).unwrap_or("");
                    format!("{n}{unit} {text}")
                }
            }
        };
        let lines = |a: &Value| {
            let mut v: Vec<&Value> = a.as_array().map(|x| x.iter().collect()).unwrap_or_default();
            v.sort_by_key(|b| b["importance"].as_i64().unwrap_or(0));
            v.iter().map(|b| line(b)).collect::<Vec<_>>().join("<br />\n")
        };
        let mut tids: Vec<(u32, String)> = Vec::new();
        if let Some(tr) = d["traits"].as_object() {
            for (k, t) in tr {
                let mut secs: Vec<String> = Vec::new();
                if let Some(sk) = t["skills"].as_object() {
                    let mut sl: Vec<(String, &Value)> = sk.iter().map(|(sid, b)| (tname(sid), b)).collect();
                    sl.sort_by(|a, b| a.0.cmp(&b.0));
                    for (n, b) in sl {
                        secs.push(format!("<b>{n} bonuses (per skill level):</b><br />\n{}", lines(b)));
                    }
                }
                for (key, head) in [("role", "Role Bonus:"), ("misc", "Misc bonus:")] {
                    if t[key].as_array().is_some_and(|a| !a.is_empty()) {
                        secs.push(format!("<b>{head}</b><br />\n{}", lines(&t[key])));
                    }
                }
                tids.push((k.parse().unwrap(), secs.join("<br />\n<br />\n")));
            }
        }
        tids.sort();
        let (tb, toff) = blob(&tids.iter().map(|x| x.1.clone()).collect::<Vec<_>>());
        writeln!(out, "pub static TRAITS_HTML: &str = {tb:?};").unwrap();
        arr(&mut out, "TRAITS_IDS", "u32", &tids.iter().map(|x| x.0).collect::<Vec<_>>());
        arr(&mut out, "TRAITS_OFF", "u32", &toff);
        // English descriptions: gzip of [u32 LE id, u32 LE byte length, UTF-8 text]* in id order, written next to the
        // generated source and embedded with include_bytes! (decoded lazily by eve-dogma's `lookup`)
        let mut dids: Vec<(u32, String)> = d["descriptions"]
            .as_object()
            .map(|o| o.iter().filter_map(|(k, v)| Some((k.parse().ok()?, v.as_str()?.to_string()))).collect())
            .unwrap_or_default();
        dids.sort();
        writeln!(out, "pub const HAS_DESCRIPTIONS: bool = {};", d["descriptions"].is_object()).unwrap();
        let mut raw = Vec::new();
        for (id, t) in &dids {
            raw.extend_from_slice(&id.to_le_bytes());
            raw.extend_from_slice(&(t.len() as u32).to_le_bytes());
            raw.extend_from_slice(t.as_bytes());
        }
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
        std::io::Write::write_all(&mut gz, &raw).unwrap();
        if let Ok(od) = std::env::var("OUT_DIR") {
            std::fs::write(std::path::Path::new(&od).join("descriptions.bin.gz"), gz.finish().unwrap()).unwrap();
            writeln!(out, "pub static DESCRIPTIONS_GZ: &[u8] = include_bytes!(concat!(env!(\"OUT_DIR\"), \"/descriptions.bin.gz\"));").unwrap();
        } else {
            writeln!(out, "pub static DESCRIPTIONS_GZ: &[u8] = &[];").unwrap();
        }
    }

    // fighter default abilities (Pyfa default: standard attack on; abilities before it on except MWD/evasive/MJD)
    let mut fdef = Vec::new();
    let mut fdef_ab = Vec::new();
    for (id, t) in &tl {
        if u(&t["category"]) != Some(87) {
            continue;
        }
        let mut ids: Vec<u32> = t["effects"].as_array().map(|v| v.iter().filter_map(|e| u(&e[0])).collect()).unwrap_or_default();
        ids.sort();
        let start = fdef_ab.len();
        let mut std_seen = false;
        for e in ids {
            let Some(ix) = eidx.get(&e) else { continue };
            let n = el[*ix].name.as_str();
            if !n.starts_with("fighterAbility") {
                continue;
            }
            if n == "fighterAbilityAttackM" {
                fdef_ab.push(e);
                std_seen = true;
            } else if !std_seen && !matches!(n, "fighterAbilityMicroWarpDrive" | "fighterAbilityEvasiveManeuvers" | "fighterAbilityMicroJumpDrive") {
                fdef_ab.push(e);
            }
        }
        fdef.push(format!("({id},{start},{})", fdef_ab.len() - start));
    }
    arr(&mut out, "FIGHTER_DEFAULTS", "(u32, u32, u32)", &fdef);
    arr(&mut out, "FIGHTER_DEFAULT_ABILITIES", "u32", &fdef_ab);

    // ------------------------------------------------------------ warfare buffs compiled to code
    let dbuffs = d["dbuffs"].as_object().unwrap();
    let mut dl: Vec<(u32, &Value)> = dbuffs.iter().map(|(k, v)| (k.parse().unwrap(), v)).collect();
    dl.sort_by_key(|x| x.0);
    out.push_str("/// Apply warfare buff `id` with value source `src`. Returns false for unknown buffs.\n");
    out.push_str("#[allow(clippy::all)]\npub fn apply_dbuff(f: &mut Fit, id: u32, src: Src, s: usize) -> bool {\n    match id {\n");
    let mut mins = Vec::new();
    for (id, b) in &dl {
        let op = b["op"].as_i64().unwrap_or(0);
        if b["aggregate"].as_str() == Some("Minimum") {
            mins.push(*id);
        }
        let mut body = String::new();
        for a in b["item"].as_array().into_iter().flatten() {
            write!(body, " f.b_item({}, {op}, src, s);", u(a).unwrap()).unwrap();
        }
        for a in b["location"].as_array().into_iter().flatten() {
            write!(body, " f.b_loc({}, {op}, src, s);", u(a).unwrap()).unwrap();
        }
        for x in b["location_group"].as_array().into_iter().flatten() {
            write!(body, " f.b_group({}, {}, {op}, src, s);", u(&x[0]).unwrap(), u(&x[1]).unwrap()).unwrap();
        }
        for x in b["location_skill"].as_array().into_iter().flatten() {
            write!(body, " f.b_skill({}, {}, {op}, src, s);", u(&x[0]).unwrap(), u(&x[1]).unwrap()).unwrap();
        }
        writeln!(out, "        {id} => {{{body} true }}").unwrap();
    }
    out.push_str("        _ => false,\n    }\n}\n");
    arr(&mut out, "DBUFF_IDS", "u32", &dl.iter().map(|x| x.0).collect::<Vec<_>>());
    arr(&mut out, "DBUFF_MIN_AGG", "u32", &mins);

    // ------------------------------------------------------------ mutaplasmids
    let mutas = d["mutaplasmids"].as_object().unwrap();
    let mut ml: Vec<(u32, &Value)> = mutas.iter().map(|(k, v)| (k.parse().unwrap(), v)).collect();
    ml.sort_by_key(|x| x.0);
    let (mut mrec, mut mattr) = (Vec::new(), Vec::new());
    for (_, m) in &ml {
        let s = mattr.len();
        let mut av: BTreeSet<(u32, String)> = BTreeSet::new();
        for (k, r) in m["attrs"].as_object().into_iter().flatten() {
            av.insert((k.parse().unwrap(), format!("({k},{},{})", lit(r[0].as_f64().unwrap()), lit(r[1].as_f64().unwrap()))));
        }
        mattr.extend(av.into_iter().map(|x| x.1));
        mrec.push(format!("({s},{})", mattr.len() - s));
    }
    // (mutaplasmid, input type, output type), sorted -- EFT import resolves base + mutaplasmid -> mutated type
    let mut mmap: Vec<(u32, u32, u32)> = Vec::new();
    for (id, m) in &ml {
        for mp in m["mapping"].as_array().into_iter().flatten() {
            let o = mp["output"].as_u64().unwrap() as u32;
            for i in mp["inputs"].as_array().into_iter().flatten() {
                mmap.push((*id, i.as_u64().unwrap() as u32, o));
            }
        }
    }
    mmap.sort();
    arr(&mut out, "MUTA_MAP", "(u32, u32, u32)", &mmap.iter().map(|x| format!("({},{},{})", x.0, x.1, x.2)).collect::<Vec<_>>());
    arr(&mut out, "MUTA_IDS", "u32", &ml.iter().map(|x| x.0).collect::<Vec<_>>());
    arr(&mut out, "MUTA_REC", "(u32, u32)", &mrec);
    arr(&mut out, "MUTA_ATTRS", "(u16, f64, f64)", &mattr);

    // stacking penalty factors exp(-(k^2)/7.1289)
    arr(&mut out, "PENALTY", "f64", &(0..16).map(|k: i32| lit((-((k * k) as f64) / 7.1289).exp())).collect::<Vec<_>>());
    // attribute id lists resolved by name at build time (validation)
    let ids_named = |fmt: &dyn Fn(u32) -> String, n: u32| -> Vec<u32> { (1..=n).filter_map(|k| attr_by_name.get(&fmt(k)).copied()).collect() };
    arr(&mut out, "CAN_FIT_GROUP_ATTRS", "u16", &ids_named(&|k| format!("canFitShipGroup{k:02}"), 20));
    arr(&mut out, "CAN_FIT_TYPE_ATTRS", "u16", &ids_named(&|k| format!("canFitShipType{k}"), 11));
    arr(&mut out, "CHARGE_GROUP_ATTRS", "u16", &ids_named(&|k| format!("chargeGroup{k}"), 5));

    out
}

/// Evaluate attribute `attr` of a skill at level `lvl` with only the skill's own Item-domain modifiers
/// (same operator order / caps as the runtime evaluator; skills are exempt from stacking penalties).
#[allow(clippy::too_many_arguments)]
fn skill_eval(
    base: &BTreeMap<u32, f64>,
    own: &[(u32, i64, u32)],
    attr: u32,
    lvl: f64,
    a_def: &[f64],
    a_flags: &[u8],
    a_min: &[u16],
    a_max: &[u16],
    amax: usize,
    busy: &mut Vec<u32>,
) -> f64 {
    let b = if attr == 280 { lvl } else { base.get(&attr).copied().unwrap_or_else(|| if (attr as usize) <= amax { a_def[attr as usize] } else { 0.0 }) };
    let present = attr == 280 || base.contains_key(&attr) || own.iter().any(|m| m.0 == attr);
    if !present {
        return b;
    }
    if busy.contains(&attr) {
        return b;
    }
    busy.push(attr);
    let mut val = b;
    let hig = (attr as usize) > amax || a_flags[attr as usize] & 1 == 0 || a_flags[attr as usize] & 4 != 0;
    for op in -1i64..=7 {
        let mut assign: Option<f64> = None;
        for &(m, o, src) in own {
            if m != attr || o != op {
                continue;
            }
            let v = skill_eval(base, own, src, lvl, a_def, a_flags, a_min, a_max, amax, busy);
            match op {
                -1 | 7 => assign = Some(match assign { None => v, Some(c) => if hig { c.max(v) } else { c.min(v) } }),
                2 => val += v,
                3 => val -= v,
                0 | 4 => val *= v,
                1 | 5 => val *= if v == 0.0 { 1.0 } else { 1.0 / v },
                6 => val *= 1.0 + v / 100.0,
                _ => {}
            }
        }
        if let Some(v) = assign {
            val = v;
        }
    }
    if (attr as usize) <= amax {
        let (mn, mx) = (a_min[attr as usize], a_max[attr as usize]);
        if mn != 0 {
            val = val.max(skill_eval(base, own, mn as u32, lvl, a_def, a_flags, a_min, a_max, amax, busy));
        }
        if mx != 0 {
            val = val.min(skill_eval(base, own, mx as u32, lvl, a_def, a_flags, a_min, a_max, amax, busy));
        }
        if a_flags[attr as usize] & 8 != 0 {
            val = (val * 100.0).round() / 100.0;
        }
    }
    busy.pop();
    val
}
