//! eve-fit-formats — fit text/JSON formats: EFT (+ mutations, EFT config), DNA (+ alt, chat link), ESI fitting JSON,
//! EVE client XML, multibuy, Pyfa "ship stats" text, item lists and format auto-detection.
//!
//! Formats are not part of the engine: this crate turns text into the structured fit input (`eve-fit-model`) and back.
//! It depends on `eve-fit-model` and the static data (`eve-sde`) only, never on the `eve-dogma` engine. The one
//! export that needs computed stats (`shipstats`) takes them from the caller (a callback or `params.stats`).
//!
//! Output layout follows the formats as Pyfa writes them (verified against Pyfa-generated cases in eve-dogma-bench
//! `formats-suite`; no Pyfa code is used here).
pub mod eft;
pub mod fitting;
pub mod formats;

use eve_fit_model::*;
use eve_sde as d;
use serde_json::{json, Value};

/// Slot of a module type from its slot effect (hi/med/lo power, rig, subsystem, service).
pub fn infer_slot(ty: usize) -> Option<Slot> {
    for &x in d::type_effects(ty) {
        match d::EFF_IDS[(x >> 1) as usize] {
            12 => return Some(Slot::High),
            13 => return Some(Slot::Mid),
            11 => return Some(Slot::Low),
            2663 => return Some(Slot::Rig),
            3772 => return Some(Slot::Subsystem),
            6306 => return Some(Slot::Service),
            _ => {}
        }
    }
    None
}

/// Computes FitStats JSON for a request (the engine's `calc`), supplied by callers that link the engine.
pub type StatsFn<'a> = &'a dyn Fn(&FitRequest) -> Value;

/// JSON-RPC methods served by this crate.
pub const METHODS: [&str; 4] = ["eft_parse", "eft_export", "format_import", "format_export"];

fn opt(p: &Value, k: &str, default: bool) -> bool {
    p.get("options").and_then(|o| o.get(k)).and_then(|v| v.as_bool()).unwrap_or(default)
}

/// `eft_parse {text}` -> FitRequest
pub fn eft_parse(p: &Value) -> Value {
    match eft::parse(p.get("text").and_then(|t| t.as_str()).unwrap_or("")) {
        Ok(r) => serde_json::to_value(r).unwrap_or(Value::Null),
        Err(e) => json!({"error": {"code": "EFT_PARSE", "message": e}}),
    }
}

/// `eft_export {fit, name}` -> `{"text"}`
pub fn eft_export(p: &Value) -> Value {
    match serde_json::from_value::<FitRequest>(p.get("fit").cloned().unwrap_or(Value::Null)) {
        Ok(r) => json!({"text": eft::export(&r, p.get("name").and_then(|n| n.as_str()).unwrap_or("EXCT fit"))}),
        Err(e) => json!({"error": {"code": "BAD_REQUEST", "message": e.to_string()}}),
    }
}

/// `format_export {fit, name, format, options, stats?}` -> `{"text"}`; formats eft, dna, esi, xml, multibuy, shipstats.
/// `shipstats` needs engine stats of the fit computed with `options.include_attributes = "all"` and no spool-up
/// (`default_spool` spool_scale 0, no per-module spool): from `stats` (the caller's engine) or `params.stats`.
pub fn format_export(p: &Value, stats: Option<StatsFn>) -> Value {
    let r = match serde_json::from_value::<FitRequest>(p.get("fit").cloned().unwrap_or(Value::Null)) {
        Ok(r) => r,
        Err(e) => return json!({"error": {"code": "BAD_REQUEST", "message": e.to_string()}}),
    };
    let name = p.get("name").and_then(|n| n.as_str()).unwrap_or("EXCT fit");
    let text = match p.get("format").and_then(|f| f.as_str()).unwrap_or("eft") {
        "eft" => eft::export_opts(&r, name, &eft::EftOpts {
            implants: opt(p, "implants", true),
            mutations: opt(p, "mutations", true),
            loaded_charges: opt(p, "loaded_charges", true),
            boosters: opt(p, "boosters", true),
            cargo: opt(p, "cargo", true),
        }),
        "dna" => formats::dna_export(&r, name, opt(p, "formatting", false)),
        "esi" => match formats::esi_export(&r, name, opt(p, "charges", true), opt(p, "implants", true), opt(p, "boosters", true)) {
            Ok(t) => t,
            Err(e) => return json!({"error": {"code": "EXPORT_ERROR", "message": e}}),
        },
        "xml" => formats::xml_export(&[(&r, name)]),
        "multibuy" => formats::multibuy_export(&r, &formats::MultibuyOpts {
            loaded_charges: opt(p, "loaded_charges", true),
            cargo: opt(p, "cargo", true),
            implants: opt(p, "implants", true),
            boosters: opt(p, "boosters", true),
        }),
        "shipstats" => {
            let st: Value = match (stats, p.get("stats")) {
                (Some(f), _) => f(&shipstats_request(&r)),
                (None, Some(s)) => s.clone(),
                (None, None) => {
                    return json!({"error": {"code": "NEEDS_STATS", "message": "shipstats needs engine stats: pass params.stats (calc of the fit with include_attributes=all, no spool-up)"}})
                }
            };
            if st.get("error").is_some() {
                return st;
            }
            formats::shipstats_export(&r, name, &st)
        }
        f => return json!({"error": {"code": "UNSUPPORTED_FORMAT", "message": f}}),
    };
    json!({"text": text})
}

/// The request whose stats the `shipstats` export needs: all attributes, Pyfa's default spool-up (none) for the
/// stats copy instead of the request's spool settings.
pub fn shipstats_request(r: &FitRequest) -> FitRequest {
    let mut r2 = r.clone();
    r2.options.include_attributes = Some("all".into());
    r2.options.default_spool = Some(Spool { kind: SpoolType::SpoolScale, amount: 0.0 });
    for m in r2.modules.iter_mut() {
        m.spool = None;
    }
    r2
}

fn imported_json(f: &formats::Imported) -> Value {
    let mut v = serde_json::to_value(&f.req).unwrap_or(Value::Null);
    if let Value::Object(o) = &mut v {
        o.insert("name".into(), json!(f.name));
        o.insert("notes".into(), json!(f.notes));
    }
    v
}

/// `format_import {text, format, path?}` -> `{"kind", "fits": [FitRequest + name/notes]}`;
/// formats eft, dna, dna_alt, dna_link, esi, eftcfg, xml, auto (Pyfa's detection order).
pub fn format_import(p: &Value) -> Value {
    let text = p.get("text").and_then(|t| t.as_str()).unwrap_or("");
    let path = p.get("path").and_then(|t| t.as_str());
    let mut fmt = p.get("format").and_then(|f| f.as_str()).unwrap_or("auto").to_string();
    if fmt == "auto" {
        match formats::detect(text, path) {
            Some(f) => fmt = f.to_string(),
            None => {
                return match formats::items_import(text) {
                    Some((kind, items)) => json!({"kind": kind, "items": items.iter().map(|(t, n, m)| json!({"type_id": t, "amount": n, "mutation": m})).collect::<Vec<_>>()}),
                    None => json!({"error": {"code": "UNRECOGNIZED_INPUT", "message": "format not recognised"}}),
                }
            }
        }
    }
    let one = |r: Result<formats::Imported, String>, kind: &str| match r {
        Ok(f) => json!({"kind": kind, "fits": [imported_json(&f)]}),
        Err(e) => json!({"error": {"code": "IMPORT_ERROR", "message": e}}),
    };
    match fmt.as_str() {
        "eft" => one(formats::eft_import(text), "EFT"),
        "dna" => one(formats::dna_import(text, None, false), "DNA"),
        "dna_alt" => {
            let s = text.find("DNA:").map(|i| &text[i + 4..]).unwrap_or(text);
            let s = s.split_whitespace().next().unwrap_or("");
            one(formats::dna_import(s, None, true), "DNA")
        }
        "dna_link" => match formats::dna_link(text) {
            Some((dna, name)) => one(formats::dna_import(&dna, Some(&name), false), "DNA"),
            None => json!({"error": {"code": "IMPORT_ERROR", "message": "bad fitting link"}}),
        },
        "esi" => one(formats::esi_import(text), "JSON"),
        "eftcfg" => {
            let stem = path.map(|p| p.rsplit('/').next().unwrap_or(p)).and_then(|f| f.split('.').next()).unwrap_or("");
            match formats::eftcfg_import(text, stem) {
                Ok(v) => json!({"kind": "EFT Config", "fits": v.iter().map(imported_json).collect::<Vec<_>>()}),
                Err(e) => json!({"error": {"code": "IMPORT_ERROR", "message": e}}),
            }
        }
        "xml" => match formats::xml_import(text) {
            Ok(v) => json!({"kind": "XML", "fits": v.iter().map(imported_json).collect::<Vec<_>>()}),
            Err(e) => json!({"error": {"code": "IMPORT_ERROR", "message": e}}),
        },
        f => json!({"error": {"code": "UNSUPPORTED_FORMAT", "message": f}}),
    }
}

/// Result of a formats RPC method, or `None` when `method` is not one of [`METHODS`].
pub fn rpc_method(method: &str, p: &Value, stats: Option<StatsFn>) -> Option<Value> {
    Some(match method {
        "eft_parse" => eft_parse(p),
        "eft_export" => eft_export(p),
        "format_export" => format_export(p, stats),
        "format_import" => format_import(p),
        _ => return None,
    })
}

/// JSONL RPC line `{"id","method","params"}` -> `{"id","result"}` for the formats methods (WASM / tools without the
/// engine); other methods answer UNKNOWN_METHOD.
pub fn rpc(line: &str) -> Value {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"id": null, "error": {"code": "BAD_JSON", "message": e.to_string()}}),
    };
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let p = v.get("params").cloned().unwrap_or(Value::Null);
    let m = v.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let result = rpc_method(m, &p, None).unwrap_or_else(|| json!({"error": {"code": "UNKNOWN_METHOD", "message": m}}));
    json!({"id": id, "result": result})
}
