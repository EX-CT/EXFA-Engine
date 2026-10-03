//! Unit tests for every format: import -> export round trips, export -> import round trips, and error paths.
//! Fixtures in `tests/fixtures` (Pyfa-written EFT for the mutated case, hand-written otherwise).
use eve_fit_formats::{format_export, format_import, formats, rpc};
use serde_json::{json, Value};

const VEXOR: &str = include_str!("fixtures/vexor.eft");
const TRISTAN_MUT: &str = include_str!("fixtures/tristan_mutated.eft");
const RIFTER_CFG: &str = include_str!("fixtures/Rifter.cfg");

fn import_all(text: &str, format: &str) -> Value {
    let r = format_import(&json!({"text": text, "format": format}));
    assert!(r.get("error").is_none(), "{format} import failed: {r}\n{text}");
    r
}
fn import(text: &str, format: &str) -> Value {
    import_all(text, format)["fits"][0].clone()
}
fn export(fit: &Value, format: &str) -> String {
    let r = format_export(&json!({"fit": fit, "name": fit["name"].as_str().unwrap_or("EXCT fit"), "format": format}), None);
    assert!(r.get("error").is_none(), "{format} export failed: {r}");
    r["text"].as_str().unwrap().to_string()
}
/// fit with every list sorted (formats may write sections in a different order than they read them)
fn normalized(f: &Value) -> Value {
    let mut g = f.clone();
    for k in ["modules", "drones", "cargo", "implants", "boosters", "fighters"] {
        if let Some(a) = g.get_mut(k).and_then(|a| a.as_array_mut()) {
            a.sort_by_key(|x| x.to_string());
        }
    }
    g
}
fn err_code(v: &Value) -> String {
    v["error"]["code"].as_str().unwrap_or_else(|| panic!("expected an error, got {v}")).to_string()
}

/// what every fit format carries: hull, fitted modules (type, slot), drones (type, count), cargo
fn modules(f: &Value) -> Vec<(u64, String)> {
    let mut v: Vec<(u64, String)> = f["modules"].as_array().unwrap().iter().map(|m| (m["type_id"].as_u64().unwrap(), m["slot"].as_str().unwrap_or("").to_string())).collect();
    v.sort();
    v
}
fn stacks(f: &Value, key: &str, n: &str) -> Vec<(u64, u64)> {
    let mut m = std::collections::BTreeMap::new();
    for x in f[key].as_array().unwrap() {
        *m.entry(x["type_id"].as_u64().unwrap()).or_insert(0) += x[n].as_u64().unwrap_or(1);
    }
    m.into_iter().collect()
}
fn ids(f: &Value, key: &str) -> Vec<u64> {
    let mut v: Vec<u64> = f[key].as_array().unwrap().iter().map(|x| x.as_u64().or_else(|| x["type_id"].as_u64()).unwrap()).collect();
    v.sort();
    v
}

// ---------------------------------------------------------------- EFT

#[test]
fn eft_import_export_import() {
    let f1 = import(VEXOR, "eft");
    assert_eq!(f1["ship"]["type_id"], json!(626));
    assert_eq!(f1["name"], json!("Unit Vexor"));
    let t1 = export(&f1, "eft");
    let f2 = import(&t1, "eft");
    assert_eq!(normalized(&f1), normalized(&f2), "EFT import -> export -> import changed the fit");
    assert_eq!(export(&f2, "eft"), t1, "EFT export is not a fixed point");
    // states, charges, drones, implants, boosters, cargo all survive
    let off = f2["modules"].as_array().unwrap().iter().filter(|m| m["state"] == json!("offline")).count();
    assert_eq!(off, 1);
    assert_eq!(f2["modules"].as_array().unwrap().iter().filter(|m| m["charge_type_id"] == json!(12789)).count(), 3);
    assert_eq!(stacks(&f2, "drones", "quantity"), vec![(2185, 5), (2456, 3)]);
    assert_eq!(ids(&f2, "implants"), vec![3237]);
    assert_eq!(ids(&f2, "boosters"), vec![10156]);
    assert_eq!(stacks(&f2, "cargo", "quantity"), vec![(12785, 2000), (28668, 50)]);
}

#[test]
fn eft_export_import_export() {
    // from a structured fit (not text): export, import, compare
    let fit = json!({"name": "Hand made", "ship": {"type_id": 587},
        "modules": [{"type_id": 2048, "state": "active"}, {"type_id": 519, "state": "online"},
                    {"type_id": 2873, "state": "active", "charge_type_id": 12608}, {"type_id": 438, "state": "offline"}],
        "drones": [], "cargo": [{"type_id": 28668, "quantity": 10}]});
    let t = export(&fit, "eft");
    let f = import(&t, "eft");
    assert_eq!(f["ship"]["type_id"], json!(587));
    assert_eq!(modules(&f).iter().map(|x| x.0).collect::<Vec<_>>(), vec![438, 519, 2048, 2873]);
    let st: Vec<(u64, String)> = f["modules"].as_array().unwrap().iter().map(|m| (m["type_id"].as_u64().unwrap(), m["state"].as_str().unwrap().to_string())).collect();
    assert!(st.contains(&(438, "offline".into())), "{st:?}");
    assert_eq!(export(&f, "eft"), t);
}

#[test]
fn eft_mutations_round_trip_exactly() {
    let f = import(TRISTAN_MUT, "eft");
    let m = f["modules"].as_array().unwrap().iter().find(|m| !m["mutation"].is_null()).expect("mutated module");
    assert!(m["mutation"]["attributes"].as_object().map(|a| !a.is_empty()).unwrap_or(true));
    assert!(f["drones"].as_array().unwrap().iter().any(|d| !d["mutation"].is_null()), "mutated drone stack lost");
    // Pyfa-written text: our export of the import is byte-identical
    assert_eq!(export(&f, "eft"), TRISTAN_MUT);
}

#[test]
fn eft_strict_parse_vs_lenient_import() {
    let bad = VEXOR.replace("Damage Control II", "No Such Module III");
    // eft_parse (strict) rejects unknown items, format_import (Pyfa behaviour) skips them
    let r = eve_fit_formats::eft_parse(&json!({"text": bad}));
    assert_eq!(err_code(&r), "EFT_PARSE");
    let f = import(&bad, "eft");
    assert_eq!(modules(&f).len(), modules(&import(VEXOR, "eft")).len() - 1);
}

#[test]
fn eft_errors() {
    for (t, why) in [("", "empty"), ("garbage text", "no header"), ("[NotAShip, x]\nDamage Control II", "unknown ship")] {
        let r = format_import(&json!({"text": t, "format": "eft"}));
        assert_eq!(err_code(&r), "IMPORT_ERROR", "{why}");
    }
    let r = eve_fit_formats::eft_export(&json!({"fit": "not a fit"}));
    assert_eq!(err_code(&r), "BAD_REQUEST");
}

// ---------------------------------------------------------------- EFT config (import only)

#[test]
fn eftcfg_import() {
    let r = format_import(&json!({"text": RIFTER_CFG, "format": "eftcfg", "path": "/x/Rifter.cfg"}));
    assert_eq!(r["kind"], json!("EFT Config"), "{r}");
    let fits = r["fits"].as_array().unwrap();
    assert_eq!(fits.len(), 2);
    // Pyfa keeps the "Ship, name" header text as the first fit's name
    assert_eq!(fits[0]["name"], json!("Rifter, Fit A"));
    assert_eq!(fits[1]["name"], json!("Fit B"));
    for f in fits {
        assert_eq!(f["ship"]["type_id"], json!(587));
    }
    assert_eq!(stacks(&fits[0], "drones", "quantity"), vec![(eve_sde::type_by_name("Warrior II").unwrap() as u64, 2)]);
    assert_eq!(fits[0]["implants"].as_array().unwrap().len(), 1);
    assert_eq!(fits[0]["boosters"].as_array().unwrap().len(), 1);
    // and the imported fits go through EFT export -> import unchanged
    for f in fits {
        let f2 = import(&export(f, "eft"), "eft");
        assert_eq!(modules(f), modules(&f2));
    }
    assert_eq!(format_import(&json!({"text": RIFTER_CFG, "path": "/x/Rifter.cfg"}))["kind"], json!("EFT Config"));
}

#[test]
fn eftcfg_errors() {
    let r = format_import(&json!({"text": "garbage", "format": "eftcfg", "path": "NotAShip.cfg"}));
    assert_eq!(err_code(&r), "IMPORT_ERROR");
}

// ---------------------------------------------------------------- DNA (+ alt, chat link)

#[test]
fn dna_round_trips() {
    let f1 = import(VEXOR, "eft");
    let t1 = export(&f1, "dna");
    assert!(t1.starts_with("626:") && t1.ends_with("::"), "{t1}");
    let f2 = import(&t1, "dna");
    assert_eq!(f2["ship"]["type_id"], json!(626));
    assert_eq!(modules(&f2), modules(&f1));
    assert_eq!(stacks(&f2, "drones", "quantity"), stacks(&f1, "drones", "quantity"));
    // export -> import -> export is a fixed point
    assert_eq!(export(&f2, "dna"), t1);
    // import -> export -> import from DNA text
    let f3 = import(&export(&f2, "dna"), "dna");
    assert_eq!(f2, f3);
    // chat link and DNA: forms
    let link = format!("<url=fitting:{t1}>Linked fit</url>");
    let fl = import(&link, "dna_link");
    assert_eq!(fl["name"], json!("Linked fit"));
    assert_eq!(modules(&fl), modules(&f2));
    let alt = import("DNA:626:2048*1:4405*2", "dna_alt");
    assert_eq!(modules(&alt).iter().map(|x| x.0).collect::<Vec<_>>(), vec![2048, 4405, 4405]);
    assert_eq!(import(&link, "auto")["name"], json!("Linked fit"));
}

#[test]
fn dna_errors() {
    for (t, f) in [("garbage", "dna"), ("999999999:2048;1::", "dna"), ("626:abc;1::", "dna"), ("no link", "dna_link")] {
        assert_eq!(err_code(&format_import(&json!({"text": t, "format": f}))), "IMPORT_ERROR", "{f} {t}");
    }
}

// ---------------------------------------------------------------- ESI fitting JSON

#[test]
fn esi_round_trips() {
    let f1 = import(VEXOR, "eft");
    let t1 = export(&f1, "esi");
    let j: Value = serde_json::from_str(&t1).unwrap();
    assert_eq!(j["ship_type_id"], json!(626));
    assert_eq!(j["name"], json!("Unit Vexor"));
    let f2 = import(&t1, "esi");
    assert_eq!(f2["name"], json!("Unit Vexor"));
    assert_eq!(modules(&f2), modules(&f1));
    assert_eq!(stacks(&f2, "drones", "quantity"), stacks(&f1, "drones", "quantity"));
    // ESI import reorders racks; from there export -> import -> export is a fixed point
    let t2 = export(&f2, "esi");
    assert_eq!(export(&import(&t2, "esi"), "esi"), t2);
    assert_eq!(normalized(&import(&t2, "esi")), normalized(&f2));
    assert_eq!(import(&t1, "auto"), f2);
}

#[test]
fn esi_errors() {
    for t in ["{bad json", "[]", "{\"items\":[]}", "{\"ship_type_id\": 999999999, \"items\": []}"] {
        assert_eq!(err_code(&format_import(&json!({"text": t, "format": "esi"}))), "IMPORT_ERROR", "{t}");
    }
    // ESI refuses a fit without modules (the client does too)
    let r = format_export(&json!({"fit": {"ship": {"type_id": 626}}, "format": "esi"}), None);
    assert_eq!(err_code(&r), "EXPORT_ERROR");
}

// ---------------------------------------------------------------- EVE client XML

#[test]
fn xml_round_trips() {
    let f1 = import(VEXOR, "eft");
    let t1 = export(&f1, "xml");
    assert!(t1.starts_with("<?xml"));
    let r = import_all(&t1, "xml");
    assert_eq!(r["fits"].as_array().unwrap().len(), 1);
    let f2 = r["fits"][0].clone();
    assert_eq!(f2["name"], json!("Unit Vexor"));
    assert_eq!(modules(&f2), modules(&f1));
    assert_eq!(stacks(&f2, "drones", "quantity"), stacks(&f1, "drones", "quantity"));
    assert_eq!(export(&f2, "xml"), t1);
    // names with markup characters are escaped and come back unchanged
    let mut g = f1.clone();
    g["name"] = json!("A & B <fit> \"q\"");
    let back = import(&export(&g, "xml"), "xml");
    assert_eq!(back["name"], g["name"]);
}

#[test]
fn xml_errors() {
    for t in ["<fittings", "<fittings count=\"0\"></fittings>",
              "<?xml version=\"1.0\"?><fittings><fitting name=\"a\"><shipType value=\"NoSuchShip\"/></fitting></fittings>"] {
        assert_eq!(err_code(&format_import(&json!({"text": t, "format": "xml"}))), "IMPORT_ERROR", "{t}");
    }
}

// ---------------------------------------------------------------- multibuy (export) and item lists (import)

#[test]
fn multibuy_export_counts_everything() {
    let f = import(VEXOR, "eft");
    let t = export(&f, "multibuy");
    let lines: Vec<&str> = t.lines().collect();
    assert_eq!(lines[0], "Vexor");
    for want in ["Drone Damage Amplifier II x2", "Heavy Neutron Blaster II x3", "Hammerhead II x5", "Null M x2000",
                 "Void M x240", "Strong Blue Pill Booster", "Medium Auxiliary Nano Pump I x2"] {
        assert!(lines.contains(&want), "missing {want:?} in\n{t}");
    }
    // every line names a real type
    for l in &lines[1..] {
        let name = l.rsplit_once(" x").filter(|(_, n)| n.parse::<u32>().is_ok()).map(|(a, _)| a).unwrap_or(l);
        assert!(eve_sde::type_by_name(name).is_some(), "unknown type {name:?}");
    }
}

#[test]
fn item_lists_import_and_export_back() {
    let r = format_import(&json!({"text": "EMP S x100\nNanite Repair Paste x50", "format": "auto"}));
    assert_eq!(r["kind"], json!("AdditionsCargo"), "{r}");
    let items = r["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    let cargo: Vec<Value> = items.iter().map(|i| json!({"type_id": i["type_id"], "quantity": i["amount"]})).collect();
    let t = export(&json!({"ship": {"type_id": 587}, "cargo": cargo}), "multibuy");
    assert!(t.contains("EMP S x100") && t.contains("Nanite Repair Paste x50"), "{t}");
    let r = format_import(&json!({"text": "completely random words", "format": "auto"}));
    assert_eq!(err_code(&r), "UNRECOGNIZED_INPUT");
}

// ---------------------------------------------------------------- ship stats (export only, needs engine stats)

#[test]
fn shipstats_export_and_errors() {
    let f = import(VEXOR, "eft");
    let stats = |r: &eve_fit_model::FitRequest| -> Value { serde_json::from_str(&eve_dogma::calc_json(&serde_json::to_string(r).unwrap())).unwrap() };
    let r = format_export(&json!({"fit": f, "name": "Unit Vexor", "format": "shipstats"}), Some(&stats));
    let t = r["text"].as_str().unwrap_or_else(|| panic!("{r}"));
    assert!(t.starts_with("Unit Vexor (Vexor)"), "{t}");
    for k in ["DPS: ", "EHP: ", "Shield: ", "Armor: ", "Hull: ", "Speed: ", "Capacitor: ", "Sensor strength: "] {
        assert!(t.contains(k), "missing {k} in\n{t}");
    }
    // the same with the stats passed as text (callers without the engine)
    let req: eve_fit_model::FitRequest = serde_json::from_value(f.clone()).unwrap();
    let st = eve_dogma::calc_json(&serde_json::to_string(&eve_fit_formats::shipstats_request(&req)).unwrap());
    let r2 = format_export(&json!({"fit": f, "name": "Unit Vexor", "format": "shipstats", "stats_json": st}), None);
    assert_eq!(r2["text"], r["text"]);
    let r3 = format_export(&json!({"fit": f, "format": "shipstats"}), None);
    assert_eq!(err_code(&r3), "NEEDS_STATS");
    let r4 = format_export(&json!({"fit": f, "format": "shipstats", "stats_json": "{oops"}), None);
    assert_eq!(err_code(&r4), "BAD_REQUEST");
}

// ---------------------------------------------------------------- detection, dispatch, RPC

#[test]
fn auto_detection_recognises_every_export() {
    let f = import(VEXOR, "eft");
    for (fmt, want) in [("eft", "eft"), ("dna", "dna"), ("esi", "esi"), ("xml", "xml")] {
        let t = export(&f, fmt);
        assert_eq!(formats::detect(&t, None), Some(want), "{fmt}");
        let g = import(&t, "auto");
        assert_eq!(modules(&g), modules(&f), "auto import of {fmt}");
    }
    assert_eq!(formats::detect("completely random words", None), None);
}

#[test]
fn dispatch_errors() {
    assert_eq!(err_code(&format_import(&json!({"text": "x", "format": "bogus"}))), "UNSUPPORTED_FORMAT");
    assert_eq!(err_code(&format_export(&json!({"fit": {"ship": {"type_id": 626}}, "format": "bogus"}), None)), "UNSUPPORTED_FORMAT");
    assert_eq!(err_code(&format_export(&json!({"fit": [1, 2], "format": "eft"}), None)), "BAD_REQUEST");
    let r = rpc("{not json");
    assert_eq!(r["error"]["code"], json!("BAD_JSON"));
    let r = rpc(r#"{"id":7,"method":"calc","params":{}}"#);
    assert_eq!(r["id"], json!(7));
    assert_eq!(r["result"]["error"]["code"], json!("UNKNOWN_METHOD"));
    let r = rpc(&json!({"id": 1, "method": "eft_parse", "params": {"text": VEXOR}}).to_string());
    assert_eq!(r["result"]["ship"]["type_id"], json!(626));
}
