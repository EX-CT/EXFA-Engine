//! Graphs (round 2, approach G4: declarative graph specs). `graphs.json` (compiled in) is the catalogue: per graph
//! its x axes with validity limiters, y series with one formula per axis, params with defaults, and named defs.
//! Formulas are `expr` trees over *observables* (`ship.<attr>`, `stat.<path>`, `p.<param>`, `s.<setting>`, `x`)
//! and *kernels* (hand-written: capacitor simulation history, subwarp speed, damage/EWAR/RR application, ...).
//! Behaviour follows Pyfa's graph getters as described in CONTRACT-GRAPHS.md; no Pyfa code is used.
pub mod expr;
pub mod app;
pub mod cycles;
pub mod dmg;
pub mod kernels;
pub mod rr;

use crate::data as d;
use crate::engine::Fit;
use crate::request::FitRequest;
use expr::{Env, Expr};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

pub const SPEC_JSON: &str = include_str!("../../graphs.json");

pub struct Axis {
    pub valid: Option<Expr>,
}
pub struct Series {
    pub by_axis: HashMap<String, Expr>,
}
pub struct Graph {
    pub axes: HashMap<String, Axis>,
    pub series: HashMap<String, Series>,
    pub params: Map<String, Value>,
    pub defs: HashMap<String, Expr>,
}
pub struct Source {
    pub from: String,
    pub effects: Vec<String>,
    pub strength: Expr,
    pub when: Option<Expr>,
    pub range: String,
    pub lock: bool,
    pub dcr: bool,
}
pub struct Spec {
    pub graphs: HashMap<String, Graph>,
    pub tables: HashMap<String, Vec<Source>>,
    pub constants: HashMap<String, f64>,
}

fn load() -> Result<Spec, String> {
    let v: Value = serde_json::from_str(SPEC_JSON).map_err(|e| e.to_string())?;
    let mut graphs = HashMap::new();
    let pe = |s: &Value| -> Result<Expr, String> { expr::parse(s.as_str().unwrap_or("null()")) };
    for (name, g) in v["graphs"].as_object().ok_or("no graphs")? {
        let mut axes = HashMap::new();
        for (a, av) in g["axes"].as_object().ok_or("no axes")? {
            axes.insert(a.clone(), Axis { valid: av.get("valid").map(pe).transpose()? });
        }
        let mut series = HashMap::new();
        for (y, yv) in g["series"].as_object().ok_or("no series")? {
            let mut by_axis = HashMap::new();
            for (a, f) in yv["by_axis"].as_object().ok_or("no by_axis")? {
                by_axis.insert(a.clone(), pe(f).map_err(|e| format!("{name}.{y}.{a}: {e}"))?);
            }
            series.insert(y.clone(), Series { by_axis });
        }
        let mut defs = HashMap::new();
        if let Some(o) = g.get("defs").and_then(|d| d.as_object()) {
            for (k, f) in o {
                defs.insert(k.clone(), pe(f).map_err(|e| format!("{name}.def {k}: {e}"))?);
            }
        }
        let params = g.get("params").and_then(|p| p.as_object()).cloned().unwrap_or_default();
        graphs.insert(name.clone(), Graph { axes, series, params, defs });
    }
    let mut tables = HashMap::new();
    if let Some(o) = v.get("source_tables").and_then(|t| t.as_object()) {
        for (k, list) in o {
            let mut out = Vec::new();
            for e in list.as_array().cloned().unwrap_or_default() {
                let strs = |key: &str| -> Vec<String> { e.get(key).and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or_default() };
                let mut effects = strs("effects");
                effects.extend(strs("abilities"));
                out.push(Source {
                    from: e["from"].as_str().unwrap_or("module").to_string(),
                    effects,
                    strength: pe(&e["strength"])?,
                    when: e.get("when").map(pe).transpose()?,
                    range: e["range"].as_str().unwrap_or("module").to_string(),
                    lock: e.get("lock").and_then(|x| x.as_f64()).unwrap_or(0.0) != 0.0,
                    dcr: e.get("dcr").and_then(|x| x.as_f64()).unwrap_or(0.0) != 0.0,
                });
            }
            tables.insert(k.clone(), out);
        }
    }
    let constants = v["constants"].as_object().map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0))).collect()).unwrap_or_default();
    Ok(Spec { graphs, tables, constants })
}

pub fn spec() -> &'static Spec {
    static S: OnceLock<Spec> = OnceLock::new();
    S.get_or_init(|| load().expect("graphs.json"))
}

/// The catalogue as JSON (RPC `graph_specs`).
pub fn specs_json() -> Value {
    serde_json::from_str(SPEC_JSON).unwrap_or(Value::Null)
}

fn err(code: &str, msg: impl Into<String>, path: &str) -> Value {
    json!({"error": {"code": code, "message": msg.into(), "path": path}})
}

/// Per-request evaluation context.
pub struct Ctx<'a> {
    pub graph: &'a Graph,
    pub req: &'a Value,
    pub fit_req: FitRequest,
    pub fit: Fit,
    pub stats: Value,
    pub axis: String,
    pub x: f64,
    pub params: Map<String, Value>,
    pub settings: Map<String, Value>,
    defs_memo: HashMap<String, Option<f64>>,
    pub k: kernels::Caches,
}

impl<'a> Ctx<'a> {
    pub fn param(&self, k: &str) -> Option<&Value> {
        self.params.get(k).filter(|v| !v.is_null())
    }
    pub fn param_f(&self, k: &str) -> Option<f64> {
        self.param(k).and_then(|v| v.as_f64().or_else(|| v.as_bool().map(|b| b as u8 as f64)))
    }
    pub fn setting_b(&self, k: &str, default: bool) -> bool {
        self.settings.get(k).and_then(|v| v.as_bool()).unwrap_or(default)
    }
    pub fn setting_s(&self, k: &str, default: &'static str) -> String {
        self.settings.get(k).and_then(|v| v.as_str()).unwrap_or(default).to_string()
    }
    pub fn ship_attr(&self, name: &str) -> f64 {
        match d::attr_by_name(name) {
            Some(a) => self.fit.get(self.fit.ship, a),
            None => 0.0,
        }
    }
    pub fn stat(&self, path: &str) -> Option<f64> {
        let mut v = &self.stats;
        for p in path.split('.') {
            v = v.get(p)?;
        }
        v.as_f64()
    }
}

impl Env for Ctx<'_> {
    fn var(&mut self, name: &str) -> Result<Option<f64>, String> {
        if name == "x" {
            return Ok(Some(self.x));
        }
        if let Some(c) = spec().constants.get(name) {
            return Ok(Some(*c));
        }
        if let Some(p) = name.strip_prefix("p.") {
            return Ok(self.param_f(p));
        }
        if let Some(p) = name.strip_prefix("s.") {
            return Ok(self.settings.get(p).and_then(|v| v.as_f64().or_else(|| v.as_bool().map(|b| b as u8 as f64))));
        }
        if let Some(a) = name.strip_prefix("ship.") {
            return Ok(Some(self.ship_attr(a)));
        }
        if let Some(p) = name.strip_prefix("stat.") {
            return Ok(self.stat(p));
        }
        if let Some(v) = self.defs_memo.get(name) {
            return Ok(*v);
        }
        let g = self.graph;
        if let Some(e) = g.defs.get(name) {
            let v = expr::eval(e, self)?;
            self.defs_memo.insert(name.to_string(), v);
            return Ok(v);
        }
        Err(format!("unknown name '{name}'"))
    }
    fn call(&mut self, name: &str, args: &[Option<f64>]) -> Result<Option<f64>, String> {
        kernels::call(self, name, args)
    }
}

fn num(v: Option<f64>) -> Value {
    match v {
        Some(f) if f.is_finite() => serde_json::Number::from_f64(f).map(Value::Number).unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// GraphRequest -> GraphResult (CONTRACT-GRAPHS.md).
pub fn graph(req: &Value) -> Value {
    // ---- structural validation (contract 0.2: BAD_REQUEST before anything else) ----
    let Some(gname) = req.get("graph").and_then(|g| g.as_str()) else { return err("BAD_REQUEST", "graph missing or not a string", "graph") };
    if !req.get("fit").map_or(false, |f| f.is_object()) {
        return err("BAD_REQUEST", "fit missing or not an object", "fit");
    }
    let Some(xv) = req.pointer("/x/values") else { return err("BAD_REQUEST", "missing x.values", "x.values") };
    let Some(xa) = xv.as_array() else { return err("BAD_REQUEST", "x.values must be an array", "x.values") };
    let mut xs: Vec<f64> = Vec::with_capacity(xa.len());
    for (i, v) in xa.iter().enumerate() {
        match v.as_f64().filter(|f| f.is_finite()) {
            Some(f) => xs.push(f),
            None => return err("BAD_REQUEST", "x values must be finite numbers", &format!("x.values[{i}]")),
        }
    }
    let Some(ya) = req.get("y").and_then(|y| y.as_array()) else { return err("BAD_REQUEST", "y missing or not an array", "y") };
    if ya.is_empty() {
        return err("BAD_REQUEST", "y must not be empty", "y");
    }
    let mut ys: Vec<String> = Vec::new();
    for (i, v) in ya.iter().enumerate() {
        match v.as_str() {
            Some(s) => ys.push(s.to_string()),
            None => return err("BAD_REQUEST", "y entries must be strings", &format!("y[{i}]")),
        }
    }
    let Some(graph) = spec().graphs.get(gname) else { return err("UNKNOWN_GRAPH", format!("unknown graph '{gname}'"), "graph") };
    let axis = req.pointer("/x/axis").and_then(|a| a.as_str()).unwrap_or("").to_string();
    let Some(ax) = graph.axes.get(&axis) else { return err("BAD_AXIS", format!("x axis '{axis}' not valid for {gname}"), "x.axis") };
    for (i, y) in ys.iter().enumerate() {
        if graph.series.get(y).and_then(|s| s.by_axis.get(&axis)).is_none() {
            return err("BAD_AXIS", format!("series '{y}' not valid for {gname} / {axis}"), &format!("y[{i}]"));
        }
    }
    for (ptr, path, allowed) in [
        ("/target/resist_mode", "target.resist_mode", &["auto", "shield", "armor", "hull", "weighted_average"][..]),
        ("/settings/mobile_drone_mode", "settings.mobile_drone_mode", &["auto", "follow_attacker", "follow_target"][..]),
        ("/params/ammo_quality", "params.ammo_quality", &["t1", "navy", "all"][..]),
    ] {
        if let Some(v) = req.pointer(ptr).filter(|v| !v.is_null()) {
            if !v.as_str().map_or(false, |s| allowed.contains(&s)) {
                return err("BAD_REQUEST", format!("unrecognised value for {path}"), path);
            }
        }
    }
    let forms: Vec<&Expr> = ys.iter().map(|y| graph.series.get(y).and_then(|s| s.by_axis.get(&axis)).unwrap()).collect();
    let fit_req: FitRequest = match serde_json::from_value(req.get("fit").cloned().unwrap_or(Value::Null)) {
        Ok(r) => r,
        Err(e) => return err("BAD_REQUEST", e.to_string(), "fit"),
    };
    let fit = match Fit::build(&fit_req) {
        Ok(f) => f,
        Err(e) => return json!({"error": {"code": e.code, "message": e.message, "path": e.path}}),
    };
    // contract 0.2 (0397d95): target.fit only validated for graphs that use a target
    if let Some(tf) = req.pointer("/target/fit").filter(|v| !v.is_null() && matches!(gname, "damage" | "application_profile" | "ewar" | "remote_reps")) {
        let treq: FitRequest = match serde_json::from_value(tf.clone()) {
            Ok(r) => r,
            Err(e) => return err("BAD_REQUEST", e.to_string(), "target.fit"),
        };
        if let Err(e) = Fit::build(&treq) {
            return json!({"error": {"code": e.code, "message": e.message, "path": format!("target.fit{}", e.path)}});
        }
    }
    let stats = fit.compute_stats(&fit_req).to_value_raw();
    let drains = crate::stats::LAST_DRAINS.with(|c| c.borrow().clone());
    let mut params = graph.params.clone();
    if let Some(o) = req.get("params").and_then(|p| p.as_object()) {
        for (k, v) in o {
            params.insert(k.clone(), v.clone());
        }
    }
    // clamp numeric params where the contract says so
    for (k, lo, hi) in [("time_s", 0.0, 2500.0), ("cap_start_pct", 0.0, 100.0), ("shield_start_pct", 0.0, 100.0), ("resist", 0.0, 1.0)] {
        if let Some(f) = params.get(k).and_then(|v| v.as_f64()) {
            let hi = if k == "time_s" && !matches!(gname, "damage" | "application_profile" | "remote_reps") { f64::INFINITY } else { hi };
            params.insert(k.to_string(), json!(f.clamp(lo, hi)));
        }
    }
    let settings = req.get("settings").and_then(|s| s.as_object()).cloned().unwrap_or_default();
    let mut ctx = Ctx {
        graph,
        req,
        fit_req,
        fit,
        stats,
        axis: axis.clone(),
        x: 0.0,
        params,
        settings,
        defs_memo: HashMap::new(),
        k: kernels::Caches { drains, ..Default::default() },
    };
    let mut out: Vec<Vec<Value>> = vec![Vec::with_capacity(xs.len()); ys.len()];
    for &x in &xs {
        ctx.x = x;
        ctx.defs_memo.clear();
        ctx.k.point_clear();
        let ok = match &ax.valid {
            Some(v) => matches!(expr::eval(v, &mut ctx), Ok(Some(b)) if b != 0.0),
            None => true,
        };
        for (i, f) in forms.iter().enumerate() {
            let v = if ok {
                match expr::eval(f, &mut ctx) {
                    Ok(v) => v,
                    Err(e) => return err("GRAPH_EVAL", e, "y"),
                }
            } else {
                None
            };
            out[i].push(num(v));
        }
    }
    let mut series = Map::new();
    for (y, v) in ys.iter().zip(out) {
        series.insert(y.clone(), Value::Array(v));
    }
    for (k, v) in std::mem::take(&mut ctx.k.extra_series) {
        series.insert(k, Value::Array(v));
    }
    if xs.is_empty() && gname == "application_profile" {
        for y in &ys {
            series.entry(format!("{y}_charge_type_id")).or_insert(Value::Array(vec![]));
        }
    }
    json!({"graph": gname, "x_axis": axis, "x": req.pointer("/x/values").cloned().unwrap_or(Value::Null), "series": series})
}

pub fn graph_json(line: &str) -> String {
    match serde_json::from_str::<Value>(line) {
        Ok(v) => graph(&v).to_string(),
        Err(e) => err("BAD_JSON", e.to_string(), "").to_string(),
    }
}
