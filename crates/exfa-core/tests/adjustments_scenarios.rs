use exfa_core::{data, graphs};
use serde_json::{json, Value};

fn calc(req: Value) -> Value {
    serde_json::from_str(&exfa_core::calc_json(&req.to_string())).unwrap()
}

fn adjustment(req: Value, code: &str) -> Value {
    let result = calc(req);
    assert!(result.get("error").is_none(), "{result}");
    result["adjustments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["code"] == code)
        .unwrap()
        .clone()
}

#[test]
fn adjustments_are_always_emitted_after_violations_and_only_for_the_top_level_fit() {
    let line = exfa_core::calc_json(
        r#"{"ship":{"type_id":587},"projected":[{"kind":"fit","fit":{"ship":{"type_id":34317}}}]}"#,
    );
    let result: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(result["adjustments"], json!([]));
    assert!(line.find("\"adjustments\"").unwrap() > line.find("\"violations\"").unwrap());
}

#[test]
fn state_clamps_are_adjustments() {
    let a = adjustment(
        json!({"ship": {"type_id": 587}, "modules": [{"type_id": 26374, "slot": "rig", "state": "active"}]}),
        "STATE_CLAMPED",
    );
    assert_eq!(
        a,
        json!({"code":"STATE_CLAMPED","path":"/modules/0/state","from":"active","to":"online","message":"module state clamped to online"})
    );
}

#[test]
fn fighter_quantity_clamps_are_adjustments() {
    let id = 40556;
    let max = data::type_attr(
        data::type_index(id).unwrap(),
        data::a::fighterSquadronMaxSize,
    )
    .unwrap() as u32;
    let result = calc(json!({
        "ship": {"type_id": 587},
        "fighters": [{"type_id": id, "quantity": max + 10}],
        "projected": [{"kind": "fighter", "fighter": {"type_id": id, "quantity": max + 10}}]
    }));
    let a = result["adjustments"].as_array().unwrap();
    assert!(a.iter().any(|x| x["code"] == "FIGHTER_QUANTITY_CLAMPED"
        && x["path"] == "/fighters/0/quantity"
        && x["from"] == max + 10
        && x["to"] == max));
    assert!(a.iter().any(|x| x["code"] == "FIGHTER_QUANTITY_CLAMPED"
        && x["path"] == "/projected/0/fighter/quantity"
        && x["from"] == max + 10
        && x["to"] == max));
}

#[test]
fn occupied_implant_and_booster_slots_are_adjustments() {
    let result = calc(json!({
        "ship": {"type_id": 587},
        "implants": [19540, 19540],
        "boosters": [{"type_id": 15466}, {"type_id": 15466}]
    }));
    let a = result["adjustments"].as_array().unwrap();
    assert!(a.iter().any(|x| x["code"] == "SLOT_OCCUPIED_SKIPPED"
        && x["path"] == "/implants/1"
        && x["from"] == 19540
        && x["to"].is_null()));
    assert!(a.iter().any(|x| x["code"] == "SLOT_OCCUPIED_SKIPPED"
        && x["path"] == "/boosters/1/type_id"
        && x["from"] == 15466
        && x["to"].is_null()));
}

#[test]
fn unknown_security_is_an_adjustment() {
    let a = adjustment(
        json!({"ship": {"type_id": 587}, "environment": {"system_security": "mystery"}}),
        "SECURITY_DEFAULTED",
    );
    assert_eq!(a["path"], "/environment/system_security");
    assert_eq!(a["from"], "mystery");
    assert_eq!(a["to"], "nullsec");
}

#[test]
fn default_tactical_mode_is_an_adjustment_not_a_warning() {
    let result = calc(json!({"ship": {"type_id": 34317}}));
    let a = result["adjustments"]
        .as_array()
        .unwrap()
        .iter()
        .find(|x| x["code"] == "MODE_DEFAULTED")
        .unwrap();
    assert_eq!(a["path"], "/ship/mode_type_id");
    assert!(a["from"].is_null());
    assert!(a["to"].is_number());
    assert!(result
        .get("warnings")
        .map_or(true, |w| !w.to_string().contains("no tactical mode given")));
}

fn graph_point(
    fit: Value,
    target: Value,
    params: Value,
    axis: &str,
    x: Value,
    settings: Value,
) -> Value {
    graphs::graph(&json!({
        "graph": "damage", "fit": fit, "target": target, "params": params, "settings": settings,
        "x": {"axis": axis, "values": [x]}, "y": ["dps", "volley"]
    }))
}

#[test]
fn scenarios_match_damage_graph_points() {
    let fit = json!({
        "ship": {"type_id": 24694},
        "character": {"skills": {"default_level": 5}},
        "modules": [{"type_id": 2961, "charge_type_id": 21894, "state": "active"}]
    });
    let target_profile = json!({"profile": {
        "em": 0.2, "thermal": 0.3, "kinetic": 0.4, "explosive": 0.5,
        "max_velocity": 500, "signature_radius": 100, "radius": 10, "hp": 100000
    }});
    let profile_params = json!({
        "distance_m": 15000, "time_s": null, "tgt_speed_mps": 250, "tgt_speed_pct": 100,
        "tgt_sig_m": 100, "atk_speed_mps": 200, "atk_speed_pct": 0, "atk_angle_deg": 45, "tgt_angle_deg": 110
    });
    let null_distance_params = json!({
        "distance_m": null, "time_s": null, "tgt_speed_mps": 300, "tgt_speed_pct": 100,
        "tgt_sig_m": 100, "atk_speed_mps": 150, "atk_speed_pct": 0, "atk_angle_deg": 90, "tgt_angle_deg": 90
    });
    let target_fit = json!({"fit": {"ship": {"type_id": 587}, "character": {"skills": {"default_level": 5}}, "modules": []}, "resist_mode": "auto"});
    let fit_target_params = json!({
        "distance_m": 20000, "time_s": null, "tgt_speed_mps": 100, "tgt_speed_pct": 100,
        "tgt_sig_m": 35, "atk_speed_mps": 250, "atk_speed_pct": 0, "atk_angle_deg": 30, "tgt_angle_deg": 145
    });
    let cases = [
        (
            target_profile.clone(),
            profile_params.clone(),
            "distance_m",
            json!(15000),
        ),
        (
            target_profile,
            null_distance_params.clone(),
            "tgt_angle_deg",
            json!(90),
        ),
        (
            target_fit.clone(),
            fit_target_params.clone(),
            "distance_m",
            json!(20000),
        ),
    ];
    let scenarios: Vec<Value> = cases
        .iter()
        .enumerate()
        .map(|(i, (target, params, _, _))| {
            json!({
                "id": format!("s{i}"), "target": target, "params": params,
            })
        })
        .collect();
    let result = calc(
        json!({"ship": fit["ship"], "character": fit["character"], "modules": fit["modules"], "scenarios": scenarios}),
    );
    let results = result["scenario_results"].as_array().unwrap();
    for (i, (target, params, axis, x)) in cases.iter().enumerate() {
        let graph = graph_point(
            fit.clone(),
            target.clone(),
            params.clone(),
            axis,
            x.clone(),
            json!({"ignore_resists": false}),
        );
        assert!(graph.get("error").is_none(), "{graph}");
        assert_eq!(results[i]["dps"], graph["series"]["dps"][0], "case {i}");
        assert_eq!(
            results[i]["volley"], graph["series"]["volley"][0],
            "case {i}"
        );
    }
}

#[test]
fn scenario_errors_are_isolated_and_absence_does_not_change_other_stats() {
    let fit = json!({"ship": {"type_id": 587}, "modules": []});
    let base = calc(fit.clone());
    let result = calc(json!({
        "ship": fit["ship"], "scenarios": [
            {"id": "bad", "target": {"profile": {}}, "params": {"distance_m": -1}},
            {"id": "ok", "target": {"profile": {}}, "params": {"distance_m": null}}
        ]
    }));
    assert_eq!(result["scenario_results"][0]["id"], "bad");
    assert_eq!(
        result["scenario_results"][0]["error"]["path"],
        "/scenarios/0/params/distance_m"
    );
    assert_eq!(result["scenario_results"][1]["id"], "ok");
    let mut with_scenarios = result;
    with_scenarios
        .as_object_mut()
        .unwrap()
        .remove("scenario_results");
    assert_eq!(with_scenarios, base);
}

#[test]
fn graph_axes_expose_and_apply_attacker_parameters() {
    let axes = &graphs::specs_json()["graphs"]["damage"]["axes"];
    for axis in [
        "atk_speed_mps",
        "atk_speed_pct",
        "atk_angle_deg",
        "tgt_angle_deg",
    ] {
        assert!(axes.get(axis).is_some());
        assert!(
            graphs::specs_json()["graphs"]["application_profile"]["axes"]
                .get(axis)
                .is_some()
        );
    }
    let fit = json!({
        "ship": {"type_id": 24694}, "character": {"skills": {"default_level": 5}},
        "modules": [{"type_id": 2961, "charge_type_id": 21894, "state": "active"}]
    });
    let graph = graphs::graph(&json!({
        "graph": "application_profile", "fit": fit,
        "target": {"profile": {"max_velocity": 400, "signature_radius": 100, "radius": 10, "hp": 100000}},
        "params": {"distance_m": 10000, "tgt_speed_mps": 400, "tgt_angle_deg": 90, "atk_speed_mps": 100},
        "x": {"axis": "tgt_angle_deg", "values": [10, 90, 170]}, "y": ["dps"]
    }));
    let series = graph["series"]["dps"].as_array().unwrap();
    assert!(series[0].is_number() && series[1].is_number() && series[2].is_number());
    assert!(series.windows(2).any(|pair| pair[0] != pair[1]));
}
