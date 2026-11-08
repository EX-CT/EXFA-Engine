//! `projected[kind=fit].select` (item-id whitelist) and `options.full_precision` through `calc` / the `calc` RPC.
use serde_json::{json, Value};

fn calc(fit: Value) -> Value {
    let r: exfa_core::FitRequest = serde_json::from_value(fit).unwrap();
    if r.options.full_precision {
        exfa_core::calc(&r).to_value_raw()
    } else {
        serde_json::to_value(exfa_core::calc(&r)).unwrap()
    }
}
fn velocity(r: &Value) -> f64 {
    r["navigation"]["max_velocity"].as_f64().unwrap()
}

/// Source fit: two active Stasis Webifier I (526), stable ids m1/m2.
fn web_source() -> Value {
    json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}},
           "modules": [{"id": "m1", "type_id": 526, "state": "active"}, {"id": "m2", "type_id": 526, "state": "active"}]})
}
fn projected(select: Value) -> Value {
    let mut e = json!({"kind": "fit", "fit": web_source()});
    if !select.is_null() {
        e["select"] = select;
    }
    calc(json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}, "projected": [e]}))
}

#[test]
fn projected_fit_select_whitelist() {
    let v_none = velocity(&projected(Value::Null));
    let v_m1 = velocity(&projected(json!({"module_ids": ["m1"]})));
    let v_both = velocity(&projected(json!({"module_ids": ["m1", "m2"]})));
    let v_empty = velocity(&projected(json!({"module_ids": []})));
    assert!(v_none < v_m1 && v_m1 < v_empty, "one web slows less than two, none slows not at all: {v_none} {v_m1} {v_empty}");
    assert_eq!(v_both, v_none, "selecting both == no select");
    // select m1 == a source that only carries m1
    let only_m1 = calc(json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}},
        "projected": [{"kind": "fit", "fit": {"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}},
            "modules": [{"id": "m1", "type_id": 526, "state": "active"}]}}]}));
    assert_eq!(v_m1, velocity(&only_m1), "select m1 == source with only m1");
    // unselected kinds contribute nothing: drone list present-but-empty + no module list -> no modules
    let v_drones_only = velocity(&projected(json!({"drone_ids": []})));
    assert_eq!(v_drones_only, v_empty, "absent module_ids contributes no modules");
    // unmatched id -> one deduplicated warning, the matched id still projects
    let miss = projected(json!({"module_ids": ["m1", "nope"]}));
    assert_eq!(velocity(&miss), v_m1);
    let w = miss["warnings"].as_array().unwrap();
    assert_eq!(w.iter().filter(|x| **x == "projected fit select: no module item with id 'nope'").count(), 1);
    // select on kind != fit is ignored (warning, no error)
    let other = calc(json!({"ship": {"type_id": 587}, "projected": [
        {"kind": "module", "module": {"type_id": 526, "state": "active"}, "select": {"module_ids": ["x"]}}]}));
    assert!(other["error"].is_null());
    assert!(other["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("select")));
}

#[test]
fn rpc_calc_and_full_precision() {
    let r = exfa_core::rpc(&json!({"id": 7, "method": "calc", "params": {"ship": {"type_id": 587}, "modules": [{"type_id": 439}]}}).to_string());
    assert_eq!(r["id"], 7);
    assert!(r["result"]["navigation"]["max_velocity"].is_number());
    // options.full_precision takes the raw path: unrounded floats
    let v = calc(json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}, "options": {"full_precision": true}}))["capacitor"]["peak_recharge_gj_s"].as_f64().unwrap();
    let rounded = calc(json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}}))["capacitor"]["peak_recharge_gj_s"].as_f64().unwrap();
    assert_eq!(rounded, (v * 1e6).round() / 1e6, "raw path keeps >6 decimals: {v} vs {rounded}");
    assert!(v != rounded, "full_precision bypasses the 6-decimal rounding");
}
