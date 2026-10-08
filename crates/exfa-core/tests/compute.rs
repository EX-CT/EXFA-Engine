//! exfa/compute@1 -> exfa/compute-result@1 (docs/27 §5.3): envelope, FitSpec shorthand normalization,
//! batch normalization-first, projected-fit `select`, and error envelopes.
use serde_json::{json, Value};

fn compute(req: Value) -> Value {
    serde_json::from_str(&exfa_core::compute_json(&req.to_string())).unwrap()
}
fn calc_result(fit: Value) -> Value {
    compute(json!({"format": "exfa/compute@1", "operation": "calc", "fit": fit}))
}
fn calc_raw(fit: &Value) -> Value {
    let r: exfa_core::FitRequest = serde_json::from_value(fit.clone()).unwrap();
    serde_json::to_value(exfa_core::calc(&r)).unwrap()
}
/// Stats equality ignoring `adjustments` (compute calc may append DEFAULTED entries).
fn stats_eq(a: &Value, b: &Value) -> bool {
    let (mut a, mut b) = (a.clone(), b.clone());
    for v in [&mut a, &mut b] {
        v.as_object_mut().unwrap().remove("adjustments");
    }
    a == b
}
fn velocity(r: &Value) -> f64 {
    r["result"]["navigation"]["max_velocity"].as_f64().unwrap()
}

#[test]
fn shorthand_equals_explicit_and_defaulted_adjustments() {
    // 439 = 1MN Afterburner I (activatable): omitted state -> "active", omitted skills -> all 5.
    let short = calc_result(json!({"ship": {"type_id": 587}, "modules": [{"type_id": 439}]}));
    let explicit = calc_result(
        json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}, "modules": [{"type_id": 439, "state": "active"}]}),
    );
    assert!(stats_eq(&short["result"], &explicit["result"]), "shorthand == explicit {}", short);
    let adj = short["result"]["adjustments"].as_array().unwrap();
    assert!(adj.iter().any(|a| a["code"] == "DEFAULTED" && a["path"] == "/character/skills/default_level" && a["to"] == 5));
    assert!(adj.iter().any(|a| a["code"] == "DEFAULTED" && a["path"] == "/modules/*/state" && a["to"] == json!(["active"])));
    assert_eq!(explicit["result"]["adjustments"].as_array().unwrap().len(), 0);
    // a module that cannot activate (519 = Gyrostabilizer II) defaults to "online", not "active"
    let passive = calc_result(json!({"ship": {"type_id": 587}, "modules": [{"type_id": 519}]}));
    assert_eq!(passive["result"]["modules"][0]["state"], "online");
    assert_eq!(velocity(&short), velocity(&explicit));
}

#[test]
fn explicit_values_always_win() {
    let v_short = velocity(&calc_result(json!({"ship": {"type_id": 587}, "modules": [{"type_id": 439}]})));
    // explicit all-0 / online is preserved (not upgraded to 5 / active)
    let zero = calc_result(
        json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 0}}, "modules": [{"type_id": 439, "state": "online"}]}),
    );
    assert_eq!(zero["result"]["adjustments"].as_array().unwrap().len(), 0);
    assert_eq!(zero["result"]["modules"][0]["state"], "online");
    assert!(velocity(&zero) < v_short, "online+0 skills is slower than active+5");
    // and equals the raw calc of the same explicit request
    assert!(stats_eq(
        &zero["result"],
        &calc_raw(&json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 0}}, "modules": [{"type_id": 439, "state": "online"}]}))
    ));
}

#[test]
fn drone_active_defaults_to_quantity() {
    // 2185 = Hammerhead II
    let short = calc_result(json!({"ship": {"type_id": 587}, "drones": [{"type_id": 2185, "quantity": 5}]}));
    let explicit = calc_result(json!({"ship": {"type_id": 587}, "drones": [{"type_id": 2185, "quantity": 5, "active": 5}]}));
    assert_eq!(short["result"]["drones"]["active"], 5);
    assert!(stats_eq(&short["result"], &explicit["result"]));
    assert!(short["result"]["adjustments"].as_array().unwrap().iter().any(|a| a["path"] == "/drones/*/active"));
    let parked = calc_result(json!({"ship": {"type_id": 587}, "drones": [{"type_id": 2185, "quantity": 5, "active": 0}]}));
    assert_eq!(parked["result"]["drones"]["active"], 0, "explicit active:0 preserved");
}

#[test]
fn batch_fits_normalize_and_echo_ids() {
    let fit_a = json!({"ship": {"type_id": 587}, "modules": [{"type_id": 439}]});
    let fit_b = json!({"ship": {"type_id": 585}});
    let r = compute(json!({"format": "exfa/compute@1", "operation": "batch", "batch": {
        "batch_version": 1, "fits": [{"id": "a", "label": "A", "fit": fit_a}, {"id": "b", "fit": fit_b}]}}));
    assert_eq!(r["operation"], "batch");
    let res = r["result"]["results"].as_array().unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0]["id"], "a");
    assert_eq!(res[0]["label"], "A");
    assert_eq!(res[1]["id"], "b");
    assert_eq!(res[1]["label"], "b");
    // each untrimmed stats equals calc of the same normalized fit (with the shorthand defaults applied)
    let norm_a = json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}, "modules": [{"type_id": 439, "state": "active"}]});
    let norm_b = json!({"ship": {"type_id": 585}, "character": {"skills": {"default_level": 5}}});
    assert_eq!(res[0]["stats"], calc_raw(&norm_a));
    assert_eq!(res[1]["stats"], calc_raw(&norm_b));
    // normalization is silent inside batch: no DEFAULTED adjustments on item stats
    assert_eq!(res[0]["stats"]["adjustments"].as_array().unwrap().len(), 0);
}

#[test]
fn batch_variants_on_normalized_base() {
    let r = compute(json!({"format": "exfa/compute@1", "operation": "batch", "batch": {
        "batch_version": 1, "base": {"ship": {"type_id": 587}, "modules": [{"type_id": 439}]},
        "variants": [
            {"id": "kept", "patch": []},
            {"id": "down", "patch": [{"op": "replace", "path": "/modules/0/state", "value": "online"}]}
        ]}}));
    let res = r["result"]["results"].as_array().unwrap();
    assert_eq!(res.len(), 2);
    let norm = json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}, "modules": [{"type_id": 439, "state": "active"}]});
    let mut down = norm.clone();
    down["modules"][0]["state"] = json!("online");
    assert_eq!(res[0]["stats"], calc_raw(&norm));
    assert_eq!(res[1]["stats"], calc_raw(&down));
    assert!(res[0]["stats"]["navigation"]["max_velocity"].as_f64() > res[1]["stats"]["navigation"]["max_velocity"].as_f64());
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
    calc_result(json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}, "projected": [e]}))
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
    let only_m1 = calc_result(json!({"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}},
        "projected": [{"kind": "fit", "fit": {"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}},
            "modules": [{"id": "m1", "type_id": 526, "state": "active"}]}}]}));
    assert_eq!(v_m1, velocity(&only_m1), "select m1 == source with only m1");
    // unselected kinds contribute nothing: drone list present-but-empty + no module list -> no modules
    let v_drones_only = velocity(&projected(json!({"drone_ids": []})));
    assert_eq!(v_drones_only, v_empty, "absent module_ids contributes no modules");
    // unmatched id -> one deduplicated warning, the matched id still projects
    let miss = projected(json!({"module_ids": ["m1", "nope"]}));
    assert_eq!(velocity(&miss), v_m1);
    let w = miss["result"]["warnings"].as_array().unwrap();
    assert_eq!(w.iter().filter(|x| **x == "projected fit select: no module item with id 'nope'").count(), 1);
    // select on kind != fit is ignored (warning, no error)
    let other = calc_result(json!({"ship": {"type_id": 587}, "projected": [
        {"kind": "module", "module": {"type_id": 526, "state": "active"}, "select": {"module_ids": ["x"]}}]}));
    assert!(other["result"]["error"].is_null());
    assert!(other["result"]["warnings"].as_array().unwrap().iter().any(|w| w.as_str().unwrap_or("").contains("select")));
}

#[test]
fn error_envelopes() {
    let bad_format = compute(json!({"format": "nope", "operation": "calc", "fit": {}}));
    assert_eq!(bad_format["format"], "exfa/compute-result@1");
    assert_eq!(bad_format["error"]["code"], "UNSUPPORTED_FORMAT");
    for op in [json!("bogus"), json!(null)] {
        let r = compute(json!({"format": "exfa/compute@1", "operation": op}));
        assert_eq!(r["error"]["code"], "BAD_REQUEST", "{op}");
    }
    // inner fit error propagates with its code/message/path
    let inner = calc_result(json!({"ship": {"type_id": 999999999}}));
    assert_eq!(inner["operation"], "calc");
    assert_eq!(inner["error"]["code"], "UNKNOWN_TYPE");
    assert_eq!(inner["error"]["path"], "/ship/type_id");
    // missing fit / batch payloads
    assert_eq!(compute(json!({"format": "exfa/compute@1", "operation": "calc"}))["error"]["code"], "BAD_REQUEST");
    assert_eq!(compute(json!({"format": "exfa/compute@1", "operation": "batch"}))["error"]["code"], "BAD_REQUEST");
    // malformed JSON still returns the error envelope (operation recovered when possible)
    let malformed: Value = serde_json::from_str(&exfa_core::compute_json("{bad \"operation\":\"batch\"}")).unwrap();
    assert_eq!(malformed["format"], "exfa/compute-result@1");
    assert_eq!(malformed["operation"], "batch");
    assert!(malformed["error"]["code"].is_string());
    let no_op: Value = serde_json::from_str(&exfa_core::compute_json("{bad")).unwrap();
    assert_eq!(no_op["operation"], "calc");
}

#[test]
fn rpc_method_and_full_precision() {
    let p = json!({"format": "exfa/compute@1", "operation": "calc", "fit": {"ship": {"type_id": 587}, "modules": [{"type_id": 439}]}});
    let r = exfa_core::rpc(&json!({"id": 7, "method": "compute", "params": p}).to_string());
    assert_eq!(r["id"], 7);
    assert_eq!(r["result"]["format"], "exfa/compute-result@1");
    assert!(r["result"]["result"]["navigation"]["max_velocity"].is_number());
    // options.full_precision inside the FitSpec still takes the raw path: unrounded floats
    let p = json!({"format": "exfa/compute@1", "operation": "calc", "fit": {"ship": {"type_id": 587}, "options": {"full_precision": true}}});
    let v = compute(p)["result"]["capacitor"]["peak_recharge_gj_s"].as_f64().unwrap();
    let rounded = compute(json!({"format": "exfa/compute@1", "operation": "calc", "fit": {"ship": {"type_id": 587}}}))["result"]["capacitor"]["peak_recharge_gj_s"]
        .as_f64()
        .unwrap();
    assert_eq!(rounded, (v * 1e6).round() / 1e6, "raw path keeps >6 decimals: {v} vs {rounded}");
    assert!(v != rounded, "full_precision bypasses the 6-decimal rounding");
}
