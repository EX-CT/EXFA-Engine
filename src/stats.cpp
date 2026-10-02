// Fit statistics on top of the evaluated dogma graph (same formulas as eve-dogma-rs / Pyfa).
#include "stats.hpp"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <optional>

#include "capsim.hpp"
#include "jsonw.hpp"

namespace evej {

double range_factor(double optimal, double falloff, std::optional<double> distance, bool restricted) {
  if (!distance) return 1.0;
  double d = *distance;
  if (falloff > 0.0) {
    if (restricted && d > optimal + 3.0 * falloff) return 0.0;
    double x = std::max(d - optimal, 0.0) / falloff;
    return std::pow(0.5, x * x);
  }
  return d <= optimal ? 1.0 : 0.0;
}

static std::optional<double> lock_time(double scan_res, double sig) {
  if (scan_res <= 0.0 || sig <= 0.0) return std::nullopt;
  double a = std::asinh(sig);
  return std::min(40000.0 / scan_res / (a * a), 1800.0);
}

static inline double float_unerr(double v) { return std::round(v * 1e9) / 1e9; }

struct SpoolRes {
  double v, cycles, time;
};
static SpoolRes spoolup(double max, double step, double cycle_s, Spool sp) {
  if (max == 0.0 || step == 0.0) return {0, 0, 0};
  double cycles = 0;
  switch (sp.kind) {
    case SpoolType::SpoolScale: cycles = std::ceil(float_unerr(max * sp.amount / step)); break;
    case SpoolType::CycleScale: cycles = std::round(sp.amount * std::ceil(float_unerr(max / step))); break;
    case SpoolType::Time: cycles = std::min(std::floor(float_unerr(sp.amount / cycle_s)), std::ceil(float_unerr(max / step))); break;
    case SpoolType::Cycles: cycles = std::min(std::floor(sp.amount), std::ceil(float_unerr(max / step))); break;
  }
  double v = std::min(cycles * step, max);
  return {v, cycles, cycles * cycle_s};
}

namespace {
struct Dmg {
  double em = 0, th = 0, ki = 0, ex = 0;
  double total() const { return em + th + ki + ex; }
  Dmg scale(double k) const { return {em * k, th * k, ki * k, ex * k}; }
  void add(const Dmg& o) {
    em += o.em;
    th += o.th;
    ki += o.ki;
    ex += o.ex;
  }
  double vs(const Resists& r) const {
    return em * (1.0 - r.em) + th * (1.0 - r.thermal) + ki * (1.0 - r.kinetic) + ex * (1.0 - r.explosive);
  }
  void json(JW& w) const {
    w.obj().kn("em", em).kn("explosive", ex).kn("kinetic", ki).kn("thermal", th).kn("total", total()).end_obj();
  }
};

const char* slot_name(Slot s) {
  switch (s) {
    case Slot::High: return "high";
    case Slot::Mid: return "mid";
    case Slot::Low: return "low";
    case Slot::Rig: return "rig";
    case Slot::Subsystem: return "subsystem";
    case Slot::Service: return "service";
    default: return nullptr;
  }
}
const char* slot_debug(Slot s) {
  switch (s) {
    case Slot::High: return "High";
    case Slot::Mid: return "Mid";
    case Slot::Low: return "Low";
    case Slot::Rig: return "Rig";
    case Slot::Subsystem: return "Subsystem";
    case Slot::Service: return "Service";
    default: return "None";
  }
}
const char* state_name(State s) {
  switch (s) {
    case State::Offline: return "offline";
    case State::Online: return "online";
    case State::Active: return "active";
    default: return "overheated";
  }
}

// Rust `{}` Display for f64: shortest round-trip digits, never exponent notation, "3" for 3.0
std::string rust_display(double v) {
  if (std::isnan(v)) return "NaN";
  if (std::isinf(v)) return v > 0 ? "inf" : "-inf";
  char buf[64];
  auto r = std::to_chars(buf, buf + 64, v, std::chars_format::scientific);
  std::string_view sc(buf, r.ptr - buf);
  std::string out;
  if (sc[0] == '-') {
    out.push_back('-');
    sc.remove_prefix(1);
  }
  size_t ep = sc.find('e');
  int e10 = 0;
  std::from_chars(sc.data() + ep + 1 + (sc[ep + 1] == '+'), sc.data() + sc.size(), e10);
  std::string d;
  for (char c : sc.substr(0, ep))
    if (c != '.') d.push_back(c);
  while (d.size() > 1 && d.back() == '0') d.pop_back();
  if (d == "0") return out + "0";
  int kk = e10 + 1;
  if (kk <= 0) {
    out += "0.";
    out.append((size_t)-kk, '0');
    out += d;
  } else if ((size_t)kk >= d.size()) {
    out += d;
    out.append((size_t)kk - d.size(), '0');
  } else {
    out += d.substr(0, kk);
    out.push_back('.');
    out += d.substr(kk);
  }
  return out;
}
std::string fixed2(double v) {
  char b[64];
  snprintf(b, sizeof b, "%.2f", v);
  return b;
}

struct Calc {
  Fit& f;
  const FitRequest& req;
  const Dataset& ds;
  const Ids& K;
  explicit Calc(Fit& f, const FitRequest& r) : f(f), req(r), ds(f.ds), K(f.K) {}

  double g(uint32_t i, uint32_t a) { return f.get(i, a); }
  bool has_eff(uint32_t i, uint32_t e) const { return e != 0 && f.items[i].has_effect(e); }

  double raw_cycle_ms(uint32_t i) {
    double s = g(i, K.speed);
    double d = g(i, K.duration);
    double v = std::max(s, d);
    for (uint32_t a : K.cycle_extra)
      if (a != 0) v = std::max(v, g(i, a));
    return v;
  }
  uint32_t num_charges(uint32_t i) {
    int32_t c = f.items[i].charge;
    if (c < 0) return 0;
    double vol = g(c, 161);
    double cap = f.base(i, 38);
    return vol <= 0.0 ? 0 : (uint32_t)std::max(0.0, std::floor(float_unerr(cap / vol)));
  }
  uint32_t num_shots(uint32_t i) {
    int32_t c = f.items[i].charge;
    if (c < 0) return 0;
    uint32_t n = num_charges(i);
    if (n > 0 && f.has(i, K.chargeRate)) {
      double r = g(i, K.chargeRate);
      return r > 0.0 ? (uint32_t)std::floor((double)n / r) : 0;
    }
    if (n > 0 && f.has(c, K.crystalsGetDamaged)) {
      if (g(c, K.crystalsGetDamaged) == 1.0) {
        double hp = g(c, 9);
        double chance = g(c, K.crystalVolatilityChance);
        double dmg = g(c, K.crystalVolatilityDamage);
        if (dmg * chance > 0.0) return (uint32_t)std::floor(((double)n * hp) / (dmg * chance));
      }
      return 0;
    }
    return 0;
  }
  double avg_cycle_ms(uint32_t i, bool factor_reload) {
    double active = raw_cycle_ms(i);
    if (active == 0.0) return 0.0;
    double inactive = g(i, K.moduleReactivationDelay);
    uint32_t shots = num_shots(i);
    double reload = g(i, K.reloadTime);
    if (!factor_reload || shots == 0 || inactive >= reload) return active + inactive;
    double early = (double)shots - 1.0;
    return ((active + inactive) * early + (active + reload)) / (double)shots;
  }
  const char* weapon_kind(uint32_t i) const {
    if (has_eff(i, K.e_turret)) return "turret";
    if (has_eff(i, K.e_launcher)) return "missile";
    if (has_eff(i, K.e_empwave)) return "smartbomb";
    if (has_eff(i, K.e_chain)) return "vorton";
    return "other";
  }
  Dmg module_volley(uint32_t i, const char* kind) {
    const Item& it = f.items[i];
    uint32_t src = it.charge >= 0 ? (uint32_t)it.charge : i;
    double mult = f.has(i, K.damageMultiplier) ? g(i, K.damageMultiplier) : 1.0;
    if (kind[0] == 'm' && it.charge >= 0) mult *= g(f.chr, K.missileDamageMultiplier);
    Dmg d;
    d.em = g(src, K.dmg[0]) * mult;
    d.th = g(src, K.dmg[1]) * mult;
    d.ki = g(src, K.dmg[2]) * mult;
    d.ex = g(src, K.dmg[3]) * mult;
    return d;
  }
  const char* fighter_class(uint32_t i) {
    if (g(i, K.fighterSquadronIsHeavy) > 0.0) return "heavy";
    if (g(i, K.fighterSquadronIsSupport) > 0.0) return "support";
    return "light";
  }
  std::string_view tname(uint32_t i) const { return ds.type_name(*f.items[i].t); }

  void idx_or_null(JW& w, int32_t v) {
    if (v < 0) w.null();
    else w.i64(v);
  }

  void dump_attrs(JW& w, uint32_t i) {
    std::vector<std::pair<std::string, uint32_t>> kv;
    for (uint32_t k : f.attr_keys(i)) {
      const AttrRec* a = ds.attr(k);
      kv.push_back({a ? std::string(ds.attr_name(*a)) : std::to_string(k), k});
    }
    std::stable_sort(kv.begin(), kv.end(), [](auto& a, auto& b) { return a.first < b.first; });
    // BTreeMap insert: later duplicate key wins (insertion in id order)
    w.obj();
    for (size_t j = 0; j < kv.size(); j++) {
      if (j + 1 < kv.size() && kv[j + 1].first == kv[j].first) continue;
      w.kn(kv[j].first, g(i, kv[j].second));
    }
    w.end_obj();
  }

  struct Viol {
    const char* code;
    std::string msg;
    int32_t idx;
  };

  void validate(std::vector<Viol>& v, const std::vector<uint32_t>& modules, double cpu, double pg, double calib, double bw) {
    const uint32_t ship = f.ship;
    auto push = [&](const char* c, std::string m, int32_t idx) { v.push_back({c, std::move(m), idx}); };
    double co = g(ship, K.cpuOutput);
    if (cpu > co + 1e-9) push("CPU_OVERLOAD", "CPU used " + fixed2(cpu) + " > output " + fixed2(g(ship, K.cpuOutput)), -1);
    double po = g(ship, K.powerOutput);
    if (pg > po + 1e-9) push("POWER_OVERLOAD", "Powergrid used " + fixed2(pg) + " > output " + fixed2(g(ship, K.powerOutput)), -1);
    double uc = g(ship, K.upgradeCapacity);
    if (calib > uc + 1e-9) push("CALIBRATION_OVERLOAD", "Calibration used " + rust_display(calib) + " > " + rust_display(uc), -1);
    double db = g(ship, K.droneBandwidth);
    if (bw > db + 1e-9) push("DRONE_BANDWIDTH", "Drone bandwidth used " + rust_display(bw) + " > " + rust_display(db), -1);
    const std::pair<Slot, uint32_t> slots[6] = {{Slot::High, K.hiSlots},      {Slot::Mid, K.medSlots},
                                                 {Slot::Low, K.lowSlots},       {Slot::Rig, K.rigSlots},
                                                 {Slot::Subsystem, K.maxSubSystems}, {Slot::Service, K.serviceSlots}};
    for (auto& [s, a] : slots) {
      double used = 0;
      for (uint32_t i : modules)
        if (f.items[i].slot == s) used += 1;
      double tot = g(ship, a);
      if (used > tot) push("SLOTS_EXCEEDED", std::string(slot_debug(s)) + " slots used " + rust_display(used) + " > " + rust_display(tot), -1);
    }
    double t = 0, l = 0;
    for (uint32_t i : modules) {
      if (has_eff(i, K.e_turret)) t += 1;
    }
    double tsl = g(ship, K.turretSlotsLeft);
    if (t > tsl) push("TURRET_HARDPOINTS", "turrets " + rust_display(t) + " > hardpoints " + rust_display(tsl), -1);
    for (uint32_t i : modules)
      if (has_eff(i, K.e_launcher)) l += 1;
    double lsl = g(ship, K.launcherSlotsLeft);
    if (l > lsl) push("LAUNCHER_HARDPOINTS", "launchers " + rust_display(l) + " > hardpoints " + rust_display(lsl), -1);
    const TypeRec& st = *f.items[ship].t;
    std::vector<std::pair<uint32_t, uint32_t>> fitted_group, fitted_type, active_group, online_group;
    auto bump = [](std::vector<std::pair<uint32_t, uint32_t>>& m, uint32_t k) {
      for (auto& p : m)
        if (p.first == k) {
          p.second++;
          return;
        }
      m.push_back({k, 1});
    };
    auto count = [](const std::vector<std::pair<uint32_t, uint32_t>>& m, uint32_t k) -> uint32_t {
      for (auto& p : m)
        if (p.first == k) return p.second;
      return 0;
    };
    for (uint32_t i : modules) {
      const Item& it = f.items[i];
      const int32_t idx = it.req_index;
      const TypeRec& mt = *it.t;
      std::string name(ds.type_name(mt));
      if (it.slot == Slot::None) push("NOT_FITTABLE", name + " is not a fittable module", idx);
      bool any = false, ok = false;
      double x;
      for (uint32_t k = 0; k < K.n_cfg; k++)
        if (ds.type_attr(mt, K.canFitShipGroup[k], x) && (uint32_t)x != 0) {
          any = true;
          if ((uint32_t)x == st.group) ok = true;
        }
      for (uint32_t k = 0; k < K.n_cft; k++)
        if (ds.type_attr(mt, K.canFitShipType[k], x) && (uint32_t)x != 0) {
          any = true;
          if ((uint32_t)x == st.id) ok = true;
        }
      if (any && !ok) push("SHIP_RESTRICTION", name + " cannot be fitted to " + std::string(ds.type_name(st)), idx);
      if (it.slot == Slot::Rig) {
        double rs = 0;
        if (!ds.type_attr(mt, K.rigSize, rs)) rs = 0;
        double srs = g(ship, K.rigSize);
        if (rs != 0.0 && rs != srs) push("RIG_SIZE", name + " rig size " + rust_display(rs) + " != ship rig size " + rust_display(srs), idx);
      }
      bump(fitted_group, it.group);
      bump(fitted_type, it.type_id);
      if (it.state >= State::Online) bump(online_group, it.group);
      if (it.state >= State::Active) bump(active_group, it.group);
      auto check = [&](uint32_t a, const std::vector<std::pair<uint32_t, uint32_t>>& m, uint32_t key, double& lim, uint32_t& n) {
        if (!a || !ds.type_attr(mt, a, lim)) return false;
        n = count(m, key);
        return lim > 0.0 && (double)n > lim;
      };
      double lim;
      uint32_t n;
      if (check(K.maxGroupFitted, fitted_group, it.group, lim, n))
        push("MAX_GROUP_FITTED", name + ": " + std::to_string(n) + " fitted of group, max " + rust_display(lim), idx);
      if (check(K.maxTypeFitted, fitted_type, it.type_id, lim, n))
        push("MAX_TYPE_FITTED", name + ": " + std::to_string(n) + " fitted, max " + rust_display(lim), idx);
      if (check(K.maxGroupOnline, online_group, it.group, lim, n))
        push("MAX_GROUP_ONLINE", name + ": " + std::to_string(n) + " online of group, max " + rust_display(lim), idx);
      if (check(K.maxGroupActive, active_group, it.group, lim, n))
        push("MAX_GROUP_ACTIVE", name + ": " + std::to_string(n) + " active of group, max " + rust_display(lim), idx);
      if (it.charge >= 0) {
        const TypeRec& ct = *f.items[it.charge].t;
        std::string cname(ds.type_name(ct));
        bool cg_ok = false;
        for (uint32_t a : K.chargeGroup)
          if (a && ds.type_attr(mt, a, x) && (uint32_t)x != 0 && (uint32_t)x == ct.group) cg_ok = true;
        if (!cg_ok) push("CHARGE_GROUP", cname + " cannot be loaded into " + name, idx);
        double ms, cs;
        if (K.chargeSize && ds.type_attr(mt, K.chargeSize, ms) && ds.type_attr(ct, K.chargeSize, cs) && ms != cs)
          push("CHARGE_SIZE", cname + " size " + rust_display(cs) + " != launcher size " + rust_display(ms), idx);
        if (ct.volume > mt.capacity && mt.capacity > 0.0) push("CHARGE_CAPACITY", cname + " does not fit into " + name, idx);
      }
    }
    // skills
    std::vector<std::pair<uint32_t, double>> have;
    for (uint32_t i = 0; i < f.items.size(); i++)
      if (f.items[i].kind == Kind::Skill) have.push_back({f.items[i].type_id, f.base(i, 280)});
    std::sort(have.begin(), have.end(), [](auto& a, auto& b) { return a.first < b.first; });
    auto have_lvl = [&](uint32_t s) {
      auto it = std::lower_bound(have.begin(), have.end(), s, [](auto& p, uint32_t x) { return p.first < x; });
      return it != have.end() && it->first == s ? it->second : 0.0;
    };
    struct Miss {
      uint32_t s;
      double need;
      uint32_t by;
    };
    std::vector<Miss> missing;
    for (const Item& it : f.items) {
      switch (it.kind) {
        case Kind::Ship:
        case Kind::Module:
        case Kind::Charge:
        case Kind::Drone:
        case Kind::Fighter:
        case Kind::Implant:
        case Kind::Booster: break;
        default: continue;
      }
      const TypeRec& t = *it.t;
      for (int k = 0; k < 6; k++) {
        double sv = 0;
        if (!K.requiredSkill[k] || !ds.type_attr(t, K.requiredSkill[k], sv)) sv = 0;
        uint32_t s = (uint32_t)sv;
        if (s == 0) continue;
        double need = 1.0;
        if (!K.requiredSkillLevel[k] || !ds.type_attr(t, K.requiredSkillLevel[k], need)) need = 1.0;
        bool dup = false;
        for (auto& m : missing)
          if (m.s == s && m.need >= need) dup = true;
        if (have_lvl(s) < need && !dup) missing.push_back({s, need, it.type_id});
      }
    }
    for (auto& m : missing) {
      const TypeRec* st2 = ds.type(m.s);
      std::string sn = st2 ? std::string(ds.type_name(*st2)) : "?";
      push("MISSING_SKILL", sn + " " + rust_display(m.need) + " required by " + std::string(ds.type_name(*ds.type(m.by))), -1);
    }
  }

  void run(JW& w);
};

void Calc::run(JW& w) {
  const uint32_t ship = f.ship, ch = f.chr;
  const bool factor_reload = req.factor_reload;
  std::vector<uint32_t> modules, drones, fighters;
  for (uint32_t i = 0; i < f.items.size(); i++) {
    Kind k = f.items[i].kind;
    if (k == Kind::Module) modules.push_back(i);
    else if (k == Kind::Drone) drones.push_back(i);
    else if (k == Kind::Fighter) fighters.push_back(i);
  }
  auto online = [&](uint32_t i) { return f.items[i].state >= State::Online; };
  auto active = [&](uint32_t i) { return f.items[i].state >= State::Active; };

  // ---------------- resources
  double cpu_used = -0.0, pg_used = -0.0, calib_used = -0.0, bw_used = -0.0, bay_used = -0.0, fbay_used = -0.0, cargo_used = -0.0;
  for (uint32_t i : modules)
    if (online(i)) cpu_used += g(i, K.cpu);
  for (uint32_t i : modules)
    if (online(i)) pg_used += g(i, K.power);
  for (uint32_t i : modules)
    if (f.items[i].slot == Slot::Rig) calib_used += g(i, K.upgradeCost);
  for (uint32_t i : drones) bw_used += g(i, K.droneBandwidthUsed) * (double)f.items[i].active_count;
  for (uint32_t i : drones) bay_used += g(i, 161) * (double)f.items[i].quantity;
  for (uint32_t i : fighters) fbay_used += g(i, 161) * (double)f.items[i].quantity;
  for (auto& c : req.cargo) {
    const TypeRec* t = ds.type(c.type_id);
    cargo_used += (t ? t->volume : 0.0) * (double)c.quantity;
  }
  double slot_cnt[6] = {0, 0, 0, 0, 0, 0};
  double turrets_used = 0, launchers_used = 0;
  for (uint32_t i : modules) {
    int s = (int)f.items[i].slot;
    if (s >= 0) slot_cnt[s] += 1;
    if (has_eff(i, K.e_turret)) turrets_used += 1;
    if (has_eff(i, K.e_launcher)) launchers_used += 1;
  }
  double tubes_used = 0;
  for (uint32_t i : fighters)
    if (f.items[i].active_count > 0) tubes_used += 1;
  double r_cpu_out = g(ship, K.cpuOutput), r_pow_out = g(ship, K.powerOutput), r_upg = g(ship, K.upgradeCapacity);
  double r_dbw = g(ship, K.droneBandwidth), r_dcap = g(ship, K.droneCapacity), r_fcap = g(ship, K.fighterCapacity);
  double r_cargo = g(ship, 38);
  double s_tot[6] = {g(ship, K.hiSlots), g(ship, K.medSlots), g(ship, K.lowSlots), 0, 0, 0};
  s_tot[3] = g(ship, K.rigSlots);
  s_tot[4] = g(ship, K.maxSubSystems);
  s_tot[5] = g(ship, K.serviceSlots);
  double hp_turret = g(ship, K.turretSlotsLeft), hp_launcher = g(ship, K.launcherSlotsLeft);
  double ft_total = g(ship, K.fighterTubes);
  auto class_used = [&](const char* c) {
    double n = 0;
    for (uint32_t i : fighters)
      if (f.items[i].active_count > 0 && std::string_view(fighter_class(i)) == c) n += 1;
    return n;
  };
  double cu_light = class_used("light");
  double ft_light = g(ship, K.fighterLightSlots);
  double cu_support = class_used("support");
  double ft_support = g(ship, K.fighterSupportSlots);
  double cu_heavy = class_used("heavy");
  double ft_heavy = g(ship, K.fighterHeavySlots);

  // ---------------- offense (computed into a side buffer, emitted later in key order)
  TargetProfile tp = req.target_profile.value_or(TargetProfile{});
  Resists tp_res{tp.em, tp.thermal, tp.kinetic, tp.explosive};
  Spool default_spool = req.default_spool.value_or(Spool{SpoolType::SpoolScale, 1.0});
  JW wo;  // weapons array
  wo.arr();
  Dmg w_vol, w_dps;
  for (uint32_t i : modules) {
    if (!active(i)) continue;
    const char* kind = weapon_kind(i);
    Dmg base = module_volley(i, kind);
    if (base.total() == 0.0) continue;
    double cyc = avg_cycle_ms(i, factor_reload);
    double raw = raw_cycle_ms(i);
    Spool sp = f.items[i].spool ? *f.items[i].spool : default_spool;
    double smax = g(i, K.damageMultiplierBonusMax);
    double sstep = g(i, K.damageMultiplierBonusPerCycle);
    double spv = spoolup(smax, sstep, raw / 1000.0, sp).v;
    Dmg vol_spooled = base.scale(1.0 + spv);
    Dmg dps = cyc > 0.0 ? vol_spooled.scale(1000.0 / cyc) : Dmg{};
    w_vol.add(vol_spooled);
    w_dps.add(dps);
    double opt = g(i, K.maxRange);
    double fo = g(i, K.falloff);
    std::string_view k(kind);
    double tracking = 0, range_m = 0, exr = 0, exv = 0;
    bool has_missile = false, has_range = true;
    if (k == "turret") tracking = g(i, K.trackingSpeed);
    else if (k == "missile") {
      if (f.items[i].charge >= 0) {
        uint32_t c = f.items[i].charge;
        // Pyfa missileMaxRangeData: flight time + ship radius, acceleration phase, floor/ceil blend, FoF limit,
        // centre-to-surface (eos/saveddata/module.py, LGPL; via eve-dogma-rs)
        double vel = g(c, K.maxVelocity);
        has_range = false;
        if (vel > 0.0) {
          double radius = g(ship, K.radius);
          double ft = g(c, K.explosionDelay) / 1000.0 + radius / vel;
          ft = std::round(ft * 1e9) / 1e9;
          double cm = g(c, K.mass);
          double ca = g(c, K.agilityA);
          double accel_cap = cm * ca / 1e6;
          auto range_at = [&](double t) {
            double acc = std::min(t, accel_cap);
            return vel / 2.0 * acc + vel * (t - acc);
          };
          double lt = std::floor(ft), ht = std::ceil(ft);
          double lr = range_at(lt), hr = range_at(ht);
          if (has_eff(c, K.e_fof)) {
            double lim = g(c, K.maxFOFTargetRange);
            if (lim > 0.0) {
              lr = std::min(lr, lim);
              hr = std::min(hr, lim);
            }
          }
          lr = std::max(lr - radius, 0.0);
          hr = std::max(hr - radius, 0.0);
          double hc = ft - lt;
          range_m = lr * (1.0 - hc) + hr * hc;
          has_range = true;
        }
        exr = g(c, K.aoeCloudSize);
        exv = g(c, K.aoeVelocity);
        has_missile = true;
      }
    } else if (k == "smartbomb") range_m = g(i, K.empFieldRange);
    const Item& it = f.items[i];
    wo.obj();
    wo.key("charge_type_id");
    if (it.charge >= 0) wo.i64(f.items[it.charge].type_id);
    else wo.null();
    wo.kn("cycle_time_ms", cyc);
    wo.key("dps");
    dps.json(wo);
    if (has_missile) wo.kn("explosion_radius", exr).kn("explosion_velocity", exv);
    if (k == "turret") wo.kn("falloff_m", fo);
    wo.ks("kind", kind);
    wo.key("module_index");
    idx_or_null(wo, it.req_index);
    wo.ks("name", tname(i));
    if (k == "turret") wo.kn("optimal_m", opt);
    if ((has_missile && has_range) || k == "smartbomb") wo.kn("range_m", range_m);
    if (spv > 0.0) wo.kn("spool_multiplier", 1.0 + spv);
    if (k == "turret") wo.kn("tracking", tracking);
    wo.ki("type_id", it.type_id);
    wo.key("volley");
    vol_spooled.json(wo);
    if (spv > 0.0) {
      wo.key("volley_unspooled");
      base.json(wo);
    }
    wo.end_obj();
  }
  wo.end_arr();
  Dmg d_vol, d_dps;
  JW wd;
  wd.arr();
  for (uint32_t i : drones) {
    double n = (double)f.items[i].active_count;
    if (n == 0.0) continue;
    double mult = f.has(i, K.damageMultiplier) ? g(i, K.damageMultiplier) : 1.0;
    Dmg v;
    v.em = g(i, K.dmg[0]);
    v.th = g(i, K.dmg[1]);
    v.ki = g(i, K.dmg[2]);
    v.ex = g(i, K.dmg[3]);
    v = v.scale(mult * n);
    double cyc = raw_cycle_ms(i);
    if (v.total() == 0.0 || cyc == 0.0) continue;
    Dmg dps = v.scale(1000.0 / cyc);
    d_vol.add(v);
    d_dps.add(dps);
    wd.obj().kn("count", n).key("dps");
    dps.json(wd);
    wd.key("drone_index");
    idx_or_null(wd, f.items[i].req_index);
    wd.ks("name", tname(i)).ki("type_id", f.items[i].type_id).key("volley");
    v.json(wd);
    wd.end_obj();
  }
  wd.end_arr();
  Dmg f_vol, f_dps;
  JW wf;
  wf.arr();
  for (uint32_t i : fighters) {
    double n = (double)f.items[i].active_count;
    if (n == 0.0) continue;
    Dmg fv, fd;
    for (int which = 0; which < 2; which++) {
      uint32_t eid = which == 0 ? K.e_fam : K.e_fmi;
      const uint32_t* at = which == 0 ? K.fam : K.fmi;
      const TEff* te = nullptr;
      for (auto& e : f.items[i].effs)
        if (e.id == eid) {
          te = &e;
          break;
        }
      if (!te) continue;
      const auto* ab = f.items[i].fighter_abilities;
      bool used = ab ? std::find(ab->begin(), ab->end(), eid) != ab->end() : te->is_default != 0;
      if (!used) continue;
      double m = g(i, at[0]);
      if (m == 0.0) m = 1.0;
      Dmg v;
      v.em = g(i, at[1]);
      v.th = g(i, at[2]);
      v.ki = g(i, at[3]);
      v.ex = g(i, at[4]);
      v = v.scale(m * n);
      double dur = g(i, at[5]);
      fv.add(v);
      if (dur > 0.0) fd.add(v.scale(1000.0 / dur));
    }
    if (fv.total() > 0.0) {
      f_vol.add(fv);
      f_dps.add(fd);
      wf.obj().key("dps");
      fd.json(wf);
      wf.key("fighter_index");
      idx_or_null(wf, f.items[i].req_index);
      wf.ks("name", tname(i)).kn("squadron_size", n).ki("type_id", f.items[i].type_id).key("volley");
      fv.json(wf);
      wf.end_obj();
    }
  }
  wf.end_arr();
  Dmg t_vol = w_vol, t_dps = w_dps;
  t_vol.add(d_vol);
  t_vol.add(f_vol);
  t_dps.add(d_dps);
  t_dps.add(f_dps);

  // ---------------- defense
  Resists dp = req.damage_pattern.value_or(Resists{25, 25, 25, 25});
  const double dp_tot = std::max(dp.em + dp.thermal + dp.kinetic + dp.explosive, 1e-12);
  auto layer = [&](const uint32_t* ids, double* r) {
    for (int k = 0; k < 4; k++) r[k] = g(ship, ids[k]);
  };
  auto effectivify = [&](double amount, const double* r) {
    double div = (dp.em * r[0] + dp.thermal * r[1] + dp.kinetic * r[2] + dp.explosive * r[3]) / dp_tot;
    return div == 0.0 ? amount : amount / div;
  };
  double rs[4], ra[4], rh[4];
  layer(K.shieldRes, rs);
  layer(K.armorRes, ra);
  layer(K.hullRes, rh);
  double hp_s = g(ship, K.shieldCapacity), hp_a = g(ship, K.armorHP), hp_h = g(ship, 9);
  double e_s = effectivify(hp_s, rs), e_a = effectivify(hp_a, ra), e_h = effectivify(hp_h, rh);
  double shield_rep = 0, armor_rep = 0, hull_rep = 0;
  for (uint32_t i : modules) {
    if (!active(i)) continue;
    double dur = g(i, K.duration) / 1000.0;
    if (dur <= 0.0) continue;
    if (has_eff(i, K.e_shieldBoosting) || has_eff(i, K.e_fueledShieldBoosting)) shield_rep += g(i, K.shieldBonus) / dur;
    if (has_eff(i, K.e_armorRepair)) armor_rep += g(i, K.armorDamageAmount) / dur;
    if (has_eff(i, K.e_fueledArmorRepair)) {
      bool paste = f.items[i].charge >= 0 && tname(f.items[i].charge) == "Nanite Repair Paste";
      armor_rep += g(i, K.armorDamageAmount) * (paste ? 3.0 : 1.0) / dur;
    }
    if (has_eff(i, K.e_structureRepair)) hull_rep += g(i, K.structureDamageAmount) / dur;
  }
  // incoming remote repairs (Pyfa __getAppliedRr diminishing-returns formula)
  {
    std::vector<std::pair<double, double>> lists[3];
    for (auto& ps : f.proj_special) {
      if (!ps.rep) continue;
      double dur = g(ps.item, K.duration) / 1000.0;
      if (dur > 0.0) {
        double amt = g(ps.item, ps.amount);
        lists[ps.layer].push_back({amt * ps.mult * ps.factor, dur});
      }
    }
    auto applied = [](const std::vector<std::pair<double, double>>& l) {
      double total = -0.0;
      for (auto& [a, c] : l) total += a / std::trunc(c);
      double sum = -0.0;
      for (auto& [a, c] : l) {
        double rrps = a / std::trunc(c);
        double m = 7000.0 + rrps * 20.0;
        double q = ((rrps + m) / (total + m)) - 1.0;
        sum += (1.0 - q * q) * a / c;
      }
      return sum;
    };
    shield_rep += applied(lists[0]);
    armor_rep += applied(lists[1]);
    hull_rep += applied(lists[2]);
  }
  double shield_rr_s = g(ship, K.shieldRechargeRate) / 1000.0;
  double passive = shield_rr_s > 0.0 ? 10.0 / shield_rr_s * 0.5 * 0.5 * hp_s : 0.0;

  // ---------------- capacitor
  double cap = g(ship, K.capacitorCapacity);
  double rr = g(ship, K.rechargeRate);
  double peak = rr > 0.0 ? 10.0 / (rr / 1000.0) * 0.5 * 0.5 * cap : 0.0;
  std::vector<Drain> drains;
  double cap_used = 0, cap_added = 0;
  JW wm;
  wm.arr();
  for (uint32_t i : modules) {
    double cap_need = g(i, K.capacitorNeed);
    const GroupRec* gr = ds.group(f.items[i].group);
    bool is_inj = gr && ds.group_name(*gr) == "Capacitor Booster";
    if (is_inj) cap_need = -(f.items[i].charge >= 0 ? g(f.items[i].charge, K.capacitorBonus) : 0.0);
    if (has_eff(i, K.e_nos) && !req.nos_no_target_cap) cap_need = -g(i, K.powerTransferAmount);
    double cyc_raw = raw_cycle_ms(i);
    double full = cyc_raw + g(i, K.moduleReactivationDelay);
    double cpu = g(i, K.cpu), pw = g(i, K.power);
    bool use_row = active(i) && cap_need != 0.0 && full > 0.0;
    double use_ = 0;
    if (use_row) {
      double avg = avg_cycle_ms(i, factor_reload);
      use_ = avg > 0.0 ? cap_need / (avg / 1000.0) : 0.0;
      if (use_ > 0.0) cap_used += use_;
      else cap_added -= use_;
      uint32_t shots = num_shots(i);
      double rl = g(i, K.reloadTime);
      drains.push_back(Drain{std::trunc(full), cap_need, shots, rl, is_inj, has_eff(i, K.e_turret)});
    }
    const Item& it = f.items[i];
    wm.obj();
    if (use_row) wm.kn("cap_use_gj_s", use_);
    wm.kn("cpu", cpu);
    if (cyc_raw > 0.0) wm.kn("cycle_time_ms", cyc_raw);
    wm.key("module_index");
    idx_or_null(wm, it.req_index);
    wm.ks("name", tname(i)).kn("power", pw).key("slot");
    if (const char* sn = slot_name(it.slot)) wm.str(sn);
    else wm.null();
    wm.ks("state", state_name(it.state)).ki("type_id", it.type_id).end_obj();
  }
  wm.end_arr();
  // incoming neuts / nos / cap transfers (Pyfa fit.addDrain): no stagger, after the fit's own modules
  {
    double sig_now = g(ship, K.signatureRadius);
    for (auto& ps : f.proj_special) {
      if (ps.rep) continue;
      double need = g(ps.item, ps.amount) * ps.factor * ps.sign;
      if (ps.resist != 0) need *= g(ship, ps.resist);
      double sres = g(ps.item, K.energyNeutralizerSignatureResolution);
      if (sres != 0.0) need *= std::min(sig_now / sres, 1.0);
      double dur = g(ps.item, ps.duration);
      if (need != 0.0 && dur > 0.0) {
        if (need > 0.0) cap_used += need / (std::trunc(dur) / 1000.0);
        else cap_added -= need / (std::trunc(dur) / 1000.0);
      }
      if (need != 0.0 && dur > 0.0) drains.push_back(Drain{std::trunc(dur), need, 0, 0.0, false, false});
    }
  }
  bool cs_stable = true, cs_have_sim = false;
  double cs_percent = 100.0, cs_depletes = 0, cs_eve = 0;
  uint64_t cs_iter = 0;
  bool cs_has_percent = true;
  if (!drains.empty()) {
    cs_have_sim = true;
    CapResult r = simulate(cap, rr, drains, 1.0, req.cs_reload || factor_reload, true, req.cs_max_time_s.value_or(6.0 * 3600.0) * 1000.0);
    double st = (r.stable_low + r.stable_high) / 2.0;
    cs_stable = r.stable && st > 0.0;
    if (cs_stable) cs_percent = std::min(st * 100.0, 100.0);
    else {
      cs_has_percent = false;
      cs_depletes = r.t_s;
    }
    cs_eve = r.eve_stable * 100.0;
    cs_iter = r.iterations;
  }

  // ---------------- sustainable tank (Pyfa Fit.sustainableTank semantics, via eve-dogma-rs): when the capacitor is
  // not stable (or reload is factored), local cap-using repairers only run as far as peak recharge allows.
  double sus[3] = {shield_rep, armor_rep, hull_rep};
  if (!cs_stable || factor_reload) {
    auto spec = [&](uint32_t i, uint32_t& attr, bool& asb) -> int {
      asb = false;
      const GroupRec* gr = ds.group(f.items[i].group);
      if (!gr) return -1;
      std::string_view gn = ds.group_name(*gr);
      if (gn == "Shield Booster" || gn == "Ancillary Shield Booster") {
        attr = K.shieldBonus;
        asb = gn == "Ancillary Shield Booster";
        return 0;
      }
      if (gn == "Armor Repair Unit" || gn == "Ancillary Armor Repairer") {
        attr = K.armorDamageAmount;
        return 1;
      }
      if (gn == "Hull Repair Unit") {
        attr = K.structureDamageAmount;
        return 2;
      }
      return -1;
    };
    auto is_paste = [&](uint32_t i) { return f.items[i].charge >= 0 && tname(f.items[i].charge) == "Nanite Repair Paste"; };
    auto cadm = [&](uint32_t i) {
      double m = g(i, K.chargedArmorDamageMultiplier);
      return m == 0.0 ? 1.0 : m;
    };
    double adj[3] = {0.0, 0.0, 0.0};
    double used = cap_used;
    struct Rep {
      uint32_t i;
      int l;
      uint32_t attr;
      double cap_use, eff;
    };
    std::vector<Rep> reps;
    for (int layer = 0; layer < 3; layer++) {
      for (uint32_t i : modules) {
        if (!active(i)) continue;
        uint32_t attr = 0;
        bool asb;
        int l = spec(i, attr, asb);
        if (l < 0 || l != layer) continue;
        double cn = g(i, K.capacitorNeed);
        double avg = avg_cycle_ms(i, factor_reload);
        double cap_use = (cn != 0.0 && avg > 0.0) ? cn / (avg / 1000.0) : 0.0;
        double cyc = raw_cycle_ms(i);
        if (cyc <= 0.0) continue;
        double amount = g(i, attr);
        if (cap_use != 0.0) {
          used -= cap_use;
          double mult = is_paste(i) ? cadm(i) : 1.0;
          adj[l] -= amount * mult / (cyc / 1000.0);
          reps.push_back(Rep{i, l, attr, cap_use, g(i, attr) * cadm(i) / g(i, K.capacitorNeed)});
        } else if (asb) {
          double reload = (factor_reload && f.items[i].charge >= 0) ? g(i, K.reloadTime) : 0.0;
          double shots = (double)std::max<uint32_t>(num_shots(i), 1);
          double off = reload / (shots * cyc + reload);
          adj[l] -= amount * off / (cyc / 1000.0);
        }
      }
    }
    std::stable_sort(reps.begin(), reps.end(), [](const Rep& a, const Rep& b) { return a.eff > b.eff; });
    double total_peak = peak + cap_added;
    for (auto& r : reps) {
      if (used > total_peak) break;
      uint32_t i = r.i;
      bool has_charge = f.items[i].charge >= 0;
      double reload = (factor_reload && has_charge) ? g(i, K.reloadTime) : 0.0;
      double cyc = raw_cycle_ms(i);
      double sustain = std::min((total_peak - used) / r.cap_use, 1.0);
      double amount = g(i, r.attr);
      if (!has_charge) {
        adj[r.l] += sustain * amount / (cyc / 1000.0);
      } else {
        double mult = is_paste(i) ? cadm(i) : 1.0;
        double shots = (double)std::max<uint32_t>(num_shots(i), 1);
        double on = shots * cyc / (shots * cyc + reload);
        adj[r.l] += sustain * amount * on * mult / (cyc / 1000.0);
      }
      used += r.cap_use;
    }
    for (int l = 0; l < 3; l++) sus[l] += adj[l];
  }

  // ---------------- navigation
  double maxv = g(ship, K.maxVelocity);
  double limit = g(ship, K.speedLimit);
  double max_speed = (limit > 0.0 && maxv > limit) ? limit : maxv;
  double mass = g(ship, 4);
  double agility = g(ship, K.agility);
  double base_warp = g(ship, K.baseWarpSpeed);
  if (base_warp == 0.0) base_warp = 1.0;
  double warp_mult = g(ship, K.warpSpeedMultiplier);
  if (warp_mult == 0.0) warp_mult = 1.0;
  double warp_need = g(ship, K.warpCapacitorNeed);
  double sig = g(ship, K.signatureRadius);
  double align = -std::log(0.25) * agility * mass / 1e6;
  double max_warp = (warp_need > 0.0 && mass > 0.0) ? cap / (mass * warp_need) : 0.0;
  double scramble = g(ship, K.warpScrambleStatus);

  // ---------------- targeting
  const char* snames[4] = {"radar", "ladar", "magnetometric", "gravimetric"};
  const char* best_n = "none";
  double best_v = 0.0;
  for (int k = 0; k < 4; k++) {
    double v = g(ship, K.scanStrength[k]);
    if (v > best_v) {
      best_v = v;
      best_n = snames[k];
    }
  }
  double scan_res = g(ship, K.scanResolution);
  double ship_targets = g(ship, K.maxLockedTargets);
  double char_targets = g(ch, K.maxLockedTargets);
  double max_targets = std::min(ship_targets, std::max(char_targets, 0.0));
  double max_range = g(ship, K.maxTargetRange);

  double dr_max_active = g(ch, K.maxActiveDrones);
  double dr_range = g(ch, K.droneControlDistance);
  uint64_t dr_active = 0;
  for (uint32_t i : drones) dr_active += f.items[i].active_count;

  std::vector<Viol> viol;
  if (req.validate) validate(viol, modules, cpu_used, pg_used, calib_used, bw_used);

  // ================= emit (keys in sorted order)
  auto usage = [&](JW& x, std::string_view k, double u, double t) { x.key(k).obj().kn("total", t).kn("used", u).end_obj(); };
  auto res4 = [&](JW& x, std::string_view k, const double* r) {
    x.key(k).obj().kn("em", r[0]).kn("explosive", r[3]).kn("kinetic", r[2]).kn("thermal", r[1]).end_obj();
  };
  auto tank4 = [&](JW& x, std::string_view k, double ar, double hr, double ps, double sr) {
    x.key(k).obj().kn("armor_repair", ar).kn("hull_repair", hr).kn("passive_shield", ps).kn("shield_repair", sr).end_obj();
  };
  w.obj();
  const std::string_view inc = req.include_attributes ? std::string_view(*req.include_attributes) : std::string_view();
  if (inc == "ship") {
    w.key("attributes").obj().key("ship");
    dump_attrs(w, ship);
    w.end_obj();
  } else if (inc == "all") {
    w.key("attributes").obj().key("character");
    dump_attrs(w, ch);
    w.key("drones").arr();
    for (uint32_t i : drones) {
      w.obj().key("attributes");
      dump_attrs(w, i);
      w.key("drone_index");
      idx_or_null(w, f.items[i].req_index);
      w.end_obj();
    }
    w.end_arr().key("modules").arr();
    for (uint32_t i : modules) {
      w.obj().key("attributes");
      dump_attrs(w, i);
      w.key("charge");
      if (f.items[i].charge >= 0) dump_attrs(w, f.items[i].charge);
      else w.null();
      w.key("module_index");
      idx_or_null(w, f.items[i].req_index);
      w.ki("type_id", f.items[i].type_id).end_obj();
    }
    w.end_arr().key("ship");
    dump_attrs(w, ship);
    w.end_obj();
  }
  // capacitor
  w.key("capacitor").obj().kn("capacity", cap).kn("delta_gj_s", peak + cap_added - cap_used);
  if (cs_have_sim && !cs_has_percent) w.kn("depletes_in_s", cs_depletes);
  if (cs_have_sim) w.kn("eve_stable_percent", cs_eve);
  w.kn("injected_gj_s", cap_added).kn("peak_recharge_gj_s", peak).kn("recharge_time_s", rr / 1000.0);
  if (cs_have_sim) w.ki("sim_iterations", (int64_t)cs_iter);
  w.kb("stable", cs_stable);
  if (cs_has_percent) w.kn("stable_percent", cs_percent);
  w.kn("use_gj_s", cap_used).end_obj();
  // defense
  w.key("defense").obj();
  w.key("damage_pattern").obj().kn("em", dp.em).kn("explosive", dp.explosive).kn("kinetic", dp.kinetic).kn("thermal", dp.thermal).end_obj();
  w.key("ehp").obj().kn("armor", e_a).kn("hull", e_h).kn("shield", e_s).kn("total", e_s + e_a + e_h).end_obj();
  w.key("hp").obj().kn("armor", hp_a).kn("hull", hp_h).kn("shield", hp_s).kn("total", hp_s + hp_a + hp_h).end_obj();
  w.key("resonance").obj();
  res4(w, "armor", ra);
  res4(w, "hull", rh);
  res4(w, "shield", rs);
  w.end_obj();
  w.key("tank").obj();
  tank4(w, "effective", effectivify(armor_rep, ra), effectivify(hull_rep, rh), effectivify(passive, rs), effectivify(shield_rep, rs));
  tank4(w, "raw", armor_rep, hull_rep, passive, shield_rep);
  tank4(w, "sustained", sus[1], sus[2], passive, sus[0]);
  tank4(w, "sustained_effective", effectivify(sus[1], ra), effectivify(sus[2], rh), effectivify(passive, rs),
        effectivify(sus[0], rs));
  w.end_obj().end_obj();
  // drones
  w.key("drones").obj().ki("active", (int64_t)dr_active).kn("control_range_m", dr_range).kn("max_active", dr_max_active).end_obj();
  // meta
  w.key("meta").obj().ks("dataset_sha256", ds.sha256).ks("engine", ENGINE_NAME).ki("schema_version", 1).ki("sde_build", (int64_t)ds.build).end_obj();
  // modules
  w.key("modules");
  w.raw(wm.s);
  // navigation
  w.key("navigation").obj().kn("agility", agility).kn("align_time_s", align).kn("mass", mass).kn("max_velocity", max_speed)
      .kn("max_warp_distance_au", max_warp).kn("signature_radius", sig).kn("warp_scramble_status", scramble)
      .kn("warp_speed_au_s", base_warp * warp_mult).end_obj();
  // offense
  w.key("offense").obj().key("drones");
  w.raw(wd.s);
  w.key("fighters");
  w.raw(wf.s);
  w.key("total").obj().key("dps");
  t_dps.json(w);
  w.kn("drone_dps", d_dps.total()).kn("drone_volley", d_vol.total()).kn("fighter_dps", f_dps.total()).kn("fighter_volley", f_vol.total());
  w.key("volley");
  t_vol.json(w);
  w.kn("weapon_dps", w_dps.total()).kn("weapon_volley", w_vol.total()).end_obj();
  w.key("vs_target_profile").obj().kn("dps", t_dps.vs(tp_res)).kn("volley", t_vol.vs(tp_res)).end_obj();
  w.key("weapons");
  w.raw(wo.s);
  w.end_obj();
  // resources
  w.key("resources").obj();
  usage(w, "calibration", calib_used, r_upg);
  usage(w, "cargo", cargo_used, r_cargo);
  usage(w, "cpu", cpu_used, r_cpu_out);
  usage(w, "drone_bandwidth", bw_used, r_dbw);
  usage(w, "drone_bay", bay_used, r_dcap);
  usage(w, "fighter_bay", fbay_used, r_fcap);
  w.key("fighter_tubes").obj();
  usage(w, "heavy", cu_heavy, ft_heavy);
  usage(w, "light", cu_light, ft_light);
  usage(w, "support", cu_support, ft_support);
  usage(w, "total", tubes_used, ft_total);
  w.end_obj();
  w.key("hardpoints").obj();
  usage(w, "launcher", launchers_used, hp_launcher);
  usage(w, "turret", turrets_used, hp_turret);
  w.end_obj();
  usage(w, "power", pg_used, r_pow_out);
  w.key("slots").obj();
  usage(w, "high", slot_cnt[0], s_tot[0]);
  usage(w, "low", slot_cnt[2], s_tot[2]);
  usage(w, "mid", slot_cnt[1], s_tot[1]);
  usage(w, "rig", slot_cnt[3], s_tot[3]);
  usage(w, "service", slot_cnt[5], s_tot[5]);
  usage(w, "subsystem", slot_cnt[4], s_tot[4]);
  w.end_obj().end_obj();
  // ship
  {
    const TypeRec& st = *f.items[ship].t;
    const GroupRec* gr = ds.group(st.group);
    w.key("ship").obj().key("group");
    if (gr) w.str(ds.group_name(*gr));
    else w.null();
    w.ks("name", ds.type_name(st)).ki("type_id", st.id).end_obj();
  }
  // targeting
  {
    auto opt = [&](std::string_view k, std::optional<double> v) {
      w.key(k);
      if (v) w.num(*v);
      else w.null();
    };
    w.key("targeting").obj().key("lock_time_s").obj();
    opt("sig_125m", lock_time(scan_res, 125.0));
    opt("sig_25m", lock_time(scan_res, 25.0));
    opt("sig_400m", lock_time(scan_res, 400.0));
    opt("sig_40m", lock_time(scan_res, 40.0));
    opt("sig_target_profile", tp.signature_radius ? lock_time(scan_res, *tp.signature_radius) : std::nullopt);
    w.end_obj();
    w.kn("max_range_m", max_range).kn("max_targets", max_targets);
    opt("probe_size", best_v > 0.0 ? std::optional<double>(std::max(sig / best_v, 1.08)) : std::nullopt);
    w.kn("scan_resolution", scan_res).kn("sensor_strength", best_v).ks("sensor_type", best_n).end_obj();
  }
  if (req.validate) {
    w.key("violations").arr();
    for (auto& v : viol) {
      w.obj().ks("code", v.code).ks("message", v.msg).key("module_index");
      idx_or_null(w, v.idx);
      w.end_obj();
    }
    w.end_arr();
  }
  if (!f.warnings.empty()) {
    w.key("warnings").arr();
    for (auto& s : f.warnings) w.str(s);
    w.end_arr();
  }
  w.end_obj();
}
}  // namespace

void compute_stats(Fit& fit, const FitRequest& req, JW& out) {
  Calc c(fit, req);
  c.run(out);
}

}  // namespace evej
