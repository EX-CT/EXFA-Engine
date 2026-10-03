//! eve-dogma — EVE Online dogma engine (EXCT Rust mainline, formerly variant F). Engine core + stats + graphs + RPC.
//!
//! Input is the structured fit (`eve-fit-model` FitRequest) only. Fit text formats (EFT, DNA, ESI, XML, …) live in
//! the separate `eve-fit-formats` crate; the `eve-fit` CLI links both.
//!
//! The SDE dataset is compiled into this crate by `build.rs`: static tables plus generated Rust code for every
//! effect's modifiers. `calc(request) -> stats` is pure: no I/O, no clocks, no global state, no data loading.
pub use eve_capsim as capsim;
pub mod data;
pub mod graphs;
pub mod engine;
pub mod j;
pub mod price;
pub mod prov;
pub mod batch;
pub mod request;
pub mod stats;

use serde_json::{json, Value};

pub use request::FitRequest;

/// Compute full fit statistics for one request (plus the `price` block when the request has price inputs,
/// `options.price`, or a market snapshot is loaded; docs/23 §6.1).
pub fn calc(req: &FitRequest) -> j::J {
    calc_priced(req, &[], &[], None, false)
}

/// `calc` with batch price layers (docs/23 §5.2): `variant` = L1 overrides, `batch_wide` = L2 overrides listed after
/// the fit's own, `batch_prices` = batch-wide injected table (the fit's own wins per type), `force` = emit the block.
pub fn calc_priced(req: &FitRequest, variant: &[request::PriceOverride], batch_wide: &[request::PriceOverride], batch_prices: Option<&request::Prices>, force: bool) -> j::J {
    let want = force || !variant.is_empty() || !batch_wide.is_empty() || batch_prices.is_some() || price::wanted(req);
    let ctx = if want {
        match price::Ctx::from_request(req, variant, batch_wide, batch_prices) {
            Ok(c) => Some(c),
            Err(e) => return jv!({"error": {"code": e.code, "message": e.message, "path": "price_overrides"}}),
        }
    } else {
        None
    };
    match engine::Fit::build(req) {
        Ok(fit) => {
            let mut out = fit.compute_stats(req);
            if let j::J::O(o) = &mut out {
                let (use_market, table) = price::request_state(req, batch_prices);
                o.push(("provenance".into(), prov::provenance_j(use_market, table)));
                if let Some(c) = ctx {
                    o.push(("price".into(), price::block(req, &c)));
                    if let Some(w) = c.warning() {
                        o.push(("warnings".into(), j::J::A(vec![j::J::Str(w)])));
                    }
                }
            }
            out
        }
        Err(e) => jv!({"error": {"code": e.code, "message": e.message, "path": e.path}}),
    }
}

/// JSON string in, JSON string out (the contract's single-request form).
pub fn calc_json(request_json: &str) -> String {
    // direct typed parse; syntax/EOF errors are BAD_JSON, shape errors BAD_REQUEST
    let v = match serde_json::from_str::<FitRequest>(request_json) {
        Ok(req) if req.options.full_precision => return calc(&req).to_json_string_full(),
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
/// Contract search kind of a type (None = not searchable).
fn search_kind(ix: usize) -> Option<&'static str> {
    let t = data::ty(ix);
    Some(match t.category {
        6 => "ship",
        7 => "module",
        8 => "charge",
        18 => "drone",
        87 => "fighter",
        20 => {
            if data::group_name(t.group).map(|g| g.contains("Booster")).unwrap_or(false) { "booster" } else { "implant" }
        }
        32 => "subsystem",
        16 => "skill",
        _ => return None,
    })
}

/// Interim search (contract 1.4.1): published ship/module/charge/drone/fighter/implant/booster/subsystem/skill types,
/// case-insensitive on the English or Chinese name; exact > prefix > substring, ties by typeID ascending.
pub fn search_kinds(q: &str, limit: usize, kinds: Option<&[String]>) -> Value {
    let ql = q.trim().to_lowercase();
    if ql.is_empty() {
        return Value::Array(vec![]);
    }
    let level = |n: &str| -> Option<u8> {
        let n = n.to_lowercase();
        if n == ql {
            Some(0)
        } else if n.starts_with(&ql) {
            Some(1)
        } else if n.contains(&ql) {
            Some(2)
        } else {
            None
        }
    };
    let mut hits: Vec<(u8, u32, usize, &'static str)> = Vec::new();
    for ix in 0..data::TYPE_COUNT {
        if !data::type_published(ix) {
            continue;
        }
        let Some(kind) = search_kind(ix) else { continue };
        if let Some(ks) = kinds {
            if !ks.iter().any(|k| k == kind) {
                continue;
            }
        }
        let l = [level(data::type_name(ix)), data::type_name_zh(ix).and_then(level)].into_iter().flatten().min();
        if let Some(l) = l {
            hits.push((l, data::TYPE_IDS[ix], ix, kind));
        }
    }
    hits.sort_unstable_by_key(|h| (h.0, h.1));
    Value::Array(
        hits.into_iter()
            .take(limit)
            .map(|(l, id, ix, kind)| {
                let t = data::ty(ix);
                let m = ["exact", "prefix", "substring"][l as usize];
                json!({"type_id": id, "name": data::type_name(ix), "name_zh": data::type_name_zh(ix), "group": data::group_name(t.group),
                       "category_id": t.category, "kind": kind, "meta_level": data::type_meta_level(ix), "slot": engine::infer_slot(ix),
                       "match": m})
            })
            .collect(),
    )
}

pub fn search(q: &str, limit: usize) -> Value {
    search_kinds(q, limit, None)
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

/// RPC `prices_load` (docs/23 §5.3): set the session market snapshot (L4) from `{"path"}`, `{"snapshot"}`, `{"isk"}`,
/// or clear it with `{"clear": true}`.
fn prices_load(p: &Value) -> Value {
    if p.get("clear").and_then(|c| c.as_bool()) == Some(true) {
        price::set_market(None);
        return json!({"ok": true, "types": 0});
    }
    let m = if let Some(path) = p.get("path").and_then(|x| x.as_str()) {
        price::read_file(path)
    } else if let Some(s) = p.get("snapshot") {
        price::market_from_value(s)
    } else if p.get("isk").is_some() {
        price::market_from_value(p)
    } else {
        return json!({"error": {"code": "BAD_PRICES", "message": "prices_load needs path, snapshot or isk"}});
    };
    match m {
        Ok(m) => {
            let r = json!({"ok": true, "types": m.isk.len(), "snapshot_time": m.time, "price_snapshot_id": m.id, "price_hash": m.hash, "warnings": m.warning.iter().collect::<Vec<_>>()});
            price::set_market(Some(m));
            r
        }
        Err(e) => json!({"error": {"code": e.code, "message": e.message}}),
    }
}

/// Batch (docs/23): JSON in, JSON out.
pub fn batch_json(s: &str) -> String {
    batch::run_json(s)
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
            Ok(r) if r.options.full_precision => calc(&r).to_value_raw(),
            Ok(r) => serde_json::to_value(calc(&r)).unwrap_or(Value::Null),
            Err(e) => json!({"error": {"code": "BAD_REQUEST", "message": e.to_string()}}),
        },
        "batch" | "calc_batch" => batch::run(&p),
        "prices_load" => prices_load(&p),
        "version" => prov::version(),
        "sde_override" => prov::sde_override(&p),
        "graph" => graphs::graph(&p),
        "graph_specs" => graphs::specs_json(),
        "search" => {
            let kinds: Option<Vec<String>> = match p.get("kinds") {
                Some(Value::Array(a)) => Some(a.iter().filter_map(|k| k.as_str().map(|s| s.to_lowercase())).collect()),
                Some(Value::String(s)) => Some(s.split(',').map(|k| k.trim().to_lowercase()).filter(|k| !k.is_empty()).collect()),
                _ => None,
            };
            search_kinds(p.get("query").and_then(|q| q.as_str()).unwrap_or(""), p.get("limit").and_then(|l| l.as_u64()).unwrap_or(20) as usize, kinds.as_deref())
        }
        "type" => type_info(&p.get("id").map(|x| x.to_string().trim_matches('"').to_string()).unwrap_or_default()),
        "meta" => meta(),
        m => json!({"error": {"code": "UNKNOWN_METHOD", "message": m}}),
    };
    json!({"id": id, "result": result})
}
