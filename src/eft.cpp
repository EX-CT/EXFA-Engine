#include "eft.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <cstring>
#include <map>

namespace evej {
namespace {
bool ws(char c) { return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\v' || c == '\f'; }
std::string_view trim(std::string_view s) {
  while (!s.empty() && ws(s.front())) s.remove_prefix(1);
  while (!s.empty() && ws(s.back())) s.remove_suffix(1);
  return s;
}
std::string_view trim_end(std::string_view s) {
  while (!s.empty() && ws(s.back())) s.remove_suffix(1);
  return s;
}
// Rust str::parse::<u32>
bool parse_u32(std::string_view s, uint32_t& out) {
  if (!s.empty() && s[0] == '+') s.remove_prefix(1);
  if (s.empty()) return false;
  uint64_t v = 0;
  for (char c : s) {
    if (c < '0' || c > '9') return false;
    v = v * 10 + (uint64_t)(c - '0');
    if (v > 0xffffffffull) return false;
  }
  out = (uint32_t)v;
  return true;
}
bool parse_f64(std::string_view s, double& out) {
  if (!s.empty() && s[0] == '+') s.remove_prefix(1);
  if (s.empty()) return false;
  auto r = std::from_chars(s.data(), s.data() + s.size(), out);
  return r.ec == std::errc() && r.ptr == s.data() + s.size();
}
// Rust str::lines(): split on \n, strip one trailing \r, no final empty line
std::vector<std::string_view> lines_of(std::string_view t) {
  std::vector<std::string_view> v;
  size_t i = 0;
  while (i < t.size()) {
    size_t j = t.find('\n', i);
    if (j == std::string_view::npos) j = t.size();
    std::string_view l = t.substr(i, j - i);
    if (!l.empty() && l.back() == '\r') l.remove_suffix(1);
    v.push_back(l);
    i = j + 1;
  }
  return v;
}
bool is_head(std::string_view l) {
  std::string_view t = trim(l);
  if (t.empty() || t[0] != '[') return false;
  size_t e = t.find(']');
  uint32_t n;
  return e != std::string_view::npos && parse_u32(t.substr(1, e - 1), n);
}
std::string S(std::string_view s) { return std::string(s); }

struct Muts {
  std::map<uint32_t, Mutation> m;
};

std::string parse_mutations(const Dataset& ds, const std::vector<std::string_view>& lines, Muts& out, size_t& first) {
  first = lines.size();
  for (size_t i = 0; i < lines.size(); i++)
    if (is_head(lines[i])) {
      first = i;
      break;
    }
  size_t i = first;
  while (i < lines.size()) {
    std::string_view t = trim(lines[i]);
    if (!is_head(t)) {
      i++;
      continue;
    }
    size_t e = t.find(']');
    uint32_t n = 0;
    parse_u32(t.substr(1, e - 1), n);
    std::string_view base_name = trim(t.substr(e + 1));
    uint32_t base = ds.type_by_name(base_name);
    if (!base) return "unknown mutated base '" + S(base_name) + "'";
    Mutation m;
    m.base_type_id = base;
    std::map<std::string, double> attrs;
    i++;
    while (i < lines.size() && !is_head(lines[i])) {
      std::string_view l = trim(lines[i]);
      i++;
      if (l.empty()) continue;
      if (!m.mutaplasmid_type_id) {
        uint32_t id = ds.type_by_name(l);
        if (!id) return "unknown mutaplasmid '" + S(l) + "'";
        m.mutaplasmid_type_id = id;
        continue;
      }
      size_t p = 0;
      while (p <= l.size()) {
        size_t c = l.find(',', p);
        if (c == std::string_view::npos) c = l.size();
        std::string_view kv = trim(l.substr(p, c - p));
        p = c + 1;
        size_t sp = kv.rfind(' ');
        if (sp != std::string_view::npos) {
          uint32_t aid = ds.attr_id(trim(kv.substr(0, sp)));
          double v;
          if (aid != 0 && parse_f64(trim(kv.substr(sp + 1)), v)) attrs[std::to_string(aid)] = v;
        }
        if (c == l.size()) break;
      }
    }
    for (auto& [k, v] : attrs) m.attributes.push_back({(uint32_t)std::stoul(k), v});
    out.m[n] = std::move(m);
  }
  return {};
}

uint32_t mutated_type(const Dataset& ds, const Mutation& m) {
  if (m.mutaplasmid_type_id) {
    uint32_t o = ds.muta_output(*m.mutaplasmid_type_id, m.base_type_id);
    if (o) return o;
  }
  return m.base_type_id;
}

constexpr uint32_t CAT_CHARGE = 8, CAT_DRONE = 18, CAT_IMPLANT = 20, CAT_FIGHTER = 87, GROUP_T3D_MODE = 1306;
constexpr uint32_t ATTR_BOOSTERNESS = 1087, ATTR_CAP_NEED = 6;
}  // namespace

std::string eft_parse(const Dataset& ds, std::string_view text, FitRequest& req) {
  auto all = lines_of(text);
  Muts muts;
  size_t first = 0;
  std::string err = parse_mutations(ds, all, muts, first);
  if (!err.empty()) return err;
  std::vector<std::string_view> lines;
  for (size_t i = 0; i < first; i++) {
    std::string_view l = trim(all[i]);
    if (!l.empty()) lines.push_back(l);
  }
  if (lines.empty()) return "empty EFT";
  std::string_view h = lines[0];
  while (!h.empty() && h.front() == '[') h.remove_prefix(1);
  while (!h.empty() && h.back() == ']') h.remove_suffix(1);
  std::string_view ship_name = trim(h.substr(0, h.find(',')));
  req = FitRequest{};
  req.ship_type_id = ds.type_by_name(ship_name);
  if (!req.ship_type_id) return "unknown ship '" + S(ship_name) + "'";
  req.validate = true;
  for (size_t li = 1; li < lines.size(); li++) {
    std::string_view line = lines[li];
    if (line.substr(0, 6) == "[Empty") continue;
    bool offline = false;
    for (const char* suf : {"/OFFLINE", "/offline"}) {
      size_t n = strlen(suf);
      if (line.size() >= n && line.substr(line.size() - n) == suf) {
        line = trim(line.substr(0, line.size() - n));
        offline = true;
        break;
      }
    }
    // trailing " [N]" mutation reference
    std::optional<Mutation> mutation;
    {
      std::string_view l = trim_end(line);
      line = l;
      if (!l.empty() && l.back() == ']') {
        size_t p = l.rfind(" [");
        uint32_t n;
        if (p != std::string_view::npos && parse_u32(l.substr(p + 2, l.size() - 1 - (p + 2)), n)) {
          line = trim_end(l.substr(0, p));
          auto it = muts.m.find(n);
          if (it == muts.m.end()) return "mutation [" + std::to_string(n) + "] not defined";
          mutation = it->second;
        }
      }
    }
    // "Name xN" => drone / fighter / cargo
    size_t pos = line.rfind(" x");
    uint32_t qty;
    if (pos != std::string_view::npos && parse_u32(trim(line.substr(pos + 2)), qty)) {
      std::string_view name = trim(line.substr(0, pos));
      uint32_t tid = ds.type_by_name(name);
      if (!tid) return "unknown item '" + S(name) + "'";
      if (mutation) tid = mutated_type(ds, *mutation);
      const TypeRec* t = ds.type(tid);
      if (!t) return "unknown item '" + S(name) + "'";
      if (t->category == CAT_DRONE) {
        DroneReq d;
        d.type_id = tid;
        d.quantity = qty;
        d.active = qty;
        d.mutation = mutation;
        req.drones.push_back(std::move(d));
      } else if (t->category == CAT_FIGHTER) {
        FighterReq f;
        f.type_id = tid;
        f.quantity = qty;
        req.fighters.push_back(std::move(f));
      } else {
        req.cargo.push_back({tid, qty});
      }
      continue;
    }
    size_t comma = line.find(',');
    std::string_view name = trim(line.substr(0, comma));
    std::optional<std::string_view> charge;
    if (comma != std::string_view::npos) charge = trim(line.substr(comma + 1));
    uint32_t tid = ds.type_by_name(name);
    if (!tid) return "unknown item '" + S(name) + "'";
    if (mutation) tid = mutated_type(ds, *mutation);
    const TypeRec* t = ds.type(tid);
    if (!t) return "unknown item '" + S(name) + "'";
    double tmp;
    if (t->category == CAT_IMPLANT) {
      if (ds.type_attr(*t, ATTR_BOOSTERNESS, tmp)) req.boosters.push_back(BoosterReq{tid, {}});
      else req.implants.push_back(tid);
    } else if (t->category == CAT_DRONE) {
      DroneReq d;
      d.type_id = tid;
      d.quantity = 1;
      d.active = 1;
      d.mutation = mutation;
      req.drones.push_back(std::move(d));
    } else if (t->category == CAT_CHARGE) {
      req.cargo.push_back({tid, 1});
    } else {
      if (t->group == GROUP_T3D_MODE) {
        req.mode_type_id = tid;
        continue;
      }
      ModuleReq m;
      m.type_id = tid;
      m.slot = (Slot)t->slot;
      if (charge) {
        uint32_t c = ds.type_by_name(*charge);
        if (!c) return "unknown charge '" + S(*charge) + "'";
        m.charge_type_id = c;
      }
      bool active_capable = false;
      for (const TEff& e : ds.type_effects(*t)) {
        const EffRec* er = ds.effect(e.id);
        if (er && er->category == 1) active_capable = true;
      }
      if (ds.type_attr(*t, ATTR_CAP_NEED, tmp) && tmp != 0.0) active_capable = true;
      if (offline) m.state = State::Offline;
      else if (active_capable && m.slot != Slot::Rig && m.slot != Slot::Subsystem) m.state = State::Active;
      else m.state = State::Online;
      m.mutation = mutation;
      req.modules.push_back(std::move(m));
    }
  }
  return {};
}

namespace {
// Rust `{}` Display of f64 (shortest round-trip, never exponent)
std::string rust_display(double v) {
  if (std::isnan(v)) return "NaN";
  if (std::isinf(v)) return v < 0 ? "-inf" : "inf";
  char buf[512];
  auto r = std::to_chars(buf, buf + sizeof buf, v, std::chars_format::fixed);
  return std::string(buf, r.ptr);
}
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
const char* state_name(State s) {
  switch (s) {
    case State::Offline: return "offline";
    case State::Online: return "online";
    case State::Active: return "active";
    default: return "overheated";
  }
}
const char* spool_name(SpoolType t) {
  switch (t) {
    case SpoolType::SpoolScale: return "spool_scale";
    case SpoolType::CycleScale: return "cycle_scale";
    case SpoolType::Time: return "time";
    default: return "cycles";
  }
}
void opt_u(JW& w, std::string_view k, const std::optional<uint32_t>& v) {
  w.key(k);
  if (v) w.i64(*v);
  else w.null();
}
void opt_d(JW& w, std::string_view k, const std::optional<double>& v) {
  w.key(k);
  if (v) w.num_raw(*v);
  else w.null();
}
void opt_s(JW& w, std::string_view k, const std::optional<std::string>& v) {
  w.key(k);
  if (v) w.str(*v);
  else w.null();
}
void mutation_json(JW& w, std::string_view k, const std::optional<Mutation>& m) {
  w.key(k);
  if (!m) {
    w.null();
    return;
  }
  // attributes: BTreeMap<String, f64> (string order)
  std::vector<std::pair<std::string, double>> kv;
  for (auto& [a, v] : m->attributes) kv.push_back({std::to_string(a), v});
  std::sort(kv.begin(), kv.end(), [](auto& x, auto& y) { return x.first < y.first; });
  w.obj().key("attributes").obj();
  for (auto& [a, v] : kv) w.key(a).num_raw(v);
  w.end_obj().ki("base_type_id", m->base_type_id);
  opt_u(w, "mutaplasmid_type_id", m->mutaplasmid_type_id);
  w.end_obj();
}
void spool_json(JW& w, std::string_view k, const std::optional<Spool>& s) {
  w.key(k);
  if (!s) w.null();
  else w.obj().key("amount").num_raw(s->amount).ks("type", spool_name(s->kind)).end_obj();
}
void module_json(JW& w, const ModuleReq& m) {
  w.obj();
  opt_u(w, "charge_type_id", m.charge_type_id);
  mutation_json(w, "mutation", m.mutation);
  const char* sn = slot_name(m.slot);
  if (sn) w.ks("slot", sn);
  else w.knull("slot");
  spool_json(w, "spool", m.spool);
  if (m.state) w.ks("state", state_name(*m.state));
  else w.knull("state");
  w.ki("type_id", m.type_id).end_obj();
}
void drone_json(JW& w, const DroneReq& d) {
  w.obj();
  opt_u(w, "active", d.active);
  mutation_json(w, "mutation", d.mutation);
  w.ki("quantity", d.quantity).ki("type_id", d.type_id).end_obj();
}
void fighter_json(JW& w, const FighterReq& f) {
  w.obj().key("abilities");
  if (f.abilities) {
    w.arr();
    for (uint32_t a : *f.abilities) w.i64(a);
    w.end_arr();
  } else {
    w.null();
  }
  w.kb("active", f.active);
  opt_u(w, "quantity", f.quantity);
  w.ki("type_id", f.type_id).end_obj();
}
}  // namespace

void fit_request_json(const FitRequest& r, JW& w) {
  w.obj().key("boosters").arr();
  for (auto& b : r.boosters) {
    w.obj().key("side_effects").arr();
    for (uint32_t s : b.side_effects) w.i64(s);
    w.end_arr().ki("type_id", b.type_id).end_obj();
  }
  w.end_arr().key("cargo").arr();
  for (auto& c : r.cargo) w.obj().ki("quantity", c.quantity).ki("type_id", c.type_id).end_obj();
  w.end_arr().key("character").obj();
  opt_d(w, "security_status", r.security_status);
  w.key("skills").obj();
  opt_u(w, "default_level", r.default_level ? std::optional<uint32_t>(*r.default_level) : std::nullopt);
  w.key("levels").obj();
  for (auto& [k, v] : r.skill_levels) w.ki(k, v);
  w.end_obj().end_obj().end_obj();
  w.key("damage_pattern");
  if (r.damage_pattern) {
    auto& d = *r.damage_pattern;
    w.obj().key("em").num_raw(d.em).key("explosive").num_raw(d.explosive).key("kinetic").num_raw(d.kinetic)
        .key("thermal").num_raw(d.thermal).end_obj();
  } else {
    w.null();
  }
  w.key("drones").arr();
  for (auto& d : r.drones) drone_json(w, d);
  w.end_arr().key("environment").obj().key("effect_type_ids").arr();
  for (uint32_t e : r.env_effects) w.i64(e);
  w.end_arr();
  opt_s(w, "system_security", r.system_security);
  w.end_obj().key("fighters").arr();
  for (auto& f : r.fighters) fighter_json(w, f);
  w.end_arr().key("fleet").obj().key("booster_fits").arr();
  for (auto& b : r.booster_fits) fit_request_json(b, w);
  w.end_arr().key("buffs").arr();
  for (auto& b : r.buffs) w.obj().ki("buff_id", b.buff_id).key("value").num_raw(b.value).end_obj();
  w.end_arr().end_obj().key("implants").arr();
  for (uint32_t i : r.implants) w.i64(i);
  w.end_arr().key("modules").arr();
  for (auto& m : r.modules) module_json(w, m);
  w.end_arr().key("options").obj().key("cap_sim").obj();
  opt_d(w, "max_time_s", r.cs_max_time_s);
  w.kb("reload", r.cs_reload).kb("stagger", r.cs_stagger).end_obj();
  spool_json(w, "default_spool", r.default_spool);
  w.kb("factor_reload", r.factor_reload);
  opt_s(w, "include_attributes", r.include_attributes);
  w.kb("nos_no_target_cap", r.nos_no_target_cap);
  opt_s(w, "rah", r.rah);
  w.kb("sources", r.sources).kb("validate", r.validate).end_obj();
  w.key("overrides").arr();
  for (auto& o : r.overrides) w.obj().ki("attribute_id", o.attribute_id).ki("type_id", o.type_id).key("value").num_raw(o.value).end_obj();
  w.end_arr().key("projected").arr();
  for (auto& p : r.projected) {
    w.obj().ki("amount", p.amount);
    opt_d(w, "distance_m", p.distance_m);
    w.key("drone");
    if (p.drone) drone_json(w, *p.drone);
    else w.null();
    w.key("fighter");
    if (p.fighter) fighter_json(w, *p.fighter);
    else w.null();
    w.key("fit");
    if (p.fit) fit_request_json(*p.fit, w);
    else w.null();
    w.ks("kind", p.kind).key("module");
    if (p.module) module_json(w, *p.module);
    else w.null();
    w.end_obj();
  }
  w.end_arr().ki("schema_version", 1).key("ship").obj();
  opt_u(w, "mode_type_id", r.mode_type_id);
  w.ki("type_id", r.ship_type_id).end_obj().key("target_profile");
  if (r.target_profile) {
    auto& t = *r.target_profile;
    w.obj().key("em").num_raw(t.em).key("explosive").num_raw(t.explosive).key("kinetic").num_raw(t.kinetic);
    opt_d(w, "max_velocity", t.max_velocity);
    opt_d(w, "radius", t.radius);
    opt_d(w, "signature_radius", t.signature_radius);
    w.key("thermal").num_raw(t.thermal).end_obj();
  } else {
    w.null();
  }
  w.end_obj();
}

std::string eft_export(const Dataset& ds, const FitRequest& req, std::string_view fit_name) {
  auto n = [&](uint32_t id) -> std::string {
    const TypeRec* t = ds.type(id);
    return t ? S(ds.type_name(*t)) : std::to_string(id);
  };
  std::string out = "[" + n(req.ship_type_id) + ", " + S(fit_name) + "]\n";
  std::vector<const Mutation*> muts;
  auto tag = [&](const std::optional<Mutation>& m) -> std::string {
    if (!m) return {};
    muts.push_back(&*m);
    return " [" + std::to_string(muts.size()) + "]";
  };
  for (Slot slot : {Slot::Low, Slot::Mid, Slot::High, Slot::Rig, Slot::Subsystem, Slot::Service}) {
    bool any = false;
    for (auto& m : req.modules) {
      Slot s = m.slot;
      if (s == Slot::None) {
        const TypeRec* t = ds.type(m.type_id);
        s = t ? (Slot)t->slot : Slot::None;
      }
      if (s != slot) continue;
      any = true;
      out += n(m.mutation ? m.mutation->base_type_id : m.type_id);
      if (m.charge_type_id) out += ", " + n(*m.charge_type_id);
      if (m.state && *m.state == State::Offline) out += " /OFFLINE";
      out += tag(m.mutation);
      out += "\n";
    }
    if (any) out += "\n";
  }
  for (auto& d : req.drones)
    out += n(d.mutation ? d.mutation->base_type_id : d.type_id) + " x" + std::to_string(d.quantity) + tag(d.mutation) + "\n";
  for (auto& f : req.fighters) out += n(f.type_id) + " x" + std::to_string(f.quantity.value_or(1)) + "\n";
  if (!req.implants.empty() || !req.boosters.empty()) {
    out += "\n";
    for (uint32_t i : req.implants) out += n(i) + "\n";
    for (auto& b : req.boosters) out += n(b.type_id) + "\n";
  }
  if (!req.cargo.empty()) {
    out += "\n";
    for (auto& c : req.cargo) out += n(c.type_id) + " x" + std::to_string(c.quantity) + "\n";
  }
  if (!muts.empty()) {
    out += "\n";
    for (size_t k = 0; k < muts.size(); k++) {
      const Mutation& m = *muts[k];
      out += "[" + std::to_string(k + 1) + "] " + n(m.base_type_id) + "\n";
      if (m.mutaplasmid_type_id) out += "  " + n(*m.mutaplasmid_type_id) + "\n";
      std::vector<std::pair<std::string, double>> kv;
      for (auto& [a, v] : m.attributes) kv.push_back({std::to_string(a), v});
      std::sort(kv.begin(), kv.end(), [](auto& x, auto& y) { return x.first < y.first; });
      std::string line;
      for (size_t i = 0; i < kv.size(); i++) {
        const AttrRec* ar = ds.attr(kv[i].first.empty() ? 0 : (uint32_t)std::stoul(kv[i].first));
        if (i) line += ", ";
        line += (ar ? S(ds.attr_name(*ar)) : kv[i].first) + " " + rust_display(kv[i].second);
      }
      if (!kv.empty()) out += "  " + line + "\n";
    }
  }
  return out;
}

std::string json_pretty(std::string_view s) {
  std::string o;
  o.reserve(s.size() * 2);
  int depth = 0;
  auto nl = [&] {
    o.push_back('\n');
    o.append((size_t)depth * 2, ' ');
  };
  for (size_t i = 0; i < s.size(); i++) {
    char c = s[i];
    if (c == '"') {
      size_t j = i + 1;
      while (j < s.size() && s[j] != '"') j += (s[j] == '\\') ? 2 : 1;
      o.append(s.substr(i, j - i + 1));
      i = j;
    } else if (c == '{' || c == '[') {
      char close = c == '{' ? '}' : ']';
      if (i + 1 < s.size() && s[i + 1] == close) {
        o.push_back(c);
        o.push_back(close);
        i++;
      } else {
        o.push_back(c);
        depth++;
        nl();
      }
    } else if (c == '}' || c == ']') {
      depth--;
      nl();
      o.push_back(c);
    } else if (c == ',') {
      o.push_back(',');
      nl();
    } else if (c == ':') {
      o.append(": ");
    } else {
      o.push_back(c);
    }
  }
  return o;
}
}  // namespace evej
