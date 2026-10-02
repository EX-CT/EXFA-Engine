#include "api.hpp"

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
  Fit fit(ds, ids);
  EngineError ee{};
  if (!fit.build(req, ee)) {
    write_error(w, ee.code, ee.message, ee.path);
    return false;
  }
  compute_stats(fit, req, w);
  return true;
}

bool Worker::calc_json(std::string_view request) {
  out.clear();
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

static std::string lower(std::string_view s) {
  std::string o(s);
  for (auto& c : o)
    if (c >= 'A' && c <= 'Z') c = c - 'A' + 'a';
  return o;
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
  for (auto& a : ds.type_attrs(*t)) {
    const AttrRec* ar = ds.attr(a.id);
    at.push_back({ar ? std::string(ds.attr_name(*ar)) : std::to_string(a.id), a.v});
  }
  std::stable_sort(at.begin(), at.end(), [](auto& a, auto& b) { return a.first < b.first; });
  w.obj().key("attributes").obj();
  for (size_t i = 0; i < at.size(); i++)
    if (i + 1 >= at.size() || at[i + 1].first != at[i].first) w.kn(at[i].first, at[i].second);
  w.end_obj().kn("capacity", t->capacity).ki("category_id", t->category).key("effects").arr();
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
  w.ki("group_id", t->group).kn("mass", t->mass).ks("name", ds.type_name(*t)).key("name_zh");
  auto zh = ds.zh_name(t->id);
  if (zh.empty()) w.null();
  else w.str(zh);
  w.kb("published", t->published != 0).key("slot");
  if (auto s = slot_str(t->slot)) w.str(s);
  else w.null();
  w.ki("type_id", t->id).kn("volume", t->volume).end_obj();
}

void search_json(const Dataset& ds, std::string_view q, size_t limit, JW& w) {
  std::string ql = lower(q);
  struct Hit {
    const TypeRec* t;
    bool not_prefix;
    std::string_view name;
  };
  std::vector<Hit> hits;
  for (auto& t : ds.types) {
    if (!t.published) continue;
    std::string_view n = ds.type_name(t);
    std::string ln = lower(n);
    bool m = ln.find(ql) != std::string::npos;
    if (!m) {
      auto zh = ds.zh_name(t.id);
      m = !zh.empty() && zh.find(q) != std::string_view::npos;
    }
    if (m) hits.push_back({&t, ln.compare(0, ql.size(), ql) != 0, n});
  }
  std::stable_sort(hits.begin(), hits.end(), [](const Hit& a, const Hit& b) {
    if (a.not_prefix != b.not_prefix) return a.not_prefix < b.not_prefix;
    if (a.name.size() != b.name.size()) return a.name.size() < b.name.size();
    return a.name < b.name;
  });
  w.arr();
  for (size_t i = 0; i < hits.size() && i < limit; i++) {
    const TypeRec& t = *hits[i].t;
    w.obj().ki("category_id", t.category).key("group");
    const GroupRec* g = ds.group(t.group);
    if (g) w.str(ds.group_name(*g));
    else w.null();
    w.key("meta_level");
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
