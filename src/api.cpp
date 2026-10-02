#include "api.hpp"
#include "serdenum.hpp"

#include <algorithm>
#include <simdjson.h>

#include "request.hpp"
#include "stats.hpp"

namespace evej {

Worker::Worker(const Dataset& d, const Ids& i) : ds(d), ids(i), parser(new simdjson::dom::parser()) {}
Worker::~Worker() { delete parser; }

void write_error(JW& w, const char* code, std::string_view message, std::string_view path) {
  w.obj().key("error").obj().ks("code", code).ks("message", message).ks("path", path).end_obj().end_obj();
}

bool Worker::calc_element(const simdjson::dom::element& root, JW& w) {
  FitRequest req;
  std::string err = parse_request(root, req);
  if (!err.empty()) {
    write_error(w, "BAD_REQUEST", err, "");
    return false;
  }
  if (!fit) fit = std::make_unique<Fit>(ds, ids);
  else fit->reset();
  EngineError ee{};
  if (!fit->build(req, ee)) {
    write_error(w, ee.code, ee.message, ee.path);
    return false;
  }
  compute_stats(*fit, req, w);
  return true;
}

bool Worker::calc_json(std::string_view request) {
  out.clear();
  std::string fixed;
  if (serde_fix_numbers(request, fixed)) request = fixed;  // serde_json's float rounding (see serdenum.hpp)
  simdjson::dom::element root;
  auto e = parser->parse(request.data(), request.size()).get(root);
  if (e) {
    write_error(out, "BAD_REQUEST", std::string("invalid JSON: ") + simdjson::error_message(e), "");
    return false;
  }
  return calc_element(root, out);
}

void meta_json(const Dataset& ds, JW& w, double load_ms) {
  w.obj().ki("attributes", (int64_t)ds.attrs.size()).ks("dataset_sha256", ds.sha256).ki("effects", (int64_t)ds.effects.size())
      .ks("engine", ENGINE_NAME);
  if (load_ms >= 0) w.key("load_ms").num_raw(load_ms);
  w.ki("schema_version", 1).ki("sde_build", (int64_t)ds.build);
  w.key("sde_release_date");
  if (ds.release_date.empty()) w.null();
  else w.str(ds.release_date);
  w.ki("types", (int64_t)ds.types.size()).end_obj();
}

static const char* slot_str(int32_t s) {
  static const char* n[6] = {"high", "mid", "low", "rig", "subsystem", "service"};
  return s >= 0 && s < 6 ? n[s] : nullptr;
}


void type_json(const Dataset& ds, std::string_view key, JW& w) {
  uint32_t id = 0;
  bool num = !key.empty() && std::all_of(key.begin(), key.end(), [](char c) { return c >= '0' && c <= '9'; });
  if (num) id = (uint32_t)std::stoul(std::string(key));
  else id = ds.type_by_name(key);
  const TypeRec* t = ds.type(id);
  if (!t) {
    w.obj().key("error").obj().ks("code", "UNKNOWN_TYPE").ks("message", key).end_obj().end_obj();
    return;
  }
  std::vector<std::pair<std::string, double>> at;
  // type-level mass/capacity/volume/radius are authoritative and always present (eve-dogma-rs merges them into the
  // type's attributes at load: a non-zero field overrides the attribute, a missing attribute takes the field)
  const uint32_t fid[4] = {4, 38, 161, 162};
  const double fval[4] = {t->mass, t->capacity, t->volume, t->radius};
  bool seen[4] = {false, false, false, false};
  auto aname = [&](uint32_t id) {
    const AttrRec* ar = ds.attr(id);
    return ar ? std::string(ds.attr_name(*ar)) : std::to_string(id);
  };
  for (auto& a : ds.type_attrs(*t)) {
    double v = a.v;
    for (int k = 0; k < 4; k++)
      if (a.id == fid[k]) {
        seen[k] = true;
        if (fval[k] != 0.0) v = fval[k];
      }
    at.push_back({aname(a.id), v});
  }
  for (int k = 0; k < 4; k++)
    if (!seen[k]) at.push_back({aname(fid[k]), fval[k]});
  std::stable_sort(at.begin(), at.end(), [](auto& a, auto& b) { return a.first < b.first; });
  w.obj().key("attributes").obj();
  for (size_t i = 0; i < at.size(); i++)
    if (i + 1 >= at.size() || at[i + 1].first != at[i].first) w.key(at[i].first).num_raw(at[i].second);
  w.end_obj().key("capacity").num_raw(t->capacity).ki("category_id", t->category).key("effects").arr();
  for (auto& e : ds.type_effects(*t)) {
    const EffRec* er = ds.effect(e.id);
    w.obj().kb("default", e.is_default != 0).ki("id", e.id).key("name");
    if (er) w.str(ds.effect_name(*er));
    else w.null();
    w.end_obj();
  }
  w.end_arr().key("group");
  const GroupRec* g = ds.group(t->group);
  if (g) w.str(ds.group_name(*g));
  else w.null();
  w.ki("group_id", t->group).key("mass").num_raw(t->mass).ks("name", ds.type_name(*t)).key("name_zh");
  auto zh = ds.zh_name(t->id);
  if (zh.empty()) w.null();
  else w.str(zh);
  w.kb("published", t->published != 0).key("slot");
  if (auto s = slot_str(t->slot)) w.str(s);
  else w.null();
  w.ki("type_id", t->id).key("volume").num_raw(t->volume).end_obj();
}

// Unicode-ish lowercase (ASCII, Latin-1, Latin Ext-A, Greek, Cyrillic, fullwidth) for search matching.
static std::string ulower(std::string_view s) {
  std::string o;
  o.reserve(s.size());
  size_t i = 0;
  auto put = [&](uint32_t c) {
    if (c < 0x80) o.push_back((char)c);
    else if (c < 0x800) { o.push_back((char)(0xC0 | (c >> 6))); o.push_back((char)(0x80 | (c & 63))); }
    else if (c < 0x10000) { o.push_back((char)(0xE0 | (c >> 12))); o.push_back((char)(0x80 | ((c >> 6) & 63))); o.push_back((char)(0x80 | (c & 63))); }
    else { o.push_back((char)(0xF0 | (c >> 18))); o.push_back((char)(0x80 | ((c >> 12) & 63))); o.push_back((char)(0x80 | ((c >> 6) & 63))); o.push_back((char)(0x80 | (c & 63))); }
  };
  while (i < s.size()) {
    unsigned char c0 = (unsigned char)s[i];
    uint32_t c;
    size_t n;
    if (c0 < 0x80) { c = c0; n = 1; }
    else if ((c0 >> 5) == 6 && i + 1 < s.size()) { c = ((c0 & 31u) << 6) | (s[i + 1] & 63); n = 2; }
    else if ((c0 >> 4) == 14 && i + 2 < s.size()) { c = ((c0 & 15u) << 12) | ((s[i + 1] & 63u) << 6) | (s[i + 2] & 63); n = 3; }
    else if ((c0 >> 3) == 30 && i + 3 < s.size()) { c = ((c0 & 7u) << 18) | ((s[i + 1] & 63u) << 12) | ((s[i + 2] & 63u) << 6) | (s[i + 3] & 63); n = 4; }
    else { o.push_back((char)c0); i++; continue; }
    if (c >= 'A' && c <= 'Z') c += 32;
    else if (c >= 0xC0 && c <= 0xDE && c != 0xD7) c += 32;
    else if (c >= 0x100 && c <= 0x17F && c != 0x130 && c != 0x138 && c != 0x149 && c != 0x178 && c != 0x17F) {
      bool odd_upper = (c >= 0x139 && c <= 0x148) || (c >= 0x179 && c <= 0x17E);
      if (odd_upper ? (c & 1) : !(c & 1)) c += 1;
    } else if (c == 0x178) c = 0xFF;
    else if (c >= 0x391 && c <= 0x3A9 && c != 0x3A2) c += 32;
    else if (c >= 0x410 && c <= 0x42F) c += 32;
    else if (c >= 0x400 && c <= 0x40F) c += 80;
    else if (c >= 0xFF21 && c <= 0xFF3A) c += 32;
    put(c);
    i += n;
  }
  return o;
}

static const char* search_kind(const Dataset& ds, const TypeRec& t) {
  switch (t.category) {
    case 6: return "ship";
    case 7: return "module";
    case 8: return "charge";
    case 18: return "drone";
    case 87: return "fighter";
    case 32: return "subsystem";
    case 16: return "skill";
    case 20: {
      const GroupRec* g = ds.group(t.group);
      return g && ds.group_name(*g).find("Booster") != std::string_view::npos ? "booster" : "implant";
    }
    default: return nullptr;
  }
}

// Interim search spec (contract v1.4.1): published types of the scored kinds, exact > prefix > substring on the
// lowercased English or Chinese name, ties by type id; default limit 20.
void search_json(const Dataset& ds, std::string_view q, size_t limit, JW& w, const std::vector<std::string>* kinds) {
  while (!q.empty() && isspace((unsigned char)q.front())) q.remove_prefix(1);
  while (!q.empty() && isspace((unsigned char)q.back())) q.remove_suffix(1);
  std::string ql = ulower(q);
  struct Hit {
    uint8_t r;
    const TypeRec* t;
    const char* kind;
  };
  std::vector<Hit> hits;
  for (auto& t : ds.types) {
    if (!t.published) continue;
    const char* k = search_kind(ds, t);
    if (!k) continue;
    if (kinds) {
      bool ok = false;
      for (auto& x : *kinds)
        if (x == k) ok = true;
      if (!ok) continue;
    }
    std::string en = ulower(ds.type_name(t));
    std::string zh = ulower(ds.zh_name(t.id));
    uint8_t r;
    if (en == ql || (!zh.empty() && zh == ql)) r = 0;
    else if (en.compare(0, ql.size(), ql) == 0 || (!zh.empty() && zh.compare(0, ql.size(), ql) == 0)) r = 1;
    else if (en.find(ql) != std::string::npos || (!zh.empty() && zh.find(ql) != std::string::npos)) r = 2;
    else continue;
    hits.push_back({r, &t, k});
  }
  std::sort(hits.begin(), hits.end(), [](const Hit& a, const Hit& b) { return a.r != b.r ? a.r < b.r : a.t->id < b.t->id; });
  static const char* MATCH[3] = {"exact", "prefix", "substring"};
  w.arr();
  for (size_t i = 0; i < hits.size() && i < limit; i++) {
    const TypeRec& t = *hits[i].t;
    w.obj().ki("category_id", t.category).key("group");
    const GroupRec* g = ds.group(t.group);
    if (g) w.str(ds.group_name(*g));
    else w.null();
    w.ks("kind", hits[i].kind).ks("match", MATCH[hits[i].r]).key("meta_level");
    if (t.has_meta_level) w.i64(t.meta_level);
    else w.null();
    w.ks("name", ds.type_name(t)).key("name_zh");
    auto zh = ds.zh_name(t.id);
    if (zh.empty()) w.null();
    else w.str(zh);
    w.key("slot");
    if (auto s = slot_str(t.slot)) w.str(s);
    else w.null();
    w.ki("type_id", t.id).end_obj();
  }
  w.end_arr();
}

}  // namespace evej
