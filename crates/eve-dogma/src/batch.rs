//! Batch compute (docs/23): many fits, base + variants (JSON Patch), cartesian product and sweeps, with `fields`,
//! deltas, filter, sort_by, top_n, per-fit errors and per-variant price overrides. Every result's `stats` is the
//! one-by-one `calc` output of the expanded FitRequest (projected by `fields`), so a batch equals N single calls.
use crate::request::{FitRequest, PriceOverride, Prices};
use serde_json::{json, Map, Value};

pub const DEFAULT_MAX_COMBINATIONS: u64 = 2000;
pub const HARD_MAX_COMBINATIONS: u64 = 100_000;

const KEYS: &[&str] = &[
    "batch_version", "fits", "base", "variants", "product", "sweep", "max_combinations", "price_overrides", "prices", "price", "fields", "deltas",
    "delta_ref", "filter", "sort_by", "sort", "top_n", "limit", "include_errors", "include_base",
];

fn err(code: &str, message: impl Into<String>) -> Value {
    json!({"error": {"code": code, "message": message.into()}})
}

// ------------------------------------------------------------------ JSON Patch (add / remove / replace, + swap_type)
fn parse_ptr(p: &str) -> Result<Vec<String>, String> {
    if p.is_empty() {
        return Ok(vec![]);
    }
    if !p.starts_with('/') {
        return Err(format!("bad JSON pointer {p:?}"));
    }
    Ok(p[1..].split('/').map(|s| s.replace("~1", "/").replace("~0", "~")).collect())
}

fn apply_op(doc: &mut Value, op: &Value) -> Result<(), String> {
    let o = op.get("op").and_then(|x| x.as_str()).ok_or("patch op without \"op\"")?;
    if o == "swap_type" {
        let from = op.get("from").and_then(|x| x.as_u64()).ok_or("swap_type needs from")?;
        let to = op.get("to").and_then(|x| x.as_u64()).ok_or("swap_type needs to")?;
        if let Some(ms) = doc.get_mut("modules").and_then(|m| m.as_array_mut()) {
            for m in ms {
                if m.get("type_id").and_then(|t| t.as_u64()) == Some(from) {
                    m["type_id"] = json!(to);
                }
            }
        }
        return Ok(());
    }
    let path = op.get("path").and_then(|x| x.as_str()).ok_or("patch op without \"path\"")?;
    let parts = parse_ptr(path)?;
    let Some((last, head)) = parts.split_last() else { return Err(format!("{path}: cannot patch the document root")) };
    let mut cur = doc;
    for p in head {
        cur = match cur {
            Value::Array(a) => p.parse::<usize>().ok().and_then(|i| a.get_mut(i)).ok_or_else(|| format!("{path}: no index {p}"))?,
            Value::Object(m) => m.get_mut(p).ok_or_else(|| format!("{path}: no key {p}"))?,
            _ => return Err(format!("{path}: not a container at {p}")),
        };
    }
    let value = || op.get("value").cloned().ok_or_else(|| format!("{path}: {o} needs a value"));
    match cur {
        Value::Array(a) => {
            let idx = if last == "-" { a.len() } else { last.parse::<usize>().map_err(|_| format!("{path}: bad index {last}"))? };
            match o {
                "add" if idx <= a.len() => a.insert(idx, value()?),
                "remove" if idx < a.len() => {
                    a.remove(idx);
                }
                "replace" if idx < a.len() => a[idx] = value()?,
                "add" | "remove" | "replace" => return Err(format!("{path}: no such index")),
                _ => return Err(format!("unsupported patch op {o:?}")),
            }
        }
        Value::Object(m) => match o {
            "add" => {
                m.insert(last.clone(), value()?);
            }
            "replace" if m.contains_key(last) => {
                m.insert(last.clone(), value()?);
            }
            "remove" if m.contains_key(last) => {
                m.remove(last);
            }
            "replace" | "remove" => return Err(format!("{path}: no such key")),
            _ => return Err(format!("unsupported patch op {o:?}")),
        },
        _ => return Err(format!("{path}: parent is not a container")),
    }
    Ok(())
}

pub fn apply_patch(base: &Value, ops: &Value) -> Result<Value, String> {
    let mut doc = base.clone();
    match ops {
        Value::Null => {}
        Value::Array(a) => {
            for op in a {
                apply_op(&mut doc, op)?;
            }
        }
        _ => return Err("patch must be a list of ops".into()),
    }
    Ok(doc)
}

/// Patch the base as given; if a path does not exist there, retry on the normalized base (the FitRequest with every
/// default present, e.g. `/character/skills/levels`), so patches need not spell out defaults. Same stats either way.
fn patch_fit(base: &Value, norm: &Option<Value>, ops: &Value) -> Result<Value, String> {
    match apply_patch(base, ops) {
        Ok(v) => Ok(v),
        Err(e) => match norm {
            Some(n) => apply_patch(n, ops).map_err(|_| e),
            None => Err(e),
        },
    }
}

// ------------------------------------------------------------------ expansion
struct Item {
    id: String,
    label: String,
    fit: Result<Value, String>,
    l1: Value, // list of price overrides (JSON)
}

fn jstr(v: &Value) -> String {
    serde_json::to_string(v).unwrap_or_default()
}

/// Number of values of a sweep without generating them.
fn sweep_count(sw: &Value) -> Result<u64, String> {
    if let Some(v) = sw.get("values") {
        return v.as_array().map(|a| a.len() as u64).ok_or_else(|| "sweep.values must be a list".to_string());
    }
    let f = |k: &str| sw.get(k).and_then(|x| x.as_f64()).ok_or_else(|| format!("sweep needs values or numeric from/to/step ({k})"));
    let (from, to, step) = (f("from")?, f("to")?, f("step")?);
    if !(step > 0.0) || !from.is_finite() || !to.is_finite() {
        return Err("sweep.step must be > 0".into());
    }
    if to + 1e-9 * step < from {
        return Ok(0);
    }
    let mut n = ((to - from) / step).floor().max(0.0) as u64 + 1;
    while from + n as f64 * step <= to + 1e-9 * step.abs() {
        n += 1;
    }
    while n > 0 && from + (n - 1) as f64 * step > to + 1e-9 * step.abs() {
        n -= 1;
    }
    Ok(n)
}

fn axis_count(ax: &Value) -> Result<u64, String> {
    match ax.get("sweep") {
        Some(sw) => sweep_count(sw),
        None => ax.get("options").and_then(|o| o.as_array()).map(|a| a.len() as u64).ok_or_else(|| "axis needs options or sweep".to_string()),
    }
}

fn sweep_values(sw: &Value) -> Result<Vec<Value>, String> {
    if let Some(v) = sw.get("values") {
        return v.as_array().cloned().ok_or_else(|| "sweep.values must be a list".to_string());
    }
    let f = |k: &str| sw.get(k).and_then(|x| x.as_f64()).ok_or_else(|| format!("sweep needs values or numeric from/to/step ({k})"));
    let (from, to, step) = (f("from")?, f("to")?, f("step")?);
    if !(step > 0.0) || !from.is_finite() || !to.is_finite() {
        return Err("sweep.step must be > 0".into());
    }
    let ints = [sw.get("from"), sw.get("to"), sw.get("step")].iter().all(|x| x.map(|x| x.is_i64() || x.is_u64()).unwrap_or(false));
    let mut out = Vec::new();
    let mut k = 0u64;
    loop {
        let v = from + k as f64 * step;
        if v > to + 1e-9 * step.abs() {
            break;
        }
        out.push(if ints { json!(v as i64) } else { json!(v) });
        k += 1;
    }
    Ok(out)
}

struct Opt {
    id: String,
    label: String,
    patch: Value,
    po: Vec<Value>,
}

fn axis_options(ax: &Value) -> Result<Vec<Opt>, String> {
    if let Some(sw) = ax.get("sweep") {
        let path = sw.get("path").and_then(|p| p.as_str()).ok_or("sweep needs path")?;
        return Ok(sweep_values(sw)?
            .into_iter()
            .map(|v| {
                let s = format!("{path}={}", jstr(&v));
                Opt { id: s.clone(), label: s, patch: json!([{"op": "add", "path": path, "value": v}]), po: vec![] }
            })
            .collect());
    }
    let opts = ax.get("options").and_then(|o| o.as_array()).ok_or("axis needs options or sweep")?;
    Ok(opts
        .iter()
        .enumerate()
        .map(|(n, o)| {
            let id = o.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or(n.to_string());
            let label = o.get("label").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or(id.clone());
            let po = o.get("price_overrides").and_then(|p| p.as_array()).cloned().unwrap_or_default();
            Opt { id, label, patch: o.get("patch").cloned().unwrap_or(json!([])), po }
        })
        .collect())
}

fn expand(req: &Map<String, Value>, cap: u64) -> Result<(&'static str, Vec<Item>), Value> {
    let sources: Vec<&str> = ["fits", "variants", "product", "sweep"].into_iter().filter(|k| req.contains_key(*k)).collect();
    if sources.len() != 1 {
        return Err(err("BATCH_BAD_REQUEST", "exactly one of fits, variants, product, sweep is required"));
    }
    let too_large = |n: u64| err_obj("BATCH_TOO_LARGE", format!("{n} combinations exceed the limit {cap}"), json!({"count": n, "limit": cap}));
    let s = sources[0];
    if s == "fits" {
        if req.contains_key("base") {
            return Err(err("BATCH_BAD_REQUEST", "fits cannot be combined with base"));
        }
        let fits = req["fits"].as_array().ok_or_else(|| err("BATCH_BAD_REQUEST", "fits must be a list"))?;
        if fits.len() as u64 > cap {
            return Err(too_large(fits.len() as u64));
        }
        let items = fits
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let id = it.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or(i.to_string());
                let label = it.get("label").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or(id.clone());
                let fit = it.get("fit").cloned().ok_or_else(|| "fits entry without fit".to_string());
                Item { id, label, fit, l1: it.get("price_overrides").cloned().unwrap_or(json!([])) }
            })
            .collect();
        return Ok(("fits", items));
    }
    let base = req.get("base").ok_or_else(|| err("BATCH_BAD_REQUEST", format!("{s} needs base")))?;
    let norm: Option<Value> = serde_json::from_value::<FitRequest>(base.clone()).ok().and_then(|f| serde_json::to_value(f).ok());
    if s == "variants" {
        let vs = req["variants"].as_array().ok_or_else(|| err("BATCH_BAD_REQUEST", "variants must be a list"))?;
        if vs.len() as u64 > cap {
            return Err(too_large(vs.len() as u64));
        }
        let items = vs
            .iter()
            .enumerate()
            .map(|(k, v)| {
                let id = v.get("id").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or(format!("v{}", k + 1));
                let label = v.get("label").and_then(|x| x.as_str()).map(|s| s.to_string()).unwrap_or(id.clone());
                Item { id, label, fit: patch_fit(base, &norm, v.get("patch").unwrap_or(&Value::Null)), l1: v.get("price_overrides").cloned().unwrap_or(json!([])) }
            })
            .collect();
        return Ok(("variants", items));
    }
    let axes: Vec<Value> = if s == "product" {
        req["product"].get("axes").and_then(|a| a.as_array()).cloned().ok_or_else(|| err("BATCH_BAD_REQUEST", "product needs axes"))?
    } else {
        vec![json!({"name": "sweep", "sweep": req["sweep"]})]
    };
    let mut n: u64 = 1;
    for ax in &axes {
        n = n.saturating_mul(axis_count(ax).map_err(|m| err("BATCH_BAD_REQUEST", m))?);
    }
    if n > cap {
        return Err(too_large(n));
    }
    let mut all = Vec::new();
    for ax in &axes {
        all.push(axis_options(ax).map_err(|m| err("BATCH_BAD_REQUEST", m))?);
    }
    let mut combos: Vec<Vec<usize>> = vec![vec![]];
    for o in &all {
        combos = combos.into_iter().flat_map(|c| (0..o.len()).map(move |k| [c.clone(), vec![k]].concat())).collect();
    }
    let items = combos
        .into_iter()
        .map(|c| {
            let opts: Vec<&Opt> = c.iter().enumerate().map(|(a, &k)| &all[a][k]).collect();
            let id = opts.iter().map(|o| o.id.as_str()).collect::<Vec<_>>().join("|");
            let label = opts.iter().map(|o| o.label.as_str()).collect::<Vec<_>>().join(" × ");
            let patch: Vec<Value> = opts.iter().flat_map(|o| o.patch.as_array().cloned().unwrap_or_default()).collect();
            let l1: Vec<Value> = opts.iter().flat_map(|o| o.po.clone()).collect();
            Item { id, label, fit: patch_fit(base, &norm, &Value::Array(patch)), l1: Value::Array(l1) }
        })
        .collect();
    Ok((if s == "product" { "product" } else { "sweep" }, items))
}

fn err_obj(code: &str, message: String, extra: Value) -> Value {
    let mut e = json!({"code": code, "message": message});
    if let (Value::Object(m), Value::Object(x)) = (&mut e, extra) {
        m.extend(x);
    }
    json!({ "error": e })
}

// ------------------------------------------------------------------ compute
struct Shared {
    batch_po: Vec<PriceOverride>,
    batch_prices: Option<Prices>,
    force_price: bool,
}

fn compute(fit: &Value, l1: &Value, sh: &Shared) -> Value {
    let req: FitRequest = match serde_json::from_value(fit.clone()) {
        Ok(r) => r,
        Err(e) => return err("BAD_REQUEST", e.to_string()),
    };
    let l1: Vec<PriceOverride> = match serde_json::from_value(l1.clone()) {
        Ok(v) => v,
        Err(e) => return err("BAD_PRICE_OVERRIDE", e.to_string()),
    };
    let out = crate::calc_priced(&req, &l1, &sh.batch_po, sh.batch_prices.as_ref(), sh.force_price);
    if req.options.full_precision {
        out.to_value_raw()
    } else {
        serde_json::to_value(out).unwrap_or(Value::Null)
    }
}

fn compute_all(items: &[Item], sh: &Shared) -> Vec<Value> {
    let one = |it: &Item| match &it.fit {
        Ok(f) => compute(f, &it.l1, sh),
        Err(m) => err("PATCH_FAILED", m.clone()),
    };
    let n = if cfg!(target_arch = "wasm32") { 1 } else { std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1).min(16) };
    if n <= 1 || items.len() < 2 {
        return items.iter().map(one).collect();
    }
    let chunk = items.len().div_ceil(n);
    std::thread::scope(|s| {
        let hs: Vec<_> = items.chunks(chunk).map(|c| s.spawn(move || c.iter().map(one).collect::<Vec<_>>())).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    })
}

// ------------------------------------------------------------------ projection, deltas, filter, sort
/// Dotted path (numeric segments index lists) or JSON Pointer.
pub fn get_path<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    if path.starts_with('/') {
        return v.pointer(path);
    }
    let mut cur = v;
    for p in path.split('.') {
        cur = match cur {
            Value::Array(a) => {
                let i: i64 = p.parse().ok()?;
                let i = if i < 0 { a.len() as i64 + i } else { i };
                a.get(usize::try_from(i).ok()?)?
            }
            Value::Object(m) => m.get(p)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(|x| if x.is_number() { x.as_f64() } else { None })
}
fn cmpv(v: Option<&Value>) -> Option<f64> {
    match v {
        Some(Value::Bool(b)) => Some(*b as u8 as f64),
        x => num(x),
    }
}
fn round6(x: f64) -> f64 {
    (x * 1e6).round() / 1e6
}

struct Row {
    index: usize,
    out: Value,
    obj: Map<String, Value>,
    delta: Option<Map<String, Value>>,
    delta_pct: Option<Map<String, Value>>,
}

impl Row {
    fn is_err(&self) -> bool {
        self.out.get("error").is_some()
    }
    fn key(&self, field: &str, on: &str, fields: bool) -> Option<f64> {
        if self.is_err() {
            return None;
        }
        match on {
            "delta" => cmpv(self.delta.as_ref()?.get(field)),
            "delta_pct" => cmpv(self.delta_pct.as_ref()?.get(field)),
            _ => {
                if fields {
                    cmpv(self.obj.get("stats").and_then(|s| s.get(field)))
                } else {
                    cmpv(get_path(&self.out, field))
                }
            }
        }
    }
}

fn filter_ok(r: &Row, f: &Value, fields: bool) -> Result<bool, String> {
    let field = f.get("field").and_then(|x| x.as_str()).ok_or("filter needs field")?;
    let on = f.get("on").and_then(|x| x.as_str()).unwrap_or("value");
    let op = f.get("op").and_then(|x| x.as_str()).ok_or("filter needs op")?;
    if r.is_err() {
        return Ok(false);
    }
    if op == "not_null" {
        let v = if on == "value" {
            if fields { r.obj.get("stats").and_then(|s| s.get(field)).cloned() } else { get_path(&r.out, field).cloned() }
        } else {
            r.key(field, on, fields).map(|x| json!(x))
        };
        return Ok(v.map(|v| !v.is_null()).unwrap_or(false));
    }
    let Some(x) = r.key(field, on, fields) else { return Ok(false) };
    if op == "in" {
        let list = f.get("value").and_then(|v| v.as_array()).ok_or("filter op in needs a list value")?;
        return Ok(list.iter().any(|y| cmpv(Some(y)) == Some(x)));
    }
    let y = cmpv(f.get("value")).ok_or("filter value must be a number or boolean")?;
    Ok(match op {
        "<" => x < y,
        "<=" => x <= y,
        ">" => x > y,
        ">=" => x >= y,
        "==" => x == y,
        "!=" => x != y,
        _ => return Err(format!("unknown filter op {op:?}")),
    })
}

/// Run one BatchRequest (JSON value) -> BatchResponse (JSON value). Whole-request errors return {"error": {...}}.
pub fn run(req: &Value) -> Value {
    let Some(r) = req.as_object() else { return err("BATCH_BAD_REQUEST", "BatchRequest must be an object") };
    if let Some(k) = r.keys().find(|k| !KEYS.contains(&k.as_str())) {
        return err("BATCH_BAD_REQUEST", format!("unknown key {k:?}"));
    }
    match r.get("batch_version") {
        None => {}
        Some(v) if v.as_u64() == Some(1) => {}
        Some(v) => return err("BATCH_BAD_REQUEST", format!("unsupported batch_version {v}")),
    }
    let cap = r.get("max_combinations").and_then(|x| x.as_u64()).unwrap_or(DEFAULT_MAX_COMBINATIONS).min(HARD_MAX_COMBINATIONS);
    let batch_po: Vec<PriceOverride> = match r.get("price_overrides").map(|v| serde_json::from_value(v.clone())) {
        None => vec![],
        Some(Ok(v)) => v,
        Some(Err(e)) => return err("BAD_PRICE_OVERRIDE", e.to_string()),
    };
    if let Err(e) = crate::price::validate(&batch_po) {
        return err(e.code, e.message);
    }
    let batch_prices: Option<Prices> = match r.get("prices").map(|v| serde_json::from_value(v.clone())) {
        None => None,
        Some(Ok(v)) => Some(v),
        Some(Err(e)) => return err("BAD_PRICES", e.to_string()),
    };
    let fields: Option<Vec<String>> = match r.get("fields") {
        None | Some(Value::Null) => None,
        Some(Value::Array(a)) if a.iter().all(|x| x.is_string()) => Some(a.iter().map(|x| x.as_str().unwrap().to_string()).collect()),
        _ => return err("BATCH_BAD_REQUEST", "fields must be a list of strings"),
    };
    let filters: Vec<Value> = r.get("filter").and_then(|f| f.as_array()).cloned().unwrap_or_default();
    let sorts: Vec<Value> = r.get("sort_by").or_else(|| r.get("sort")).and_then(|f| f.as_array()).cloned().unwrap_or_default();
    let top_n = r.get("top_n").or_else(|| r.get("limit")).and_then(|x| x.as_u64());
    let deltas = r.get("deltas").and_then(|x| x.as_bool()).unwrap_or(false);
    let want_price = r.get("price").and_then(|x| x.as_bool()).unwrap_or(false);
    let refers_price = |s: &str| s.starts_with("price.") || s == "price" || s.starts_with("/price");
    let force_price = want_price
        || fields.iter().flatten().any(|f| refers_price(f))
        || filters.iter().chain(sorts.iter()).any(|f| f.get("field").and_then(|x| x.as_str()).map(refers_price).unwrap_or(false));
    if deltas && fields.is_none() {
        return err("BATCH_BAD_REQUEST", "deltas needs fields");
    }
    let (form, items) = match expand(r, cap) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let delta_ref = r.get("delta_ref").and_then(|x| x.as_str());
    if deltas && form == "fits" && delta_ref.map(|d| !items.iter().any(|i| i.id == d)).unwrap_or(true) {
        return err("BATCH_BAD_REQUEST", "deltas on fits needs delta_ref naming one fit id");
    }
    // every price override list is input validation: a bad one fails the whole batch (docs/23 §5.1)
    for (k, it) in items.iter().enumerate() {
        let l: Result<Vec<PriceOverride>, _> = serde_json::from_value(it.l1.clone());
        match l {
            Ok(l) => {
                if let Err(e) = crate::price::validate(&l) {
                    return err(e.code, format!("result {k} ({}): {}", it.id, e.message));
                }
            }
            Err(e) => return err("BAD_PRICE_OVERRIDE", format!("result {k} ({}): {e}", it.id)),
        }
        if let Ok(f) = &it.fit {
            if let Some(po) = f.get("price_overrides") {
                match serde_json::from_value::<Vec<PriceOverride>>(po.clone()) {
                    Ok(l) => {
                        if let Err(e) = crate::price::validate(&l) {
                            return err(e.code, format!("result {k} ({}): {}", it.id, e.message));
                        }
                    }
                    Err(e) => return err("BAD_PRICE_OVERRIDE", format!("result {k} ({}): {e}", it.id)),
                }
            }
        }
    }
    let sh_prices = batch_prices.clone();
    let sh = Shared { batch_po, batch_prices, force_price };
    let outs = compute_all(&items, &sh);
    let include_base = r.get("include_base").and_then(|x| x.as_bool()).unwrap_or(false);
    let base_out = if form != "fits" && (deltas || include_base) { Some(compute(&r["base"], &json!([]), &sh)) } else { None };
    let ref_out: Option<Value> = if form == "fits" { delta_ref.and_then(|d| items.iter().position(|i| i.id == d)).map(|k| outs[k].clone()) } else { base_out.clone() };

    let mut warnings: Vec<Value> = Vec::new();
    let project = |o: &Value| -> Value {
        match &fields {
            None => o.clone(),
            Some(fs) => Value::Object(fs.iter().map(|f| (f.clone(), get_path(o, f).cloned().unwrap_or(Value::Null))).collect()),
        }
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut n_err = 0;
    for (k, (it, out)) in items.iter().zip(outs).enumerate() {
        let mut obj = Map::new();
        obj.insert("index".into(), json!(k));
        obj.insert("id".into(), json!(it.id));
        obj.insert("label".into(), json!(it.label));
        let (mut delta, mut delta_pct) = (None, None);
        if let Some(e) = out.get("error") {
            n_err += 1;
            obj.insert("error".into(), e.clone());
        } else {
            obj.insert("stats".into(), project(&out));
            if let Some(p) = out.get("provenance") {
                obj.insert("provenance".into(), p.clone());
            }
            if want_price {
                if let Some(p) = out.get("price") {
                    obj.insert("price".into(), p.clone());
                }
            }
            if deltas {
                let rf = ref_out.as_ref().filter(|x| x.get("error").is_none());
                let (mut d, mut dp) = (Map::new(), Map::new());
                for f in fields.iter().flatten() {
                    let a = num(get_path(&out, f));
                    let b = rf.and_then(|x| num(get_path(x, f)));
                    let dv = match (a, b) {
                        (Some(a), Some(b)) => Some(round6(round6(a) - round6(b))),
                        _ => None,
                    };
                    let pv = match (dv, b) {
                        (Some(dv), Some(b)) if b != 0.0 => Some(round6(dv / b.abs() * 100.0)),
                        _ => None,
                    };
                    d.insert(f.clone(), dv.map(|x| json!(x)).unwrap_or(Value::Null));
                    dp.insert(f.clone(), pv.map(|x| json!(x)).unwrap_or(Value::Null));
                }
                obj.insert("delta".into(), Value::Object(d.clone()));
                obj.insert("delta_pct".into(), Value::Object(dp.clone()));
                delta = Some(d);
                delta_pct = Some(dp);
            }
        }
        rows.push(Row { index: k, out, obj, delta, delta_pct });
    }
    if let Some(fs) = &fields {
        for f in fs {
            if !rows.iter().filter(|r| !r.is_err()).any(|r| get_path(&r.out, f).is_some()) && rows.iter().any(|r| !r.is_err()) {
                warnings.push(json!(format!("field {f:?} not found")));
            }
        }
    }
    let total = rows.len();
    let has_fields = fields.is_some();
    // filter
    if !filters.is_empty() {
        let mut kept = Vec::new();
        for row in rows {
            let mut ok = true;
            for f in &filters {
                match filter_ok(&row, f, has_fields) {
                    Ok(true) => {}
                    Ok(false) => {
                        ok = false;
                        break;
                    }
                    Err(m) => return err("BATCH_BAD_REQUEST", m),
                }
            }
            if ok {
                kept.push(row);
            }
        }
        rows = kept;
    } else if !r.get("include_errors").and_then(|x| x.as_bool()).unwrap_or(true) {
        rows.retain(|r| !r.is_err());
    }
    let matched = rows.len();
    // sort (stable, multi-key; nulls / errors last)
    let mut specs = Vec::new();
    for s in &sorts {
        let Some(field) = s.get("field").and_then(|x| x.as_str()) else { return err("BATCH_BAD_REQUEST", "sort_by needs field") };
        let desc = match s.get("order").and_then(|x| x.as_str()).unwrap_or("asc") {
            "asc" => false,
            "desc" => true,
            o => return err("BATCH_BAD_REQUEST", format!("unknown sort order {o:?}")),
        };
        specs.push((field.to_string(), s.get("on").and_then(|x| x.as_str()).unwrap_or("value").to_string(), desc));
    }
    if !specs.is_empty() {
        let keys: Vec<Vec<Option<f64>>> = rows.iter().map(|r| specs.iter().map(|(f, on, _)| r.key(f, on, has_fields)).collect()).collect();
        let mut idx: Vec<usize> = (0..rows.len()).collect();
        idx.sort_by(|&a, &b| {
            for (s, (_, _, desc)) in specs.iter().enumerate() {
                let o = match (keys[a][s], keys[b][s]) {
                    (None, None) => std::cmp::Ordering::Equal,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (Some(x), Some(y)) => {
                        let o = x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal);
                        if *desc { o.reverse() } else { o }
                    }
                };
                if o != std::cmp::Ordering::Equal {
                    return o;
                }
            }
            std::cmp::Ordering::Equal
        });
        let mut slots: Vec<Option<Row>> = rows.into_iter().map(Some).collect();
        rows = idx.into_iter().map(|i| slots[i].take().unwrap()).collect();
    }
    if let Some(n) = top_n {
        rows.truncate(n as usize);
    }
    let mut resp = Map::new();
    resp.insert("batch_version".into(), json!(1));
    resp.insert("form".into(), json!(form));
    resp.insert("total".into(), json!(total));
    resp.insert("computed".into(), json!(total));
    resp.insert("errors".into(), json!(n_err));
    resp.insert("matched".into(), json!(matched));
    if let Some(b) = &base_out {
        let mut bo = Map::new();
        bo.insert("id".into(), json!("base"));
        bo.insert("label".into(), json!("base"));
        if let Some(e) = b.get("error") {
            bo.insert("error".into(), e.clone());
        } else {
            bo.insert("stats".into(), project(b));
            if want_price {
                if let Some(p) = b.get("price") {
                    bo.insert("price".into(), p.clone());
                }
            }
        }
        resp.insert("base".into(), Value::Object(bo));
    }
    let _ = rows.iter().map(|r| r.index).count();
    resp.insert("results".into(), Value::Array(rows.into_iter().map(|r| Value::Object(r.obj)).collect()));
    resp.insert("warnings".into(), Value::Array(warnings));
    let (mut use_market, mut table) = (true, false);
    if let Some(p) = &sh_prices {
        table = !p.isk.is_empty();
        if p.mode.as_deref() == Some("replace") {
            use_market = false;
        }
        if let Some(u) = p.use_snapshot {
            use_market = u;
        }
    }
    resp.insert("provenance".into(), crate::prov::provenance_value(use_market, table));
    Value::Object(resp)
}

/// JSON string in, JSON string out.
pub fn run_json(s: &str) -> String {
    let v = match serde_json::from_str::<Value>(s) {
        Ok(v) => run(&v),
        Err(e) => err("BAD_JSON", e.to_string()),
    };
    serde_json::to_string(&v).unwrap_or_default()
}
