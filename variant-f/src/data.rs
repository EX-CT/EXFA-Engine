//! Generated static tables + thin accessors. Everything here is compiled in; nothing is parsed at runtime.
#![allow(clippy::all, dead_code)]
use crate::engine::{Fit, Src};

#[derive(Clone, Copy)]
pub struct EffMeta {
    pub cat: u8,
    /// 1 = fitting usage chance (booster side effect), 2 = all modifiers on Item domain, 4 = has modifierInfo, 8 = fighter ability
    pub flags: u8,
    pub range: u16,
    pub falloff: u16,
    pub resist: u16,
    /// projected special without modifierInfo: 1 web, 2 target painter, 3 sensor damp, 4 sensor booster
    pub proj: u8,
}

#[derive(Clone, Copy)]
pub struct TypeRec {
    pub group: u32,
    pub category: u32,
    /// 1 = published, 2 = has hi/low/nullSecModifier
    pub flags: u8,
    pub attrs: u32,
    pub n_attrs: u16,
    pub effs: u32,
    pub n_effs: u8,
    pub req: u32,
    pub n_req: u8,
    pub mass: u16,
    pub volume: u16,
    pub capacity: u16,
}

include!(concat!(env!("OUT_DIR"), "/gen.rs"));

pub const AF_STACKABLE: u8 = 2;
pub const AF_HIGH_IS_GOOD: u8 = 4;
pub const AF_ROUND2: u8 = 8;
/// `overload*` attribute (read by overheat effects; see Fit::eval_before)
pub const AF_OVERLOAD: u8 = 16;

#[inline]
pub fn type_index(id: u32) -> Option<usize> {
    TYPE_IDS.binary_search(&id).ok()
}
#[inline]
pub fn ty(ix: usize) -> &'static TypeRec {
    &TYPES[ix]
}
pub fn type_name(ix: usize) -> &'static str {
    &TYPE_NAMES[TYPE_NAME_OFF[ix] as usize..TYPE_NAME_OFF[ix + 1] as usize]
}
pub fn type_name_by_id(id: u32) -> &'static str {
    type_index(id).map(type_name).unwrap_or("?")
}
pub fn type_attr_ids(ix: usize) -> &'static [u16] {
    let t = &TYPES[ix];
    &TA_ID[t.attrs as usize..t.attrs as usize + t.n_attrs as usize]
}
/// Base (unmodified) attribute value of a type.
#[inline]
pub fn type_attr(ix: usize, attr: u16) -> Option<f64> {
    let t = &TYPES[ix];
    let s = t.attrs as usize;
    let ids = &TA_ID[s..s + t.n_attrs as usize];
    ids.binary_search(&attr).ok().map(|k| VALS[TA_VAL[s + k] as usize])
}
pub fn type_effects(ix: usize) -> &'static [u16] {
    let t = &TYPES[ix];
    &TE[t.effs as usize..t.effs as usize + t.n_effs as usize]
}
pub fn type_req_skills(ix: usize) -> &'static [u32] {
    let t = &TYPES[ix];
    &TRS[t.req as usize..t.req as usize + t.n_req as usize]
}
pub fn type_volume(ix: usize) -> f64 {
    VALS[TYPES[ix].volume as usize]
}
pub fn type_capacity(ix: usize) -> f64 {
    VALS[TYPES[ix].capacity as usize]
}
pub fn type_mass(ix: usize) -> f64 {
    VALS[TYPES[ix].mass as usize]
}
pub fn type_published(ix: usize) -> bool {
    TYPES[ix].flags & 1 != 0
}
/// Lookup by (case-insensitive) name; published types win.
pub fn type_by_name(name: &str) -> Option<u32> {
    let key = name.trim().to_lowercase();
    NAME_INDEX
        .binary_search_by(|&ix| type_name(ix as usize).trim().to_lowercase().as_str().cmp(key.as_str()))
        .ok()
        .map(|k| TYPE_IDS[NAME_INDEX[k] as usize])
}

#[inline]
pub fn attr_default(a: u16) -> f64 {
    ATTR_DEFAULT.get(a as usize).copied().unwrap_or(0.0)
}
#[inline]
pub fn attr_flags(a: u16) -> u8 {
    ATTR_FLAGS.get(a as usize).copied().unwrap_or(0)
}
#[inline]
pub fn attr_stackable(a: u16) -> bool {
    let f = attr_flags(a);
    f & 1 == 0 || f & AF_STACKABLE != 0
}
pub fn attr_name(a: u16) -> Option<&'static str> {
    let i = a as usize;
    if i >= ATTR_COUNT || ATTR_FLAGS[i] & 1 == 0 {
        return None;
    }
    Some(&ATTR_NAMES[ATTR_NAME_OFF[i] as usize..ATTR_NAME_OFF[i + 1] as usize])
}

pub fn eff_name(ei: usize) -> &'static str {
    &EFF_NAMES[EFF_NAME_OFF[ei] as usize..EFF_NAME_OFF[ei + 1] as usize]
}
pub fn eff_index(id: u32) -> Option<usize> {
    EFF_IDS.binary_search(&id).ok()
}

pub fn group_index(id: u32) -> Option<usize> {
    GROUP_IDS.binary_search(&id).ok()
}
pub fn group_name(id: u32) -> Option<&'static str> {
    group_index(id).map(|g| &GROUP_NAMES[GROUP_NAME_OFF[g] as usize..GROUP_NAME_OFF[g + 1] as usize])
}

pub fn default_mode(ship: u32) -> Option<u32> {
    SHIP_DEFAULT_MODE.binary_search_by_key(&ship, |x| x.0).ok().map(|k| SHIP_DEFAULT_MODE[k].1)
}
pub fn fighter_default_abilities(t: u32) -> &'static [u32] {
    match FIGHTER_DEFAULTS.binary_search_by_key(&t, |x| x.0) {
        Ok(k) => {
            let (_, s, n) = FIGHTER_DEFAULTS[k];
            &FIGHTER_DEFAULT_ABILITIES[s as usize..(s + n) as usize]
        }
        Err(_) => &[],
    }
}
pub fn dbuff_exists(id: u32) -> bool {
    DBUFF_IDS.binary_search(&id).is_ok()
}
pub fn dbuff_min_aggregate(id: u32) -> bool {
    DBUFF_MIN_AGG.binary_search(&id).is_ok()
}
pub fn muta_attrs(id: u32) -> Option<&'static [(u16, f64, f64)]> {
    let k = MUTA_IDS.binary_search(&id).ok()?;
    let (s, n) = MUTA_REC[k];
    Some(&MUTA_ATTRS[s as usize..(s + n) as usize])
}

/// Mutated type produced by applying `muta` to `base` (EFT import).
pub fn muta_output(muta: u32, base: u32) -> Option<u32> {
    let k = MUTA_MAP.partition_point(|x| (x.0, x.1) < (muta, base));
    MUTA_MAP.get(k).filter(|x| x.0 == muta && x.1 == base).map(|x| x.2)
}

/// Attribute id by name (linear scan; only used by EFT import).
pub fn attr_by_name(name: &str) -> Option<u16> {
    (0..ATTR_DEFAULT.len() as u16).find(|&a| attr_name(a) == Some(name))
}

pub fn type_name_zh(ix: usize) -> Option<&'static str> {
    let s = &TYPE_NAMES_ZH[TYPE_NAME_ZH_OFF[ix] as usize..TYPE_NAME_ZH_OFF[ix + 1] as usize];
    if s.is_empty() { None } else { Some(s) }
}
pub fn type_meta_level(ix: usize) -> Option<i16> {
    let v = TYPE_META_LEVEL[ix];
    if v < 0 { None } else { Some(v) }
}

/// Pyfa EFT export drone sort rank (DRONE_ORDER index of the drone's market group; 99 = not listed).
pub fn drone_eft_rank(type_id: u32) -> u8 {
    DRONE_EFT_RANK.binary_search_by_key(&type_id, |x| x.0).map(|k| DRONE_EFT_RANK[k].1).unwrap_or(99)
}
pub fn category_name(id: u32) -> Option<&'static str> {
    CAT_IDS.binary_search(&id).ok().map(|c| &CAT_NAMES[CAT_NAME_OFF[c] as usize..CAT_NAME_OFF[c + 1] as usize])
}
