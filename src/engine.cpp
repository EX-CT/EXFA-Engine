#include "engine.hpp"

#include <algorithm>
#include <array>
#include <cmath>
#include <string>

namespace evej {

static constexpr uint32_t EXEMPT_CATEGORIES[6] = {6, 8, 16, 20, 32, 65};
static constexpr uint32_t ATTR_SKILL_LEVEL = 280;
static constexpr uint32_t EFFECT_SKILL_EFFECT = 132;
static constexpr uint32_t HULL_RESONANCES[4] = {113, 111, 109, 110};
static constexpr uint64_t EMPTY = ~0ull;

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
  if (hkeys_.empty()) return -1;
  uint64_t k = hkey(item, attr);
  uint64_t p = hmix(k) & hmask_;
  while (true) {
    uint64_t c = hkeys_[p];
    if (c == k) return (int32_t)hvals_[p];
    if (c == EMPTY) return -1;
    p = (p + 1) & hmask_;
  }
}

void Fit::grow() {
  size_t n = hkeys_.empty() ? 4096 : hkeys_.size() * 2;
  std::vector<uint64_t> ok = std::move(hkeys_);
  std::vector<uint32_t> ov = std::move(hvals_);
  hkeys_.assign(n, EMPTY);
  hvals_.assign(n, 0);
  hmask_ = n - 1;
  for (size_t i = 0; i < ok.size(); i++) {
    if (ok[i] == EMPTY) continue;
    uint64_t p = hmix(ok[i]) & hmask_;
    while (hkeys_[p] != EMPTY) p = (p + 1) & hmask_;
    hkeys_[p] = ok[i];
    hvals_[p] = ov[i];
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
  int32_t f = find(item, attr);
  if (f >= 0) return (uint32_t)f;
  if ((hcount_ + 1) * 2 > hkeys_.size()) grow();
  double b;
  if (!type_base(item, attr, b)) b = ds.attr_default(attr);
  uint64_t k = hkey(item, attr);
  uint64_t p = hmix(k) & hmask_;
  while (hkeys_[p] != EMPTY) p = (p + 1) & hmask_;
  hkeys_[p] = k;
  uint32_t idx = (uint32_t)la_.size();
  hvals_[p] = idx;
  hcount_++;
  la_.push_back(LAttr{b, 0.0, UINT32_MAX, UINT32_MAX, item, attr, 0});
  return idx;
}

void Fit::set_base(uint32_t item, uint32_t attr, double v) {
  uint32_t i = ensure(item, attr);
  LAttr& a = la_[i];
  a.base = v;
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
  if (f >= 0) return la_[f].base;
  double v;
  if (type_base(item, attr, v)) return v;
  return ds.attr_default(attr);
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
  if (info->round2) val = std::round(val * 100.0) / 100.0;
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
    if (a.st == 1) return a.base;
    a.st = 1;
  }
  const uint32_t item = la_[idx].item, attr = la_[idx].attr;
  const AttrRec* info = ds.attr(attr);
  double val = la_[idx].base;
  uint32_t head = la_[idx].head;
  if (head != UINT32_MAX) {
    Val sbuf[48];
    std::vector<Val> hbuf;
    Val* vals = sbuf;
    int n = 0, cap = 48;
    uint8_t opmask = 0;  // bit (op+1)
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
      if (op >= -1 && op <= 7) opmask |= (uint8_t)(1u << (op + 1));
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
    if (la_[find(idx, 4)].base == 0.0 && base_t->mass != 0.0) set_base(idx, 4, base_t->mass);
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

static bool parse_u32_rust(std::string_view s, uint32_t& out) {
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

bool Fit::build(const FitRequest& req, EngineError& err) {
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
    for (auto& [sk, l] : lv) {
      if (!ds.type(sk)) continue;
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
      for (uint32_t m : ds.modes) {
        const TypeRec* mt = ds.type(m);
        if (!mt) continue;
        std::string mn = lower(ds.type_name(*mt));
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
    if (f.abilities) {
      it.fighter_abilities = &*f.abilities;
    } else {
      std::vector<uint32_t> ids;
      for (auto& e : it.effs) ids.push_back(e.id);
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
      it.fighter_abilities = on.get();
      list_store_.push_back(std::move(on));
    }
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
  for (size_t i = 0; i < req.projected.size(); i++) {
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
  register_all(req);
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
          for (uint32_t i : ship_loc_)
            if (items[i].group == extra) f(i);
          return;
        case 3:
          for (uint32_t i : ship_loc_)
            if (items[i].needs_skill(extra)) f(i);
          return;
        case 4:
          for (uint32_t i : owned_)
            if (items[i].needs_skill(extra)) f(i);
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
          for (uint32_t i : char_loc_)
            if (items[i].group == extra) f(i);
          return;
        case 3:
        case 4:
          for (uint32_t i : char_skill_tgt_)
            if (items[i].needs_skill(extra)) f(i);
          return;
        default: return;
      }
    default: return;
  }
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

void Fit::register_all(const FitRequest& req) {
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
      if (!state_ok(e->category, state)) continue;
      // ---- special effects (no modifierInfo in the SDE)
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
  register_buffs(req);
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
  for (const TEff& te : effs) {
    const EffRec* e = ds.effect(te.id);
    if (!e) continue;
    if (e->category != 2 && e->category != 3) continue;
    if (state < State::Active) continue;
    double opt = 0, fo = 0;
    if (e->range_attr && find(i, e->range_attr) >= 0) opt = base(i, e->range_attr);
    else if (e->range_attr && has(i, e->range_attr)) opt = base(i, e->range_attr);
    if (e->falloff_attr && has(i, e->falloff_attr)) fo = base(i, e->falloff_attr);
    double factor = range_factor_local(opt, fo, items[i].has_distance, items[i].distance, true);
    uint32_t resist = e->resistance_attr;
    if (!e->resistance_attr) {
      resist = 0;
      if (K.remoteResistanceID && has(i, K.remoteResistanceID)) resist = (uint32_t)base(i, K.remoteResistanceID);
    }
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
    if (!mods.empty()) {
      for (const ModRec& m : mods)
        if ((m.domain == 5 || m.domain == 6 || m.domain == 1) && m.func == 0) push(m.modified, m.modifying, m.op);
      continue;
    }
    std::string_view name = ds.effect_name(*e);
    auto starts = [&](std::string_view p) { return name.substr(0, p.size()) == p; };
    if (starts("remoteWebifier") || name == "structureModuleEffectStasisWebifier") {
      push(K.maxVelocity, K.speedFactor, 6);
    } else if (starts("remoteTargetPaint") || name == "structureModuleEffectTargetPainter") {
      push(K.signatureRadius, K.signatureRadiusBonus, 6);
    } else if (starts("remoteSensorDamp") || name == "structureModuleEffectRemoteSensorDampener" || starts("remoteSensorBoost")) {
      push(K.maxTargetRange, K.maxTargetRangeBonus, 6);
      push(K.scanResolution, K.scanResolutionBonus, 6);
    } else {
      warnings.push_back("projected effect '" + std::string(name) + "' not modelled yet");
    }
  }
}

void Fit::register_buffs(const FitRequest& req) {
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
  std::sort(agg.begin(), agg.end(), [](auto& x, auto& y) { return x.first < y.first; });
  for (auto& [id, v] : agg) apply_buff(id, const_src(v), ship);
  const uint32_t n = (uint32_t)items.size();
  for (uint32_t i = 0; i < n; i++) {
    if (items[i].kind != Kind::Module || items[i].state < State::Active) continue;
    for (int k = 0; k < 4; k++) {
      uint32_t ida = K.warfareBuffID[k];
      uint32_t id = has(i, ida) ? (uint32_t)get(i, ida) : 0;
      if (id == 0) continue;
      bool explicit_ = false;
      for (auto& b : req.buffs)
        if (b.buff_id == id) explicit_ = true;
      if (explicit_) continue;
      apply_buff(id, attr_src(i, K.warfareBuffValue[k]), i);
    }
  }
}

void Fit::apply_buff(uint32_t id, const Src& src, uint32_t) {
  const DbuffRec* info = ds.dbuff(id);
  if (!info) return;
  const int op = info->op;
  for (uint32_t a : ds.pool(info->item_off, info->item_cnt)) push_mod(ship, a, op, src, 0);
  for (uint32_t a : ds.pool(info->loc_off, info->loc_cnt))
    for_targets(ship, 1, 1, 0, [&](uint32_t t) { push_mod(t, a, op, src, 0); });
  auto lg = ds.pool(info->lgrp_off, info->lgrp_cnt);
  for (size_t k = 0; k + 1 < lg.size(); k += 2)
    for_targets(ship, 2, 1, lg[k + 1], [&](uint32_t t) { push_mod(t, lg[k], op, src, 0); });
  auto ls = ds.pool(info->lskill_off, info->lskill_cnt);
  for (size_t k = 0; k + 1 < ls.size(); k += 2)
    for_targets(ship, 3, 1, ls[k + 1], [&](uint32_t t) { push_mod(t, ls[k], op, src, 0); });
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
