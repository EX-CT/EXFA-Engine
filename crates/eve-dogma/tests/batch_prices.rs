//! docs/23: batch API and prices.
use serde_json::{json, Value};

fn calc(fit: &Value) -> Value {
    let r: eve_dogma::FitRequest = serde_json::from_value(fit.clone()).unwrap();
    serde_json::to_value(eve_dogma::calc(&r)).unwrap()
}
fn batch(req: Value) -> Value {
    eve_dogma::batch::run(&req)
}
fn rifter() -> Value {
    json!({"ship":{"type_id":587},"modules":[{"type_id":2873,"state":"active","charge_type_id":12608},{"type_id":2048,"state":"active"}],
           "implants":[],"cargo":[{"type_id":12608,"quantity":100}]})
}
fn prices() -> Value {
    json!({"isk":{"587":350000.0,"2873":1250000.0,"2048":900000.0,"12608":20.0}})
}

#[test]
fn batch_equals_one_by_one_all_forms() {
    let fits = json!([{"id":"a","fit":rifter()},{"fit":{"ship":{"type_id":585}}},{"fit":{"ship":{"type_id":1}}}]);
    let r = batch(json!({"batch_version":1,"fits":fits}));
    let res = r["results"].as_array().unwrap();
    assert_eq!(res.len(), 3);
    assert_eq!(res[0]["id"], "a");
    assert_eq!(res[1]["id"], "1");
    assert_eq!(res[0]["stats"], calc(&rifter()));
    assert!(res[2]["error"]["code"].is_string(), "per-fit error in place: {}", res[2]);
    // variants
    let r = batch(json!({"batch_version":1,"base":rifter(),"variants":[
        {"id":"heat","patch":[{"op":"replace","path":"/modules/0/state","value":"overheated"}]},
        {"patch":[{"op":"swap_type","from":2873,"to":484}]},
        {"patch":[{"op":"remove","path":"/modules/9"}]}]}));
    let res = r["results"].as_array().unwrap();
    let mut f = rifter();
    f["modules"][0]["state"] = json!("overheated");
    assert_eq!(res[0]["stats"], calc(&f));
    assert_eq!(res[1]["id"], "v2");
    let mut g = rifter();
    g["modules"][0]["type_id"] = json!(484);
    assert_eq!(res[1]["stats"], calc(&g));
    assert_eq!(res[2]["error"]["code"], "PATCH_FAILED");
    // product: first axis slowest, ids joined with |, labels with ×
    let r = batch(json!({"batch_version":1,"base":rifter(),"product":{"axes":[
        {"options":[{"id":"t2","label":"T2"},{"id":"t1","label":"T1","patch":[{"op":"swap_type","from":2873,"to":484}]}]},
        {"sweep":{"path":"/character/skills/default_level","from":3,"to":5,"step":1}}]}}));
    let res = r["results"].as_array().unwrap();
    assert_eq!(res.len(), 6);
    assert_eq!(res[0]["id"], "t2|/character/skills/default_level=3");
    assert_eq!(res[5]["label"], "T1 × /character/skills/default_level=5");
    let mut h = g.clone();
    h["character"] = json!({"skills":{"default_level":4}});
    assert_eq!(res[4]["stats"], calc(&h), "patch on a default path works (normalized base)");
}

#[test]
fn fields_deltas_filter_sort_topn() {
    let r = batch(json!({"batch_version":1,"base":{"ship":{"type_id":587}},
        "sweep":{"path":"/character/skills/default_level","values":[0,5,3]},
        "fields":["navigation.max_velocity","nope.x"],"deltas":true,
        "filter":[{"field":"navigation.max_velocity","op":">","value":0}],
        "sort_by":[{"field":"navigation.max_velocity","order":"desc"}],"top_n":2}));
    assert_eq!(r["total"], 3);
    assert_eq!(r["matched"], 3);
    let res = r["results"].as_array().unwrap();
    assert_eq!(res.len(), 2);
    assert_eq!(res[0]["index"], 1);
    assert_eq!(res[1]["index"], 2);
    assert!(res[0]["stats"]["nope.x"].is_null());
    let v = res[0]["stats"]["navigation.max_velocity"].as_f64().unwrap();
    let b = r["base"]["stats"]["navigation.max_velocity"].as_f64().unwrap();
    assert!((res[0]["delta"]["navigation.max_velocity"].as_f64().unwrap() - (v - b)).abs() < 1e-6);
    assert!(!r["warnings"].as_array().unwrap().is_empty());
}

#[test]
fn deterministic_and_capped() {
    let req = json!({"batch_version":1,"base":rifter(),"sweep":{"path":"/character/skills/default_level","from":0,"to":5,"step":1},"price":true,"prices":prices()});
    assert_eq!(serde_json::to_string(&batch(req.clone())).unwrap(), serde_json::to_string(&batch(req)).unwrap());
    let r = batch(json!({"batch_version":1,"base":rifter(),"sweep":{"path":"/x","from":0,"to":10,"step":1},"max_combinations":5}));
    assert_eq!(r["error"]["code"], "BATCH_TOO_LARGE");
    assert_eq!(r["error"]["count"], 11);
    assert_eq!(batch(json!({"batch_version":1,"fits":[],"variants":[]}))["error"]["code"], "BATCH_BAD_REQUEST");
}

fn line<'a>(p: &'a Value, sec: &str, idx: u64) -> &'a Value {
    p["sections"][sec]["items"].as_array().unwrap().iter().find(|l| l["index"] == idx).unwrap()
}

#[test]
fn calc_price_block_and_absence() {
    assert!(calc(&rifter()).get("price").is_none(), "no price inputs: no price block (output unchanged)");
    let mut f = rifter();
    f["prices"] = prices();
    let out = calc(&f);
    assert_eq!(out["provenance"]["price_source"], "request");
    let p = out["price"].clone();
    // Hail S: floor(capacity / volume) loaded charges
    let hail = line(&p, "charges", 0);
    assert_eq!(hail["quantity"], 200);
    assert_eq!(line(&p, "ship", 0)["kind"], "ship");
    assert_eq!(line(&p, "ship", 0)["base_source"], "injected");
    assert_eq!(line(&p, "ship", 0)["multiplier"], 1.0);
    let total = 350000.0 + 1250000.0 + 900000.0 + 200.0 * 20.0 + 100.0 * 20.0;
    assert!((p["total_isk"].as_f64().unwrap() - total).abs() < 1e-6);
    assert_eq!(p["complete"], true);
}

#[test]
fn precedence_and_multiplier_chain() {
    let mut f = rifter();
    f["prices"] = prices();
    // L2: type beats group beats category; multiplier on injected
    f["price_overrides"] = json!([{"category_id":7,"price":1.0},{"group_id":55,"price":2.0},{"type_id":2873,"multiplier":0.5}]);
    let p = calc(&f)["price"].clone();
    let gun = line(&p, "modules", 0);
    assert_eq!(gun["source"], "override:type");
    assert_eq!(gun["base_source"], "injected");
    assert!((gun["unit_isk"].as_f64().unwrap() - 625000.0).abs() < 1e-9);
    assert_eq!(line(&p, "modules", 1)["source"], "override:category"); // DC II: group 60, category 7
    // L1 (variant) x0.9 over L2 x0.5 over injected = 0.45; a fixed L1 price stops the chain
    let r = batch(json!({"batch_version":1,"base":f,"price":true,"variants":[
        {"id":"m","price_overrides":[{"type_id":2873,"multiplier":0.9}]},
        {"id":"x","price_overrides":[{"type_id":2873,"price":7.0}]},
        {"id":"z","price_overrides":[{"type_id":2873,"price":0.0}]}]}));
    let res = r["results"].as_array().unwrap();
    let g = line(&res[0]["price"], "modules", 0);
    assert!((g["unit_isk"].as_f64().unwrap() - 1250000.0 * 0.45).abs() < 1e-6);
    assert!((g["multiplier"].as_f64().unwrap() - 0.45).abs() < 1e-12);
    assert_eq!(g["layer"], "variant");
    assert_eq!(line(&res[1]["price"], "modules", 0)["unit_isk"], 7.0);
    assert_eq!(line(&res[2]["price"], "modules", 0)["unit_isk"], 0.0);
    // multiplier without base
    let mut h = rifter();
    h["price_overrides"] = json!([{"type_id":587,"multiplier":2.0}]);
    h["prices"] = json!({"use_snapshot": false}); // the embedded snapshot would price both
    let p = calc(&h)["price"].clone();
    let m = p["missing"].as_array().unwrap();
    assert!(m.iter().any(|x| x["type_id"] == 587 && x["reason"] == "multiplier_without_base"));
    assert!(m.iter().any(|x| x["type_id"] == 2873 && x["reason"] == "no_price"));
    assert_eq!(p["complete"], false);
}

#[test]
fn market_group_deepest_wins() {
    if !eve_dogma::data::HAS_MARKET_GROUP_TREE {
        return;
    }
    let ix = eve_dogma::data::type_index(2873).unwrap();
    let own = eve_dogma::data::type_market_group(ix).unwrap();
    let parent = eve_dogma::data::market_group_parent(own).unwrap();
    let mut f = rifter();
    f["price_overrides"] = json!([{"market_group_id":parent,"price":5.0},{"market_group_id":own,"price":3.0}]);
    let p = calc(&f)["price"].clone();
    assert_eq!(line(&p, "modules", 0)["unit_isk"], 3.0);
    assert_eq!(line(&p, "modules", 0)["source"], "override:market_group");
    f["price_overrides"] = json!([{"market_group_id":parent,"price":5.0}]);
    assert_eq!(line(&calc(&f)["price"], "modules", 0)["unit_isk"], 5.0, "children included");
}

#[test]
fn bad_overrides() {
    let mut f = rifter();
    f["price_overrides"] = json!([{"type_id":1,"group_id":2,"price":1.0}]);
    assert_eq!(calc(&f)["error"]["code"], "BAD_PRICE_OVERRIDE");
    f["price_overrides"] = json!([{"type_id":1,"price":1.0},{"type_id":1,"multiplier":2.0}]);
    assert_eq!(calc(&f)["error"]["code"], "BAD_PRICE_OVERRIDE");
    f["price_overrides"] = json!([{"type_id":1,"price":-1.0}]);
    assert_eq!(calc(&f)["error"]["code"], "BAD_PRICE_OVERRIDE");
    let r = batch(json!({"batch_version":1,"fits":[{"fit":rifter()}],"price_overrides":[{"price":1.0}]}));
    assert_eq!(r["error"]["code"], "BAD_PRICE_OVERRIDE");
}

#[test]
fn rpc_batch_method() {
    let line = json!({"id":7,"method":"batch","params":{"batch_version":1,"fits":[{"fit":{"ship":{"type_id":587}}}],"fields":["navigation.max_velocity"]}});
    let r = eve_dogma::rpc(&line.to_string());
    assert_eq!(r["id"], 7);
    assert_eq!(r["result"]["total"], 1);
}

#[test]
fn embedded_snapshot_identity_and_jcs() {
    let m = eve_dogma::price::embedded().expect("embedded snapshot parses and validates");
    assert_eq!(m.id.as_deref(), Some(eve_dogma::price::EMBEDDED_ID));
    assert_eq!(m.time.as_deref(), Some(eve_dogma::price::EMBEDDED_TIME));
    assert_eq!(m.hash.as_deref(), Some(eve_dogma::price::EMBEDDED_HASH));
    use eve_dogma::prov::es_number as es;
    assert_eq!(es(1240000.0), "1240000");
    assert_eq!(es(4988.83), "4988.83");
    assert_eq!(es(1e21), "1e+21");
    assert_eq!(es(1e-7), "1e-7");
    assert_eq!(es(0.000001), "0.000001");
    assert_eq!(eve_dogma::prov::jcs(&json!({"34":1,"1000":2.0,"b":[1.5,"x"]})), r#"{"1000":2,"34":1,"b":[1.5,"x"]}"#);
    // provenance: embedded snapshot by default, none with use_snapshot false
    let out = calc(&json!({"ship":{"type_id":587},"options":{"price":true}}));
    assert_eq!(out["provenance"]["price_source"], "snapshot");
    assert_eq!(out["provenance"]["sde_build"], 3569502);
    assert_eq!(line(&out["price"], "ship", 0)["source"], "snapshot");
    let out = calc(&json!({"ship":{"type_id":587},"prices":{"use_snapshot":false}}));
    assert_eq!(out["provenance"]["price_source"], "none");
    assert_eq!(out["price"]["complete"], false);
}

#[test]
fn sde_override_errors() {
    use eve_dogma::prov::check_pack;
    let code = |b: &[u8]| check_pack(b).unwrap_err()["error"]["reason"].as_str().unwrap().to_string();
    assert_eq!(code(b""), "corrupt");
    let mut p = vec![0u8; 64];
    p[..4].copy_from_slice(b"XXXX");
    assert_eq!(code(&p), "corrupt");
    p[..4].copy_from_slice(b"EDPK");
    p[4] = 2;
    assert_eq!(code(&p), "incompatible_version");
    p[4] = 1;
    assert_eq!(code(&p), "hash_mismatch");
    assert_eq!(eve_dogma::prov::load_pack_path("/nonexistent.edp").unwrap_err()["error"]["reason"], "not_found");
}
