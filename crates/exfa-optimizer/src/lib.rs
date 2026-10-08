//! exfa-optimizer — skill-based fit optimizer (docs/21 "exfa-optimizer" v1).
//!
//! `optimize(request)` searches module / charge / drone choices for a base `FitRequest` under an objective and hard
//! constraints. Every candidate fit is scored by a real engine `calc` (validation on), so stacking penalties, set
//! bonuses, skills and fitting rules are never re-implemented here. Pure: no I/O; the only clock use is the optional
//! native `time_ms` budget (wasm32-unknown-unknown has no clock, `max_evaluations` is its budget).
//!
//! Algorithm (v1): candidate sets per changeable position (variations / group / all fittable, filtered by slot,
//! ship restrictions, rig size, capital size, skills, meta, include/exclude, pruned to the best `per_slot` by a
//! one-module probe) → greedy beam construction rack by rack (rig, high, low, mid) → local search (single swaps,
//! charge choice per weapon, drone stack choice, pair swaps) until nothing improves or the budget is spent.
use exfa_core::data::{self as d, a};
use exfa_core::request::{DroneReq, FitRequest, ModuleReq, Slot, State};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};

// ------------------------------------------------------------------------------------------------ request

#[derive(Debug, Clone, Deserialize)]
pub struct OptimizeRequest {
    pub base: FitRequest,
    #[serde(default)]
    pub objective: Objective,
    #[serde(default)]
    pub constraints: Constraints,
    #[serde(default)]
    pub search: Search,
    #[serde(default)]
    pub limits: Limits,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Objective {
    Single { metric: String, #[serde(default)] direction: Option<String> },
    Weighted(Vec<Term>),
}
impl Default for Objective {
    fn default() -> Self {
        Objective::Single { metric: "dps".into(), direction: None }
    }
}
#[derive(Debug, Clone, Deserialize)]
pub struct Term {
    pub metric: String,
    #[serde(default = "one")]
    pub weight: f64,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub direction: Option<String>,
}
fn one() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Constraints {
    /// "character" (default) or "ignore"
    #[serde(default)]
    pub skills: Option<String>,
    #[serde(default)]
    pub meta: Option<Meta>,
    /// true, or {"min_percent": p}
    #[serde(default)]
    pub cap_stable: Option<Value>,
    #[serde(default)]
    pub min: BTreeMap<String, f64>,
    #[serde(default)]
    pub max: BTreeMap<String, f64>,
    #[serde(default)]
    pub price: Option<Price>,
}
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Meta {
    #[serde(default)]
    pub max_meta_level: Option<i16>,
    #[serde(default)]
    pub meta_groups: Option<Vec<String>>,
}
#[derive(Debug, Clone, Deserialize, Default)]
pub struct Price {
    #[serde(default)]
    pub max_isk: Option<f64>,
    #[serde(default)]
    pub prices: BTreeMap<String, f64>,
    /// "error" (default) or "zero"
    #[serde(default)]
    pub missing: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Search {
    #[serde(default = "default_slots")]
    pub slots: Vec<String>,
    #[serde(default = "yes")]
    pub charges: bool,
    #[serde(default = "yes")]
    pub drones: bool,
    #[serde(default)]
    pub keep: Vec<usize>,
    /// "variations" (default), "group", "all_fittable"
    #[serde(default = "variations")]
    pub candidates: String,
    #[serde(default)]
    pub include_type_ids: Vec<u32>,
    #[serde(default)]
    pub exclude_type_ids: Vec<u32>,
    #[serde(default)]
    pub include_groups: Vec<u32>,
    #[serde(default)]
    pub exclude_groups: Vec<u32>,
}
impl Default for Search {
    fn default() -> Self {
        serde_json::from_value(json!({})).unwrap()
    }
}
fn default_slots() -> Vec<String> {
    vec!["high".into(), "mid".into(), "low".into(), "rig".into()]
}
fn yes() -> bool {
    true
}
fn variations() -> String {
    "variations".into()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Limits {
    #[serde(default = "d_evals")]
    pub max_evaluations: usize,
    #[serde(default = "d_time")]
    pub time_ms: u64,
    #[serde(default = "d_results")]
    pub results: usize,
    #[serde(default)]
    pub seed: u64,
    #[serde(default = "d_beam")]
    pub beam: usize,
    /// candidates kept per position after the probe (docs/21 §3: default 40)
    #[serde(default = "d_per_slot")]
    pub per_slot: usize,
}
impl Default for Limits {
    fn default() -> Self {
        serde_json::from_value(json!({})).unwrap()
    }
}
fn d_evals() -> usize {
    20000
}
fn d_time() -> u64 {
    3000
}
fn d_results() -> usize {
    5
}
fn d_beam() -> usize {
    4
}
fn d_per_slot() -> usize {
    40
}

#[derive(Debug, Clone)]
pub struct OptError {
    pub code: &'static str,
    pub message: String,
}
fn err(code: &'static str, message: impl Into<String>) -> OptError {
    OptError { code, message: message.into() }
}

// ------------------------------------------------------------------------------------------------ evaluator

/// Scores one fit. Default: the engine's `calc` (full precision values).
pub trait Evaluator {
    fn eval(&mut self, req: &FitRequest) -> Value;
}
pub struct Engine;
impl Evaluator for Engine {
    fn eval(&mut self, req: &FitRequest) -> Value {
        exfa_core::calc(req).to_value_raw()
    }
}

// ------------------------------------------------------------------------------------------------ metrics

fn ptr(v: &Value, p: &str) -> Option<f64> {
    let x = v.pointer(p)?;
    x.as_f64().or_else(|| x.as_bool().map(|b| if b { 1.0 } else { 0.0 }))
}

/// Metric value from calc output (None = unknown metric / missing value).
fn metric(v: &Value, name: &str, price: Option<f64>) -> Option<f64> {
    Some(match name {
        "dps" => ptr(v, "/offense/total/dps/total")?,
        "weapon_dps" => ptr(v, "/offense/total/weapon_dps")?,
        "drone_dps" => ptr(v, "/offense/total/drone_dps")?,
        "applied_dps" => ptr(v, "/offense/vs_target_profile/dps")?,
        "volley" => ptr(v, "/offense/total/volley/total")?,
        "ehp" => ptr(v, "/defense/ehp/total")?,
        "tank" => {
            let t = v.pointer("/defense/tank/sustained_effective")?.as_object()?;
            t.values().filter_map(|x| x.as_f64()).fold(0.0, f64::max)
        }
        "speed" | "max_velocity" => ptr(v, "/navigation/max_velocity")?,
        "align_time" => ptr(v, "/navigation/align_time_s")?,
        "cap_stable" => {
            if v.pointer("/capacitor/stable").and_then(|x| x.as_bool()).unwrap_or(false) {
                ptr(v, "/capacitor/stable_percent").unwrap_or(0.0)
            } else {
                ptr(v, "/capacitor/depletes_in_s").unwrap_or(0.0) - 1e9
            }
        }
        "price" | "price_isk" => price?,
        p if p.starts_with('/') => ptr(v, p)?,
        _ => return None,
    })
}
fn known_metric(name: &str) -> bool {
    matches!(
        name,
        "dps" | "weapon_dps" | "drone_dps" | "applied_dps" | "volley" | "ehp" | "tank" | "speed" | "max_velocity" | "align_time" | "cap_stable" | "price" | "price_isk"
    ) || name.starts_with('/')
}
fn default_min(name: &str) -> bool {
    matches!(name, "align_time" | "price" | "price_isk")
}

// ------------------------------------------------------------------------------------------------ static data helpers

fn tattr(ix: usize, at: u16) -> Option<f64> {
    d::type_attr(ix, at)
}
const REQ_SKILLS: [(u16, u16); 6] = [
    (a::requiredSkill1, a::requiredSkill1Level),
    (a::requiredSkill2, a::requiredSkill2Level),
    (a::requiredSkill3, a::requiredSkill3Level),
    (a::requiredSkill4, a::requiredSkill4Level),
    (a::requiredSkill5, a::requiredSkill5Level),
    (a::requiredSkill6, a::requiredSkill6Level),
];
fn slot_of(name: &str) -> Option<Slot> {
    Some(match name {
        "high" => Slot::High,
        "mid" | "med" => Slot::Mid,
        "low" => Slot::Low,
        "rig" => Slot::Rig,
        _ => return None,
    })
}
fn slot_name(s: Slot) -> &'static str {
    match s {
        Slot::High => "high",
        Slot::Mid => "mid",
        Slot::Low => "low",
        Slot::Rig => "rig",
        Slot::Subsystem => "subsystem",
        Slot::Service => "service",
    }
}
fn meta_group_id(name: &str) -> Option<u16> {
    Some(match name {
        "tech1" | "t1" => 1,
        "tech2" | "t2" => 2,
        "storyline" => 3,
        "faction" => 4,
        "officer" => 5,
        "deadspace" => 6,
        "tech3" | "t3" => 14,
        "abyssal" => 15,
        "premium" => 17,
        "structure_tech1" => 52,
        "structure_tech2" => 53,
        _ => return None,
    })
}

/// Static item filter shared by modules, charges and drones.
struct Filter<'a> {
    levels: HashMap<u32, u8>,
    default_level: u8,
    check_skills: bool,
    max_meta: Option<i16>,
    meta_groups: Option<Vec<u16>>,
    search: &'a Search,
    ship_ix: usize,
    ship_group: u32,
    ship_type: u32,
    ship_capital: bool,
    ship_rig_size: f64,
}
impl Filter<'_> {
    fn level(&self, s: u32) -> u8 {
        self.levels.get(&s).copied().unwrap_or(self.default_level)
    }
    fn item_ok(&self, ix: usize) -> bool {
        let id = d::type_id_at(ix);
        let t = d::ty(ix);
        if !d::type_published(ix) || d::is_muta_output(id) {
            return false;
        }
        if self.search.exclude_type_ids.contains(&id) || self.search.exclude_groups.contains(&t.group) {
            return false;
        }
        let restricted = !self.search.include_type_ids.is_empty() || !self.search.include_groups.is_empty();
        if restricted && !self.search.include_type_ids.contains(&id) && !self.search.include_groups.contains(&t.group) {
            return false;
        }
        if let Some(m) = self.max_meta {
            if d::type_meta_level(ix).unwrap_or(0) > m {
                return false;
            }
        }
        if let Some(gs) = &self.meta_groups {
            if !gs.contains(&d::type_meta_group(ix).unwrap_or(1)) {
                return false;
            }
        }
        if self.check_skills {
            for (sa, la) in REQ_SKILLS {
                let s = tattr(ix, sa).unwrap_or(0.0) as u32;
                if s != 0 && (self.level(s) as f64) < tattr(ix, la).unwrap_or(1.0) {
                    return false;
                }
            }
        }
        true
    }
    fn module_ok(&self, ix: usize, slot: Slot) -> bool {
        let t = d::ty(ix);
        if t.category != 7 || exfa_core::engine::infer_slot(ix) != Some(slot) || !self.item_ok(ix) {
            return false;
        }
        let gr: Vec<u32> = d::CAN_FIT_GROUP_ATTRS.iter().filter_map(|x| tattr(ix, *x)).map(|v| v as u32).filter(|v| *v != 0).collect();
        let ty: Vec<u32> = d::CAN_FIT_TYPE_ATTRS.iter().filter_map(|x| tattr(ix, *x)).map(|v| v as u32).filter(|v| *v != 0).collect();
        if (!gr.is_empty() || !ty.is_empty()) && !gr.contains(&self.ship_group) && !ty.contains(&self.ship_type) {
            return false;
        }
        if !self.ship_capital && d::type_volume(ix) >= 4000.0 {
            return false;
        }
        if slot == Slot::Rig && tattr(ix, a::rigSize).unwrap_or(0.0) != self.ship_rig_size {
            return false;
        }
        let _ = self.ship_ix;
        true
    }
    /// charges a weapon type can load (group, size, capacity), allowed by the filter
    fn charges_for(&self, mix: usize) -> Vec<usize> {
        let groups: Vec<u32> = d::CHARGE_GROUP_ATTRS.iter().filter_map(|x| tattr(mix, *x)).map(|v| v as u32).filter(|v| *v != 0).collect();
        if groups.is_empty() {
            return Vec::new();
        }
        let size = tattr(mix, a::chargeSize);
        let cap = d::type_capacity(mix);
        (0..d::type_count())
            .filter(|&c| {
                let t = d::ty(c);
                t.category == 8
                    && groups.contains(&t.group)
                    && (size.is_none() || tattr(c, a::chargeSize) == size)
                    && (cap <= 0.0 || d::type_volume(c) <= cap)
                    && self.item_ok(c)
            })
            .collect()
    }
}

/// static "how much damage does this charge carry" used to pick a default charge for a candidate weapon
fn charge_damage(c: usize) -> f64 {
    [a::emDamage, a::thermalDamage, a::kineticDamage, a::explosiveDamage].iter().map(|x| tattr(c, *x).unwrap_or(0.0)).sum()
}

// ------------------------------------------------------------------------------------------------ search state

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct Pick {
    type_id: u32,
    charge: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct State_ {
    /// one entry per changeable position (None = empty slot)
    picks: Vec<Option<Pick>>,
    /// drone stack override (type, quantity, active); None = base drones
    drones: Option<(u32, u32, u32)>,
}

struct Pos {
    slot: Slot,
    /// index into base.modules when the position starts from a base module
    base_index: Option<usize>,
    base_state: Option<State>,
    cands: Vec<Pick>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Score {
    feasible: bool,
    violation: f64,
    obj: f64,
}
impl Score {
    fn better(&self, o: &Score) -> bool {
        if self.feasible != o.feasible {
            return self.feasible;
        }
        if !self.feasible && (self.violation - o.violation).abs() > 1e-12 {
            return self.violation < o.violation;
        }
        self.obj > o.obj + 1e-9 * o.obj.abs().max(1.0)
    }
}

struct Ctx<'a, E: Evaluator> {
    req: &'a OptimizeRequest,
    ev: &'a mut E,
    positions: Vec<Pos>,
    cache: HashMap<State_, (Score, Value)>,
    evaluated: usize,
    stopped: Option<&'static str>,
    #[cfg(not(target_arch = "wasm32"))]
    start: std::time::Instant,
    terms: Vec<(String, f64, f64, f64)>, // metric, weight, scale, sign
    skills_ignore: bool,
    /// skills missing for the part of the fit the search cannot change (hull, implants, kept modules):
    /// skill type id -> highest required level; such MISSING_SKILL entries do not make a candidate infeasible
    unfixable: HashMap<u64, f64>,
    prices: Option<&'a Price>,
}

impl<'a, E: Evaluator> Ctx<'a, E> {
    fn out_of_budget(&mut self) -> bool {
        if self.evaluated >= self.req.limits.max_evaluations {
            self.stopped = Some("evaluations");
            return true;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.req.limits.time_ms > 0 && self.start.elapsed().as_millis() as u64 >= self.req.limits.time_ms {
            self.stopped = Some("time");
            return true;
        }
        false
    }

    fn build(&self, s: &State_) -> FitRequest {
        self.build_mapped(s).0
    }
    /// Built fit plus, per search position, the index of its module in `fit.modules` (None = left empty).
    fn build_mapped(&self, s: &State_) -> (FitRequest, Vec<Option<usize>>) {
        let mut r = self.req.base.clone();
        let mut mods: Vec<ModuleReq> = Vec::new();
        let mut at: Vec<Option<usize>> = vec![None; s.picks.len()];
        let mut placed = vec![false; s.picks.len()];
        for (i, m) in self.req.base.modules.iter().enumerate() {
            if let Some(p) = self.positions.iter().position(|p| p.base_index == Some(i)) {
                placed[p] = true;
                if let Some(pk) = &s.picks[p] {
                    at[p] = Some(mods.len());
                    mods.push(self.module_req(p, pk));
                }
            } else {
                mods.push(m.clone());
            }
        }
        for (p, done) in placed.iter().enumerate() {
            if !done {
                if let Some(pk) = &s.picks[p] {
                    at[p] = Some(mods.len());
                    mods.push(self.module_req(p, pk));
                }
            }
        }
        r.modules = mods;
        if let Some((t, q, act)) = s.drones {
            r.drones = vec![DroneReq { type_id: t, id: None, quantity: q, active: Some(act), mutation: None }];
        }
        (r, at)
    }
    fn module_req(&self, p: usize, pk: &Pick) -> ModuleReq {
        let pos = &self.positions[p];
        let base = pos.base_index.map(|i| &self.req.base.modules[i]);
        let same = base.map(|b| b.type_id == pk.type_id).unwrap_or(false);
        if let (true, Some(b)) = (same, base) {
            let mut m = b.clone();
            m.charge_type_id = pk.charge;
            return m;
        }
        let state = if pos.slot == Slot::Rig { State::Online } else { pos.base_state.filter(|s| *s != State::Offline).unwrap_or(State::Active) };
        ModuleReq { type_id: pk.type_id, id: None, slot: Some(pos.slot), state: Some(state), charge_type_id: pk.charge, mutation: None, spool: None }
    }

    fn price_of(&self, r: &FitRequest) -> Result<Option<f64>, OptError> {
        let Some(pr) = self.prices else { return Ok(None) };
        let zero = pr.missing.as_deref() == Some("zero");
        let mut total = 0.0;
        let mut add = |t: u32, n: f64| -> Result<(), OptError> {
            match pr.prices.get(&t.to_string()) {
                Some(p) => total += p * n,
                None if zero => {}
                None => return Err(err("OPT_MISSING_PRICE", format!("no price for type {t} ({})", d::type_name_by_id(t)))),
            }
            Ok(())
        };
        add(r.ship.type_id, 1.0)?;
        for m in &r.modules {
            add(m.type_id, 1.0)?;
            if let Some(c) = m.charge_type_id {
                add(c, 1.0)?;
            }
        }
        for dr in &r.drones {
            add(dr.type_id, dr.quantity as f64)?;
        }
        Ok(Some(total))
    }

    fn score(&self, v: &Value, price: Option<f64>) -> Score {
        if v.get("error").is_some() {
            return Score { feasible: false, violation: f64::INFINITY, obj: f64::NEG_INFINITY };
        }
        let mut viol = 0.0;
        if let Some(vs) = v.get("violations").and_then(|x| x.as_array()) {
            for x in vs {
                let code = x.get("code").and_then(|c| c.as_str()).unwrap_or("");
                if code == "MISSING_SKILL" {
                    if self.skills_ignore {
                        continue;
                    }
                    let sid = x.get("skill_type_id").and_then(|i| i.as_u64());
                    let lvl = x.get("level").and_then(|l| l.as_f64()).unwrap_or(5.0);
                    if sid.and_then(|i| self.unfixable.get(&i)).map(|&l| lvl <= l).unwrap_or(false) {
                        continue;
                    }
                }
                viol += 1.0;
            }
        }
        // resource overloads as a gradient (the engine reports them as single codes)
        for (u, t) in [("/resources/cpu/used", "/resources/cpu/total"), ("/resources/power/used", "/resources/power/total"), ("/resources/calibration/used", "/resources/calibration/total")] {
            if let (Some(u), Some(t)) = (ptr(v, u), ptr(v, t)) {
                if u > t + 1e-9 {
                    viol += (u - t) / t.max(1.0);
                }
            }
        }
        let c = &self.req.constraints;
        for (k, lim) in &c.min {
            if let Some(x) = metric(v, k, price) {
                if x < *lim {
                    viol += (lim - x) / lim.abs().max(1.0);
                }
            }
        }
        for (k, lim) in &c.max {
            if let Some(x) = metric(v, k, price) {
                if x > *lim {
                    viol += (x - lim) / lim.abs().max(1.0);
                }
            }
        }
        if let Some(cs) = &c.cap_stable {
            let stable = v.pointer("/capacitor/stable").and_then(|x| x.as_bool()).unwrap_or(false);
            let pct = ptr(v, "/capacitor/stable_percent").unwrap_or(0.0);
            let need = cs.get("min_percent").and_then(|x| x.as_f64());
            if cs.as_bool() == Some(true) || need.is_some() {
                if !stable {
                    viol += 1.0;
                } else if let Some(n) = need {
                    if pct < n {
                        viol += (n - pct) / 100.0;
                    }
                }
            }
        }
        if let (Some(p), Some(maxp)) = (price, c.price.as_ref().and_then(|p| p.max_isk)) {
            if p > maxp {
                viol += (p - maxp) / maxp.max(1.0);
            }
        }
        let mut obj = 0.0;
        for (m, w, sc, sign) in &self.terms {
            obj += sign * w * metric(v, m, price).unwrap_or(f64::NEG_INFINITY.max(-1e300)) / sc;
        }
        Score { feasible: viol == 0.0, violation: viol, obj }
    }

    /// evaluate (cached); None when the budget is spent
    fn eval(&mut self, s: &State_) -> Option<Score> {
        if let Some((sc, _)) = self.cache.get(s) {
            return Some(*sc);
        }
        if self.out_of_budget() {
            return None;
        }
        let r = self.build(s);
        let price = self.price_of(&r).ok().flatten();
        let v = self.ev.eval(&r);
        self.evaluated += 1;
        let sc = self.score(&v, price);
        self.cache.insert(s.clone(), (sc, v));
        Some(sc)
    }
}

// ------------------------------------------------------------------------------------------------ optimize

/// Run the optimizer with the engine as evaluator.
pub fn optimize(req: &OptimizeRequest) -> Result<Value, OptError> {
    optimize_with(req, &mut Engine)
}

/// JSON in (OptimizeRequest), JSON out (result or {"error":{code,message}}).
pub fn optimize_value(p: &Value) -> Value {
    if p.pointer("/base/character/clone").and_then(|c| c.as_str()) == Some("alpha") {
        return json!({"error": {"code": "UNSUPPORTED", "message": "clone \"alpha\" is planned for 1.0 (TODO); not implemented yet"}});
    }
    match serde_json::from_value::<OptimizeRequest>(p.clone()) {
        Ok(r) => match optimize(&r) {
            Ok(v) => v,
            Err(e) => json!({"error": {"code": e.code, "message": e.message}}),
        },
        Err(e) => json!({"error": {"code": "BAD_REQUEST", "message": e.to_string()}}),
    }
}

fn sort_scored(v: &mut [(Score, Pick)]) {
    v.sort_by(|x, y| if x.0.better(&y.0) { std::cmp::Ordering::Less } else if y.0.better(&x.0) { std::cmp::Ordering::Greater } else { x.1.cmp(&y.1) });
}

/// one-module probes of `list` at position `p` on `start`, best first (stops when the budget is spent)
fn probe_list<E: Evaluator>(ctx: &mut Ctx<E>, start: &State_, p: usize, list: Vec<Pick>) -> Vec<(Score, Pick)> {
    let mut out = Vec::new();
    for c in list {
        let mut s = start.clone();
        s.picks[p] = Some(c.clone());
        match ctx.eval(&s) {
            Some(sc) => out.push((sc, c)),
            None => break,
        }
    }
    sort_scored(&mut out);
    out
}

pub fn optimize_with<E: Evaluator>(req: &OptimizeRequest, ev: &mut E) -> Result<Value, OptError> {
    // objective terms
    let raw_terms: Vec<(String, f64, f64, Option<String>)> = match &req.objective {
        Objective::Single { metric, direction } => vec![(metric.clone(), 1.0, 1.0, direction.clone())],
        Objective::Weighted(ts) => ts.iter().map(|t| (t.metric.clone(), t.weight, t.scale, t.direction.clone())).collect(),
    };
    let mut terms = Vec::new();
    for (m, w, sc, dir) in raw_terms {
        if !known_metric(&m) {
            return Err(err("OPT_BAD_METRIC", format!("unknown metric '{m}'")));
        }
        let minimize = match dir.as_deref() {
            Some("min") => true,
            Some("max") => false,
            None => default_min(&m),
            Some(o) => return Err(err("OPT_BAD_METRIC", format!("direction must be min or max, got '{o}'"))),
        };
        terms.push((m, w, if sc == 0.0 { 1.0 } else { sc }, if minimize { -1.0 } else { 1.0 }));
    }
    for k in req.constraints.min.keys().chain(req.constraints.max.keys()) {
        if !known_metric(k) {
            return Err(err("OPT_BAD_METRIC", format!("unknown constraint metric '{k}'")));
        }
    }
    let uses_price = terms.iter().any(|t| t.0.starts_with("price")) || req.constraints.price.as_ref().and_then(|p| p.max_isk).is_some();
    let ship_ix = d::type_index(req.base.ship.type_id).ok_or_else(|| err("UNKNOWN_TYPE", format!("unknown ship type {}", req.base.ship.type_id)))?;
    let has_levels = !req.base.character.skills.levels.is_empty() || req.base.character.skills.default_level.is_some();
    let skills_mode = req.constraints.skills.clone().unwrap_or_else(|| if has_levels { "character".into() } else { "ignore".into() });
    let skills_ignore = skills_mode == "ignore";
    let mut levels = HashMap::new();
    for (k, v) in &req.base.character.skills.levels {
        if let Ok(id) = k.parse::<u32>().or_else(|_| d::type_by_name(k).ok_or(())) {
            levels.insert(id, (*v).min(5));
        }
    }
    let meta = req.constraints.meta.clone().unwrap_or_default();
    let meta_groups = match &meta.meta_groups {
        Some(gs) => {
            let mut ids = Vec::new();
            for g in gs {
                ids.push(meta_group_id(g).ok_or_else(|| err("BAD_REQUEST", format!("unknown meta group '{g}'")))?);
            }
            Some(ids)
        }
        None => None,
    };
    let filter = Filter {
        levels,
        default_level: req.base.character.skills.default_level.unwrap_or(0).min(5),
        check_skills: !skills_ignore,
        max_meta: meta.max_meta_level,
        meta_groups,
        search: &req.search,
        ship_ix,
        ship_group: d::ty(ship_ix).group,
        ship_type: req.base.ship.type_id,
        ship_capital: tattr(ship_ix, a::isCapitalSize).unwrap_or(0.0) == 1.0,
        ship_rig_size: tattr(ship_ix, a::rigSize).unwrap_or(0.0),
    };
    let mut warnings: Vec<String> = Vec::new();

    // base evaluation (also gives slot totals)
    let base_v = ev.eval(&req.base);
    if let Some(e) = base_v.get("error") {
        return Err(err("BAD_REQUEST", format!("base fit: {e}")));
    }
    let mut ctx = Ctx {
        req,
        ev,
        positions: Vec::new(),
        cache: HashMap::new(),
        evaluated: 1,
        stopped: None,
        #[cfg(not(target_arch = "wasm32"))]
        start: std::time::Instant::now(),
        terms,
        skills_ignore,
        unfixable: HashMap::new(),
        prices: if uses_price { Some(req.constraints.price.as_ref().unwrap()) } else { req.constraints.price.as_ref().filter(|p| !p.prices.is_empty()) },
    };
    if uses_price {
        ctx.price_of(&req.base)?;
    }
    let base_price = ctx.price_of(&req.base).ok().flatten();

    // ---- positions and candidate sets
    let slots: Vec<Slot> = req.search.slots.iter().filter_map(|s| slot_of(s)).collect();
    let all_by_slot = |slot: Slot| -> Vec<usize> { (0..d::type_count()).filter(|&ix| filter.module_ok(ix, slot)).collect() };
    let mut family: HashMap<u32, Vec<usize>> = HashMap::new();
    if req.search.candidates == "variations" {
        for ix in 0..d::type_count() {
            if d::ty(ix).category == 7 {
                let root = d::type_variation_parent(ix).unwrap_or(d::type_id_at(ix));
                family.entry(root).or_default().push(ix);
            }
        }
    }
    let charge_cache: std::cell::RefCell<HashMap<usize, Vec<usize>>> = Default::default();
    let charges_of = |mix: usize| -> Vec<usize> { charge_cache.borrow_mut().entry(mix).or_insert_with(|| filter.charges_for(mix)).clone() };
    let default_charge = |mix: usize, base: Option<u32>| -> Option<u32> {
        let cs = charges_of(mix);
        if cs.is_empty() {
            return None;
        }
        if let Some(b) = base {
            if cs.iter().any(|&c| d::type_id_at(c) == b) {
                return Some(b);
            }
        }
        cs.iter().copied().max_by(|&x, &y| charge_damage(x).partial_cmp(&charge_damage(y)).unwrap().then(d::type_id_at(y).cmp(&d::type_id_at(x)))).map(d::type_id_at)
    };
    let mut start = State_ { picks: Vec::new(), drones: None };
    for (i, m) in req.base.modules.iter().enumerate() {
        let Some(mix) = d::type_index(m.type_id) else { continue };
        let slot = m.slot.or_else(|| exfa_core::engine::infer_slot(mix));
        let Some(slot) = slot else { continue };
        if req.search.keep.contains(&i) || !slots.contains(&slot) || m.mutation.is_some() {
            continue;
        }
        let pool: Vec<usize> = match req.search.candidates.as_str() {
            "group" => (0..d::type_count()).filter(|&ix| d::ty(ix).group == d::ty(mix).group && filter.module_ok(ix, slot)).collect(),
            "all_fittable" => all_by_slot(slot),
            _ => {
                let root = d::type_variation_parent(mix).unwrap_or(m.type_id);
                family.get(&root).map(|v| v.iter().copied().filter(|&ix| filter.module_ok(ix, slot)).collect()).unwrap_or_default()
            }
        };
        let mut cands: Vec<Pick> = pool.iter().map(|&ix| Pick { type_id: d::type_id_at(ix), charge: if req.search.charges { default_charge(ix, m.charge_type_id) } else { None } }).collect();
        let cur = Pick { type_id: m.type_id, charge: m.charge_type_id };
        if !cands.contains(&cur) {
            cands.push(cur.clone());
        }
        cands.sort();
        ctx.positions.push(Pos { slot, base_index: Some(i), base_state: m.state, cands });
        start.picks.push(Some(cur));
    }
    // empty slots draw from everything fittable to that rack
    for &slot in &slots {
        let attr = match slot {
            Slot::High => "/resources/slots/high/total",
            Slot::Mid => "/resources/slots/mid/total",
            Slot::Low => "/resources/slots/low/total",
            Slot::Rig => "/resources/slots/rig/total",
            _ => continue,
        };
        let total = ptr(&base_v, attr).unwrap_or(0.0) as usize;
        let used = req
            .base
            .modules
            .iter()
            .filter(|m| m.slot.or_else(|| d::type_index(m.type_id).and_then(exfa_core::engine::infer_slot)) == Some(slot))
            .count();
        if total > used {
            let pool = all_by_slot(slot);
            let cands: Vec<Pick> = pool.iter().map(|&ix| Pick { type_id: d::type_id_at(ix), charge: if req.search.charges { default_charge(ix, None) } else { None } }).collect();
            for _ in used..total {
                ctx.positions.push(Pos { slot, base_index: None, base_state: None, cands: cands.clone() });
                start.picks.push(None);
            }
        }
    }

    // ---- skills the search cannot fix (fixed part of the fit evaluated alone)
    if !skills_ignore {
        let mut fixed = ctx.build(&State_ { picks: vec![None; ctx.positions.len()], drones: None });
        if req.search.drones {
            fixed.drones.clear();
        }
        let fv = ctx.ev.eval(&fixed);
        ctx.evaluated += 1;
        let mut names = Vec::new();
        for x in fv.get("violations").and_then(|x| x.as_array()).into_iter().flatten() {
            if x.get("code").and_then(|c| c.as_str()) == Some("MISSING_SKILL") {
                if let Some(i) = x.get("skill_type_id").and_then(|i| i.as_u64()) {
                    let l = x.get("level").and_then(|l| l.as_f64()).unwrap_or(5.0);
                    let e = ctx.unfixable.entry(i).or_insert(0.0);
                    *e = e.max(l);
                    names.push(x.get("message").and_then(|m| m.as_str()).unwrap_or("").to_string());
                }
            }
        }
        if !names.is_empty() {
            warnings.push(format!("the character lacks skills for the fixed part of the fit (not counted as infeasible): {}", names.join("; ")));
        }
    }
    let base_score = ctx.score(&base_v, base_price);

    // ---- probe pruning: keep the best `per_slot` candidates per position (one-module probe on the base fit)
    let per_slot = req.limits.per_slot.max(1);
    let orig: Vec<Vec<Pick>> = ctx.positions.iter().map(|x| x.cands.clone()).collect();
    for p in 0..ctx.positions.len() {
        if ctx.positions[p].cands.len() <= per_slot {
            continue;
        }
        // positions with the same rack and candidate list (empty slots of a rack, identical fitted modules)
        // are probed once and reuse the pruned list
        if let Some(q) = (0..p).find(|&q| ctx.positions[q].slot == ctx.positions[p].slot && orig[q] == ctx.positions[p].cands && start.picks[q] == start.picks[p]) {
            ctx.positions[p].cands = ctx.positions[q].cands.clone();
            continue;
        }
        let cands = ctx.positions[p].cands.clone();
        // two stages for long lists: one representative (highest meta level) per variation family,
        // then every member of the best families
        let fam = |c: &Pick| d::type_index(c.type_id).and_then(d::type_variation_parent).unwrap_or(c.type_id);
        let mut reps: BTreeMap<u32, Pick> = BTreeMap::new();
        for c in &cands {
            let ml = |x: &Pick| d::type_index(x.type_id).and_then(d::type_meta_level).unwrap_or(0);
            let e = reps.entry(fam(c)).or_insert_with(|| c.clone());
            if ml(c) > ml(e) {
                *e = c.clone();
            }
        }
        let mut scored = if cands.len() > 2 * per_slot && reps.len() * 2 <= cands.len() {
            let first = probe_list(&mut ctx, &start, p, reps.values().cloned().collect());
            let n = (per_slot / 8).max(3);
            let mut by_obj = first.clone();
            by_obj.sort_by(|x, y| y.0.obj.partial_cmp(&x.0.obj).unwrap_or(std::cmp::Ordering::Equal).then(x.1.cmp(&y.1)));
            let top: Vec<u32> = first.iter().take(n).chain(by_obj.iter().take(n)).map(|x| fam(&x.1)).collect();
            let more: Vec<Pick> = cands.iter().filter(|c| top.contains(&fam(c)) && !reps.values().any(|r| r == *c)).cloned().collect();
            let mut all = first;
            all.extend(probe_list(&mut ctx, &start, p, more));
            all
        } else {
            probe_list(&mut ctx, &start, p, cands)
        };
        sort_scored(&mut scored);
        // keep a mix: best by (violation, objective) and best by objective alone, so modules that
        // only help the objective survive while constraints (floors, fitting) are still unmet
        let mut by_obj = scored.clone();
        by_obj.sort_by(|x, y| y.0.obj.partial_cmp(&x.0.obj).unwrap_or(std::cmp::Ordering::Equal).then(x.1.cmp(&y.1)));
        let mut keep: Vec<Pick> = Vec::new();
        let (mut i, mut j) = (0, 0);
        while keep.len() < per_slot && (i < scored.len() || j < by_obj.len()) {
            for (list, k) in [(&scored, &mut i), (&by_obj, &mut j)] {
                while *k < list.len() && keep.contains(&list[*k].1) {
                    *k += 1;
                }
                if *k < list.len() && keep.len() < per_slot {
                    keep.push(list[*k].1.clone());
                    *k += 1;
                }
            }
        }
        if let Some(cur) = &start.picks[p] {
            if !keep.contains(cur) {
                keep.push(cur.clone());
            }
        }
        keep.sort();
        ctx.positions[p].cands = keep;
    }

    // ---- greedy beam construction: rig, high, low, mid
    let order: Vec<usize> = {
        let rank = |s: Slot| match s {
            Slot::Rig => 0,
            Slot::High => 1,
            Slot::Low => 2,
            Slot::Mid => 3,
            _ => 4,
        };
        let mut o: Vec<usize> = (0..ctx.positions.len()).collect();
        o.sort_by_key(|&p| (rank(ctx.positions[p].slot), p));
        o
    };
    let beam_w = req.limits.beam.max(1);
    let mut beam: Vec<(Score, State_)> = vec![(ctx.eval(&start).unwrap_or(base_score), start.clone())];
    'greedy: for &p in &order {
        let mut next: Vec<(Score, State_)> = beam.clone();
        for (_, st) in &beam {
            // candidates, plus "leave empty" (a fitted module the character cannot use may have to go)
            let mut cands: Vec<Option<Pick>> = ctx.positions[p].cands.iter().cloned().map(Some).collect();
            if st.picks[p].is_some() {
                cands.push(None);
            }
            for c in cands {
                let mut s = st.clone();
                s.picks[p] = c;
                match ctx.eval(&s) {
                    Some(sc) => next.push((sc, s)),
                    None => {
                        beam = next;
                        break 'greedy;
                    }
                }
            }
        }
        next.sort_by(|x, y| if x.0.better(&y.0) { std::cmp::Ordering::Less } else if y.0.better(&x.0) { std::cmp::Ordering::Greater } else { std::cmp::Ordering::Equal });
        next.dedup_by(|x, y| x.1 == y.1);
        next.truncate(beam_w);
        beam = next;
    }

    // ---- local search on the best beam entries
    let rot = (req.limits.seed as usize) % ctx.positions.len().max(1);
    let mut finals: Vec<(Score, State_)> = Vec::new();
    for (mut best_sc, mut best) in beam.clone() {
        loop {
            let mut improved = false;
            // single swaps (module + its default charge)
            for k in 0..ctx.positions.len() {
                let p = (k + rot) % ctx.positions.len();
                for c in ctx.positions[p].cands.clone() {
                    let mut s = best.clone();
                    s.picks[p] = Some(c);
                    let Some(sc) = ctx.eval(&s) else { break };
                    if sc.better(&best_sc) {
                        best_sc = sc;
                        best = s;
                        improved = true;
                    }
                }
                // empty slot is also an option (frees CPU / PG, drops an unusable module)
                if best.picks[p].is_some() {
                    let mut s = best.clone();
                    s.picks[p] = None;
                    if let Some(sc) = ctx.eval(&s) {
                        if sc.better(&best_sc) {
                            best_sc = sc;
                            best = s;
                            improved = true;
                        }
                    }
                }
            }
            // charges per weapon
            if req.search.charges {
                for p in 0..ctx.positions.len() {
                    let Some(pk) = best.picks[p].clone() else { continue };
                    let Some(mix) = d::type_index(pk.type_id) else { continue };
                    let mut cs = charges_of(mix);
                    cs.sort_by(|&x, &y| charge_damage(y).partial_cmp(&charge_damage(x)).unwrap().then(d::type_id_at(x).cmp(&d::type_id_at(y))));
                    cs.truncate(per_slot);
                    for c in cs {
                        let mut s = best.clone();
                        s.picks[p] = Some(Pick { type_id: pk.type_id, charge: Some(d::type_id_at(c)) });
                        let Some(sc) = ctx.eval(&s) else { break };
                        if sc.better(&best_sc) {
                            best_sc = sc;
                            best = s;
                            improved = true;
                        }
                    }
                }
            }
            // drones: one stack of the best usable drone type, as many as bandwidth / bay / max active allow
            if req.search.drones {
                let v = ctx.cache.get(&best).map(|x| x.1.clone()).unwrap_or(Value::Null);
                let bw = ptr(&v, "/resources/drone_bandwidth/total").unwrap_or(0.0);
                let bay = ptr(&v, "/resources/drone_bay/total").unwrap_or(0.0);
                let max_active = ptr(&v, "/drones/max_active").unwrap_or(5.0).max(0.0) as u32;
                if bw > 0.0 && bay > 0.0 && max_active > 0 {
                    let drones: Vec<usize> = (0..d::type_count()).filter(|&ix| d::ty(ix).category == 18 && filter.item_ok(ix)).collect();
                    for ix in drones {
                        let dbw = tattr(ix, a::droneBandwidthUsed).unwrap_or(0.0);
                        let vol = d::type_volume(ix);
                        if dbw <= 0.0 || vol <= 0.0 {
                            continue;
                        }
                        let n_bay = (bay / vol + 1e-9).floor() as u32;
                        let n_act = ((bw / dbw + 1e-9).floor() as u32).min(max_active).min(n_bay);
                        if n_act == 0 {
                            continue;
                        }
                        let mut s = best.clone();
                        s.drones = Some((d::type_id_at(ix), n_bay.min(n_act.max(1)), n_act));
                        let Some(sc) = ctx.eval(&s) else { break };
                        if sc.better(&best_sc) {
                            best_sc = sc;
                            best = s;
                            improved = true;
                        }
                    }
                }
            }
            if !improved && ctx.stopped.is_none() {
                // pair swaps over the top 5 candidates of each position (trades CPU / PG between modules)
                let n = ctx.positions.len();
                'pairs: for p in 0..n {
                    for q in (p + 1)..n {
                        let (cp, cq): (Vec<Pick>, Vec<Pick>) = (ctx.positions[p].cands.iter().take(5).cloned().collect(), ctx.positions[q].cands.iter().take(5).cloned().collect());
                        for x in &cp {
                            for y in &cq {
                                let mut s = best.clone();
                                s.picks[p] = Some(x.clone());
                                s.picks[q] = Some(y.clone());
                                let Some(sc) = ctx.eval(&s) else { break 'pairs };
                                if sc.better(&best_sc) {
                                    best_sc = sc;
                                    best = s;
                                    improved = true;
                                }
                            }
                        }
                    }
                }
            }
            if !improved || ctx.stopped.is_some() {
                break;
            }
        }
        finals.push((best_sc, best));
        if ctx.stopped.is_some() {
            break;
        }
    }

    // ---- results: distinct fits (by module multiset + drones), best first, from everything evaluated
    let mut all: Vec<(Score, State_)> = ctx.cache.iter().map(|(s, (sc, _))| (*sc, s.clone())).collect();
    all.extend(finals);
    all.sort_by(|x, y| {
        if x.0.better(&y.0) {
            std::cmp::Ordering::Less
        } else if y.0.better(&x.0) {
            std::cmp::Ordering::Greater
        } else {
            format!("{:?}", x.1).cmp(&format!("{:?}", y.1))
        }
    });
    let mut seen: Vec<(Vec<Option<Pick>>, Option<(u32, u32, u32)>)> = Vec::new();
    let mut results = Vec::new();
    let base_metrics = metrics_of(&ctx, &base_v, base_price);
    let any_feasible = all.first().map(|x| x.0.feasible).unwrap_or(false);
    for (sc, st) in all {
        if results.len() >= req.limits.results.max(1) {
            break;
        }
        if !sc.feasible && any_feasible {
            break;
        }
        let mut key = st.picks.clone();
        key.sort();
        if seen.iter().any(|x| x.0 == key && x.1 == st.drones) {
            continue;
        }
        seen.push((key, st.drones));
        let (fit, at) = ctx.build_mapped(&st);
        let v = ctx.cache.get(&st).map(|x| x.1.clone()).unwrap_or_else(|| ctx.ev.eval(&fit));
        let price = ctx.price_of(&fit).ok().flatten();
        let m = metrics_of(&ctx, &v, price);
        let delta = if base_score.feasible {
            let mut dl = serde_json::Map::new();
            for (k, x) in m.as_object().unwrap() {
                if let (Some(a1), Some(b1)) = (x.as_f64(), base_metrics.get(k).and_then(|b| b.as_f64())) {
                    dl.insert(k.clone(), json!(a1 - b1));
                }
            }
            Value::Object(dl)
        } else {
            Value::Null
        };
        let mut changes = Vec::new();
        for (p, pos) in ctx.positions.iter().enumerate() {
            let from = pos.base_index.map(|i| req.base.modules[i].type_id);
            let to = st.picks[p].as_ref().map(|x| x.type_id);
            let fc = pos.base_index.and_then(|i| req.base.modules[i].charge_type_id);
            let tc = st.picks[p].as_ref().and_then(|x| x.charge);
            if from != to || (to.is_some() && fc != tc) {
                let index = at[p].or(pos.base_index);
                changes.push(json!({"slot": slot_name(pos.slot), "index": index, "from": from, "to": to, "charge_from": fc, "charge_to": tc}));
            }
        }
        let missing: Vec<Value> = v
            .get("violations")
            .and_then(|x| x.as_array())
            .map(|a| a.iter().filter(|x| x.get("code").and_then(|c| c.as_str()) == Some("MISSING_SKILL")).filter_map(|x| x.get("skill_type_id").cloned()).collect())
            .unwrap_or_default();
        let mut row = json!({"rank": results.len() + 1, "fit": serde_json::to_value(&fit).unwrap(), "objective": sc.obj,
                             "feasible": sc.feasible, "metrics": m, "delta": delta, "changes": changes, "missing_skills": missing});
        if !sc.feasible {
            row["violation"] = json!(sc.violation);
            row["violations"] = v.get("violations").cloned().unwrap_or(Value::Null);
        }
        results.push(row);
    }
    let stopped = ctx.stopped.unwrap_or("converged");
    if ctx.positions.is_empty() && !req.search.drones {
        warnings.push("nothing to optimize: no changeable module position (check search.slots / keep)".into());
    }
    let mut out = json!({"results": results, "base_metrics": base_metrics, "base_feasible": base_score.feasible,
                         "evaluated": ctx.evaluated, "stopped_by": stopped, "warnings": warnings});
    if !any_feasible {
        out["error"] = json!({"code": "OPT_NO_FEASIBLE", "message": "no evaluated fit meets the constraints; results hold the closest ones with their violations"});
    }
    Ok(out)
}

fn metrics_of<E: Evaluator>(ctx: &Ctx<E>, v: &Value, price: Option<f64>) -> Value {
    let mut m = serde_json::Map::new();
    for k in ["dps", "volley", "ehp", "tank", "max_velocity", "align_time"] {
        if let Some(x) = metric(v, k, price) {
            m.insert(k.into(), json!(x));
        }
    }
    m.insert("cap_stable".into(), v.pointer("/capacitor/stable").cloned().unwrap_or(Value::Null));
    if let Some(p) = price {
        m.insert("price_isk".into(), json!(p));
    }
    for (k, ..) in &ctx.terms {
        if let Some(x) = metric(v, k, price) {
            m.insert(k.clone(), json!(x));
        }
    }
    for k in ctx.req.constraints.min.keys().chain(ctx.req.constraints.max.keys()) {
        if let Some(x) = metric(v, k, price) {
            m.insert(k.clone(), json!(x));
        }
    }
    Value::Object(m)
}
