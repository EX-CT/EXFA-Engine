#include "request.hpp"

#include <algorithm>
#include <simdjson.h>

namespace evej {
using simdjson::dom::element;

namespace {
struct Err {
  std::string msg;
};

bool present(const element& o, std::string_view k, element& out) {
  if (o[k].get(out) != simdjson::SUCCESS) return false;
  return !out.is_null();
}
uint64_t as_uint(const element& e, const char* what) {
  uint64_t u;
  if (e.get_uint64().get(u) == simdjson::SUCCESS) return u;  // serde: integers only (2.0 is a float)
  throw Err{std::string("invalid type for ") + what + ", expected unsigned integer"};
}
uint32_t as_u32(const element& e, const char* what) {
  uint64_t u = as_uint(e, what);
  if (u > 0xffffffffull) throw Err{std::string("integer out of range for ") + what};
  return (uint32_t)u;
}
double as_f64(const element& e, const char* what) {
  double d;
  if (e.get_double().get(d) == simdjson::SUCCESS) return d;
  throw Err{std::string("invalid type for ") + what + ", expected number"};
}
bool as_bool(const element& e, const char* what) {
  bool b;
  if (e.get_bool().get(b) == simdjson::SUCCESS) return b;
  throw Err{std::string("invalid type for ") + what + ", expected boolean"};
}
std::string_view as_str(const element& e, const char* what) {
  std::string_view s;
  if (e.get_string().get(s) == simdjson::SUCCESS) return s;
  throw Err{std::string("invalid type for ") + what + ", expected string"};
}
simdjson::dom::array as_arr(const element& e, const char* what) {
  simdjson::dom::array a;
  if (e.get_array().get(a) == simdjson::SUCCESS) return a;
  throw Err{std::string("invalid type for ") + what + ", expected array"};
}
simdjson::dom::object as_obj(const element& e, const char* what) {
  simdjson::dom::object o;
  if (e.get_object().get(o) == simdjson::SUCCESS) return o;
  throw Err{std::string("invalid type for ") + what + ", expected object"};
}
// non-Option field with a serde default: absent -> default, explicit null -> error (serde rejects it)
bool nn(const element& o, std::string_view k, element& out) {
  if (o[k].get(out) != simdjson::SUCCESS) return false;
  if (out.is_null()) throw Err{"invalid type: null for `" + std::string(k) + "`"};
  return true;
}
uint32_t def_u32(const element& o, std::string_view k, uint32_t def, const char* what) {
  element e;
  return nn(o, k, e) ? as_u32(e, what) : def;
}
double def_f64(const element& o, std::string_view k, double def, const char* what) {
  element e;
  return nn(o, k, e) ? as_f64(e, what) : def;
}
std::optional<uint32_t> opt_u32(const element& o, std::string_view k, const char* what) {
  element e;
  if (!present(o, k, e)) return std::nullopt;
  return as_u32(e, what);
}
std::optional<double> opt_f64(const element& o, std::string_view k, const char* what) {
  element e;
  if (!present(o, k, e)) return std::nullopt;
  return as_f64(e, what);
}
bool opt_bool(const element& o, std::string_view k, bool def, const char* what) {
  element e;
  if (!nn(o, k, e)) return def;
  return as_bool(e, what);
}
uint32_t req_u32(const element& o, std::string_view k, const char* what) {
  element e;
  if (!present(o, k, e)) throw Err{std::string("missing field `") + std::string(k) + "`"};
  return as_u32(e, what);
}

State parse_state(std::string_view s) {
  if (s == "offline") return State::Offline;
  if (s == "online") return State::Online;
  if (s == "active") return State::Active;
  if (s == "overheated") return State::Overheated;
  throw Err{"unknown variant `" + std::string(s) + "`, expected one of `offline`, `online`, `active`, `overheated`"};
}
Slot parse_slot(std::string_view s) {
  if (s == "high") return Slot::High;
  if (s == "mid") return Slot::Mid;
  if (s == "low") return Slot::Low;
  if (s == "rig") return Slot::Rig;
  if (s == "subsystem") return Slot::Subsystem;
  if (s == "service") return Slot::Service;
  throw Err{"unknown variant `" + std::string(s) + "` for slot"};
}
Spool parse_spool(const element& e) {
  Spool sp;
  as_obj(e, "spool");
  element t;
  if (!present(e, "type", t)) throw Err{"missing field `type`"};
  std::string_view s = as_str(t, "spool.type");
  if (s == "spool_scale") sp.kind = SpoolType::SpoolScale;
  else if (s == "cycle_scale") sp.kind = SpoolType::CycleScale;
  else if (s == "time") sp.kind = SpoolType::Time;
  else if (s == "cycles") sp.kind = SpoolType::Cycles;
  else throw Err{"unknown spool type `" + std::string(s) + "`"};
  element a;
  if (!present(e, "amount", a)) throw Err{"missing field `amount`"};
  sp.amount = as_f64(a, "spool.amount");
  return sp;
}
Mutation parse_mutation(const element& e) {
  as_obj(e, "mutation");
  Mutation m;
  m.base_type_id = req_u32(e, "base_type_id", "base_type_id");
  m.mutaplasmid_type_id = opt_u32(e, "mutaplasmid_type_id", "mutaplasmid_type_id");
  element a;
  if (nn(e, "attributes", a)) {
    std::vector<std::pair<std::string, double>> kv;
    for (auto [k, v] : as_obj(a, "mutation.attributes")) kv.push_back({std::string(k), as_f64(v, "mutation attribute")});
    std::stable_sort(kv.begin(), kv.end(), [](auto& x, auto& y) { return x.first < y.first; });
    for (auto& [k, v] : kv) {
      if (k.empty() || k.size() > 10) continue;
      uint64_t id = 0;
      bool ok = true;
      for (char c : k) {
        if (c < '0' || c > '9') { ok = false; break; }
        id = id * 10 + (c - '0');
      }
      if (ok && id <= 0xffffffffull) m.attributes.push_back({(uint32_t)id, v});
    }
  }
  return m;
}
ModuleReq parse_module(const element& e) {
  as_obj(e, "module");
  ModuleReq m;
  m.type_id = req_u32(e, "type_id", "type_id");
  element x;
  if (present(e, "slot", x)) m.slot = parse_slot(as_str(x, "slot"));
  if (present(e, "state", x)) m.state = parse_state(as_str(x, "state"));
  m.charge_type_id = opt_u32(e, "charge_type_id", "charge_type_id");
  if (present(e, "mutation", x)) m.mutation = parse_mutation(x);
  if (present(e, "spool", x)) m.spool = parse_spool(x);
  return m;
}
DroneReq parse_drone(const element& e) {
  as_obj(e, "drone");
  DroneReq d;
  d.type_id = req_u32(e, "type_id", "type_id");
  d.quantity = def_u32(e, "quantity", 1, "quantity");
  d.active = opt_u32(e, "active", "active");
  element x;
  if (present(e, "mutation", x)) d.mutation = parse_mutation(x);
  return d;
}
void u32_list(const element& o, std::string_view k, std::vector<uint32_t>& out, const char* what) {
  element e;
  if (!nn(o, k, e)) return;
  for (auto x : as_arr(e, what)) out.push_back(as_u32(x, what));
}
void parse_fit(const element& root, FitRequest& r, int depth);
FighterReq parse_fighter(const element& m) {
  as_obj(m, "fighter");
  FighterReq f;
  f.type_id = req_u32(m, "type_id", "type_id");
  f.quantity = opt_u32(m, "quantity", "quantity");
  f.active = opt_bool(m, "active", true, "active");
  element ab;
  if (present(m, "abilities", ab)) {
    std::vector<uint32_t> v;
    for (auto a : as_arr(ab, "abilities")) v.push_back(as_u32(a, "ability"));
    f.abilities = std::move(v);
  }
  return f;
}
}  // namespace

std::string parse_request(const element& root, FitRequest& r) {
  try {
    parse_fit(root, r, 0);
  } catch (const Err& e) {
    return e.msg;
  }
  return {};
}

namespace {
void parse_fit(const element& root, FitRequest& r, int depth) {
  if (depth > 16) throw Err{"request nesting too deep"};
  {
    as_obj(root, "request");
    element x, y;
    (void)opt_u32(root, "schema_version", "schema_version");
    if (!present(root, "ship", x)) throw Err{"missing field `ship`"};
    as_obj(x, "ship");
    r.ship_type_id = req_u32(x, "type_id", "ship.type_id");
    r.mode_type_id = opt_u32(x, "mode_type_id", "mode_type_id");
    if (nn(root, "character", x)) {
      as_obj(x, "character");
      if (nn(x, "skills", y)) {
        as_obj(y, "skills");
        element z;
        if (present(y, "default_level", z)) {
          uint32_t v = as_u32(z, "default_level");
          if (v > 255) throw Err{"default_level out of range"};
          r.default_level = (uint8_t)v;
        }
        if (nn(y, "levels", z)) {
          for (auto [k, v] : as_obj(z, "levels")) {
            uint32_t lv = as_u32(v, "skill level");
            if (lv > 255) throw Err{"skill level out of range"};
            r.skill_levels.push_back({std::string(k), (uint8_t)lv});
          }
          // BTreeMap semantics: sorted by key, last duplicate wins
          std::stable_sort(r.skill_levels.begin(), r.skill_levels.end(), [](auto& a, auto& b) { return a.first < b.first; });
        }
      }
      r.security_status = opt_f64(x, "security_status", "security_status");
    }
    if (nn(root, "modules", x))
      for (auto m : as_arr(x, "modules")) r.modules.push_back(parse_module(m));
    if (nn(root, "drones", x))
      for (auto m : as_arr(x, "drones")) r.drones.push_back(parse_drone(m));
    if (nn(root, "fighters", x))
      for (auto m : as_arr(x, "fighters")) r.fighters.push_back(parse_fighter(m));
    u32_list(root, "implants", r.implants, "implants");
    if (nn(root, "boosters", x))
      for (auto m : as_arr(x, "boosters")) {
        as_obj(m, "booster");
        BoosterReq b;
        b.type_id = req_u32(m, "type_id", "type_id");
        u32_list(m, "side_effects", b.side_effects, "side_effects");
        r.boosters.push_back(std::move(b));
      }
    if (nn(root, "cargo", x))
      for (auto m : as_arr(x, "cargo")) {
        as_obj(m, "cargo");
        CargoReq c;
        c.type_id = req_u32(m, "type_id", "type_id");
        c.quantity = def_u32(m, "quantity", 1, "quantity");
        r.cargo.push_back(c);
      }
    if (nn(root, "fleet", x)) {
      as_obj(x, "fleet");
      if (nn(x, "buffs", y))
        for (auto b : as_arr(y, "buffs")) {
          as_obj(b, "buff");
          Buff bf;
          bf.buff_id = req_u32(b, "buff_id", "buff_id");
          element v;
          if (!present(b, "value", v)) throw Err{"missing field `value`"};
          bf.value = as_f64(v, "buff value");
          r.buffs.push_back(bf);
        }
      if (nn(x, "booster_fits", y))
        for (auto bf : as_arr(y, "booster_fits")) {
          r.booster_fits.emplace_back();
          parse_fit(bf, r.booster_fits.back(), depth + 1);
        }
    }
    if (nn(root, "projected", x))
      for (auto p : as_arr(x, "projected")) {
        as_obj(p, "projected");
        Projected pr;
        element k;
        if (!present(p, "kind", k)) throw Err{"missing field `kind`"};
        pr.kind = std::string(as_str(k, "kind"));
        if (present(p, "module", k)) pr.module = parse_module(k);
        if (present(p, "drone", k)) pr.drone = parse_drone(k);
        if (present(p, "fighter", k)) pr.fighter = parse_fighter(k);
        if (present(p, "fit", k)) {
          pr.fit = std::make_shared<FitRequest>();
          parse_fit(k, *pr.fit, depth + 1);
        }
        pr.amount = def_u32(p, "amount", 1, "amount");
        pr.distance_m = opt_f64(p, "distance_m", "distance_m");
        r.projected.push_back(std::move(pr));
      }
    if (nn(root, "environment", x)) {
      as_obj(x, "environment");
      u32_list(x, "effect_type_ids", r.env_effects, "effect_type_ids");
      if (present(x, "system_security", y)) r.system_security = std::string(as_str(y, "system_security"));
    }
    if (present(root, "damage_pattern", x)) {
      as_obj(x, "damage_pattern");
      Resists d;
      d.em = def_f64(x, "em", 0, "em");
      d.thermal = def_f64(x, "thermal", 0, "thermal");
      d.kinetic = def_f64(x, "kinetic", 0, "kinetic");
      d.explosive = def_f64(x, "explosive", 0, "explosive");
      r.damage_pattern = d;
    }
    if (present(root, "target_profile", x)) {
      as_obj(x, "target_profile");
      TargetProfile t;
      t.em = def_f64(x, "em", 0, "em");
      t.thermal = def_f64(x, "thermal", 0, "thermal");
      t.kinetic = def_f64(x, "kinetic", 0, "kinetic");
      t.explosive = def_f64(x, "explosive", 0, "explosive");
      t.signature_radius = opt_f64(x, "signature_radius", "signature_radius");
      t.max_velocity = opt_f64(x, "max_velocity", "max_velocity");
      t.radius = opt_f64(x, "radius", "radius");
      r.target_profile = t;
    }
    if (nn(root, "overrides", x))
      for (auto o : as_arr(x, "overrides")) {
        as_obj(o, "override");
        Override ov;
        ov.type_id = req_u32(o, "type_id", "type_id");
        ov.attribute_id = req_u32(o, "attribute_id", "attribute_id");
        element v;
        if (!present(o, "value", v)) throw Err{"missing field `value`"};
        ov.value = as_f64(v, "override value");
        r.overrides.push_back(ov);
      }
    if (nn(root, "options", x)) {
      as_obj(x, "options");
      r.nos_no_target_cap = opt_bool(x, "nos_no_target_cap", false, "nos_no_target_cap");
      r.factor_reload = opt_bool(x, "factor_reload", false, "factor_reload");
      r.sources = opt_bool(x, "sources", false, "sources");
      r.validate = opt_bool(x, "validate", true, "validate");
      if (present(x, "default_spool", y)) r.default_spool = parse_spool(y);
      if (present(x, "rah", y)) r.rah = std::string(as_str(y, "rah"));
      if (present(x, "include_attributes", y)) r.include_attributes = std::string(as_str(y, "include_attributes"));
      if (nn(x, "cap_sim", y)) {
        as_obj(y, "cap_sim");
        r.cs_reload = opt_bool(y, "reload", false, "cap_sim.reload");
        r.cs_stagger = opt_bool(y, "stagger", false, "cap_sim.stagger");
        r.cs_max_time_s = opt_f64(y, "max_time_s", "cap_sim.max_time_s");
      }
    }
  }
}
}  // namespace

}  // namespace evej
