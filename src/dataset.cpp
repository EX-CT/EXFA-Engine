#include "dataset.hpp"

#include <fcntl.h>
#include <libdeflate.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <unistd.h>

#include <algorithm>
#include <cstdio>
#include <map>
#include <simdjson.h>
#include <unordered_map>

namespace evej {

// ------------------------------------------------------------------ SHA-256 (FIPS 180-4)
std::string sha256_hex(const uint8_t* data, size_t n) {
  static const uint32_t K[64] = {
      0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98,
      0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
      0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8,
      0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
      0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819,
      0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
      0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
      0xc67178f2};
  uint32_t h[8] = {0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19};
  auto rotr = [](uint32_t x, int r) { return (x >> r) | (x << (32 - r)); };
  auto block = [&](const uint8_t* c) {
    uint32_t w[64];
    for (int i = 0; i < 16; i++) w[i] = (uint32_t)c[4 * i] << 24 | (uint32_t)c[4 * i + 1] << 16 | (uint32_t)c[4 * i + 2] << 8 | c[4 * i + 3];
    for (int i = 16; i < 64; i++) {
      uint32_t s0 = rotr(w[i - 15], 7) ^ rotr(w[i - 15], 18) ^ (w[i - 15] >> 3);
      uint32_t s1 = rotr(w[i - 2], 17) ^ rotr(w[i - 2], 19) ^ (w[i - 2] >> 10);
      w[i] = w[i - 16] + s0 + w[i - 7] + s1;
    }
    uint32_t a = h[0], b = h[1], cc = h[2], d = h[3], e = h[4], f = h[5], g = h[6], hh = h[7];
    for (int i = 0; i < 64; i++) {
      uint32_t S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      uint32_t ch = (e & f) ^ (~e & g);
      uint32_t t1 = hh + S1 + ch + K[i] + w[i];
      uint32_t S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      uint32_t mj = (a & b) ^ (a & cc) ^ (b & cc);
      uint32_t t2 = S0 + mj;
      hh = g; g = f; f = e; e = d + t1; d = cc; cc = b; b = a; a = t1 + t2;
    }
    h[0] += a; h[1] += b; h[2] += cc; h[3] += d; h[4] += e; h[5] += f; h[6] += g; h[7] += hh;
  };
  size_t full = n / 64;
  for (size_t i = 0; i < full; i++) block(data + 64 * i);
  uint8_t tail[128] = {0};
  size_t rem = n - full * 64;
  memcpy(tail, data + full * 64, rem);
  tail[rem] = 0x80;
  size_t tl = rem + 1 + 8 <= 64 ? 64 : 128;
  uint64_t bits = (uint64_t)n * 8;
  for (int i = 0; i < 8; i++) tail[tl - 1 - i] = (uint8_t)(bits >> (8 * i));
  block(tail);
  if (tl == 128) block(tail + 64);
  static const char* hx = "0123456789abcdef";
  std::string out(64, '0');
  for (int i = 0; i < 8; i++)
    for (int j = 0; j < 8; j++) out[i * 8 + j] = hx[(h[i] >> (28 - 4 * j)) & 15];
  return out;
}

static uint64_t fast_hash(const uint8_t* p, size_t n) {
  uint64_t h = 0x9E3779B97F4A7C15ull ^ n;
  size_t i = 0;
  for (; i + 8 <= n; i += 8) {
    uint64_t w;
    memcpy(&w, p + i, 8);
    h = (h ^ w) * 0xff51afd7ed558ccdull;
    h ^= h >> 32;
  }
  for (; i < n; i++) h = (h ^ p[i]) * 0x100000001b3ull;
  return h;
}

// ------------------------------------------------------------------ image builder
namespace {
struct Builder {
  std::vector<uint8_t> buf;
  Header hdr{};
  template <class T>
  void put(Section s, const std::vector<T>& v) {
    while (buf.size() % 16) buf.push_back(0);
    hdr.sec[s].off = buf.size();
    hdr.sec[s].bytes = v.size() * sizeof(T);
    const uint8_t* p = reinterpret_cast<const uint8_t*>(v.data());
    buf.insert(buf.end(), p, p + v.size() * sizeof(T));
  }
};

std::string lower_ascii(std::string_view s) {
  std::string o(s);
  for (auto& c : o)
    if (c >= 'A' && c <= 'Z') c = c - 'A' + 'a';
  return o;
}

using RE = simdjson::simdjson_result<simdjson::dom::element>;
uint32_t opt_u32(RE r) {
  simdjson::dom::element e;
  if (r.get(e) != simdjson::SUCCESS || e.is_null()) return 0;
  int64_t v;
  if (e.get_int64().get(v) == simdjson::SUCCESS) return (uint32_t)v;
  double d;
  if (e.get_double().get(d) == simdjson::SUCCESS) return (uint32_t)d;
  return 0;
}
double num(RE r, double def = 0.0) {
  simdjson::dom::element e;
  if (r.get(e) != simdjson::SUCCESS) return def;
  double d;
  if (e.get_double().get(d) == simdjson::SUCCESS) return d;
  return def;
}
bool flag(RE r, bool def) {
  simdjson::dom::element e;
  if (r.get(e) != simdjson::SUCCESS) return def;
  bool b;
  if (e.get_bool().get(b) == simdjson::SUCCESS) return b;
  int64_t i;
  if (e.get_int64().get(i) == simdjson::SUCCESS) return i != 0;
  return def;
}
uint32_t opt_u32(const simdjson::dom::element& e) { return opt_u32(RE(simdjson::dom::element(e))); }
double num(const simdjson::dom::element& e, double def = 0.0) { return num(RE(simdjson::dom::element(e)), def); }
uint32_t parse_key(std::string_view k) {
  uint32_t v = 0;
  for (char c : k) {
    if (c < '0' || c > '9') return 0;
    v = v * 10 + (c - '0');
  }
  return v;
}
}  // namespace

bool Dataset::build_image(const std::vector<uint8_t>& src, std::vector<uint8_t>& out, std::string& err) {
  std::vector<uint8_t> json;
  if (src.size() > 18 && src[0] == 0x1f && src[1] == 0x8b) {
    uint32_t isize = src[src.size() - 4] | src[src.size() - 3] << 8 | src[src.size() - 2] << 16 | (uint32_t)src[src.size() - 1] << 24;
    size_t cap = std::max<size_t>(isize, src.size() * 4);
    libdeflate_decompressor* d = libdeflate_alloc_decompressor();
    for (int attempt = 0; attempt < 4; attempt++) {
      json.resize(cap);
      size_t got = 0;
      auto r = libdeflate_gzip_decompress(d, src.data(), src.size(), json.data(), json.size(), &got);
      if (r == LIBDEFLATE_SUCCESS) {
        json.resize(got);
        break;
      }
      if (r != LIBDEFLATE_INSUFFICIENT_SPACE) {
        libdeflate_free_decompressor(d);
        err = "gunzip failed";
        return false;
      }
      cap *= 4;
      if (attempt == 3) {
        libdeflate_free_decompressor(d);
        err = "gunzip: output too large";
        return false;
      }
    }
    libdeflate_free_decompressor(d);
  } else {
    json = src;
  }
  Builder b;
  memcpy(b.hdr.magic, "EVEJBIN1", 8);
  b.hdr.version = CACHE_VERSION;
  std::string sha = sha256_hex(json.data(), json.size());
  memcpy(b.hdr.sha256, sha.data(), 64);

  simdjson::dom::parser parser;
  simdjson::padded_string ps(reinterpret_cast<const char*>(json.data()), json.size());
  simdjson::dom::element root;
  if (parser.parse(ps).get(root)) {
    err = "dataset json: parse error";
    return false;
  }
  std::string_view fmt;
  if (root["format"].get_string().get(fmt) || fmt != "exct-eve-dataset" || opt_u32(root["format_version"]) != 1) {
    err = "unsupported dataset format";
    return false;
  }
  b.hdr.build = (uint64_t)num(root["sde"]["build"]);
  {
    std::string_view rd;
    if (!root["sde"]["release_date"].get_string().get(rd)) {
      size_t n = std::min<size_t>(rd.size(), 31);
      memcpy(b.hdr.release_date, rd.data(), n);
    }
  }
  std::vector<char> strs;
  auto add_str = [&](std::string_view s, uint32_t& off, uint32_t& len) {
    off = (uint32_t)strs.size();
    len = (uint32_t)s.size();
    strs.insert(strs.end(), s.begin(), s.end());
  };
  auto get_str = [](RE e) -> std::string_view {
    std::string_view s;
    if (e.get_string().get(s)) return {};
    return s;
  };

  // attributes
  std::vector<AttrRec> attrs;
  std::vector<NameIdx> attr_names;
  uint32_t max_attr = 0;
  for (auto [k, v] : root["attributes"].get_object()) {
    AttrRec a{};
    a.id = parse_key(k);
    std::string_view name = get_str(v["name"]);
    add_str(name, a.name_off, a.name_len);
    a.def = num(v["default"]);
    a.stackable = flag(v["stackable"], true);
    a.high_is_good = flag(v["high_is_good"], true);
    a.min_attr = opt_u32(v["min_attr"]);
    a.max_attr = opt_u32(v["max_attr"]);
    a.round2 = (name == "cpu" || name == "power" || name == "cpuOutput" || name == "powerOutput");
    a.has_info = 1;
    attrs.push_back(a);
    attr_names.push_back({a.name_off, a.name_len, a.id, 0});
    max_attr = std::max(max_attr, a.id);
  }
  std::sort(attrs.begin(), attrs.end(), [](auto& x, auto& y) { return x.id < y.id; });
  std::vector<int32_t> attr_idx(max_attr + 1, -1);
  for (size_t i = 0; i < attrs.size(); i++) attr_idx[attrs[i].id] = (int32_t)i;

  // effects
  std::vector<EffRec> effects;
  std::vector<ModRec> mods;
  std::vector<NameIdx> eff_names;
  uint32_t max_eff = 0;
  {
    std::vector<std::pair<uint32_t, simdjson::dom::element>> raw;
    for (auto [k, v] : root["effects"].get_object()) raw.push_back({parse_key(k), v});
    std::sort(raw.begin(), raw.end(), [](auto& x, auto& y) { return x.first < y.first; });
    for (auto& [id, v] : raw) {
      EffRec e{};
      e.id = id;
      add_str(get_str(v["name"]), e.name_off, e.name_len);
      e.category = opt_u32(v["category"]);
      e.duration_attr = opt_u32(v["duration_attr"]);
      e.discharge_attr = opt_u32(v["discharge_attr"]);
      e.range_attr = opt_u32(v["range_attr"]);
      e.falloff_attr = opt_u32(v["falloff_attr"]);
      e.tracking_attr = opt_u32(v["tracking_attr"]);
      e.resistance_attr = opt_u32(v["resistance_attr"]);
      simdjson::dom::element fu;
      e.has_fuc = v["fitting_usage_chance_attr"].get(fu) == simdjson::SUCCESS && !fu.is_null();
      e.fitting_usage_chance_attr = e.has_fuc ? opt_u32(fu) : 0;
      e.is_offensive = flag(v["is_offensive"], false);
      e.is_assistance = flag(v["is_assistance"], false);
      e.mod_off = (uint32_t)mods.size();
      e.all_item_domain = 1;
      simdjson::dom::array ma;
      if (!v["mods"].get_array().get(ma)) {
        for (auto m : ma) {
          int64_t t[6] = {0, 0, 0, 0, 0, 0};
          int i = 0;
          for (auto x : m.get_array()) {
            if (i < 6) t[i++] = (int64_t)num(x);
          }
          ModRec r{(int32_t)t[0], (int32_t)t[1], (int32_t)t[4], (uint32_t)t[2], (uint32_t)t[3], (uint32_t)t[5]};
          if (r.domain != 0) e.all_item_domain = 0;
          mods.push_back(r);
        }
      }
      e.mod_cnt = (uint32_t)mods.size() - e.mod_off;
      effects.push_back(e);
      eff_names.push_back({e.name_off, e.name_len, e.id, 0});
      max_eff = std::max(max_eff, id);
    }
  }
  std::vector<int32_t> eff_idx(max_eff + 1, -1);
  for (size_t i = 0; i < effects.size(); i++) eff_idx[effects[i].id] = (int32_t)i;

  // groups
  std::vector<GroupRec> groups;
  uint32_t max_group = 0;
  for (auto [k, v] : root["groups"].get_object()) {
    GroupRec g{};
    g.id = parse_key(k);
    g.category = opt_u32(v["category"]);
    add_str(get_str(v["name"]), g.name_off, g.name_len);
    groups.push_back(g);
    max_group = std::max(max_group, g.id);
  }
  std::sort(groups.begin(), groups.end(), [](auto& x, auto& y) { return x.id < y.id; });
  std::vector<int32_t> group_idx(max_group + 1, -1);
  for (size_t i = 0; i < groups.size(); i++) group_idx[groups[i].id] = (int32_t)i;

  // types
  std::vector<TypeRec> types;
  std::vector<TAttr> tattrs;
  std::vector<TEff> teffs;
  std::vector<uint32_t> skills, modes;
  uint32_t max_type = 0;
  {
    std::vector<std::pair<uint32_t, simdjson::dom::element>> raw;
    for (auto [k, v] : root["types"].get_object()) raw.push_back({parse_key(k), v});
    std::sort(raw.begin(), raw.end(), [](auto& x, auto& y) { return x.first < y.first; });
    const uint32_t req_attrs[6] = {182, 183, 184, 1285, 1289, 1290};
    for (auto& [id, v] : raw) {
      TypeRec t{};
      t.id = id;
      add_str(get_str(v["name"]), t.name_off, t.name_len);
      t.group = opt_u32(v["group"]);
      t.category = opt_u32(v["category"]);
      t.published = flag(v["published"], false);
      t.mass = num(v["mass"]);
      t.volume = num(v["volume"]);
      t.capacity = num(v["capacity"]);
      t.radius = num(v["radius"]);
      simdjson::dom::element ml;
      t.has_meta_level = v["meta_level"].get(ml) == simdjson::SUCCESS && !ml.is_null() && ml.is_number();
      t.meta_level = t.has_meta_level ? (int32_t)num(ml) : 0;
      t.attr_off = (uint32_t)tattrs.size();
      simdjson::dom::object ao;
      if (!v["attrs"].get_object().get(ao))
        for (auto [ak, av] : ao) tattrs.push_back({parse_key(ak), 0, num(av)});
      std::sort(tattrs.begin() + t.attr_off, tattrs.end(), [](auto& x, auto& y) { return x.id < y.id; });
      t.attr_cnt = (uint32_t)tattrs.size() - t.attr_off;
      t.eff_off = (uint32_t)teffs.size();
      t.slot = -1;
      simdjson::dom::array ea;
      if (!v["effects"].get_array().get(ea))
        for (auto e : ea) {
          uint32_t eid = 0, def = 0;
          int i = 0;
          for (auto x : e.get_array()) {
            if (i == 0) eid = opt_u32(x);
            else if (i == 1) def = opt_u32(x) != 0;
            i++;
          }
          teffs.push_back({eid, def});
          if (t.slot < 0) {
            switch (eid) {
              case 12: t.slot = 0; break;
              case 13: t.slot = 1; break;
              case 11: t.slot = 2; break;
              case 2663: t.slot = 3; break;
              case 3772: t.slot = 4; break;
              case 6306: t.slot = 5; break;
            }
          }
        }
      t.eff_cnt = (uint32_t)teffs.size() - t.eff_off;
      std::span<const TAttr> ta(tattrs.data() + t.attr_off, t.attr_cnt);
      for (uint32_t ra : req_attrs) {
        const TAttr* p = find_tattr(ta, ra);
        if (p && (uint32_t)p->v != 0) t.req_skills[t.n_req++] = (uint32_t)p->v;
      }
      if (t.category == 16 && t.published) skills.push_back(id);
      if (t.group == 1306) modes.push_back(id);
      types.push_back(t);
      max_type = std::max(max_type, id);
    }
  }
  std::vector<int32_t> type_idx(max_type + 1, -1);
  for (size_t i = 0; i < types.size(); i++) type_idx[types[i].id] = (int32_t)i;
  // lowercase name index: smallest-id published type wins, else smallest-id
  std::vector<NameIdx> type_names;
  {
    std::map<std::string, std::pair<uint32_t, bool>> best;
    for (auto& t : types) {
      std::string ln = lower_ascii(std::string_view(strs.data() + t.name_off, t.name_len));
      auto it = best.find(ln);
      if (it == best.end()) best.emplace(ln, std::make_pair(t.id, (bool)t.published));
      else if (t.published && !it->second.second) it->second = {t.id, true};
    }
    for (auto& [n, v] : best) {
      NameIdx x{};
      add_str(n, x.off, x.len);
      x.id = v.first;
      type_names.push_back(x);
    }
  }

  // dbuffs
  std::vector<DbuffRec> dbuffs;
  std::vector<uint32_t> pool;
  for (auto [k, v] : root["dbuffs"].get_object()) {
    DbuffRec d{};
    d.id = parse_key(k);
    d.op = (int32_t)num(v["op"]);
    d.aggregate_min = get_str(v["aggregate"]) == "Minimum";
    auto list = [&](const char* key, uint32_t& off, uint32_t& cnt) {
      off = (uint32_t)pool.size();
      simdjson::dom::array a;
      if (!v[key].get_array().get(a))
        for (auto x : a) {
          if (x.is_array())
            for (auto y : x.get_array()) pool.push_back(opt_u32(y));
          else pool.push_back(opt_u32(x));
        }
      cnt = (uint32_t)pool.size() - off;
    };
    list("item", d.item_off, d.item_cnt);
    list("location", d.loc_off, d.loc_cnt);
    list("location_group", d.lgrp_off, d.lgrp_cnt);
    list("location_skill", d.lskill_off, d.lskill_cnt);
    dbuffs.push_back(d);
  }
  std::sort(dbuffs.begin(), dbuffs.end(), [](auto& x, auto& y) { return x.id < y.id; });

  // mutaplasmids
  std::vector<MutaRec> mutas;
  std::vector<MutaAttr> muta_attrs;
  for (auto [k, v] : root["mutaplasmids"].get_object()) {
    MutaRec m{};
    m.id = parse_key(k);
    m.off = (uint32_t)muta_attrs.size();
    simdjson::dom::object ao;
    if (!v["attrs"].get_object().get(ao))
      for (auto [ak, av] : ao) {
        double r[2] = {0, 0};
        int i = 0;
        for (auto x : av.get_array())
          if (i < 2) r[i++] = num(x);
        muta_attrs.push_back({parse_key(ak), 0, r[0], r[1]});
      }
    std::sort(muta_attrs.begin() + m.off, muta_attrs.end(), [](auto& x, auto& y) { return x.attr < y.attr; });
    m.cnt = (uint32_t)muta_attrs.size() - m.off;
    mutas.push_back(m);
  }
  std::sort(mutas.begin(), mutas.end(), [](auto& x, auto& y) { return x.id < y.id; });

  // zh names
  std::vector<NameIdx> zh;
  {
    simdjson::dom::object zo;
    if (!root["names"]["zh"].get_object().get(zo))
      for (auto [k, v] : zo) {
        NameIdx x{};
        add_str(get_str(RE(simdjson::dom::element(v))), x.off, x.len);
        x.id = parse_key(k);
        zh.push_back(x);
      }
    std::sort(zh.begin(), zh.end(), [](auto& x, auto& y) { return x.id < y.id; });
  }

  auto by_name = [&](std::vector<NameIdx>& v) {
    std::stable_sort(v.begin(), v.end(), [&](const NameIdx& x, const NameIdx& y) {
      return std::string_view(strs.data() + x.off, x.len) < std::string_view(strs.data() + y.off, y.len);
    });
  };
  by_name(attr_names);
  by_name(eff_names);
  // type_names already sorted (std::map order)

  b.buf.resize(sizeof(Header));
  b.put(S_STR, strs);
  b.put(S_ATTRS, attrs);
  b.put(S_ATTR_IDX, attr_idx);
  b.put(S_TYPES, types);
  b.put(S_TYPE_IDX, type_idx);
  b.put(S_TATTRS, tattrs);
  b.put(S_TEFFS, teffs);
  b.put(S_EFFECTS, effects);
  b.put(S_EFF_IDX, eff_idx);
  b.put(S_MODS, mods);
  b.put(S_GROUPS, groups);
  b.put(S_GROUP_IDX, group_idx);
  b.put(S_DBUFFS, dbuffs);
  b.put(S_U32POOL, pool);
  b.put(S_MUTAS, mutas);
  b.put(S_MUTA_ATTRS, muta_attrs);
  b.put(S_ATTR_NAMES, attr_names);
  b.put(S_EFF_NAMES, eff_names);
  b.put(S_TYPE_NAMES, type_names);
  b.put(S_SKILLS, skills);
  b.put(S_MODES, modes);
  b.put(S_ZH, zh);
  b.hdr.total_bytes = b.buf.size();
  memcpy(b.buf.data(), &b.hdr, sizeof(Header));
  out = std::move(b.buf);
  return true;
}

template <class T>
static std::span<const T> sec(const uint8_t* base, const Header& h, Section s) {
  return {reinterpret_cast<const T*>(base + h.sec[s].off), (size_t)(h.sec[s].bytes / sizeof(T))};
}

bool Dataset::attach(const uint8_t* base, size_t size, std::string& err) {
  if (size < sizeof(Header)) {
    err = "cache too small";
    return false;
  }
  Header h;
  memcpy(&h, base, sizeof(Header));
  if (memcmp(h.magic, "EVEJBIN1", 8) != 0 || h.version != CACHE_VERSION || h.total_bytes != size) {
    err = "bad cache header";
    return false;
  }
  build = h.build;
  sha256.assign(h.sha256, 64);
  release_date.assign(h.release_date, strnlen(h.release_date, sizeof h.release_date));
  strs_ = sec<char>(base, h, S_STR);
  attrs = sec<AttrRec>(base, h, S_ATTRS);
  attr_idx_ = sec<int32_t>(base, h, S_ATTR_IDX);
  types = sec<TypeRec>(base, h, S_TYPES);
  type_idx_ = sec<int32_t>(base, h, S_TYPE_IDX);
  tattrs_ = sec<TAttr>(base, h, S_TATTRS);
  teffs_ = sec<TEff>(base, h, S_TEFFS);
  effects = sec<EffRec>(base, h, S_EFFECTS);
  eff_idx_ = sec<int32_t>(base, h, S_EFF_IDX);
  mods_ = sec<ModRec>(base, h, S_MODS);
  groups = sec<GroupRec>(base, h, S_GROUPS);
  group_idx_ = sec<int32_t>(base, h, S_GROUP_IDX);
  dbuffs = sec<DbuffRec>(base, h, S_DBUFFS);
  u32pool_ = sec<uint32_t>(base, h, S_U32POOL);
  mutas = sec<MutaRec>(base, h, S_MUTAS);
  muta_attrs_ = sec<MutaAttr>(base, h, S_MUTA_ATTRS);
  attr_names_ = sec<NameIdx>(base, h, S_ATTR_NAMES);
  eff_names_ = sec<NameIdx>(base, h, S_EFF_NAMES);
  type_names_ = sec<NameIdx>(base, h, S_TYPE_NAMES);
  skills = sec<uint32_t>(base, h, S_SKILLS);
  modes = sec<uint32_t>(base, h, S_MODES);
  zh_ = sec<NameIdx>(base, h, S_ZH);
  return true;
}

static bool read_file(const std::string& p, std::vector<uint8_t>& out) {
  FILE* f = fopen(p.c_str(), "rb");
  if (!f) return false;
  fseek(f, 0, SEEK_END);
  long n = ftell(f);
  fseek(f, 0, SEEK_SET);
  out.resize(n > 0 ? (size_t)n : 0);
  size_t got = n > 0 ? fread(out.data(), 1, (size_t)n, f) : 0;
  fclose(f);
  return got == out.size();
}

Dataset* Dataset::open(const std::string& path, const std::string& cache_path, bool use_cache, std::string& err) {
  struct stat st {};
  if (stat(path.c_str(), &st) != 0) {
    err = "read " + path + ": cannot open";
    return nullptr;
  }
  const int64_t mt = (int64_t)st.st_mtim.tv_sec * 1000000000ll + st.st_mtim.tv_nsec;
  std::vector<uint8_t> src;
  bool have_src = false;
  uint64_t h = 0;
  auto load_src = [&]() {
    if (have_src) return true;
    if (!read_file(path, src)) return false;
    h = fast_hash(src.data(), src.size());
    have_src = true;
    return true;
  };
  auto* ds = new Dataset();
  if (use_cache && !cache_path.empty()) {
    int fd = ::open(cache_path.c_str(), O_RDONLY);
    if (fd >= 0) {
      struct stat cs {};
      if (fstat(fd, &cs) == 0 && cs.st_size >= (off_t)sizeof(Header)) {
        void* m = mmap(nullptr, (size_t)cs.st_size, PROT_READ, MAP_PRIVATE, fd, 0);
        if (m != MAP_FAILED) {
          Header hh;
          memcpy(&hh, m, sizeof hh);
          std::string e2;
          // fast path: same size + mtime as when the image was built; otherwise compare the content hash
          bool fresh = hh.src_size == (uint64_t)st.st_size && hh.src_mtime_ns == mt;
          if (!fresh && load_src()) fresh = hh.src_size == src.size() && hh.src_hash == h;
          if (fresh && ds->attach((const uint8_t*)m, (size_t)cs.st_size, e2)) {
            ds->map_ = m;
            ds->map_size_ = (size_t)cs.st_size;

            ::close(fd);
            return ds;
          }
          munmap(m, (size_t)cs.st_size);
        }
      }
      ::close(fd);
    }
  }
  if (!load_src()) {
    delete ds;
    err = "read " + path + ": cannot open";
    return nullptr;
  }
  std::vector<uint8_t> img;
  if (!build_image(src, img, err)) {
    delete ds;
    return nullptr;
  }
  Header hh;
  memcpy(&hh, img.data(), sizeof hh);
  hh.src_size = src.size();
  hh.src_mtime_ns = mt;
  hh.src_hash = h;
  memcpy(img.data(), &hh, sizeof hh);
  if (use_cache && !cache_path.empty()) {
    std::string tmp = cache_path + ".tmp." + std::to_string(getpid());
    FILE* f = fopen(tmp.c_str(), "wb");
    if (f) {
      bool ok = fwrite(img.data(), 1, img.size(), f) == img.size();
      ok = fclose(f) == 0 && ok;
      if (ok) rename(tmp.c_str(), cache_path.c_str());
      else unlink(tmp.c_str());
    }
  }
  ds->owned_ = std::move(img);
  if (!ds->attach(ds->owned_.data(), ds->owned_.size(), err)) {
    delete ds;
    return nullptr;
  }
  return ds;
}

Dataset::~Dataset() {
  if (map_) munmap(map_, map_size_);
}

uint32_t Dataset::lookup(std::span<const NameIdx> idx, std::string_view name) const {
  size_t lo = 0, hi = idx.size();
  while (lo < hi) {
    size_t m = (lo + hi) >> 1;
    if (str(idx[m].off, idx[m].len) < name) lo = m + 1;
    else hi = m;
  }
  return lo < idx.size() && str(idx[lo].off, idx[lo].len) == name ? idx[lo].id : 0;
}

uint32_t Dataset::type_by_name(std::string_view name) const {
  while (!name.empty() && (unsigned char)name.front() <= ' ') name.remove_prefix(1);
  while (!name.empty() && (unsigned char)name.back() <= ' ') name.remove_suffix(1);
  return lookup(type_names_, lower_ascii(name));
}

const DbuffRec* Dataset::dbuff(uint32_t id) const {
  auto it = std::lower_bound(dbuffs.begin(), dbuffs.end(), id, [](const DbuffRec& d, uint32_t v) { return d.id < v; });
  return it != dbuffs.end() && it->id == id ? &*it : nullptr;
}
const MutaRec* Dataset::muta(uint32_t id) const {
  auto it = std::lower_bound(mutas.begin(), mutas.end(), id, [](const MutaRec& d, uint32_t v) { return d.id < v; });
  return it != mutas.end() && it->id == id ? &*it : nullptr;
}
std::string_view Dataset::zh_name(uint32_t id) const {
  auto it = std::lower_bound(zh_.begin(), zh_.end(), id, [](const NameIdx& d, uint32_t v) { return d.id < v; });
  return it != zh_.end() && it->id == id ? str(it->off, it->len) : std::string_view{};
}

}  // namespace evej
