//! Unified compute envelope `exfa/compute@1` -> `exfa/compute-result@1` (docs/27 §5.3): ONE input object,
//! ONE output object. `operation=calc` takes a `FitSpec` and returns FitStats; `operation=batch` takes a
//! `BatchSpec` (the existing BatchRequest) and returns the BatchResponse. The workspace/group semantics and
//! host-side references stay outside; the engine only sees fully resolved fits.
//!
//! FitSpec = FitRequest + stable equipment `id`s (modules/drones/fighters, used by
//! `projected[kind=fit].select`) + shorthand normalization, applied by this entry point only: omitted
//! `character.skills.default_level` = 5, omitted module `state` = `active` when the type can activate else
//! `online`, omitted drone `active` = `quantity` (fighters already default active). Explicit values win
//! (`default_level: 0` stays 0). Normalization recurses into every nested FitRequest (`projected[kind=fit].fit`,
//! `fleet.booster_fits[]`, `scenarios[].target.fit`); for `operation=calc` the top-level defaults are reported
//! as `DEFAULTED` adjustments, for `operation=batch` the same rules apply silently before expansion.
use crate::data::{self as d, a};
use crate::j::J;
use crate::jv;
use crate::request::{FitRequest, State};
use serde_json::{json, Value};
use std::ops::IndexMut;

pub const FORMAT: &str = "exfa/compute@1";
pub const RESULT_FORMAT: &str = "exfa/compute-result@1";

fn err_env(op: &str, code: &str, message: impl Into<String>, path: &str) -> Value {
    json!({"format": RESULT_FORMAT, "operation": op, "error": {"code": code, "message": message.into(), "path": path}})
}

/// Best-effort `operation` for the error envelope of an unparseable request.
fn recover_op(s: &str) -> Option<String> {
    let k = s.find("\"operation\"")? + "\"operation\"".len();
    let rest = s[k..].trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    rest.find('"').map(|e| rest[..e].to_string())
}

/// The engine-side `state` shorthand default (Pyfa isValidState, same rule as `type` RPC `allowed_states`):
/// `active` when the type has an activatable effect (category 1|2) and `activationBlocked` <= 0, else `online`.
/// Unknown types stay `None` so the existing error path reports them.
fn state_default(type_id: u32) -> Option<State> {
    let ty = d::type_index(type_id)?;
    let can_active = d::type_effects(ty).iter().any(|&x| matches!(d::EFF_META[(x >> 1) as usize].cat, 1 | 2))
        && d::type_attr(ty, a::activationBlocked).unwrap_or(0.0) <= 0.0;
    Some(if can_active { State::Active } else { State::Online })
}

/// Defaults applied to the top-level fit (calc reports them as `DEFAULTED` adjustments).
#[derive(Default)]
struct Notes {
    default_level: bool,
    module_states: Vec<State>,
    drone_actives: Vec<u32>,
}

/// Normalize a FitSpec in place, recursing into every nested FitRequest. Notes are recorded for the
/// top-level fit only (the existing convention: `adjustments` reports the top-level request).
fn normalize(req: &mut FitRequest, top: bool, notes: &mut Notes) {
    if req.character.skills.default_level.is_none() {
        req.character.skills.default_level = Some(5);
        if top {
            notes.default_level = true;
        }
    }
    for m in &mut req.modules {
        if m.state.is_none() {
            if let Some(st) = state_default(m.type_id) {
                m.state = Some(st);
                if top {
                    notes.module_states.push(st);
                }
            }
        }
    }
    for dr in &mut req.drones {
        if dr.active.is_none() {
            dr.active = Some(dr.quantity);
            if top {
                notes.drone_actives.push(dr.quantity);
            }
        }
    }
    for bf in &mut req.fleet.booster_fits {
        normalize(bf, false, notes);
    }
    for p in &mut req.projected {
        if p.kind == "fit" {
            if let Some(f) = p.fit.as_deref_mut() {
                normalize(f, false, notes);
            }
        }
    }
    if let Some(Value::Array(list)) = req.scenarios.as_mut() {
        for s in list.iter_mut() {
            if let Some(v) = s.get_mut("target").and_then(|t| t.get_mut("fit")).filter(|v| v.is_object()) {
                normalize_fit_json(v);
            }
        }
    }
}

/// Normalize a FitSpec held as JSON (batch base / fits[].fit / scenario target.fit); on a shape error the
/// value is left as-is so the downstream path reports it.
fn normalize_fit_json(v: &mut Value) {
    if let Ok(mut req) = serde_json::from_value::<FitRequest>(v.clone()) {
        normalize(&mut req, false, &mut Notes::default());
        if let Ok(nv) = serde_json::to_value(&req) {
            *v = nv;
        }
    }
}

/// One `DEFAULTED` adjustment per defaulted category (wildcard paths cover every defaulted item; `to` lists
/// the applied values in request order).
fn defaulted_adjustments(n: &Notes) -> Vec<J> {
    let mut v = Vec::new();
    if n.default_level {
        v.push(jv!({"code": "DEFAULTED", "path": "/character/skills/default_level", "from": J::Null, "to": 5u8, "message": "compute shorthand default applied"}));
    }
    if !n.module_states.is_empty() {
        v.push(jv!({"code": "DEFAULTED", "path": "/modules/*/state", "from": J::Null, "to": n.module_states.clone(), "message": "compute shorthand default applied"}));
    }
    if !n.drone_actives.is_empty() {
        v.push(jv!({"code": "DEFAULTED", "path": "/drones/*/active", "from": J::Null, "to": n.drone_actives.clone(), "message": "compute shorthand default applied"}));
    }
    v
}

/// Wrap an inner `{"error": {...}}` output (calc or batch) into the error envelope, keeping its code/message/path.
fn inner_error(op: &str, e: &Value) -> Value {
    let mut e = e.clone();
    if let Some(m) = e.as_object_mut() {
        m.entry("path").or_insert(json!(""));
    }
    json!({"format": RESULT_FORMAT, "operation": op, "error": e})
}

fn calc_op(input: &Value) -> Value {
    let fit_v = match input.get("fit") {
        Some(v) if v.is_object() => v.clone(),
        _ => return err_env("calc", "BAD_REQUEST", "fit must be a FitRequest object", "/fit"),
    };
    let mut req: FitRequest = match serde_json::from_value(fit_v) {
        Ok(r) => r,
        Err(e) => return err_env("calc", "BAD_REQUEST", e.to_string(), "/fit"),
    };
    let mut notes = Notes::default();
    normalize(&mut req, true, &mut notes);
    let mut out = crate::calc(&req);
    if let J::O(o) = &out {
        if let Some((_, e)) = o.iter().find(|(k, _)| k.as_ref() == "error") {
            return inner_error("calc", &e.to_value_raw());
        }
    }
    if let J::O(_) = &out {
        let adj = out.index_mut("adjustments");
        if !matches!(adj, J::A(_)) {
            *adj = J::A(Vec::new());
        }
        if let J::A(a) = adj {
            a.extend(defaulted_adjustments(&notes));
        }
    }
    let result = if req.options.full_precision { out.to_value_raw() } else { serde_json::to_value(&out).unwrap_or(Value::Null) };
    json!({"format": RESULT_FORMAT, "operation": "calc", "result": result})
}

fn batch_op(input: &Value) -> Value {
    let mut b = match input.get("batch") {
        Some(v) if v.is_object() => v.clone(),
        _ => return err_env("batch", "BAD_REQUEST", "batch must be a BatchRequest object", "/batch"),
    };
    // FitSpec shorthand applies to the base and every explicit fit BEFORE expansion, so patches act on a
    // fully normalized base and there is no shorthand-vs-normalized ambiguity downstream.
    if let Some(v) = b.get_mut("base").filter(|v| v.is_object()) {
        normalize_fit_json(v);
    }
    if let Some(fits) = b.get_mut("fits").and_then(Value::as_array_mut) {
        for it in fits.iter_mut() {
            if let Some(v) = it.get_mut("fit").filter(|v| v.is_object()) {
                normalize_fit_json(v);
            }
        }
    }
    let r = crate::batch::run(&b);
    match r.get("error") {
        Some(e) => inner_error("batch", e),
        None => json!({"format": RESULT_FORMAT, "operation": "batch", "result": r}),
    }
}

/// `compute` params (`exfa/compute@1`) -> `exfa/compute-result@1`.
pub fn compute(input: &Value) -> Value {
    if input.get("format").and_then(Value::as_str) != Some(FORMAT) {
        let op = input.get("operation").and_then(Value::as_str).unwrap_or("calc");
        return err_env(op, "UNSUPPORTED_FORMAT", "format must be \"exfa/compute@1\"", "/format");
    }
    let op = input.get("operation").and_then(Value::as_str).unwrap_or("");
    match op {
        "calc" => calc_op(input),
        "batch" => batch_op(input),
        _ => err_env(if op.is_empty() { "calc" } else { op }, "BAD_REQUEST", "operation must be \"calc\" or \"batch\"", "/operation"),
    }
}

/// JSON string in, JSON string out; never panics and always answers with the result envelope.
pub fn compute_json(input: &str) -> String {
    let v = match serde_json::from_str::<Value>(input) {
        Ok(v) => compute(&v),
        Err(e) => {
            let op = recover_op(input).unwrap_or_else(|| "calc".into());
            err_env(&op, "BAD_JSON", e.to_string(), "")
        }
    };
    serde_json::to_string(&v).unwrap_or_default()
}
