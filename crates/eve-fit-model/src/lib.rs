//! eve-fit-model — the structured fit input (FitRequest v1, contract: eve-dogma-rs/docs/contract.md) shared by the
//! engine (`eve-dogma`), the fit formats (`eve-fit-formats`) and tools. Plain serde types; no data, no engine.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Offline,
    #[default]
    Online,
    Active,
    Overheated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Slot {
    High,
    Mid,
    Low,
    Rig,
    Subsystem,
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpoolType {
    SpoolScale,
    CycleScale,
    Time,
    Cycles,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Spool {
    #[serde(rename = "type")]
    pub kind: SpoolType,
    pub amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Mutation {
    pub base_type_id: u32,
    #[serde(default)]
    pub mutaplasmid_type_id: Option<u32>,
    #[serde(default)]
    pub attributes: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModuleReq {
    pub type_id: u32,
    #[serde(default)]
    pub slot: Option<Slot>,
    #[serde(default)]
    pub state: Option<State>,
    #[serde(default)]
    pub charge_type_id: Option<u32>,
    #[serde(default)]
    pub mutation: Option<Mutation>,
    #[serde(default)]
    pub spool: Option<Spool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DroneReq {
    pub type_id: u32,
    #[serde(default = "one")]
    pub quantity: u32,
    #[serde(default)]
    pub active: Option<u32>,
    #[serde(default)]
    pub mutation: Option<Mutation>,
}
fn one() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FighterReq {
    pub type_id: u32,
    #[serde(default)]
    pub quantity: Option<u32>,
    #[serde(default = "yes")]
    pub active: bool,
    #[serde(default)]
    pub abilities: Option<Vec<u32>>,
}
fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BoosterReq {
    pub type_id: u32,
    #[serde(default)]
    pub side_effects: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CargoReq {
    pub type_id: u32,
    #[serde(default = "one")]
    pub quantity: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Skills {
    #[serde(default)]
    pub default_level: Option<u8>,
    #[serde(default)]
    pub levels: BTreeMap<String, u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Character {
    #[serde(default)]
    pub skills: Skills,
    #[serde(default)]
    pub security_status: Option<f64>,
    /// Character implants (Pyfa ImplantLocation.CHARACTER); used instead of the fit's `implants` when
    /// `options.implant_source` is "character" (bench draft 1.11 cimp_*).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implants: Vec<u32>,
    /// Alpha clone: every skill capped at its Alpha level, untrainable skills at 0 (Pyfa alphaCloneID 1).
    #[serde(default, skip_serializing_if = "is_false")]
    pub alpha_clone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Buff {
    pub buff_id: u32,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Fleet {
    #[serde(default)]
    pub buffs: Vec<Buff>,
    #[serde(default)]
    pub booster_fits: Vec<FitRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Projected {
    pub kind: String,
    #[serde(default)]
    pub module: Option<ModuleReq>,
    #[serde(default)]
    pub drone: Option<DroneReq>,
    #[serde(default)]
    pub fit: Option<Box<FitRequest>>,
    #[serde(default)]
    pub fighter: Option<FighterReq>,
    #[serde(default = "one")]
    pub amount: u32,
    #[serde(default)]
    pub distance_m: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Environment {
    #[serde(default)]
    pub effect_type_ids: Vec<u32>,
    #[serde(default)]
    pub system_security: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resists {
    #[serde(default)]
    pub em: f64,
    #[serde(default)]
    pub thermal: f64,
    #[serde(default)]
    pub kinetic: f64,
    #[serde(default)]
    pub explosive: f64,
    /// Pyfa builtin damage pattern by raw name (`DamagePattern.getBuiltinList`, e.g. "[NPC][Asteroid]Guristas");
    /// resolved by the engine from the runtime Pyfa data (bench draft 1.11 `dpb_*`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TargetProfile {
    #[serde(default)]
    pub em: f64,
    #[serde(default)]
    pub thermal: f64,
    #[serde(default)]
    pub kinetic: f64,
    #[serde(default)]
    pub explosive: f64,
    #[serde(default)]
    pub signature_radius: Option<f64>,
    #[serde(default)]
    pub max_velocity: Option<f64>,
    #[serde(default)]
    pub radius: Option<f64>,
    /// Pyfa builtin target profile by raw name (`TargetProfile.getBuiltinList`); resolved by the engine from the
    /// runtime Pyfa data (bench draft 1.11 `tpb_*`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builtin: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Override {
    pub type_id: u32,
    pub attribute_id: u32,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapSimOpts {
    #[serde(default)]
    pub reload: bool,
    #[serde(default)]
    pub stagger: bool,
    #[serde(default)]
    pub max_time_s: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Options {
    /// Treat local nosferatu as cap income (default) - set true to ignore it (target without cap).
    #[serde(default)]
    pub nos_no_target_cap: bool,
    #[serde(default)]
    pub factor_reload: bool,
    #[serde(default)]
    pub default_spool: Option<Spool>,
    #[serde(default)]
    pub rah: Option<String>,
    #[serde(default)]
    pub include_attributes: Option<String>,
    #[serde(default)]
    pub sources: bool,
    #[serde(default = "yes")]
    pub validate: bool,
    #[serde(default)]
    pub cap_sim: CapSimOpts,
    /// Output floats unrounded (shortest round-trip form) instead of rounded to 6 decimals. Off by default; used
    /// when a consumer formats the numbers itself (e.g. the formats layer's ship-stats text).
    #[serde(default, skip_serializing_if = "is_false")]
    pub full_precision: bool,
    /// Emit the `price` block (docs/23 §6) even without price inputs.
    #[serde(default, skip_serializing_if = "is_false")]
    pub price: bool,
    /// "fit" (default): the fit's `implants`; "character": `character.implants` (the fit's are ignored).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implant_source: Option<String>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Default for Options {
    /// `options` missing entirely: same as `{}` (contract 1.4.1: validate defaults to true).
    fn default() -> Self {
        Options {
            nos_no_target_cap: false,
            factor_reload: false,
            default_spool: None,
            rah: None,
            include_attributes: None,
            sources: false,
            validate: true,
            cap_sim: CapSimOpts::default(),
            full_precision: false,
            price: false,
            implant_source: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipReq {
    pub type_id: u32,
    #[serde(default)]
    pub mode_type_id: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FitRequest {
    #[serde(default)]
    pub schema_version: Option<u32>,
    pub ship: ShipReq,
    #[serde(default)]
    pub character: Character,
    #[serde(default)]
    pub modules: Vec<ModuleReq>,
    #[serde(default)]
    pub drones: Vec<DroneReq>,
    #[serde(default)]
    pub fighters: Vec<FighterReq>,
    #[serde(default)]
    pub implants: Vec<u32>,
    #[serde(default)]
    pub boosters: Vec<BoosterReq>,
    #[serde(default)]
    pub cargo: Vec<CargoReq>,
    #[serde(default)]
    pub fleet: Fleet,
    #[serde(default)]
    pub projected: Vec<Projected>,
    #[serde(default)]
    pub environment: Environment,
    #[serde(default)]
    pub damage_pattern: Option<Resists>,
    #[serde(default)]
    pub target_profile: Option<TargetProfile>,
    #[serde(default)]
    pub overrides: Vec<Override>,
    #[serde(default)]
    pub options: Options,
    /// Price overrides (docs/23 §5.1): by type / market group (incl. children) / group / category, fixed or multiplier.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub price_overrides: Vec<PriceOverride>,
    /// Injected price table (docs/23 §5.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prices: Option<Prices>,
}

/// One price override entry: exactly one target and exactly one of `price` / `multiplier` (checked by the engine).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct PriceOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_group_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiplier: Option<f64>,
}

/// Injected prices: `isk` = type id (string key) -> ISK per unit; `use_snapshot: false` disables the market snapshot
/// layer. `mode` (`override` | `replace`) is the docs/22 alias for `use_snapshot` (true | false).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Prices {
    /// values are checked by the engine (BAD_PRICES for non-numbers), so any JSON value is accepted here
    #[serde(default)]
    pub isk: BTreeMap<String, serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub use_snapshot: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}
