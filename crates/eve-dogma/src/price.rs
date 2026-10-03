//! Prices (docs/23 §5–§6): price overrides, injected prices, market snapshot, and the `price` output block.
//!
//! Layers, highest first: L1 variant overrides (batch), L2 request overrides, L3 request `prices.isk`, L4 market
//! snapshot (`--prices FILE` / RPC `prices_load`; later the embedded release snapshot, docs/22). Within an override
//! layer the most specific entry wins: type > market group (deepest first) > group > category; equal specificity →
//! lower id, then first listed. A fixed price stops the chain; a multiplier applies to the price resolved by the next
//! lower layer (so multipliers of several layers multiply). A type with no price in any layer is listed as missing.
//!
//! The only process-wide state is the optional market snapshot set by the CLI / RPC session (`set_market`); `calc`
//! itself stays deterministic for a given request and snapshot.
use crate::data as d;
use crate::j::J;
use crate::request::FitRequest;
use eve_fit_model::PriceOverride;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// A market price table (L4): from `--prices FILE` / `prices_load`, labelled `injected`, or the embedded release
/// snapshot (docs/22), labelled `snapshot`.
#[derive(Debug, Clone, Default)]
pub struct Market {
    pub isk: BTreeMap<u32, f64>,
    /// `market_time` of an eve-price-snapshot file (RFC 3339), if known.
    pub time: Option<String>,
    /// `injected` (file / session) or `snapshot` (embedded).
    pub source: &'static str,
}

static MARKET: RwLock<Option<Arc<Market>>> = RwLock::new(None);

/// Set (or clear) the process / session market snapshot (L4).
pub fn set_market(m: Option<Market>) {
    *MARKET.write().unwrap() = m.map(Arc::new);
}
pub fn market() -> Option<Arc<Market>> {
    MARKET.read().unwrap().clone()
}

#[derive(Debug, Clone)]
pub struct PriceError {
    pub code: &'static str,
    pub message: String,
}

fn bad(code: &'static str, message: String) -> PriceError {
    PriceError { code, message }
}

/// Parse a price file / session payload: an eve-price-snapshot v1 object (docs/22 §4, `prices` keyed by type id with
/// `price` per entry, or a list of entries with `type_id`), a `{"isk": {...}}` object, or a plain `{"<type_id>": isk}` map.
pub fn market_from_value(v: &serde_json::Value) -> Result<Market, PriceError> {
    let obj = v.as_object().ok_or_else(|| bad("BAD_PRICES", "price file must be a JSON object".into()))?;
    let mut m = Market { source: "injected", ..Default::default() };
    let (table, snap) = if obj.get("schema").and_then(|s| s.as_str()) == Some("eve-price-snapshot") {
        m.time = obj.get("market_time").and_then(|t| t.as_str()).map(|s| s.to_string());
        (obj.get("prices").ok_or_else(|| bad("PRICE_SNAPSHOT_INVALID", "snapshot has no prices".into()))?, true)
    } else if let Some(t) = obj.get("isk") {
        (t, false)
    } else {
        (v, false)
    };
    let ok = |p: Option<f64>| p.filter(|p| p.is_finite() && *p >= 0.0);
    match table {
        serde_json::Value::Object(o) => {
            for (k, x) in o {
                let id: u32 = k.parse().map_err(|_| bad("BAD_PRICES", format!("bad type id key {k:?}")))?;
                if snap {
                    match x.get("price") {
                        None | Some(serde_json::Value::Null) => continue, // no orders: unpriced
                        Some(p) => {
                            let p = ok(p.as_f64()).ok_or_else(|| bad("PRICE_SNAPSHOT_INVALID", format!("bad price for type {k}")))?;
                            m.isk.insert(id, p);
                        }
                    }
                } else {
                    let p = ok(x.as_f64()).ok_or_else(|| bad("BAD_PRICES", format!("bad price for type {k}")))?;
                    m.isk.insert(id, p);
                }
            }
        }
        serde_json::Value::Array(a) if snap => {
            for x in a {
                let id = x.get("type_id").and_then(|i| i.as_u64()).ok_or_else(|| bad("PRICE_SNAPSHOT_INVALID", "entry without type_id".into()))?;
                if let Some(p) = ok(x.get("price").and_then(|p| p.as_f64())) {
                    m.isk.insert(id as u32, p);
                }
            }
        }
        _ => return Err(bad("BAD_PRICES", "price table must be an object".into())),
    }
    Ok(m)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Type,
    MarketGroup,
    Group,
    Category,
}

impl Kind {
    fn source(self) -> &'static str {
        match self {
            Kind::Type => "override:type",
            Kind::MarketGroup => "override:market_group",
            Kind::Group => "override:group",
            Kind::Category => "override:category",
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Val {
    Fixed(f64),
    Mult(f64),
}

#[derive(Clone, Copy, Debug)]
struct Entry {
    kind: Kind,
    id: u32,
    val: Val,
}

fn parse_entry(o: &PriceOverride) -> Result<Entry, String> {
    let targets = [(Kind::Type, o.type_id), (Kind::MarketGroup, o.market_group_id), (Kind::Group, o.group_id), (Kind::Category, o.category_id)];
    let set: Vec<(Kind, u32)> = targets.iter().filter_map(|(k, v)| v.map(|v| (*k, v))).collect();
    if set.len() != 1 {
        return Err("each price override needs exactly one of type_id, market_group_id, group_id, category_id".into());
    }
    let val = match (o.price, o.multiplier) {
        (Some(p), None) if p.is_finite() && p >= 0.0 => Val::Fixed(p),
        (None, Some(m)) if m.is_finite() && m >= 0.0 => Val::Mult(m),
        (Some(_), None) | (None, Some(_)) => return Err("price / multiplier must be finite and >= 0".into()),
        _ => return Err("each price override needs exactly one of price, multiplier".into()),
    };
    Ok(Entry { kind: set[0].0, id: set[0].1, val })
}

/// Validate one input list (exactly one target, one value, no duplicate target).
pub fn validate(list: &[PriceOverride]) -> Result<(), PriceError> {
    let mut seen = std::collections::BTreeSet::new();
    for (k, o) in list.iter().enumerate() {
        let e = parse_entry(o).map_err(|m| bad("BAD_PRICE_OVERRIDE", format!("price_overrides[{k}]: {m}")))?;
        if !seen.insert((e.kind, e.id)) {
            return Err(bad("BAD_PRICE_OVERRIDE", format!("price_overrides[{k}]: duplicate target {} {}", e.kind.source(), e.id)));
        }
    }
    Ok(())
}

/// One override layer: entries in listing priority (earlier lists first).
#[derive(Clone, Default, Debug)]
pub struct Layer {
    entries: Vec<Entry>,
}

impl Layer {
    /// Build from lists in listing priority (e.g. [fit's own, batch-wide]); `validate` each list first.
    pub fn new(lists: &[&[PriceOverride]]) -> Layer {
        Layer { entries: lists.iter().flat_map(|l| l.iter()).filter_map(|o| parse_entry(o).ok()).collect() }
    }
    fn has_market_group(&self) -> bool {
        self.entries.iter().any(|e| e.kind == Kind::MarketGroup)
    }
}

/// Full price context of one fit.
pub struct Ctx {
    /// [L1 variant, L2 request] (either may be empty).
    layers: [Layer; 2],
    injected: BTreeMap<u32, f64>,
    use_market: bool,
    market: Option<Arc<Market>>,
}

#[derive(Clone, Debug)]
struct Resolved {
    price: f64,
    source: &'static str,
    layer: &'static str,
    mult: Option<f64>,
    base_source: Option<&'static str>,
    snapshot_time: Option<String>,
}

const LAYER_NAMES: [&str; 2] = ["variant", "request"];

impl Ctx {
    pub fn from_request(req: &FitRequest, variant: &[PriceOverride], batch_wide: &[PriceOverride], batch_prices: Option<&eve_fit_model::Prices>) -> Result<Ctx, PriceError> {
        validate(variant)?;
        validate(&req.price_overrides)?;
        validate(batch_wide)?;
        let mut injected = BTreeMap::new();
        let mut use_market = true;
        for p in [batch_prices, req.prices.as_ref()].into_iter().flatten() {
            for (k, v) in &p.isk {
                let id: u32 = k.parse().map_err(|_| bad("BAD_PRICES", format!("prices.isk: bad type id key {k:?}")))?;
                if !v.is_finite() || *v < 0.0 {
                    return Err(bad("BAD_PRICES", format!("prices.isk[{k}]: must be finite and >= 0")));
                }
                injected.insert(id, *v);
            }
            match p.mode.as_deref() {
                None | Some("override") => {}
                Some("replace") => use_market = false,
                Some(m) => return Err(bad("BAD_PRICES", format!("prices.mode: unknown {m:?}"))),
            }
            if let Some(u) = p.use_snapshot {
                use_market = u;
            }
        }
        Ok(Ctx { layers: [Layer::new(&[variant]), Layer::new(&[&req.price_overrides, batch_wide])], injected, use_market, market: market() })
    }

    fn pick(&self, layer: usize, t: u32) -> Option<Entry> {
        let ix = d::type_index(t);
        let (group, category) = ix.map(|ix| (d::ty(ix).group, d::ty(ix).category)).unwrap_or((u32::MAX, u32::MAX));
        // market-group chain: own group first (depth 0), then parents
        let mut chain: Vec<u32> = Vec::new();
        if d::HAS_MARKET_GROUP_TREE {
            let mut g = ix.and_then(d::type_market_group);
            while let Some(x) = g {
                if chain.contains(&x) {
                    break;
                }
                chain.push(x);
                g = d::market_group_parent(x);
            }
        }
        let mut best: Option<((u8, usize, u32, usize), Entry)> = None;
        for (pos, e) in self.layers[layer].entries.iter().enumerate() {
            let rank = match e.kind {
                Kind::Type if e.id == t => (0, 0),
                Kind::MarketGroup => match chain.iter().position(|&g| g == e.id) {
                    Some(depth) => (1, depth),
                    None => continue,
                },
                Kind::Group if e.id == group => (2, 0),
                Kind::Category if e.id == category => (3, 0),
                _ => continue,
            };
            let key = (rank.0, rank.1, e.id, pos);
            if best.as_ref().map(|b| key < b.0).unwrap_or(true) {
                best = Some((key, *e));
            }
        }
        best.map(|b| b.1)
    }

    fn resolve(&self, t: u32, k: usize) -> Option<Resolved> {
        if k < 2 {
            let Some(e) = self.pick(k, t) else { return self.resolve(t, k + 1) };
            return match e.val {
                Val::Fixed(p) => Some(Resolved { price: p, source: e.kind.source(), layer: LAYER_NAMES[k], mult: None, base_source: None, snapshot_time: None }),
                Val::Mult(m) => {
                    let b = self.resolve(t, k + 1)?;
                    Some(Resolved {
                        price: m * b.price,
                        source: e.kind.source(),
                        layer: LAYER_NAMES[k],
                        mult: Some(m * b.mult.unwrap_or(1.0)),
                        base_source: Some(b.base_source.unwrap_or(b.source)),
                        snapshot_time: b.snapshot_time,
                    })
                }
            };
        }
        if k == 2 {
            if let Some(&p) = self.injected.get(&t) {
                return Some(Resolved { price: p, source: "injected", layer: "injected", mult: None, base_source: None, snapshot_time: None });
            }
            return self.resolve(t, 3);
        }
        if !self.use_market {
            return None;
        }
        let m = self.market.as_ref()?;
        let p = *m.isk.get(&t)?;
        Some(Resolved { price: p, source: m.source, layer: m.source, mult: None, base_source: None, snapshot_time: m.time.clone() })
    }

    /// Why a type has no price: a multiplier without a base price, or no price anywhere.
    fn missing_reason(&self, t: u32) -> &'static str {
        for k in 0..2 {
            if let Some(e) = self.pick(k, t) {
                return if matches!(e.val, Val::Mult(_)) { "multiplier_without_base" } else { "no_price" };
            }
        }
        "no_price"
    }
}

/// Is a price block wanted for this request (docs/23 §6.1)?
pub fn wanted(req: &FitRequest) -> bool {
    req.options.price || !req.price_overrides.is_empty() || req.prices.is_some() || market().is_some()
}

const KINDS: [&str; 8] = ["ship", "module", "charge", "drone", "fighter", "implant", "booster", "cargo"];
const SECTIONS: [&str; 8] = ["ship", "modules", "charges", "drones", "fighters", "implants", "boosters", "cargo"];

fn fighter_size(t: u32) -> u64 {
    const FIGHTER_SQUADRON_MAX_SIZE: u16 = 2215;
    d::type_index(t).and_then(|ix| d::type_attr(ix, FIGHTER_SQUADRON_MAX_SIZE)).filter(|v| *v > 0.0).map(|v| v as u64).unwrap_or(1)
}

/// Loaded charges (eve 14:36 ruling, Pyfa): floor(module type's base capacity / charge volume), 0 if it does not fit.
fn charges(module: u32, charge: u32) -> u64 {
    let (Some(mi), Some(ci)) = (d::type_index(module), d::type_index(charge)) else { return 0 };
    let vol = d::type_volume(ci);
    if vol <= 0.0 {
        return 0;
    }
    let n = crate::graphs::cycles::float_unerr(d::type_capacity(mi) / vol).floor();
    if n.is_finite() && n >= 0.0 { n as u64 } else { 0 }
}

/// Item lines of the fit: (section, index, type_id, quantity). Mutated items are priced as their base type.
fn lines(req: &FitRequest) -> Vec<(usize, usize, u32, u64)> {
    let mut v = vec![(0, 0, req.ship.type_id, 1)];
    for (i, m) in req.modules.iter().enumerate() {
        v.push((1, i, m.mutation.as_ref().map(|x| x.base_type_id).unwrap_or(m.type_id), 1));
    }
    for (i, m) in req.modules.iter().enumerate() {
        if let Some(c) = m.charge_type_id {
            v.push((2, i, c, charges(m.type_id, c)));
        }
    }
    for (i, x) in req.drones.iter().enumerate() {
        v.push((3, i, x.mutation.as_ref().map(|m| m.base_type_id).unwrap_or(x.type_id), x.quantity as u64));
    }
    for (i, x) in req.fighters.iter().enumerate() {
        v.push((4, i, x.type_id, x.quantity.map(|q| q as u64).unwrap_or_else(|| fighter_size(x.type_id))));
    }
    for (i, &x) in req.implants.iter().enumerate() {
        v.push((5, i, x, 1));
    }
    for (i, x) in req.boosters.iter().enumerate() {
        v.push((6, i, x.type_id, 1));
    }
    for (i, x) in req.cargo.iter().enumerate() {
        v.push((7, i, x.type_id, x.quantity as u64));
    }
    v
}

fn name(t: u32) -> J {
    match d::type_index(t) {
        Some(ix) => J::S(d::type_name(ix)),
        None => J::Null,
    }
}

/// The `price` output block (docs/23 §6.2).
pub fn block(req: &FitRequest, ctx: &Ctx) -> J {
    let mut sec_items: Vec<Vec<J>> = vec![Vec::new(); SECTIONS.len()];
    let mut sec_total = [0.0f64; 8];
    let mut missing = Vec::new();
    let mut sources: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut snap_time: Option<String> = None;
    for (s, i, t, q) in lines(req) {
        match ctx.resolve(t, 0) {
            Some(r) => {
                let total = r.price * q as f64;
                sec_total[s] += total;
                *sources.entry(r.source).or_default() += 1;
                if snap_time.is_none() {
                    snap_time = r.snapshot_time.clone();
                }
                let mut line = vec![
                    ("kind".into(), J::S(KINDS[s])),
                    ("index".into(), J::U(i as u64)),
                    ("type_id".into(), J::U(t as u64)),
                    ("name".into(), name(t)),
                    ("quantity".into(), J::U(q)),
                    ("unit_isk".into(), J::F(r.price)),
                    ("total_isk".into(), J::F(total)),
                    ("source".into(), J::S(r.source)),
                    ("layer".into(), J::S(r.layer)),
                    // eve 14:36: without a multiplier, `multiplier` is omitted and base_source = source
                    ("base_source".into(), J::S(r.base_source.unwrap_or(r.source))),
                    ("snapshot_time".into(), r.snapshot_time.map(J::Str).unwrap_or(J::Null)),
                ];
                if let Some(m) = r.mult {
                    line.push(("multiplier".into(), J::F(m)));
                }
                sec_items[s].push(J::O(line));
            }
            None => missing.push(J::O(vec![
                ("kind".into(), J::S(KINDS[s])),
                ("section".into(), J::S(SECTIONS[s])),
                ("index".into(), J::U(i as u64)),
                ("type_id".into(), J::U(t as u64)),
                ("name".into(), name(t)),
                ("quantity".into(), J::U(q)),
                ("reason".into(), J::S(ctx.missing_reason(t))),
            ])),
        }
    }
    let mut warnings = Vec::new();
    if !d::HAS_MARKET_GROUP_TREE && ctx.layers.iter().any(|l| l.has_market_group()) {
        warnings.push(J::S("market_group overrides unsupported until SDE dataset r5 (ignored)"));
    }
    let total: f64 = sec_total.iter().sum();
    let sections = SECTIONS
        .iter()
        .enumerate()
        .map(|(k, n)| (std::borrow::Cow::Borrowed(*n), J::O(vec![("total_isk".into(), J::F(sec_total[k])), ("items".into(), J::A(std::mem::take(&mut sec_items[k])))])))
        .collect();
    J::O(vec![
        ("total_isk".into(), J::F(total)),
        ("complete".into(), J::Bool(missing.is_empty())),
        ("sections".into(), J::O(sections)),
        ("missing".into(), J::A(missing)),
        ("snapshot_time".into(), snap_time.map(J::Str).unwrap_or(J::Null)),
        ("sources".into(), J::O(sources.into_iter().map(|(k, v)| (std::borrow::Cow::Borrowed(k), J::U(v))).collect())),
        ("warnings".into(), J::A(warnings)),
    ])
}
