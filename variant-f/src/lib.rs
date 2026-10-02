//! eve-dogma-f — EVE Online dogma engine, variant F (EXCT).
//!
//! The SDE dataset is compiled into this crate by `build.rs`: static tables plus generated Rust code for every
//! effect's modifiers. `calc(request) -> stats` is pure: no I/O, no clocks, no global state, no data loading.
pub mod capsim;
pub mod data;
pub mod eft;
pub mod engine;
pub mod j;
pub mod request;
pub mod stats;
#[cfg(target_arch = "wasm32")]
pub mod wasm;

use serde_json::{json, Value};

pub use request::FitRequest;

/// Compute full fit statistics for one request.
pub fn calc(req: &FitRequest) -> j::J {
    match engine::Fit::build(req) {
        Ok(fit) => fit.compute_stats(req),
        Err(e) => jv!({"error": {"code": e.code, "message": e.message, "path": e.path}}),
    }
}

/// JSON string in, JSON string out (the contract's single-request form).
pub fn calc_json(request_json: &str) -> String {
    // direct typed parse; syntax/EOF errors are BAD_JSON, shape errors BAD_REQUEST
    let v = match serde_json::from_str::<FitRequest>(request_json) {
        Ok(req) => calc(&req),
        Err(e) => {
            let code = match e.classify() {
                serde_json::error::Category::Data => "BAD_REQUEST",
                _ => "BAD_JSON",
            };
            jv!({"error": {"code": code, "message": e.to_string(), "path": ""}})
        }
    };
    v.to_json_string()
}

/// Dataset / engine info.
pub fn meta() -> Value {
    json!({"engine": concat!("eve-dogma-f ", env!("CARGO_PKG_VERSION")), "schema_version": 1, "sde_build": data::SDE_BUILD,
           "sde_release_date": data::SDE_RELEASE_DATE, "dataset_sha256": data::DATASET_SHA256, "types": data::TYPE_COUNT,
           "effects": data::EFFECT_COUNT, "attributes": data::ATTR_N, "compiled_local_modifiers": data::COMPILED_LOCAL_MODIFIERS})
}

/// Search published types by English (case-insensitive) or Chinese name.
pub fn search(q: &str, limit: usize) -> Value {
    let ql = q.to_lowercase();
    let mut hits: Vec<usize> =
        (0..data::TYPE_COUNT).filter(|&ix| data::type_published(ix) && (data::type_name(ix).to_lowercase().contains(&ql) || data::type_name_zh(ix).map(|z| z.contains(q)).unwrap_or(false)))
            .collect();
    hits.sort_by_key(|&ix| {
        let n = data::type_name(ix);
        (!n.to_lowercase().starts_with(&ql), n.len(), n)
    });
    Value::Array(
        hits.into_iter()
            .take(limit)
            .map(|ix| {
                let t = data::ty(ix);
                json!({"type_id": data::TYPE_IDS[ix], "name": data::type_name(ix), "name_zh": data::type_name_zh(ix), "group": data::group_name(t.group),
                       "category_id": t.category, "meta_level": data::type_meta_level(ix), "slot": engine::infer_slot(ix)})
            })
            .collect(),
    )
}

/// Type info with base attributes and effects.
pub fn type_info(key: &str) -> Value {
    let id = key.trim().parse::<u32>().ok().or_else(|| data::type_by_name(key));
    let Some(ix) = id.and_then(data::type_index) else {
        return json!({"error": {"code": "UNKNOWN_TYPE", "message": key}});
    };
    let t = data::ty(ix);
    // mass / capacity / volume / radius are separate type fields in the dataset (folded into the attribute table here)
    let attrs: serde_json::Map<String, Value> = data::type_attr_ids(ix)
        .iter()
        .filter(|&&x| !matches!(x, 4 | 38 | 161 | 162))
        .map(|&x| (data::attr_name(x).map(|s| s.to_string()).unwrap_or(x.to_string()), json!(data::type_attr(ix, x))))
        .collect();
    let effects: Vec<Value> = data::type_effects(ix)
        .iter()
        .map(|&x| {
            let ei = (x >> 1) as usize;
            json!({"id": data::EFF_IDS[ei], "name": data::eff_name(ei), "default": x & 1 != 0})
        })
        .collect();
    json!({"type_id": data::TYPE_IDS[ix], "name": data::type_name(ix), "name_zh": data::type_name_zh(ix), "group": data::group_name(t.group), "group_id": t.group,
           "category_id": t.category, "published": data::type_published(ix), "mass": data::type_mass(ix), "volume": data::type_volume(ix),
           "capacity": data::type_capacity(ix), "slot": engine::infer_slot(ix), "attributes": attrs, "effects": effects})
}

/// JSONL RPC line: {"id","method","params"} -> {"id","result"}
pub fn rpc(line: &str) -> Value {
    let v: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => return json!({"id": null, "error": {"code": "BAD_JSON", "message": e.to_string()}}),
    };
    let id = v.get("id").cloned().unwrap_or(Value::Null);
    let p = v.get("params").cloned().unwrap_or(Value::Null);
    let result = match v.get("method").and_then(|m| m.as_str()).unwrap_or("calc") {
        "calc" => match serde_json::from_value::<FitRequest>(p) {
            Ok(r) => serde_json::to_value(calc(&r)).unwrap_or(Value::Null),
            Err(e) => json!({"error": {"code": "BAD_REQUEST", "message": e.to_string()}}),
        },
        "eft_parse" => match eft::parse(p.get("text").and_then(|t| t.as_str()).unwrap_or("")) {
            Ok(r) => serde_json::to_value(r).unwrap_or(Value::Null),
            Err(e) => json!({"error": {"code": "EFT_PARSE", "message": e}}),
        },
        "eft_export" => match serde_json::from_value::<FitRequest>(p.get("fit").cloned().unwrap_or(Value::Null)) {
            Ok(r) => json!({"text": eft::export(&r, p.get("name").and_then(|n| n.as_str()).unwrap_or("EXCT fit"))}),
            Err(e) => json!({"error": {"code": "BAD_REQUEST", "message": e.to_string()}}),
        },
        "search" => search(p.get("query").and_then(|q| q.as_str()).unwrap_or(""), p.get("limit").and_then(|l| l.as_u64()).unwrap_or(20) as usize),
        "type" => type_info(&p.get("id").map(|x| x.to_string().trim_matches('"').to_string()).unwrap_or_default()),
        "meta" => meta(),
        m => json!({"error": {"code": "UNKNOWN_METHOD", "message": m}}),
    };
    json!({"id": id, "result": result})
}
