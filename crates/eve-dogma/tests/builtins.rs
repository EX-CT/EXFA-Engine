//! damage_pattern / target_profile {"builtin": name} (bench draft 1.11 dpb_* / tpb_*), resolved from runtime Pyfa data.
use serde_json::{json, Value};

fn calc(req: Value) -> Value {
    serde_json::from_str(&eve_dogma::calc_json(&req.to_string())).unwrap()
}

#[test]
fn builtin_pattern_and_profile() {
    let rpc = |m: &str, p: Value| eve_dogma::rpc(&json!({"id": 1, "method": m, "params": p}).to_string())["result"].clone();
    rpc("pyfa_data_load", json!({"clear": true}));
    let base = json!({"schema_version": 1, "ship": {"type_id": 587}, "modules": [{"type_id": 2048, "state": "active"}]});
    let mut q = base.clone();
    q["damage_pattern"] = json!({"builtin": "[Generic]EM"});
    assert_eq!(calc(q.clone())["error"]["code"], "UNKNOWN_BUILTIN");
    let st = rpc("pyfa_data_load", json!({"data": {
        "damage_patterns": {"items": [{"name": "[Generic]EM", "amounts": {"em": 1, "thermal": 0, "kinetic": 0, "explosive": 0}}]},
        "target_profiles": {"items": [{"name": "Uniform (50%)", "em": 0.5, "thermal": 0.5, "kinetic": 0.5, "explosive": 0.5}]}}}));
    assert_eq!(st["damage_patterns"], 1);
    let mut e = base.clone();
    e["damage_pattern"] = json!({"em": 1, "thermal": 0, "kinetic": 0, "explosive": 0});
    let (a, b) = (calc(q.clone()), calc(e));
    assert!(a.get("error").is_none());
    assert_eq!(a["defense"]["ehp"], b["defense"]["ehp"]);
    q["target_profile"] = json!({"builtin": "Uniform (50%)"});
    assert!(calc(q)["offense"]["vs_target_profile"].is_object());
    rpc("pyfa_data_load", json!({"clear": true}));
}
