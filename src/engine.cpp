#include "engine.hpp"

#include <algorithm>
#include <mutex>
#include <array>
#include <charconv>
#include <cmath>
#include <optional>
#include <string>

namespace evej {

static constexpr uint32_t EXEMPT_CATEGORIES[6] = {6, 8, 16, 20, 32, 65};
static constexpr uint32_t ATTR_SKILL_LEVEL = 280;
static constexpr uint32_t EFFECT_SKILL_EFFECT = 132;
static constexpr uint32_t HULL_RESONANCES[4] = {113, 111, 109, 110};

// --------------------------------------------------------------------------- ids
Ids::Ids(const Dataset& ds) {
  auto a = [&](const char* n) { return ds.attr_id(n); };
  auto e = [&](const char* n) { return ds.effect_id(n); };
  pilotSecurityStatus = a("pilotSecurityStatus");
  fighterSquadronMaxSize = a("fighterSquadronMaxSize");
  hiSecModifier = a("hiSecModifier");
  lowSecModifier = a("lowSecModifier");
  nullSecModifier = a("nullSecModifier");
  securityModifier = a("securityModifier");
  massAddition = a("massAddition");
  speedFactor = a("speedFactor");
  speedBoostFactor = a("speedBoostFactor");
  maxVelocity = a("maxVelocity");
  signatureRadiusBonus = a("signatureRadiusBonus");
  signatureRadius = a("signatureRadius");
  signatureRadiusBonusPercent = a("signatureRadiusBonusPercent");
  hiSlots = a("hiSlots");
  medSlots = a("medSlots");
  lowSlots = a("lowSlots");
  hiSlotModifier = a("hiSlotModifier");
  medSlotModifier = a("medSlotModifier");
  lowSlotModifier = a("lowSlotModifier");
  turretSlotsLeft = a("turretSlotsLeft");
  launcherSlotsLeft = a("launcherSlotsLeft");
  turretHardPointModifier = a("turretHardPointModifier");
  launcherHardPointModifier = a("launcherHardPointModifier");
  remoteResistanceID = a("remoteResistanceID");
  maxTargetRange = a("maxTargetRange");
  maxTargetRangeBonus = a("maxTargetRangeBonus");
  scanResolution = a("scanResolution");
  scanResolutionBonus = a("scanResolutionBonus");
  resistanceShiftAmount = a("resistanceShiftAmount");
  for (int k = 0; k < 4; k++) {
    warfareBuffID[k] = a(("warfareBuff" + std::to_string(k + 1) + "ID").c_str());
    warfareBuffValue[k] = a(("warfareBuff" + std::to_string(k + 1) + "Value").c_str());
  }
  const char* dn[4] = {"Em", "Thermal", "Kinetic", "Explosive"};
  const char* dl[4] = {"em", "thermal", "kinetic", "explosive"};
  for (int k = 0; k < 4; k++) {
    armorRes[k] = a((std::string("armor") + dn[k] + "DamageResonance").c_str());
    shieldRes[k] = a((std::string("shield") + dn[k] + "DamageResonance").c_str());
    hullRes[k] = a((std::string(dl[k]) + "DamageResonance").c_str());
  }
  e_ab = e("moduleBonusAfterburner");
  e_mwd = e("moduleBonusMicrowarpdrive");
  e_slot = e("slotModifier");
  e_hp = e("hardPointModifierEffect");
  e_mjd = e("microJumpDrive");
  e_bastion = e("moduleBonusBastionModule");
  e_rah = e("adaptiveArmorHardener");
  const char* so[5] = {"targetingMaxTargetBonusModAddMaxLockedTargetsLocationChar", "skillStructureMissileDamageBonus",
                       "skillStructureElectronicSystemsCapNeedBonus", "skillStructureEngineeringSystemsCapNeedBonus",
                       "skillStructureDoomsdayDurationBonus"};
  for (int k = 0; k < 5; k++) structure_ok[k] = e(so[k]);
  cpu = a("cpu");
  power = a("power");
  cpuOutput = a("cpuOutput");
  powerOutput = a("powerOutput");
  upgradeCost = a("upgradeCost");
  upgradeCapacity = a("upgradeCapacity");
  speed = a("speed");
  duration = a("duration");
  capacitorNeed = a("capacitorNeed");
  reloadTime = a("reloadTime");
  chargedArmorDamageMultiplier = a("chargedArmorDamageMultiplier");
  moduleReactivationDelay = a("moduleReactivationDelay");
  chargeRate = a("chargeRate");
  damageMultiplier = a("damageMultiplier");
  dmg[0] = a("emDamage");
  dmg[1] = a("thermalDamage");
  dmg[2] = a("kineticDamage");
  dmg[3] = a("explosiveDamage");
  const char* ce[5] = {"durationHighisGood", "durationSensorDampeningBurstProjector", "durationTargetIlluminationBurstProjector",
                       "durationECMJammerBurstProjector", "durationWeaponDisruptionBurstProjector"};
  for (int k = 0; k < 5; k++) cycle_extra[k] = a(ce[k]);
  crystalsGetDamaged = a("crystalsGetDamaged");
  crystalVolatilityChance = a("crystalVolatilityChance");
  crystalVolatilityDamage = a("crystalVolatilityDamage");
  missileDamageMultiplier = a("missileDamageMultiplier");
  droneBandwidthUsed = a("droneBandwidthUsed");
  droneBandwidth = a("droneBandwidth");
  droneCapacity = a("droneCapacity");
  fighterCapacity = a("fighterCapacity");
  rigSlots = a("rigSlots");
  maxSubSystems = a("maxSubSystems");
  serviceSlots = a("serviceSlots");
  fighterSquadronIsHeavy = a("fighterSquadronIsHeavy");
  fighterSquadronIsSupport = a("fighterSquadronIsSupport");
  fighterTubes = a("fighterTubes");
  fighterLightSlots = a("fighterLightSlots");
  fighterSupportSlots = a("fighterSupportSlots");
  fighterHeavySlots = a("fighterHeavySlots");
  damageMultiplierBonusMax = a("damageMultiplierBonusMax");
  damageMultiplierBonusPerCycle = a("damageMultiplierBonusPerCycle");
  maxRange = a("maxRange");
  falloff = a("falloff");
  trackingSpeed = a("trackingSpeed");
  explosionDelay = a("explosionDelay");
  aoeCloudSize = a("aoeCloudSize");
  aoeVelocity = a("aoeVelocity");
  empFieldRange = a("empFieldRange");
  const char* fs[6] = {"DamageMultiplier", "DamageEM", "DamageTherm", "DamageKin", "DamageExp", "Duration"};
  for (int k = 0; k < 6; k++) {
    fam[k] = a((std::string("fighterAbilityAttackMissile") + fs[k]).c_str());
    fmi[k] = a((std::string("fighterAbilityMissiles") + fs[k]).c_str());
  }
  shieldCapacity = a("shieldCapacity");
  armorHP = a("armorHP");
  shieldBonus = a("shieldBonus");
  armorDamageAmount = a("armorDamageAmount");
  structureDamageAmount = a("structureDamageAmount");
  shieldRechargeRate = a("shieldRechargeRate");
  capacitorCapacity = a("capacitorCapacity");
  rechargeRate = a("rechargeRate");
  capacitorBonus = a("capacitorBonus");
  powerTransferAmount = a("powerTransferAmount");
  speedLimit = a("speedLimit");
  agility = a("agility");
  baseWarpSpeed = a("baseWarpSpeed");
  warpSpeedMultiplier = a("warpSpeedMultiplier");
  warpCapacitorNeed = a("warpCapacitorNeed");
  warpScrambleStatus = a("warpScrambleStatus");
  scanStrength[0] = a("scanRadarStrength");
  scanStrength[1] = a("scanLadarStrength");
  scanStrength[2] = a("scanMagnetometricStrength");
  scanStrength[3] = a("scanGravimetricStrength");
  maxLockedTargets = a("maxLockedTargets");
  maxActiveDrones = a("maxActiveDrones");
  droneControlDistance = a("droneControlDistance");
  rigSize = a("rigSize");
  for (int k = 1; k <= 20; k++) {
    char b[32];
    snprintf(b, sizeof b, "canFitShipGroup%02d", k);
    uint32_t x = a(b);
    if (x) canFitShipGroup[n_cfg++] = x;
  }
  for (int k = 1; k <= 11; k++) {
    uint32_t x = a(("canFitShipType" + std::to_string(k)).c_str());
    if (x) canFitShipType[n_cft++] = x;
  }
  if (n_cfg + n_cft) {
    cf_lo = UINT32_MAX;
    for (uint32_t k = 0; k < n_cfg; k++) cf_lo = std::min(cf_lo, canFitShipGroup[k]), cf_hi = std::max(cf_hi, canFitShipGroup[k]);
    for (uint32_t k = 0; k < n_cft; k++) cf_lo = std::min(cf_lo, canFitShipType[k]), cf_hi = std::max(cf_hi, canFitShipType[k]);
    cf_ok = cf_hi - cf_lo < sizeof cf_kind;
    memset(cf_kind, 0, sizeof cf_kind);
    if (cf_ok) {
      for (uint32_t k = 0; k < n_cfg; k++) cf_kind[canFitShipGroup[k] - cf_lo] |= 1;
      for (uint32_t k = 0; k < n_cft; k++) cf_kind[canFitShipType[k] - cf_lo] |= 2;
    }
  }
  maxGroupFitted = a("maxGroupFitted");
  maxTypeFitted = a("maxTypeFitted");
  maxGroupOnline = a("maxGroupOnline");
  maxGroupActive = a("maxGroupActive");
  for (int k = 0; k < 5; k++) chargeGroup[k] = a(("chargeGroup" + std::to_string(k + 1)).c_str());
  chargeSize = a("chargeSize");
  for (int k = 0; k < 6; k++) {
    requiredSkill[k] = a(("requiredSkill" + std::to_string(k + 1)).c_str());
    requiredSkillLevel[k] = a(("requiredSkill" + std::to_string(k + 1) + "Level").c_str());
  }
  e_turret = e("turretFitted");
  e_launcher = e("launcherFitted");
  e_empwave = e("empWave");
  e_chain = e("ChainLightning");
  e_shieldBoosting = e("shieldBoosting");
  e_fueledShieldBoosting = e("fueledShieldBoosting");
  e_armorRepair = e("armorRepair");
  e_fueledArmorRepair = e("fueledArmorRepair");
  e_structureRepair = e("structureRepair");
  e_nos = e("energyNosferatuFalloff");
  e_fam = e("fighterAbilityAttackM");
  e_fmi = e("fighterAbilityMissiles");
  g_cap_booster_group_ok = 0;
  falloffEffectiveness = a("falloffEffectiveness");
  disallowAssistance = a("disallowAssistance");
  energyNeutralizerAmount = a("energyNeutralizerAmount");
  energyNeutralizerDuration = a("energyNeutralizerDuration");
  energyNeutralizerRangeOptimal = a("energyNeutralizerRangeOptimal");
  energyNeutralizerSignatureResolution = a("energyNeutralizerSignatureResolution");
  radius = a("radius");
  mass = a("mass");
  agilityA = a("agility");
  maxFOFTargetRange = a("maxFOFTargetRange");
  const char* st[4] = {"Gravimetric", "Ladar", "Magnetometric", "Radar"};
  for (int k = 0; k < 4; k++) {
    scanStrengthG[k] = a((std::string("scan") + st[k] + "Strength").c_str());
    scanStrengthPercent[k] = a((std::string("scan") + st[k] + "StrengthPercent").c_str());
  }
  e_fof = e("fofMissileLaunching");
}

static std::string rust_debug_str(std::string_view s) {
  std::string o = "\"";
  for (char c : s) {
    if (c == '"' || c == '\\') o.push_back('\\');
    o.push_back(c);
  }
  o.push_back('"');
  return o;
}
static std::string debug_error(const EngineError& e) {
  return "EngineError { code: " + rust_debug_str(e.code) + ", message: " + rust_debug_str(e.message) + ", path: " +
         rust_debug_str(e.path) + " }";
}

// --------------------------------------------------------------------------- attribute table
static inline uint64_t hkey(uint32_t item, uint32_t attr) { return (uint64_t)item << 32 | attr; }
static inline uint64_t hmix(uint64_t k) {
  k ^= k >> 29;
  k *= 0xbf58476d1ce4e5b9ull;
  k ^= k >> 32;
  return k;
}

int32_t Fit::find(uint32_t item, uint32_t attr) const {
  if (ht_.empty()) return -1;
  const uint64_t k = hkey(item, attr);
  const uint32_t g = hgen_;
  uint64_t p = hmix(k) & hmask_;
  while (true) {
    const HSlot& sl = ht_[p];
    if (sl.g != g) return -1;
    if (sl.k == k) return (int32_t)sl.v;
    p = (p + 1) & hmask_;
  }
}

void Fit::reset() {
  items.clear();
  ship = chr = 0;
  is_structure = false;
  warnings.clear();
  skill_levels.clear();
  proj_special.clear();
  if (hcount_) {
    if (++hgen_ == 0) {  // generation wrap: really clear
      for (auto& sl : ht_) sl.g = 0;
      hgen_ = 1;
    }
  }
  hcount_ = 0;
  la_.clear();
  mods_.clear();
  eff_store_.clear();
  list_store_.clear();
  spool_store_.clear();
  ship_loc_.clear();
  char_loc_.clear();
  owned_.clear();
  char_skill_tgt_.clear();
  ix_ship_grp_.clear();
  ix_char_grp_.clear();
  ix_ship_skill_.clear();
  ix_owned_skill_.clear();
  ix_char_skill_.clear();
}

void Fit::grow() {
  size_t n = ht_.empty() ? 4096 : ht_.size() * 2;
  std::vector<HSlot> old = std::move(ht_);
  const uint32_t g = hgen_;
  ht_.assign(n, HSlot{0, 0, 0});
  if (g == 0) hgen_ = 1;
  hmask_ = n - 1;
  for (const HSlot& sl : old) {
    if (sl.g != g) continue;
    uint64_t p = hmix(sl.k) & hmask_;
    while (ht_[p].g == hgen_) p = (p + 1) & hmask_;
    ht_[p] = HSlot{sl.k, sl.v, hgen_};
  }
}

bool Fit::type_base(uint32_t item, uint32_t attr, double& v) const {
  const TypeRec& t = *items[item].t;
  double field;
  switch (attr) {
    case 4: field = t.mass; break;
    case 38: field = t.capacity; break;
    case 161: field = t.volume; break;
    case 162: field = t.radius; break;
    default: return ds.type_attr(t, attr, v);
  }
  if (field != 0.0) {
    v = field;
    return true;
  }
  if (!ds.type_attr(t, attr, v)) v = 0.0;
  return true;
}

uint32_t Fit::ensure(uint32_t item, uint32_t attr) {
  if ((hcount_ + 1) * 2 > ht_.size()) grow();
  const uint64_t k = hkey(item, attr);
  const uint32_t g = hgen_;
  uint64_t p = hmix(k) & hmask_;
  for (; ht_[p].g == g; p = (p + 1) & hmask_)
    if (ht_[p].k == k) return ht_[p].v;
  uint32_t idx = (uint32_t)la_.size();
  ht_[p] = HSlot{k, idx, g};
  hcount_++;
  la_.push_back(LAttr{0.0, 0.0, UINT32_MAX, UINT32_MAX, item, attr, 0, 0});
  return idx;
}

void Fit::set_base(uint32_t item, uint32_t attr, double v) {
  uint32_t i = ensure(item, attr);
  LAttr& a = la_[i];
  a.base = v;
  a.bl = 1;
  a.head = a.tail = UINT32_MAX;  // HashMap::insert replaces the whole Attr (drops modifiers)
  a.st = 0;
}

bool Fit::has(uint32_t item, uint32_t attr) const {
  if (find(item, attr) >= 0) return true;
  double v;
  return type_base(item, attr, v);
}

double Fit::base(uint32_t item, uint32_t attr) const {
  int32_t f = find(item, attr);
  if (f >= 0) return lbase((uint32_t)f);
  double v;
  if (type_base(item, attr, v)) return v;
  return ds.attr_default(attr);
}

// Python round(v, 2): correctly rounded on the exact binary value (ties to even), as eve-dogma-rs's
// format!("{v:.2}").parse() (since aa46025)
static double py_round2_slow(double v) {
  char buf[400];
  auto r = std::to_chars(buf, buf + sizeof buf, v, std::chars_format::fixed, 2);
  if (r.ec != std::errc()) return v;
  double o = v;
  std::from_chars(buf, r.ptr, o);
  return o;
}

// Fast exact path: n = the integer nearest to the exact product v*100 (t + e, e from fma), unless it is within a hair
// of a tie; then n/100 (one correctly rounded division) is the double nearest the decimal n/100, i.e. what parsing
// the formatted string gives. Ties and huge values take the formatting path.
static double py_round2(double v) {
  if (!std::isfinite(v)) return v;
  if (std::fabs(v) < 1e12) {
    const double t = v * 100.0;
    const double e = std::fma(v, 100.0, -t);
    const double n = std::nearbyint(t);
    const double d = (t - n) + e;
    if (std::fabs(d) < 0.4999999) return n / 100.0;
  }
  return py_round2_slow(v);
}

double Fit::caps(uint32_t item, const AttrRec* info, double val) {
  if (!info) return val;
  if (info->min_attr) {
    double m = get(item, info->min_attr);
    val = std::max(val, m);
  }
  if (info->max_attr) {
    double m = get(item, info->max_attr);
    val = std::min(val, m);
  }
  if (info->round2) val = py_round2(val);
  return val;
}

double Fit::get(uint32_t item, uint32_t attr) {
  int32_t f = find(item, attr);
  if (f >= 0) return eval((uint32_t)f);
  double v;
  if (type_base(item, attr, v)) return caps(item, ds.attr(attr), v);
  return ds.attr_default(attr);
}

bool Fit::get_opt(uint32_t item, uint32_t attr, double& out) {
  if (!has(item, attr)) return false;
  out = get(item, attr);
  return true;
}

std::vector<uint32_t> Fit::attr_keys(uint32_t item) const {
  std::vector<uint32_t> k;
  for (auto& a : la_)
    if (a.item == item) k.push_back(a.attr);
  for (auto& ta : ds.type_attrs(*items[item].t)) k.push_back(ta.id);
  for (uint32_t s : {4u, 38u, 161u, 162u}) k.push_back(s);
  std::sort(k.begin(), k.end());
  k.erase(std::unique(k.begin(), k.end()), k.end());
  return k;
}

void Fit::clear_cache() {
  for (auto& a : la_) a.st = 0;
}

double Fit::src_value(const Src& s) {
  switch (s.k) {
    case Src::Attr: return get(s.a, s.b);
    case Src::Const: return s.f;
    case Src::Prop: {
      double m = get(s.b, s.e);
      if (m == 0.0) return 1.0;
      double sp = get(s.a, s.c);
      double th = get(s.a, s.d);
      return 1.0 + sp / 100.0 * th / m;
    }
    case Src::Proj: {
      double f = s.f;
      if (s.d != 0) f *= get(s.c, s.d);
      double v = get(s.a, s.b);
      return s.mul ? (v - 1.0) * f + 1.0 : v * f;
    }
  }
  return 0.0;
}

namespace {
struct Val {
  int8_t op;
  uint8_t pen;
  double v;
};
const double* penalty_table() {
  static double t[64];
  static bool init = false;
  if (!init) {
    for (int i = 0; i < 64; i++) t[i] = std::exp(-((double)(i * i)) / 7.1289);
    init = true;
  }
  return t;
}
const double* PEN = penalty_table();

inline void apply_pen(double& val, double* l, int n) {
  // stable sort by |m-1| descending
  for (int i = 1; i < n; i++) {
    double x = l[i];
    double kx = std::fabs(x - 1.0);
    int j = i - 1;
    while (j >= 0 && std::fabs(l[j] - 1.0) < kx) {
      l[j + 1] = l[j];
      j--;
    }
    l[j + 1] = x;
  }
  for (int i = 0; i < n; i++) {
    double e = i < 64 ? PEN[i] : std::exp(-((double)i * (double)i) / 7.1289);
    val *= 1.0 + (l[i] - 1.0) * e;
  }
}
}  // namespace

double Fit::eval(uint32_t idx) {
  {
    LAttr& a = la_[idx];
    if (a.st == 2) return a.val;
    if (a.st == 1) return lbase(idx);
    a.st = 1;
  }
  const uint32_t item = la_[idx].item, attr = la_[idx].attr;
  const AttrRec* info = ds.attr(attr);
  double val = lbase(idx);
  uint32_t head = la_[idx].head;
  if (head != UINT32_MAX) {
    Val sbuf[48];
    std::vector<Val> hbuf;
    Val* vals = sbuf;
    int n = 0, cap = 48;
    uint16_t opmask = 0;  // bit (op+1)
    for (uint32_t m = head; m != UINT32_MAX; m = mods_[m].next) {
      int8_t op = mods_[m].op;
      uint8_t pen = mods_[m].pen;
      Src s = mods_[m].src;
      double v = src_value(s);
      if (n == cap) {
        hbuf.assign(vals, vals + n);
        hbuf.resize(cap * 2);
        cap *= 2;
        vals = hbuf.data();
      }
      vals[n++] = Val{op, pen, v};
      if (op >= -1 && op <= 7) opmask |= (uint16_t)(1u << (op + 1));
    }
    const bool hig = info ? info->high_is_good : true;
    double pbuf[64], nbuf[64];
    std::vector<double> ph, nh;
    for (int op = -1; op <= 7; op++) {
      if (!(opmask & (1u << (op + 1)))) continue;
      int np = 0, nn = 0;
      double* pos = pbuf;
      double* neg = nbuf;
      if (n > 64) {
        ph.resize(n);
        nh.resize(n);
        pos = ph.data();
        neg = nh.data();
      }
      bool has_assign = false;
      double assign = 0;
      for (int i = 0; i < n; i++) {
        if (vals[i].op != op) continue;
        double v = vals[i].v;
        switch (op) {
          case -1:
          case 7:
            if (!has_assign) {
              assign = v;
              has_assign = true;
            } else {
              assign = hig ? std::max(assign, v) : std::min(assign, v);
            }
            break;
          case 2: val += v; break;
          case 3: val -= v; break;
          default: {
            double m;
            if (op == 0 || op == 4) m = v;
            else if (op == 1 || op == 5) m = v == 0.0 ? 1.0 : 1.0 / v;
            else m = 1.0 + v / 100.0;  // 6
            if (vals[i].pen) {
              if (m > 1.0) pos[np++] = m;
              else if (m < 1.0) neg[nn++] = m;
            } else {
              val *= m;
            }
          }
        }
      }
      if (has_assign) val = assign;
      if (np) apply_pen(val, pos, np);
      if (nn) apply_pen(val, neg, nn);
    }
  }
  val = caps(item, info, val);
  LAttr& a = la_[idx];
  a.st = 2;
  a.val = val;
  return val;
}

// Rust f64::max/min semantics are fine with std::max/min here (no NaNs expected).

// --------------------------------------------------------------------------- graph building
int32_t Fit::new_item(uint32_t type_id, Kind kind, Loc loc, const char* path, long pidx, EngineError& err) {
  const TypeRec* t = ds.type(type_id);
  if (!t) {
    err.code = "UNKNOWN_TYPE";
    err.message = "unknown type_id " + std::to_string(type_id);
    err.path = path;
    if (pidx >= 0) {
      std::string p = path;
      size_t pos = p.find("{}");
      if (pos != std::string::npos) p.replace(pos, 2, std::to_string(pidx));
      err.path = p;
    }
    return -1;
  }
  Item it{};
  it.t = t;
  it.type_id = type_id;
  it.group = t->group;
  it.category = t->category;
  it.kind = kind;
  it.state = State::Online;
  it.loc = loc;
  it.owned = kind == Kind::Module || kind == Kind::Charge || kind == Kind::Drone || kind == Kind::Fighter || kind == Kind::Ship;
  it.slot = Slot::None;
  it.parent = it.charge = it.req_index = -1;
  it.quantity = 1;
  it.active_count = 0;
  it.n_req = t->n_req;
  for (uint32_t i = 0; i < t->n_req; i++) it.req_skills[i] = t->req_skills[i];
  it.effs = ds.type_effects(*t);
  items.push_back(it);
  return (int32_t)items.size() - 1;
}

void Fit::apply_mutation(uint32_t idx, const Mutation& m) {
  const TypeRec* base_t = ds.type(m.base_type_id);
  if (base_t) {
    const TypeRec& own = *items[idx].t;
    // set_type_attrs(own) as the HashMap would hold it
    for (auto& a : ds.type_attrs(own)) set_base(idx, a.id, a.v);
    const double fields[4] = {own.mass, own.capacity, own.volume, own.radius};
    const uint32_t fids[4] = {4, 38, 161, 162};
    for (int k = 0; k < 4; k++)
      if (fields[k] != 0.0 || find(idx, fids[k]) < 0) set_base(idx, fids[k], fields[k]);
    for (auto& a : ds.type_attrs(*base_t)) set_base(idx, a.id, a.v);
    for (auto& a : ds.type_attrs(own)) set_base(idx, a.id, a.v);
    auto own_effs = items[idx].effs;
    auto v = std::make_unique<std::vector<TEff>>(own_effs.begin(), own_effs.end());
    for (auto& e : ds.type_effects(*base_t)) {
      bool dup = false;
      for (auto& o : own_effs)
        if (o.id == e.id) dup = true;
      if (!dup) v->push_back(e);
    }
    items[idx].effs = std::span<const TEff>(v->data(), v->size());
    eff_store_.push_back(std::move(v));
    if (items[idx].n_req == 0) {
      items[idx].n_req = base_t->n_req;
      for (uint32_t i = 0; i < base_t->n_req; i++) items[idx].req_skills[i] = base_t->req_skills[i];
    }
    if (lbase((uint32_t)find(idx, 4)) == 0.0 && base_t->mass != 0.0) set_base(idx, 4, base_t->mass);
  }
  const MutaRec* mu = m.mutaplasmid_type_id ? ds.muta(*m.mutaplasmid_type_id) : nullptr;
  for (auto& [aid, v0] : m.attributes) {
    double val = v0;
    if (mu && base_t) {
      const MutaAttr* ma = nullptr;
      for (auto& x : ds.muta_attrs(*mu))
        if (x.attr == aid) ma = &x;
      double bv;
      if (ma && ds.type_attr(*base_t, aid, bv)) {
        double a = bv * ma->lo, b = bv * ma->hi;
        double mn = a < b ? a : b, mx = a < b ? b : a;
        if (bv != 0.0) val = std::clamp(val, mn, mx);
      }
    }
    set_base(idx, aid, val);
  }
}

bool Fit::add_module(uint32_t i, const ModuleReq& m, EngineError& err) {
  int32_t idx = new_item(m.type_id, Kind::Module, Loc::Ship, "/modules/{}", i, err);
  if (idx < 0) return false;
  Slot slot = m.slot != Slot::None ? m.slot : (Slot)items[idx].t->slot;
  Item& it = items[idx];
  it.slot = slot;
  it.req_index = (int32_t)i;
  it.spool = m.spool ? &*m.spool : nullptr;
  it.state = m.state.value_or(State::Online);
  if ((slot == Slot::Rig || slot == Slot::Subsystem) && it.state != State::Offline) it.state = State::Online;
  if (m.mutation) apply_mutation(idx, *m.mutation);
  if (m.charge_type_id) {
    int32_t c = new_item(*m.charge_type_id, Kind::Charge, Loc::Ship, "/modules/{}/charge_type_id", i, err);
    if (c < 0) return false;
    items[c].parent = idx;
    items[c].req_index = (int32_t)i;
    items[idx].charge = c;
  }
  return true;
}

static std::string lower(std::string_view s) {
  std::string o(s);
  for (auto& c : o)
    if (c >= 'A' && c <= 'Z') c = c - 'A' + 'a';
  return o;
}

bool parse_u32_rust(std::string_view s, uint32_t& out) {
  // Rust's str::parse::<u32>: optional '+', digits only, no overflow
  if (!s.empty() && s[0] == '+') s.remove_prefix(1);
  if (s.empty()) return false;
  uint64_t v = 0;
  for (char c : s) {
    if (c < '0' || c > '9') return false;
    v = v * 10 + (c - '0');
    if (v > 0xffffffffull) return false;
  }
  out = (uint32_t)v;
  return true;
}

bool Fit::build(const FitRequest& req, EngineError& err, bool no_projected, bool no_boosters) {
  items.reserve(640);
  la_.reserve(4096);
  mods_.reserve(4096);
  int32_t s = new_item(req.ship_type_id, Kind::Ship, Loc::Ship, "/ship/type_id", -1, err);
  if (s < 0) return false;
  ship = (uint32_t)s;
  is_structure = items[ship].category == 65;
  int32_t c = new_item(1373, Kind::Char, Loc::Char, "/character", -1, err);
  if (c < 0) return false;
  chr = (uint32_t)c;
  if (req.security_status && K.pilotSecurityStatus) set_base(chr, K.pilotSecurityStatus, *req.security_status);
  // skills: every published skill at default level, then overrides (by id or name)
  {
    uint8_t dl = req.default_level.value_or(0);
    std::vector<std::pair<uint32_t, uint8_t>>& lv = skill_levels;
    lv.reserve(ds.skills.size() + req.skill_levels.size());
    for (uint32_t sk : ds.skills) lv.push_back({sk, dl});
    for (auto& [k, v] : req.skill_levels) {
      uint32_t id;
      if (!parse_u32_rust(k, id)) {
        id = ds.type_by_name(k);
        if (!id) continue;
      }
      auto it = std::lower_bound(lv.begin(), lv.end(), id, [](auto& p, uint32_t x) { return p.first < x; });
      if (it != lv.end() && it->first == id) it->second = v;
      else lv.insert(it, {id, v});
    }
    std::vector<std::pair<uint32_t, uint8_t>> kept;
    kept.reserve(lv.size());
    // perf (as eve-dogma-rs): a skill whose modifiers can reach nothing in this fit is not instantiated
    static const bool no_prune = getenv("EVEJ_NO_PRUNE") != nullptr;
    std::vector<uint32_t>& need = prune_need_;
    std::vector<uint32_t>& groups = prune_groups_;
    need.clear();
    groups.clear();
    auto add = [&](uint32_t tid) {
      const TypeRec* t = ds.type(tid);
      if (!t) return;
      groups.push_back(t->group);
      for (uint32_t k = 0; k < t->n_req; k++) need.push_back(t->req_skills[k]);
    };
    add(req.ship_type_id);
    if (req.mode_type_id) add(*req.mode_type_id);
    for (auto& m : req.modules) {
      add(m.type_id);
      if (m.charge_type_id) add(*m.charge_type_id);
      if (m.mutation) add(m.mutation->base_type_id);
    }
    for (auto& d : req.drones) {
      add(d.type_id);
      if (d.mutation) add(d.mutation->base_type_id);
    }
    for (auto& f : req.fighters) add(f.type_id);
    for (uint32_t i : req.implants) add(i);
    for (auto& b : req.boosters) add(b.type_id);
    for (auto& c : req.cargo) add(c.type_id);
    std::sort(need.begin(), need.end());
    std::sort(groups.begin(), groups.end());
    auto in = [](const std::vector<uint32_t>& v, uint32_t x) { return std::binary_search(v.begin(), v.end(), x); };
    auto relevant = [&](uint32_t sk, const TypeRec& t) {
      if (in(need, sk)) return true;
      for (const TEff& te : ds.type_effects(t)) {
        if (te.id == 132) continue;  // skillEffect
        const EffRec* e = ds.effect(te.id);
        if (!e) continue;
        if (e->mod_cnt == 0) return true;  // hand-written / special effect
        for (const ModRec& m : ds.effect_mods(*e)) {
          // Only modifiers register_all would actually push to an item other than the skill itself can matter:
          // a skill's own attributes are read only by its own modifiers (sources are always the owning item).
          if (m.func < 0 || m.func > 4 || m.op == 9) continue;
          if (m.domain == 0 || m.domain == 3 || m.domain >= 5) continue;  // self / other (none for skills) / target
          if (m.domain == 4 && !is_structure) continue;
          bool hit;
          switch (m.func) {
            case 2: {
              const GroupRec* g = ds.group(m.extra);
              hit = in(groups, m.extra) || !g || (m.domain == 2 && g->category == 16);
              break;
            }
            case 3:
            case 4: hit = in(need, m.extra == 0 ? sk : m.extra); break;
            default: hit = true;
          }
          if (hit) return true;
        }
      }
      return false;
    };
    // relevance flag per ds.skills index, from the inverted table (same predicate as `relevant`)
    std::vector<uint8_t>& flag = prune_flag_;
    if (!no_prune) {
      {
        auto al = is_structure ? ds.skrel_always_st : ds.skrel_always_ns;
        flag.assign(al.begin(), al.end());
      }
      auto mark = [&](std::span<const uint64_t> lst, const std::vector<uint32_t>& keys) {
        uint32_t prev = 0;
        bool first = true;
        for (uint32_t k : keys) {
          if (!first && k == prev) continue;
          first = false;
          prev = k;
          for (auto it = std::lower_bound(lst.begin(), lst.end(), (uint64_t)k << 32); it != lst.end() && (uint32_t)(*it >> 32) == k; ++it)
            if (!(*it & 1) || is_structure) flag[(uint32_t)*it >> 1] = 1;
        }
      };
      mark(ds.skrel_by_group, groups);
      mark(ds.skrel_by_need, need);
      for (uint32_t k : need) {
        auto it = std::lower_bound(ds.skills.begin(), ds.skills.end(), k);
        if (it != ds.skills.end() && *it == k) flag[(size_t)(it - ds.skills.begin())] = 1;
      }
    }
    size_t sp = 0;  // cursor into ds.skills (lv is sorted by id)
    const size_t nsk = ds.skills.size();
    for (auto& [sk, l] : lv) {
      const TypeRec* st = ds.type(sk);
      if (!st) continue;
      if (!no_prune) {
        while (sp < nsk && ds.skills[sp] < sk) sp++;
        bool r = (sp < nsk && ds.skills[sp] == sk) ? flag[sp] != 0 : relevant(sk, *st);
        if (!r) continue;
      }
      int32_t idx = new_item(sk, Kind::Skill, Loc::Char, "/character/skills", -1, err);
      if (idx < 0) return false;
      set_base(idx, ATTR_SKILL_LEVEL, (double)std::min<uint8_t>(l, 5));
      items[idx].owned = false;
      kept.push_back({sk, l});
    }
    lv.swap(kept);
  }
  // tactical destroyer mode (default: lowest type id whose name starts with the ship name)
  {
    std::optional<uint32_t> mode = req.mode_type_id;
    if (!mode) {
      std::string sn = lower(ds.type_name(*items[ship].t));
      uint32_t best = 0;
      bool found = false;
      // lowercased mode names, once per process (the dataset is immutable)
      static std::vector<std::pair<uint32_t, std::string>> lmodes;
      static std::once_flag lm_once;
      std::call_once(lm_once, [&] {
        for (uint32_t m : ds.modes)
          if (const TypeRec* mt = ds.type(m)) lmodes.push_back({m, lower(ds.type_name(*mt))});
      });
      for (const auto& [m, mn] : lmodes) {
        if (mn.compare(0, sn.size(), sn) == 0 && (!found || m < best)) {
          best = m;
          found = true;
        }
      }
      if (found) {
        warnings.push_back("no tactical mode given; defaulted to type " + std::to_string(best));
        mode = best;
      }
    }
    if (mode) {
      int32_t idx = new_item(*mode, Kind::Mode, Loc::Nowhere, "/ship/mode_type_id", -1, err);
      if (idx < 0) return false;
      items[idx].owned = false;
    }
  }
  for (size_t i = 0; i < req.modules.size(); i++)
    if (!add_module((uint32_t)i, req.modules[i], err)) return false;
  for (size_t i = 0; i < req.drones.size(); i++) {
    const DroneReq& d = req.drones[i];
    int32_t idx = new_item(d.type_id, Kind::Drone, Loc::Space, "/drones/{}", (long)i, err);
    if (idx < 0) return false;
    if (d.mutation) apply_mutation(idx, *d.mutation);
    Item& it = items[idx];
    it.quantity = std::max<uint32_t>(d.quantity, 1);
    it.active_count = std::min(d.active.value_or(0), it.quantity);
    it.state = it.active_count > 0 ? State::Active : State::Offline;
    it.req_index = (int32_t)i;
  }
  for (size_t i = 0; i < req.fighters.size(); i++) {
    const FighterReq& f = req.fighters[i];
    int32_t idx = new_item(f.type_id, Kind::Fighter, Loc::Space, "/fighters/{}", (long)i, err);
    if (idx < 0) return false;
    uint32_t sq = K.fighterSquadronMaxSize;
    uint32_t maxsq = 1;
    if (find(idx, sq) >= 0 || has(idx, sq)) maxsq = (uint32_t)std::max(0.0, base(idx, sq));
    Item& it = items[idx];
    uint32_t q = f.quantity.value_or(maxsq);
    it.quantity = std::clamp<uint32_t>(q, 1, std::max<uint32_t>(maxsq, 1));
    if (f.quantity.value_or(0) > maxsq)
      warnings.push_back("fighters/" + std::to_string(i) + ": squadron size " + std::to_string(f.quantity.value_or(0)) +
                         " capped to " + std::to_string(maxsq));
    it.active_count = f.active ? it.quantity : 0;
    it.state = f.active ? State::Active : State::Offline;
    it.fighter_abilities = f.abilities ? &*f.abilities : default_fighter_abilities(idx);
    it.req_index = (int32_t)i;
  }
  for (size_t i = 0; i < req.implants.size(); i++) {
    int32_t idx = new_item(req.implants[i], Kind::Implant, Loc::Char, "/implants/{}", (long)i, err);
    if (idx < 0) return false;
    items[idx].owned = false;
    items[idx].req_index = (int32_t)i;
  }
  for (size_t i = 0; i < req.boosters.size(); i++) {
    int32_t idx = new_item(req.boosters[i].type_id, Kind::Booster, Loc::Char, "/boosters/{}", (long)i, err);
    if (idx < 0) return false;
    items[idx].owned = false;
    items[idx].booster_side_effects = &req.boosters[i].side_effects;
    items[idx].req_index = (int32_t)i;
  }
  for (size_t i = 0; i < req.env_effects.size(); i++) {
    int32_t idx = new_item(req.env_effects[i], Kind::Beacon, Loc::Nowhere, "/environment/effect_type_ids/{}", (long)i, err);
    if (idx < 0) return false;
    items[idx].owned = false;
  }
  for (size_t i = 0; i < (no_projected ? 0 : req.projected.size()); i++) {
    const Projected& p = req.projected[i];
    if (p.kind == "module") {
      if (p.module) {
        uint32_t n = std::max<uint32_t>(p.amount, 1);
        for (uint32_t k = 0; k < n; k++) {
          int32_t idx = new_item(p.module->type_id, Kind::Projected, Loc::Nowhere, "/projected/{}", (long)i, err);
          if (idx < 0) return false;
          Item& it = items[idx];
          it.owned = false;
          it.state = p.module->state.value_or(State::Active);
          it.has_distance = p.distance_m.has_value();
          it.distance = p.distance_m.value_or(0);
          it.req_index = (int32_t)i;
          if (p.module->charge_type_id) {
            int32_t c = new_item(*p.module->charge_type_id, Kind::Charge, Loc::Nowhere, "/projected/{}/module/charge_type_id", (long)i, err);
            if (c < 0) return false;
            items[c].parent = idx;
            items[c].owned = false;
            items[idx].charge = c;
          }
        }
      }
    } else if (p.kind == "fighter") {
      if (p.fighter) {
        uint32_t n = std::max<uint32_t>(p.amount, 1);
        for (uint32_t k = 0; k < n; k++) {
          int32_t idx = new_item(p.fighter->type_id, Kind::Projected, Loc::Nowhere, "/projected/{}", (long)i, err);
          if (idx < 0) return false;
          uint32_t sq = ds.attr_id("fighterSquadronMaxSize");
          uint32_t maxsq = 1;
          if (sq && has(idx, sq)) {
            double b = base(idx, sq);
            maxsq = b <= 0.0 ? 0u : (b >= 4294967295.0 ? 4294967295u : (uint32_t)b);
          }
          maxsq = std::max<uint32_t>(maxsq, 1);
          Item& it = items[idx];
          it.owned = false;
          it.state = p.fighter->active ? State::Active : State::Offline;
          it.quantity = std::clamp<uint32_t>(p.fighter->quantity.value_or(maxsq), 1, maxsq);
          it.active_count = it.quantity;
          it.has_distance = p.distance_m.has_value();
          it.distance = p.distance_m.value_or(0);
          it.req_index = (int32_t)i;
          it.fighter_abilities = p.fighter->abilities ? &*p.fighter->abilities : default_fighter_abilities(idx);
        }
      }
    } else if (p.kind == "fit") {
      // whole projected fit: compute the source fit on its own, then project each active module / drone as a
      // frozen item carrying the source-modified values
      if (p.fit) {
        Fit src(ds, K);
        EngineError se{};
        if (!src.build(*p.fit, se, true, false)) {
          warnings.push_back("projected[" + std::to_string(i) + "] fit: " + debug_error(se));
          continue;
        }
        struct Frozen {
          uint32_t type_id, copies;
          std::vector<std::pair<uint32_t, double>> vals;
          Kind kind;
          uint32_t qty;
          const std::vector<uint32_t>* abil;
        };
        std::vector<Frozen> frozen;
        for (uint32_t si = 0; si < src.items.size(); si++) {
          const Item& it = src.items[si];
          uint32_t copies = 0;
          if (it.kind == Kind::Module && it.state >= State::Active) copies = 1;
          else if (it.kind == Kind::Drone) copies = it.active_count;
          else if (it.kind == Kind::Fighter && it.state >= State::Active) copies = 1;
          if (copies == 0) continue;
          const std::vector<uint32_t>* abil = nullptr;
          if (it.fighter_abilities) {  // copy: the source fit (and its lists) goes away
            auto cp = std::make_unique<std::vector<uint32_t>>(*it.fighter_abilities);
            abil = cp.get();
            list_store_.push_back(std::move(cp));
          }
          Frozen fz{it.type_id, copies, {}, it.kind, it.quantity, abil};
          for (uint32_t a : src.attr_keys(si)) fz.vals.push_back({a, src.get(si, a)});
          frozen.push_back(std::move(fz));
        }
        for (auto& fz : frozen) {
          uint64_t n = (uint64_t)fz.copies * std::max<uint32_t>(p.amount, 1);
          for (uint64_t k = 0; k < n; k++) {
            int32_t idx = new_item(fz.type_id, Kind::Projected, Loc::Nowhere, "/projected/{}", (long)i, err);
            if (idx < 0) return false;
            Item& it = items[idx];
            it.owned = false;
            it.state = State::Active;
            it.has_distance = p.distance_m.has_value();
            it.distance = p.distance_m.value_or(0);
            it.req_index = (int32_t)i;
            if (fz.kind == Kind::Fighter) {
              it.quantity = fz.qty;
              it.active_count = fz.qty;
              it.fighter_abilities = fz.abil;
            }
            for (auto& [a, v] : fz.vals) set_base(idx, a, v);
          }
        }
      }
    } else if (p.kind == "drone") {
      if (p.drone) {
        uint64_t n = (uint64_t)std::max<uint32_t>(p.amount, 1) * std::max<uint32_t>(p.drone->quantity, 1);
        for (uint64_t k = 0; k < n; k++) {
          int32_t idx = new_item(p.drone->type_id, Kind::Projected, Loc::Nowhere, "/projected/{}", (long)i, err);
          if (idx < 0) return false;
          Item& it = items[idx];
          it.owned = false;
          it.state = State::Active;
          it.has_distance = p.distance_m.has_value();
          it.distance = p.distance_m.value_or(0);
        }
      }
    } else {
      warnings.push_back("projected kind '" + p.kind + "' not supported yet (index " + std::to_string(i) + ")");
    }
  }
  // system security -> securityModifier (default nullsec, like Pyfa)
  {
    std::string sec = lower(req.system_security.value_or("nullsec"));
    const char* src;
    if (sec == "hisec" || sec == "highsec" || sec == "high") src = "hiSecModifier";
    else if (sec == "lowsec" || sec == "low") src = "lowSecModifier";
    else if (sec == "nullsec" || sec == "null" || sec == "wspace" || sec == "wormhole" || sec == "w-space") src = "nullSecModifier";
    else {
      warnings.push_back("unknown system_security '" + sec + "', using nullsec");
      src = "nullSecModifier";
    }
    uint32_t src_id = ds.attr_id(src), dst_id = K.securityModifier;
    if (src_id)
      for (uint32_t i = 0; i < items.size(); i++)
        if (has(i, src_id)) set_base(i, dst_id, base(i, src_id));
  }
  for (auto& o : req.overrides)
    for (uint32_t i = 0; i < items.size(); i++)
      if (items[i].type_id == o.type_id) set_base(i, o.attribute_id, o.value);
  // target lists (item order preserved)
  for (uint32_t i = 0; i < items.size(); i++) {
    const Item& it = items[i];
    if (it.loc == Loc::Ship) ship_loc_.push_back(i);
    if (it.loc == Loc::Char) char_loc_.push_back(i);
    if (it.owned && it.n_req) owned_.push_back(i);
    if ((it.owned || it.loc == Loc::Char) && it.kind != Kind::Skill && it.n_req) char_skill_tgt_.push_back(i);
  }
  {
    auto add_sk = [&](std::vector<uint64_t>& ix, uint32_t i) {
      const Item& it = items[i];
      for (uint32_t k = 0; k < it.n_req; k++) {
        bool dup = false;
        for (uint32_t j = 0; j < k; j++)
          if (it.req_skills[j] == it.req_skills[k]) dup = true;
        if (!dup) ix.push_back((uint64_t)it.req_skills[k] << 32 | i);
      }
    };
    for (uint32_t i : ship_loc_) {
      ix_ship_grp_.push_back((uint64_t)items[i].group << 32 | i);
      add_sk(ix_ship_skill_, i);
    }
    for (uint32_t i : char_loc_) ix_char_grp_.push_back((uint64_t)items[i].group << 32 | i);
    for (uint32_t i : owned_) add_sk(ix_owned_skill_, i);
    for (uint32_t i : char_skill_tgt_) add_sk(ix_char_skill_, i);
    for (auto* v : {&ix_ship_grp_, &ix_char_grp_, &ix_ship_skill_, &ix_owned_skill_, &ix_char_skill_})
      std::sort(v->begin(), v->end());
  }
  register_all(req, no_boosters);
  apply_rah(req);
  return true;
}

// --------------------------------------------------------------------------- registration
void Fit::push_mod(uint32_t target, uint32_t attr, int op, const Src& src, uint32_t source_cat) {
  const AttrRec* info = ds.attr(attr);
  bool stackable = info ? info->stackable : true;
  bool exempt = false;
  for (uint32_t c : EXEMPT_CATEGORIES)
    if (c == source_cat) exempt = true;
  bool pen = !stackable && !exempt;
  uint32_t ai = ensure(target, attr);
  uint32_t mi = (uint32_t)mods_.size();
  mods_.push_back(AMod{UINT32_MAX, (int8_t)op, (uint8_t)pen, src});
  LAttr& a = la_[ai];
  if (a.tail == UINT32_MAX) a.head = mi;
  else mods_[a.tail].next = mi;
  a.tail = mi;
}

template <class F>
void Fit::each_key(const std::vector<uint64_t>& ix, uint32_t key, F&& f) {
  auto it = std::lower_bound(ix.begin(), ix.end(), (uint64_t)key << 32);
  for (; it != ix.end() && (uint32_t)(*it >> 32) == key; ++it) f((uint32_t)*it);
}

// func: 0 Item, 1 Location, 2 LocationGroup, 3 LocationRequiredSkill, 4 OwnerRequiredSkill, else EffectStopper
// domain: 0 Item, 1 Ship, 2 Char, 3 Other, 4 Structure, 5 TargetId, 6 Target, else None
template <class F>
void Fit::for_targets(uint32_t src, int func, int domain, uint32_t extra, F&& f) {
  const Item& s = items[src];
  switch (domain) {
    case 0:
      if (func == 0) f(src);
      return;
    case 3:
      if (s.charge >= 0) f((uint32_t)s.charge);
      else if (s.parent >= 0) f((uint32_t)s.parent);
      return;
    case 1:
    case 4:
      if (domain == 4 && !is_structure) return;
      switch (func) {
        case 0: f(ship); return;
        case 1:
          for (uint32_t i : ship_loc_) f(i);
          return;
        case 2:
          each_key(ix_ship_grp_, extra, f);
          return;
        case 3:
          each_key(ix_ship_skill_, extra, f);
          return;
        case 4:
          each_key(ix_owned_skill_, extra, f);
          return;
        default: return;
      }
    case 2:
      switch (func) {
        case 0: f(chr); return;
        case 1:
          for (uint32_t i : char_loc_) f(i);
          return;
        case 2:
          each_key(ix_char_grp_, extra, f);
          return;
        case 3:
        case 4:
          each_key(ix_char_skill_, extra, f);
          return;
        default: return;
      }
    default: return;
  }
}

// Pyfa default: standard attack on; other abilities (except MWD/evasive/MJD) on only if they come before the
// standard attack in effect order.
const std::vector<uint32_t>* Fit::default_fighter_abilities(uint32_t idx) {
  std::vector<uint32_t> ids;
  for (auto& e : items[idx].effs) ids.push_back(e.id);
  std::sort(ids.begin(), ids.end());
  auto on = std::make_unique<std::vector<uint32_t>>();
  bool std_seen = false;
  for (uint32_t e : ids) {
    const EffRec* er = ds.effect(e);
    if (!er) continue;
    std::string_view n = ds.effect_name(*er);
    if (n.substr(0, 14) != "fighterAbility") continue;
    if (n == "fighterAbilityAttackM") {
      on->push_back(e);
      std_seen = true;
    } else if (!std_seen && n != "fighterAbilityMicroWarpDrive" && n != "fighterAbilityEvasiveManeuvers" &&
               n != "fighterAbilityMicroJumpDrive") {
      on->push_back(e);
    }
  }
  const std::vector<uint32_t>* r = on.get();
  list_store_.push_back(std::move(on));
  return r;
}

// base value of an attribute present on the item (fit-local or type), else 0 (like attrs.get(..).base)
double Fit::pbase(uint32_t i, std::string_view attr_name) const {
  uint32_t a = ds.attr_id(attr_name);
  return a && has(i, a) ? base(i, a) : 0.0;
}

State Fit::effective_state(uint32_t i) const {
  const Item& it = items[i];
  switch (it.kind) {
    case Kind::Charge: return it.parent >= 0 ? items[it.parent].state : State::Online;
    case Kind::Ship:
    case Kind::Char:
    case Kind::Skill:
    case Kind::Implant:
    case Kind::Booster:
    case Kind::Mode:
    case Kind::Beacon: return State::Online;
    case Kind::Drone:
    case Kind::Fighter: return it.active_count > 0 ? State::Active : State::Offline;
    default: return it.state;
  }
}

static inline bool state_ok(uint32_t category, State s) {
  switch (category) {
    case 0:
    case 4: return s >= State::Online;
    case 1: return s >= State::Active;
    case 5: return s >= State::Overheated;
    case 7: return true;
    default: return false;
  }
}

static inline Src attr_src(uint32_t item, uint32_t attr) {
  Src s;
  s.k = Src::Attr;
  s.a = item;
  s.b = attr;
  return s;
}
static inline Src const_src(double v) {
  Src s;
  s.k = Src::Const;
  s.f = v;
  return s;
}

void Fit::register_all(const FitRequest& req, bool no_boosters) {
  const uint32_t n = (uint32_t)items.size();
  for (uint32_t i = 0; i < n; i++) {
    const Kind kind = items[i].kind;
    if (kind == Kind::Projected) {
      register_projected(i);
      continue;
    }
    if (is_structure && (kind == Kind::Drone || kind == Kind::Implant || kind == Kind::Booster)) continue;
    const State state = effective_state(i);
    const uint32_t src_cat = items[i].category;
    const std::span<const TEff> effs = items[i].effs;
    for (const TEff& te : effs) {
      const uint32_t eid = te.id;
      if (eid == EFFECT_SKILL_EFFECT) continue;
      const EffRec* e = ds.effect(eid);
      if (!e) continue;
      if (is_structure && kind == Kind::Skill && !e->all_item_domain) {
        bool ok = false;
        for (uint32_t x : K.structure_ok)
          if (x == eid) ok = true;
        if (!ok) continue;
      }
      if (e->has_fuc) {
        const auto* se = items[i].booster_side_effects;
        if (!se || std::find(se->begin(), se->end(), eid) == se->end()) continue;
      }
      if (kind == Kind::Fighter && e->category != 0) {
        const auto* ab = items[i].fighter_abilities;
        bool used = ab ? std::find(ab->begin(), ab->end(), eid) != ab->end() : te.is_default != 0;
        if (!used) continue;
      }
      // Pyfa 'active' handlers for SDE effects without modifiers (some are target-category in the SDE)
      if (e->mod_cnt == 0 && kind == Kind::Module && state >= State::Active && local_special(i, ds.effect_name(*e), src_cat))
        continue;
      if (kind == Kind::Beacon && ds.effect_name(*e) == "OffensiveDefensiveReduction") {
        incursion_effect(i);
        continue;
      }
      if (!state_ok(e->category, state)) continue;
      // ---- special effects (no modifierInfo in the SDE)
      if (kind == Kind::Fighter && ds.effect_mods(*e).empty()) {
        // fighter self abilities (Pyfa hand-written handlers; via eve-dogma-rs)
        struct FM {
          const char* t;
          const char* a;
          int op;
        };
        static const FM MWD[] = {{"maxVelocity", "fighterAbilityMicroWarpDriveSpeedBonus", 6},
                                 {"signatureRadius", "fighterAbilityMicroWarpDriveSignatureRadiusBonus", 6}};
        static const FM AB[] = {{"maxVelocity", "fighterAbilityAfterburnerSpeedBonus", 6}};
        static const FM EVA[] = {{"maxVelocity", "fighterAbilityEvasiveManeuversSpeedBonus", 6},
                                 {"signatureRadius", "fighterAbilityEvasiveManeuversSignatureRadiusBonus", 6},
                                 {"shieldEmDamageResonance", "fighterAbilityEvasiveManeuversEmResonance", 4},
                                 {"shieldThermalDamageResonance", "fighterAbilityEvasiveManeuversThermResonance", 4},
                                 {"shieldKineticDamageResonance", "fighterAbilityEvasiveManeuversKinResonance", 4},
                                 {"shieldExplosiveDamageResonance", "fighterAbilityEvasiveManeuversExpResonance", 4}};
        std::string_view en = ds.effect_name(*e);
        std::span<const FM> fm;
        if (en == "fighterAbilityMicroWarpDrive") fm = MWD;
        else if (en == "fighterAbilityAfterburner") fm = AB;
        else if (en == "fighterAbilityEvasiveManeuvers") fm = EVA;
        if (!fm.empty()) {
          for (const FM& x : fm) push_mod(i, ds.attr_id(x.t), x.op, attr_src(i, ds.attr_id(x.a)), src_cat);
          continue;
        }
      }
      if (eid == K.e_ab || eid == K.e_mwd) {
        push_mod(ship, 4, 2, attr_src(i, K.massAddition), src_cat);
        Src p;
        p.k = Src::Prop;
        p.a = i;
        p.b = ship;
        p.c = K.speedFactor;
        p.d = K.speedBoostFactor;
        p.e = 4;
        push_mod(ship, K.maxVelocity, 4, p, src_cat);
        if (eid == K.e_mwd) push_mod(ship, K.signatureRadius, 6, attr_src(i, K.signatureRadiusBonus), src_cat);
        continue;
      }
      if (eid == K.e_mjd) {
        push_mod(ship, K.signatureRadius, 6, attr_src(i, K.signatureRadiusBonusPercent), 6);
        continue;
      }
      if (eid == K.e_slot) {
        push_mod(ship, K.hiSlots, 2, attr_src(i, K.hiSlotModifier), src_cat);
        push_mod(ship, K.medSlots, 2, attr_src(i, K.medSlotModifier), src_cat);
        push_mod(ship, K.lowSlots, 2, attr_src(i, K.lowSlotModifier), src_cat);
        continue;
      }
      if (eid == K.e_hp) {
        push_mod(ship, K.turretSlotsLeft, 2, attr_src(i, K.turretHardPointModifier), src_cat);
        push_mod(ship, K.launcherSlotsLeft, 2, attr_src(i, K.launcherHardPointModifier), src_cat);
        continue;
      }
      for (const ModRec& m : ds.effect_mods(*e)) {
        if (m.func < 0 || m.func > 4 || m.op == 9) continue;
        if (m.domain == 5 || m.domain == 6) continue;
        uint32_t extra = m.extra;
        if (extra == 0 && (m.func == 3 || m.func == 4)) extra = items[i].type_id;
        uint32_t cat = src_cat;
        if (eid == K.e_bastion)
          for (uint32_t h : HULL_RESONANCES)
            if (h == m.modified) cat = 6;
        const Src s = attr_src(i, m.modifying);
        for_targets(i, m.func, m.domain, extra, [&](uint32_t t) { push_mod(t, m.modified, m.op, s, cat); });
      }
    }
  }
  register_buffs(req, no_boosters);
}

static double range_factor_local(double optimal, double falloff, bool has_d, double d, bool restricted) {
  if (!has_d) return 1.0;
  if (falloff > 0.0) {
    if (restricted && d > optimal + 3.0 * falloff) return 0.0;
    double x = std::max(d - optimal, 0.0) / falloff;
    return std::pow(0.5, x * x);
  }
  return d <= optimal ? 1.0 : 0.0;
}

void Fit::register_projected(uint32_t i) {
  const uint32_t src_cat = items[i].category;
  const State state = items[i].state;
  const std::span<const TEff> effs = items[i].effs;
  const std::vector<uint32_t>* abilities = items[i].fighter_abilities;
  const double qty = (double)std::max<uint32_t>(items[i].quantity, 1);
  for (const TEff& te : effs) {
    const EffRec* e = ds.effect(te.id);
    if (!e) continue;
    std::string_view ename = ds.effect_name(*e);
    const bool fab = ename.substr(0, 14) == "fighterAbility";
    if (e->category != 2 && e->category != 3 && ename != "ECMBurstJammer" && ename.substr(0, 11) != "doomsdayAOE") continue;
    if (abilities && fab && std::find(abilities->begin(), abilities->end(), te.id) == abilities->end()) continue;
    if (state < State::Active) continue;
    double opt = 0, fo = 0;
    if (e->range_attr && find(i, e->range_attr) >= 0) opt = base(i, e->range_attr);
    else if (e->range_attr && has(i, e->range_attr)) opt = base(i, e->range_attr);
    if (e->falloff_attr && has(i, e->falloff_attr)) fo = base(i, e->falloff_attr);
    double factor = range_factor_local(opt, fo, items[i].has_distance, items[i].distance, true);
    uint32_t resist = e->resistance_attr;
    if (!e->resistance_attr) {
      auto look = [&](uint32_t a) -> uint32_t {
        if (!a || !has(i, a)) return 0;
        double b = base(i, a);
        return b <= 0.0 ? 0u : (b >= 4294967295.0 ? 4294967295u : (uint32_t)b);
      };
      if (fab) {
        std::string en(ename);
        resist = look(ds.attr_id(en + "ResistanceID"));
        if (resist == 0) resist = look(ds.attr_id(en + "RemoteResistanceID"));
      } else {
        resist = look(K.remoteResistanceID);
      }
    }
    uint32_t dom = ds.attr_id("disallowOffensiveModifiers");
    const bool target_offense_ok = !(dom && has(ship, dom)) || base(ship, dom) == 0.0;
    auto push = [&](uint32_t target_attr, uint32_t src_attr, int op) {
      Src s;
      s.k = Src::Proj;
      s.a = i;
      s.b = src_attr;
      s.c = ship;
      s.d = resist;
      s.f = factor;
      s.mul = (op == 4 || op == 0);
      push_mod(ship, target_attr, op, s, src_cat);
    };
    auto mods = ds.effect_mods(*e);
    // burst projectors and the Standup weapon disruptor stay engine-side even if a dataset revision gives them
    // modifiers (as eve-dogma-rs: the generic path has no AoE full-strength rule)
    const bool engine_side = ename.substr(0, 11) == "doomsdayAOE" || ename == "structureModuleEffectWeaponDisruption";
    if (!mods.empty() && !engine_side) {
      for (const ModRec& m : mods)
        if ((m.domain == 5 || m.domain == 6 || m.domain == 1) && m.func == 0) push(m.modified, m.modifying, m.op);
      continue;
    }
    std::string_view name = ename;
    auto starts = [&](std::string_view p) { return name.substr(0, p.size()) == p; };
    auto push_f = [&](uint32_t target_attr, int op, uint32_t src_attr, double f) {
      Src s;
      s.k = Src::Proj;
      s.a = i;
      s.b = src_attr;
      s.c = ship;
      s.d = resist;
      s.f = f;
      s.mul = false;
      push_mod(ship, target_attr, op, s, src_cat);
    };
    if (name == "fighterAbilityStasisWebifier") {
      if (target_offense_ok) {
        double f = range_factor_local(pbase(i, "fighterAbilityStasisWebifierOptimalRange"),
                                      pbase(i, "fighterAbilityStasisWebifierFalloffRange"), items[i].has_distance,
                                      items[i].distance, true) *
                   qty;
        push_f(K.maxVelocity, 6, ds.attr_id("fighterAbilityStasisWebifierSpeedPenalty"), f);
      }
      continue;
    }
    if (name == "fighterAbilityWarpDisruption") {
      if (target_offense_ok && pbase(i, "fighterAbilityWarpDisruptionRange") >= (items[i].has_distance ? items[i].distance : 0.0))
        push_f(K.warpScrambleStatus, 2, ds.attr_id("fighterAbilityWarpDisruptionPointStrength"), qty);
      continue;
    }
    // burst projectors (Pyfa Effect6476-6482/6513, via eve-dogma-rs): full strength in the AoE (no range factor)
    if (name == "doomsdayAOEWeb" || name == "doomsdayAOEPaint" || name == "doomsdayAOEDamp") {
      if (target_offense_ok) {
        if (name == "doomsdayAOEWeb") push_f(K.maxVelocity, 6, K.speedFactor, 1.0);
        else if (name == "doomsdayAOEPaint") push_f(K.signatureRadius, 6, K.signatureRadiusBonus, 1.0);
        else {
          push_f(K.maxTargetRange, 6, K.maxTargetRangeBonus, 1.0);
          push_f(K.scanResolution, 6, K.scanResolutionBonus, 1.0);
        }
      }
      continue;
    }
    if (name == "doomsdayAOENeut") {
      proj_special.push_back(ProjSpecial{false, 0, i, K.energyNeutralizerAmount, K.duration, resist, 1.0, 1.0, 1.0});
      continue;
    }
    if (name == "doomsdayAOEECM") {
      if (target_offense_ok) {
        ProjSpecial ps{false, 0, i, 0, 0, resist, 1.0, 1.0, 0.0};
        ps.ecm = true;
        ps.fighter = false;
        proj_special.push_back(ps);
      }
      continue;
    }
    if (name == "doomsdayAOEBubble" || name == "doomsdayAOEGuide") continue;
    const bool weapon_disruption = name == "doomsdayAOETrack" || name == "structureModuleEffectWeaponDisruption";
    if (starts("remoteWebifier") || name == "structureModuleEffectStasisWebifier") {
      push(K.maxVelocity, K.speedFactor, 6);
    } else if (starts("remoteTargetPaint") || name == "structureModuleEffectTargetPainter") {
      push(K.signatureRadius, K.signatureRadiusBonus, 6);
    } else if (starts("remoteSensorDamp") || name == "structureModuleEffectRemoteSensorDampener" || starts("remoteSensorBoost")) {
      push(K.maxTargetRange, K.maxTargetRangeBonus, 6);
      push(K.scanResolution, K.scanResolutionBonus, 6);
      if (starts("remoteSensorBoost"))
        for (int k = 0; k < 4; k++) push(K.scanStrengthG[k], K.scanStrengthPercent[k], 6);
    } else if (weapon_disruption) {
      // AoE weapon disruption burst (full strength) / Standup Weapon Disruptor (range factor): turrets and missiles
      if (target_offense_ok) {
        const double tf = name == "doomsdayAOETrack"
                              ? 1.0
                              : range_factor_local(pbase(i, "maxRange"), pbase(i, "falloffEffectiveness"),
                                                   items[i].has_distance, items[i].distance, true);
        const uint32_t gun = ds.type_by_name("Gunnery"), mls = ds.type_by_name("Missile Launcher Operation");
        static const char* TDP[][2] = {{"trackingSpeedBonus", "trackingSpeed"}, {"maxRangeBonus", "maxRange"}, {"falloffBonus", "falloff"}};
        static const char* GDP[][2] = {{"aoeCloudSizeBonus", "aoeCloudSize"}, {"aoeVelocityBonus", "aoeVelocity"},
                                       {"missileVelocityBonus", "maxVelocity"}, {"explosionDelayBonus", "explosionDelay"}};
        const size_t n = items.size();
        for (size_t t = 0; t < n; t++) {
          const Item& it = items[t];
          if (it.loc != Loc::Ship || !it.owned) continue;
          const char* (*pairs)[2];
          int np;
          if (it.kind == Kind::Module && it.needs_skill(gun)) {
            pairs = TDP;
            np = 3;
          } else if (it.kind == Kind::Charge && it.needs_skill(mls)) {
            pairs = GDP;
            np = 4;
          } else {
            continue;
          }
          for (int k = 0; k < np; k++) {
            Src s;
            s.k = Src::Proj;
            s.a = i;
            s.b = ds.attr_id(pairs[k][0]);
            s.c = ship;
            s.d = resist;
            s.f = tf;
            s.mul = false;
            push_mod((uint32_t)t, ds.attr_id(pairs[k][1]), 6, s, src_cat);
          }
        }
      }
    } else if (name == "shipModuleTrackingDisruptor" || name == "shipModuleGuidanceDisruptor" ||
               name == "shipModuleRemoteTrackingComputer" || name == "npcEntityWeaponDisruptor") {
      // Pyfa Effect6424 / Effect6423 / shipModuleRemoteTrackingComputer (via eve-dogma-rs): modify the target's
      // gunnery modules (TD, remote tracking computer) / missile charges (GD).
      bool allowed = target_offense_ok;
      if (name == "shipModuleRemoteTrackingComputer") {
        const uint32_t da = ds.attr_id("disallowAssistance");
        allowed = !(da && has(ship, da)) || base(ship, da) == 0.0;
      }
      if (allowed) {
        const bool td = name != "shipModuleGuidanceDisruptor";
        static const char* TDP[][2] = {{"trackingSpeedBonus", "trackingSpeed"}, {"maxRangeBonus", "maxRange"}, {"falloffBonus", "falloff"}};
        static const char* GDP[][2] = {{"aoeCloudSizeBonus", "aoeCloudSize"}, {"aoeVelocityBonus", "aoeVelocity"},
                                       {"missileVelocityBonus", "maxVelocity"}, {"explosionDelayBonus", "explosionDelay"}};
        const uint32_t sk = ds.type_by_name(td ? "Gunnery" : "Missile Launcher Operation");
        // TD drones (Pyfa Effect6694): full strength inside maxRange, nothing beyond
        const double tf = name == "npcEntityWeaponDisruptor"
                              ? (pbase(i, "maxRange") < (items[i].has_distance ? items[i].distance : 0.0) ? 0.0 : 1.0)
                              : range_factor_local(pbase(i, "maxRange"), pbase(i, "falloffEffectiveness"),
                                                   items[i].has_distance, items[i].distance, true);
        uint32_t src_a[4], tgt_a[4];
        const int np = td ? 3 : 4;
        for (int k = 0; k < np; k++) {
          src_a[k] = ds.attr_id(td ? TDP[k][0] : GDP[k][0]);
          tgt_a[k] = ds.attr_id(td ? TDP[k][1] : GDP[k][1]);
        }
        const size_t n = items.size();
        for (size_t t = 0; t < n; t++) {
          const Item& it = items[t];
          if (!(it.loc == Loc::Ship && it.owned && it.kind == (td ? Kind::Module : Kind::Charge) && it.needs_skill(sk))) continue;
          for (int k = 0; k < np; k++) {
            Src s;
            s.k = Src::Proj;
            s.a = i;
            s.b = src_a[k];
            s.c = ship;
            s.d = resist;
            s.f = tf;
            s.mul = false;
            push_mod((uint32_t)t, tgt_a[k], 6, s, src_cat);
          }
        }
      }
    } else {
      std::vector<ProjSpecial> ps;
      if (proj_special_for(i, name, resist, ps)) {
        proj_special.insert(proj_special.end(), ps.begin(), ps.end());
      } else {
        static const char* DAMAGE_EFFECTS[] = {"projectileFired", "targetAttack", "useMissiles", "barrage", "targetDisintegratorAttack",
                                               "missileLaunchingForEntity", "fighterAbilityAttackM", "fighterAbilityMissiles",
                                               "superWeaponAmarr", "superWeaponCaldari", "superWeaponGallente", "superWeaponMinmatar",
                                               "mining", "miningLaser", "miningClouds", "dotMissileLaunching", "ChainLightning", "salvageDroneEffect"};
        bool dmg = false;
        for (auto d : DAMAGE_EFFECTS)
          if (name == d) dmg = true;
        if (!dmg) warnings.push_back("projected effect '" + std::string(name) + "' not modelled yet");
      }
    }
  }
}

// Local module effects that have no modifierInfo in the SDE but a hand-written Pyfa handler (via eve-dogma-rs,
// LGPL). Returns true when the effect was handled. Source category 6 marks a boost Pyfa applies without stacking
// penalty.
bool Fit::local_special(uint32_t i, std::string_view name, uint32_t src_cat) {
  auto a = [&](std::string_view n) { return ds.attr_id(n); };
  if (name == "superWeaponAmarr" || name == "superWeaponCaldari" || name == "superWeaponGallente" ||
      name == "superWeaponMinmatar" || name == "doomsdaySlash" || name == "doomsdayBeamDOT" || name == "doomsdayConeDOT" ||
      name == "doomsdayHOG" || name == "debuffLance") {
    push_mod(ship, a("maxVelocity"), 6, attr_src(i, a("speedFactor")), src_cat);
    push_mod(ship, a("warpScrambleStatus"), 2, attr_src(i, a("siegeModeWarpStatus")), src_cat);
  } else if (name == "emergencyHullEnergizer") {
    static const char* T[4][2] = {{"emDamageResonance", "hullEmDamageResonance"},
                                  {"thermalDamageResonance", "hullThermalDamageResonance"},
                                  {"kineticDamageResonance", "hullKineticDamageResonance"},
                                  {"explosiveDamageResonance", "hullExplosiveDamageResonance"}};
    for (auto& t : T) push_mod(ship, a(t[0]), 4, attr_src(i, a(t[1])), src_cat);
  } else if (name == "entosisLink") {
    push_mod(ship, a("disallowAssistance"), 7, attr_src(i, a("disallowAssistance")), 6);
    static const char* S[4] = {"Gravimetric", "Magnetometric", "Radar", "Ladar"};
    for (auto s : S)
      push_mod(ship, a(std::string("scan") + s + "Strength"), 6, attr_src(i, a(std::string("scan") + s + "StrengthPercent")), src_cat);
  } else if (name == "moduleBonusBreacherPodDamageControl") {
    push_mod(ship, a("breacherPodDamageResistance"), 6, attr_src(i, a("breacherPodActivatedDamageReceivedPercentage")), 6);
  } else if (name == "microJumpPortalDrive" || name == "microJumpPortalDriveCapital") {
    push_mod(ship, a("signatureRadius"), 6, attr_src(i, a("signatureRadiusBonusPercent")), src_cat);
  } else if (name == "warpDisruptSphere") {
    push_mod(ship, a("disallowAssistance"), 7, const_src(1.0), 6);
    if (items[i].charge < 0) {
      push_mod(ship, 4, 6, attr_src(i, a("massBonusPercentage")), 6);
      push_mod(ship, a("signatureRadius"), 6, attr_src(i, a("signatureRadiusBonus")), 6);
      std::vector<uint32_t> props;
      for (uint32_t t = 0; t < items.size(); t++) {
        const Item& it = items[t];
        if (it.kind != Kind::Module || it.loc != Loc::Ship) continue;
        const GroupRec* g = ds.group(it.group);
        if (g && ds.group_name(*g) == "Propulsion Module") props.push_back(t);
      }
      for (uint32_t t : props) {
        push_mod(t, a("speedBoostFactor"), 6, attr_src(i, a("speedBoostFactorBonus")), 6);
        push_mod(t, a("speedFactor"), 6, attr_src(i, a("speedFactorBonus")), 6);
      }
    }
  } else {
    return false;
  }
  return true;
}

// Sansha / Drifter incursion system effects (Pyfa Effect4728 OffensiveDefensiveReduction, LGPL; via eve-dogma-rs):
// unpenalised PostPercent of missile-charge and smartbomb damage, turret and drone damageMultiplier by
// systemEffectDamageReduction, and of the ship's armor/shield resonances by the beacon's resistance bonuses.
void Fit::incursion_effect(uint32_t b) {
  auto a = [&](std::string_view n) { return ds.attr_id(n); };
  const uint32_t red = a("systemEffectDamageReduction");
  const uint32_t mls = ds.type_by_name("Missile Launcher Operation"), gunnery = ds.type_by_name("Gunnery");
  uint32_t smartbomb = 0;
  for (const GroupRec& g : ds.groups)
    if (ds.group_name(g) == "Smart Bomb") {
      smartbomb = g.id;
      break;
    }
  static const char* DMG[4] = {"emDamage", "thermalDamage", "kineticDamage", "explosiveDamage"};
  const size_t n = items.size();
  for (size_t t = 0; t < n; t++) {
    const Item& it = items[t];
    if (!it.owned || (it.loc != Loc::Ship && it.kind != Kind::Drone)) continue;
    bool dmg = false, mult = false;
    if (it.kind == Kind::Charge) dmg = it.needs_skill(mls);
    else if (it.kind == Kind::Module) {
      dmg = it.group == smartbomb;
      mult = it.needs_skill(gunnery);
    } else if (it.kind == Kind::Drone) {
      mult = true;
    }
    if (dmg)
      for (auto d : DMG) push_mod((uint32_t)t, a(d), 6, attr_src(b, red), 6);
    if (mult) push_mod((uint32_t)t, a("damageMultiplier"), 6, attr_src(b, red), 6);
  }
  static const char* D[4] = {"Em", "Thermal", "Kinetic", "Explosive"};
  for (auto d : D)
    for (const char* l : {"armor", "shield"})
      push_mod(ship, a(std::string(l) + d + "DamageResonance"), 6, attr_src(b, a(std::string(l) + d + "DamageResistanceBonus")), 6);
}

// Pyfa's 'projected' handlers for remote reps, cap transfers and neuts/nos (eos/effects.py, LGPL; via eve-dogma-rs).
bool Fit::proj_special_for(uint32_t i, std::string_view name, uint32_t resist, std::vector<ProjSpecial>& out) {
  const Item& it = items[i];
  auto bse = [&](uint32_t a) { return a && has(i, a) ? base(i, a) : 0.0; };
  std::optional<double> dist;
  if (it.has_distance) dist = it.distance;
  auto falloff_factor = [&]() {
    double opt = bse(K.maxRange), fo = bse(K.falloffEffectiveness);
    if (!dist) return 1.0;
    return range_factor_local(opt, fo, true, *dist, true);
  };
  auto gate = [&](double opt) { return opt < dist.value_or(0.0) ? 0.0 : 1.0; };
  bool no_assist = K.disallowAssistance && has(ship, K.disallowAssistance) && base(ship, K.disallowAssistance) != 0.0;
  auto rep = [&](uint8_t layer, uint32_t amt, double mult, double factor) {
    if (!no_assist) out.push_back(ProjSpecial{true, layer, i, amt, 0, 0, mult, factor, 0});
  };
  auto drain = [&](uint32_t amt, uint32_t dur, double factor, double sign) {
    out.push_back(ProjSpecial{false, 0, i, amt, dur, resist, 1.0, factor, sign});
  };
  uint32_t dom = ds.attr_id("disallowOffensiveModifiers");
  const bool no_offense = dom && has(ship, dom) && base(ship, dom) != 0.0;
  auto ecm = [&](bool fighter, double factor) {
    if (!no_offense) {
      ProjSpecial ps{false, 0, i, 0, 0, resist, 1.0, factor, 0.0};
      ps.ecm = true;
      ps.fighter = fighter;
      out.push_back(ps);
    }
  };
  const double fq = (double)std::max<uint32_t>(it.quantity, 1);
  bool paste = it.charge >= 0 && ds.type_name(*items[it.charge].t) == "Nanite Repair Paste";
  if (name == "shipModuleRemoteShieldBooster" || name == "shipModuleAncillaryRemoteShieldBooster") rep(0, K.shieldBonus, 1.0, falloff_factor());
  else if (name == "shipModuleRemoteArmorRepairer" || name == "ShipModuleRemoteArmorMutadaptiveRepairer") rep(1, K.armorDamageAmount, 1.0, falloff_factor());
  else if (name == "shipModuleAncillaryRemoteArmorRepairer") rep(1, K.armorDamageAmount, paste ? 3.0 : 1.0, falloff_factor());
  else if (name == "shipModuleRemoteHullRepairer") rep(2, K.structureDamageAmount, 1.0, falloff_factor());
  else if (name == "npcEntityRemoteShieldBooster") rep(0, K.shieldBonus, 1.0, gate(bse(K.maxRange)));
  else if (name == "npcEntityRemoteArmorRepairer") rep(1, K.armorDamageAmount, 1.0, gate(bse(K.maxRange)));
  else if (name == "npcEntityRemoteHullRepairer") rep(2, K.structureDamageAmount, 1.0, gate(bse(K.maxRange)));
  else if (name == "shipModuleRemoteCapacitorTransmitter") {
    if (!no_assist) drain(K.powerTransferAmount, K.duration, gate(bse(K.maxRange)), -1.0);
  } else if (name == "energyNeutralizerFalloff") drain(K.energyNeutralizerAmount, K.duration, falloff_factor(), 1.0);
  else if (name == "fighterAbilityEnergyNeutralizer") {
    double f = range_factor_local(pbase(i, "fighterAbilityEnergyNeutralizerOptimalRange"),
                                  pbase(i, "fighterAbilityEnergyNeutralizerFalloffRange"), it.has_distance, it.distance, true);
    drain(ds.attr_id("fighterAbilityEnergyNeutralizerAmount"), ds.attr_id("fighterAbilityEnergyNeutralizerDuration"), f * fq, 1.0);
  } else if (name == "remoteECMFalloff" || name == "structureModuleEffectECM") ecm(false, falloff_factor());
  else if (name == "entityECMFalloff") ecm(false, gate(pbase(i, "ECMRangeOptimal")));
  else if (name == "ECMBurstJammer") ecm(false, gate(pbase(i, "ecmBurstRange")));
  else if (name == "fighterAbilityECM") {
    double f = range_factor_local(pbase(i, "fighterAbilityECMRangeOptimal"), pbase(i, "fighterAbilityECMRangeFalloff"),
                                  it.has_distance, it.distance, true);
    ecm(true, f * fq);
  }
  else if (name == "energyNosferatuFalloff") drain(K.powerTransferAmount, K.duration, falloff_factor(), 1.0);
  else if (name == "structureEnergyNeutralizerFalloff") drain(K.energyNeutralizerAmount, K.duration, 1.0, 1.0);
  else if (name == "entityEnergyNeutralizerFalloff")
    drain(K.energyNeutralizerAmount, K.energyNeutralizerDuration, gate(bse(K.energyNeutralizerRangeOptimal)), 1.0);
  else return false;
  return true;
}

void Fit::register_buffs(const FitRequest& req, bool no_boosters) {
  // explicit buffs aggregated per id by the collection's aggregate mode
  std::vector<std::pair<uint32_t, double>> agg;
  for (auto& b : req.buffs) {
    const DbuffRec* info = ds.dbuff(b.buff_id);
    if (!info) {
      warnings.push_back("unknown warfare buff " + std::to_string(b.buff_id));
      continue;
    }
    auto it = std::find_if(agg.begin(), agg.end(), [&](auto& p) { return p.first == b.buff_id; });
    if (it == agg.end()) agg.push_back({b.buff_id, b.value});
    else it->second = info->aggregate_min ? std::min(it->second, b.value) : std::max(it->second, b.value);
  }
  auto in_agg = [&](uint32_t id) {
    for (auto& p : agg)
      if (p.first == id) return true;
    return false;
  };
  // Pyfa keeps, per buff id, the strongest (|value|) source among own bursts and booster fits; explicit buffs override
  struct Best {
    uint32_t id;
    double v;
    Src src;
  };
  std::vector<Best> best;
  auto offer = [&](uint32_t id, double v, const Src& src) {
    for (auto& b : best)
      if (b.id == id) {
        if (!(std::fabs(b.v) >= std::fabs(v))) {
          b.v = v;
          b.src = src;
        }
        return;
      }
    best.push_back({id, v, src});
  };
  const uint32_t n = (uint32_t)items.size();
  for (uint32_t i = 0; i < n; i++) {
    if (items[i].kind != Kind::Module || items[i].state < State::Active) continue;
    for (int k = 0; k < 4; k++) {
      uint32_t ida = K.warfareBuffID[k];
      uint32_t id = has(i, ida) ? (uint32_t)get(i, ida) : 0;
      if (id == 0 || in_agg(id)) continue;
      double v = get(i, K.warfareBuffValue[k]);
      offer(id, v, attr_src(i, K.warfareBuffValue[k]));
    }
  }
  // abyssal weather / AoE cloud beacons (Pyfa weather_* / aoe_beacon_* effects): warfareBuff1/2 of the environment
  // item join the same pool (strongest |value| per buff id)
  for (uint32_t i = 0; i < n; i++) {
    if (items[i].kind != Kind::Beacon) continue;
    bool weather = false;
    for (const TEff& te : items[i].effs)
      if (const EffRec* er = ds.effect(te.id)) {
        std::string_view en = ds.effect_name(*er);
        if (en.substr(0, 8) == "weather_" || en.substr(0, 11) == "aoe_beacon_") weather = true;
      }
    if (!weather) continue;
    for (int k = 0; k < 2; k++) {
      uint32_t ida = K.warfareBuffID[k];
      uint32_t id = has(i, ida) ? (uint32_t)get(i, ida) : 0;
      if (id == 0 || in_agg(id)) continue;
      double v = get(i, K.warfareBuffValue[k]);
      offer(id, v, const_src(v));
    }
  }
  if (!no_boosters)
    for (size_t bk = 0; bk < req.booster_fits.size(); bk++) {
      Fit b(ds, K);
      EngineError be{};
      if (!b.build(req.booster_fits[bk], be, false, true)) {
        warnings.push_back("fleet.booster_fits[" + std::to_string(bk) + "]: " + debug_error(be));
        continue;
      }
      for (uint32_t i = 0; i < b.items.size(); i++) {
        if (b.items[i].kind != Kind::Module || b.items[i].state < State::Active) continue;
        for (int k = 0; k < 4; k++) {
          uint32_t ida = K.warfareBuffID[k];
          uint32_t id = b.has(i, ida) ? (uint32_t)b.get(i, ida) : 0;
          if (id == 0 || in_agg(id)) continue;
          double v = b.get(i, K.warfareBuffValue[k]);
          offer(id, v, const_src(v));
        }
      }
    }
  for (auto& [id, v] : agg) {
    bool found = false;
    for (auto& b : best)
      if (b.id == id) {
        b.v = v;
        b.src = const_src(v);
        found = true;
      }
    if (!found) best.push_back({id, v, const_src(v)});
  }
  std::sort(best.begin(), best.end(), [](const Best& x, const Best& y) { return x.id < y.id; });
  for (auto& b : best) apply_buff(b.id, b.src, b.src.k == Src::Attr ? b.src.a : ship);
  clear_cache();
}

void Fit::apply_buff(uint32_t id, const Src& src, uint32_t) {
  const DbuffRec* info = ds.dbuff(id);
  if (!info) return;
  const int op = info->op;
  // Pyfa applies most buffs stacking-penalised; the abyssal weather resistance/HP/velocity buffs are not
  const uint32_t cat = (id == 90 || id == 93 || id == 94 || id == 95 || id == 96 || id == 98 || id == 99) ? 6 : 0;
  for (uint32_t a : ds.pool(info->item_off, info->item_cnt)) push_mod(ship, a, op, src, cat);
  // AoE cloud / weather buffs also hit drones that require the Drones skill (Pyfa fit.py commandBonus)
  {
    static const char* const D79[] = {"signatureRadius", nullptr};
    static const char* const D90[] = {"shieldEmDamageResonance", "armorEmDamageResonance", "emDamageResonance", nullptr};
    static const char* const D93[] = {"shieldExplosiveDamageResonance", "armorExplosiveDamageResonance", "explosiveDamageResonance", nullptr};
    static const char* const D95[] = {"shieldThermalDamageResonance", "armorThermalDamageResonance", "thermalDamageResonance", nullptr};
    static const char* const D99[] = {"shieldKineticDamageResonance", "armorKineticDamageResonance", "kineticDamageResonance", nullptr};
    static const char* const D94[] = {"shieldCapacity", nullptr};
    static const char* const D96[] = {"armorHP", nullptr};
    static const char* const D97[] = {"maxRange", "falloff", nullptr};
    static const char* const D98[] = {"maxVelocity", nullptr};
    const char* const* da = nullptr;
    switch (id) {
      case 79: da = D79; break;
      case 90: da = D90; break;
      case 93: da = D93; break;
      case 95: da = D95; break;
      case 99: da = D99; break;
      case 94: da = D94; break;
      case 96: da = D96; break;
      case 97: da = D97; break;
      case 98: da = D98; break;
      default: break;
    }
    if (da) {
      std::vector<uint32_t> drones;
      for (uint32_t d = 0; d < items.size(); d++)
        if (items[d].kind == Kind::Drone && items[d].needs_skill(3436)) drones.push_back(d);
      for (uint32_t d : drones)
        for (const char* const* nm = da; *nm; nm++) {
          const uint32_t a = ds.attr_id(*nm);
          if (a != 0) push_mod(d, a, op, src, cat);
        }
    }
  }
  for (uint32_t a : ds.pool(info->loc_off, info->loc_cnt))
    for_targets(ship, 1, 1, 0, [&](uint32_t t) { push_mod(t, a, op, src, cat); });
  auto lg = ds.pool(info->lgrp_off, info->lgrp_cnt);
  for (size_t k = 0; k + 1 < lg.size(); k += 2)
    for_targets(ship, 2, 1, lg[k + 1], [&](uint32_t t) { push_mod(t, lg[k], op, src, cat); });
  auto ls = ds.pool(info->lskill_off, info->lskill_cnt);
  for (size_t k = 0; k + 1 < ls.size(); k += 2)
    for_targets(ship, 3, 1, ls[k + 1], [&](uint32_t t) { push_mod(t, ls[k], op, src, cat); });
}

// Reactive Armor Hardener adaptation: same cycle simulation as Pyfa/eos (LGPL), via eve-dogma-rs.
void Fit::apply_rah(const FitRequest& req) {
  const uint32_t eid = K.e_rah;
  if (eid == 0) return;
  const uint32_t* attrs = K.armorRes;
  std::vector<uint32_t> rahs;
  for (uint32_t i = 0; i < items.size(); i++)
    if (items[i].kind == Kind::Module && items[i].state >= State::Active && items[i].has_effect(eid)) rahs.push_back(i);
  const bool disable = req.rah && *req.rah == "disable";
  Resists dp = req.damage_pattern.value_or(Resists{25, 25, 25, 25});
  const double pattern[4] = {dp.em, dp.thermal, dp.kinetic, dp.explosive};
  for (uint32_t m : rahs) {
    clear_cache();
    double res[4];
    for (int k = 0; k < 4; k++) res[k] = get(m, attrs[k]);
    if (!disable) {
      double basev[4];
      for (int k = 0; k < 4; k++) {
        double g = get(ship, attrs[k]);
        basev[k] = pattern[k] * g;
      }
      double shift = get(m, K.resistanceShiftAmount) / 100.0;
      std::vector<std::array<double, 4>> cycles;
      long loop_start = -20;
      for (int it = 0; it < 50; it++) {
        struct T {
          int k;
          double dmg, r;
        } t[4];
        const int order[4] = {0, 3, 2, 1};
        for (int j = 0; j < 4; j++) t[j] = {order[j], basev[order[j]] * res[order[j]], res[order[j]]};
        std::stable_sort(t, t + 4, [](const T& a, const T& b) { return a.dmg < b.dmg; });
        double c0, c1, c2, c3;
        if (t[2].dmg == 0.0) {
          c0 = 1.0 - t[0].r;
          c1 = 1.0 - t[1].r;
          c2 = 1.0 - t[2].r;
          c3 = -(c0 + c1 + c2);
        } else if (t[1].dmg == 0.0) {
          c0 = 1.0 - t[0].r;
          c1 = 1.0 - t[1].r;
          c2 = -(c0 + c1) / 2.0;
          c3 = c2;
        } else {
          c0 = std::min(shift, 1.0 - t[0].r);
          c1 = std::min(shift, 1.0 - t[1].r);
          c2 = -(c0 + c1) / 2.0;
          c3 = c2;
        }
        res[t[0].k] = t[0].r + c0;
        res[t[1].k] = t[1].r + c1;
        res[t[2].k] = t[2].r + c2;
        res[t[3].k] = t[3].r + c3;
        long found = -1;
        for (size_t ci = 0; ci < cycles.size(); ci++) {
          bool eq = true;
          for (int k = 0; k < 4; k++)
            if (!(std::fabs(res[k] - cycles[ci][k]) <= 1e-6)) eq = false;
          if (eq) {
            found = (long)ci;
            break;
          }
        }
        if (found >= 0) {
          loop_start = found;
          break;
        }
        cycles.push_back({res[0], res[1], res[2], res[3]});
      }
      size_t start = loop_start >= 0 ? (size_t)loop_start : (cycles.size() >= 20 ? cycles.size() - 20 : 0);
      if (start < cycles.size()) {
        size_t cnt = cycles.size() - start;
        for (int k = 0; k < 4; k++) {
          double sum = 0;
          for (size_t ci = start; ci < cycles.size(); ci++) sum += cycles[ci][k];
          res[k] = std::round((sum / (double)cnt) * 1000.0) / 1000.0;
        }
      }
    }
    const uint32_t cat = items[m].category;
    for (int k = 0; k < 4; k++) {
      if (!disable) push_mod(m, attrs[k], 7, const_src(res[k]), cat);
      push_mod(ship, attrs[k], 0, const_src(res[k]), cat);
    }
  }
  clear_cache();
}

}  // namespace evej
