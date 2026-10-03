//! ext/rpc lookups (bench pending-1.11 ext/rpc, Pyfa service layer behaviour).
use serde_json::{json, Value};

fn rpc(method: &str, params: Value) -> Value {
    eve_dogma::rpc(&json!({"id": 1, "method": method, "params": params}).to_string())["result"].clone()
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect()
}

#[test]
fn variations_compare_market() {
    let v = ids(&rpc("item.variations", json!({"type_id": 2048}))["type_ids"]);
    assert!(v.contains(&2046) && v.contains(&2048) && v.contains(&521));
    let c = rpc("item.compare", json!({"type_id": 2364, "attributes": ["cpu", "damageMultiplier"]}));
    let hs2 = c["items"].as_array().unwrap().iter().find(|x| x["type_id"] == 2364).unwrap();
    assert_eq!(hs2["attributes"]["damageMultiplier"], 1.1);
    let root = rpc("market.group", json!({"market_group_id": null}));
    assert_eq!(ids(&root["groups"]), vec![9, 11, 24, 157, 404, 1111, 1112, 2202, 2203, 2456]);
    let g = rpc("market.group", json!({"market_group_id": 615}));
    assert!(ids(&g["items"]).contains(&2048));
}

#[test]
fn search_without_and_with_jargon() {
    rpc("pyfa_data_load", json!({"clear": true})); // independent of EVE_DOGMA_PYFA_DATA
    let r = ids(&rpc("market.search", json!({"query": "damage control", "filter": "market"}))["type_ids"]);
    assert!(r.contains(&2048) && !r.contains(&52227), "abyssal items are unpublished in Pyfa");
    let r = ids(&rpc("market.search", json!({"query": "re:^Small Focused", "filter": "market"}))["type_ids"]);
    assert!(!r.is_empty());
    assert_eq!(rpc("market.search", json!({"query": "ab"}))["type_ids"], json!([]));
    let st = rpc("pyfa_data_load", json!({"data": {"jargon": {"items": {"dc": ["dc", "damage control"]}}}}));
    assert_eq!(st["jargon"], 1);
    let r = ids(&rpc("market.search", json!({"query": "dc", "filter": "market"}))["type_ids"]);
    assert!(r.contains(&2048));
    rpc("pyfa_data_load", json!({"clear": true}));
}

#[test]
fn implant_sets_evemon_names_type() {
    let s = rpc("implant_sets.list", json!({}));
    assert_eq!(ids(&s["sets"]["Amulet"]["High-grade"]), vec![20499, 20501, 20503, 20505, 20507, 20509]);
    let x = r#"<?xml version="1.0"?><SerializableCCPCharacter><name>A &amp; B</name><securityStatus>2.5</securityStatus><skills><skill typeID="3449" level="5"/><skill typeID="587" level="3"/><skill typeID="3300" level="7"/></skills></SerializableCCPCharacter>"#;
    let c = rpc("character.import_evemon", json!({"xml": x}));
    assert_eq!(c, json!({"name": "A & B (EVEMon)", "security_status": 2.5, "skills": {"3449": 5}}));
    assert!(rpc("character.import_evemon", json!({"xml": "<Character/>"}))["error"].is_object());
    let n = rpc("names.resolve", json!({"names": ["Damage Control II", "No Such Item"]}));
    assert_eq!(n["resolved"], json!({"Damage Control II": 2048, "No Such Item": null}));
    let t = rpc("type", json!({"id": 587}));
    assert_eq!(t["required_skills"], json!({"3329": 1}));
    assert!(t["traits_html"].as_str().unwrap().starts_with("<b>Minmatar Frigate bonuses (per skill level):</b><br />\n7.5% bonus"));
    assert_eq!(t["attributes"]["capacity"], 140.0);
    assert_eq!(rpc("type", json!({"id": 2048}))["attributes"]["radius"], 1.0);
}
