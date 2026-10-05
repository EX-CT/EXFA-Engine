//! Regression: EFT import (`eft_parse`) gives every module Pyfa's default state (active when it can be activated,
//! online for the online-only kinds such as MJD and cloaks, offline for `/OFFLINE`), so EFT input keeps its weapon
//! DPS. Expected values come from the Pyfa oracle (tests/data/eft_import/pyfa_expected.json).
use serde_json::Value;
use std::path::Path;

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

#[test]
fn eft_import_module_states_and_weapon_dps_match_pyfa() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/data/eft_import");
    let exp: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("pyfa_expected.json")).unwrap()).unwrap();
    let cases = exp["cases"].as_object().unwrap();
    assert!(cases.len() >= 5);
    for (name, want) in cases {
        let text = std::fs::read_to_string(dir.join(format!("{name}.eft"))).unwrap();
        let mut req = exfa_formats::eft::parse(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        let got: Vec<(u64, String)> = req
            .modules
            .iter()
            .map(|m| (m.type_id as u64, serde_json::to_value(m.state).unwrap().as_str().unwrap().to_string()))
            .collect();
        let want_mods: Vec<(u64, String)> =
            want["modules"].as_array().unwrap().iter().map(|m| (m[0].as_u64().unwrap(), m[1].as_str().unwrap().to_string())).collect();
        assert_eq!(got, want_mods, "{name}: module (type_id, state) list differs from Pyfa importEft");
        req.character.skills.default_level = Some(5);
        let out = serde_json::to_value(exfa_core::calc(&req)).unwrap();
        let t = &out["offense"]["total"];
        for k in ["weapon_dps", "weapon_volley"] {
            let (g, w) = (t[k].as_f64().unwrap(), want[k].as_f64().unwrap());
            assert!(close(g, w), "{name}: {k} {g} vs Pyfa {w}");
        }
    }
}
