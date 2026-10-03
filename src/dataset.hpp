// Dataset: exct-eve-dataset v1 (eve-sde-pipeline) loaded into a flat, mmap-able binary image.
#pragma once
#include <cstdint>
#include <cstring>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace evej {

// ---- POD records stored verbatim in the binary cache (all 4/8-byte aligned) ----
struct AttrRec {
  uint32_t id, min_attr, max_attr, name_off, name_len;
  uint8_t stackable, high_is_good, round2, has_info;
  double def;
};
struct TAttr {
  uint32_t id, pad;
  double v;
};
struct TEff {
  uint32_t id, is_default;
};
struct TypeRec {
  uint32_t id, group, category, published;
  double mass, volume, capacity, radius;
  uint32_t attr_off, attr_cnt, eff_off, eff_cnt, name_off, name_len;
  uint32_t req_skills[6];
  uint32_t n_req;
  int32_t slot;  // -1 none, else Slot enum
  int32_t meta_level;
  uint32_t has_meta_level;
  uint32_t market_group, has_market_group;
};
struct ModRec {
  int32_t func, domain, op;
  uint32_t modified, modifying, extra;
};
struct EffRec {
  uint32_t id, category, duration_attr, discharge_attr, range_attr, falloff_attr, tracking_attr, resistance_attr;
  uint32_t fitting_usage_chance_attr, has_fuc, is_offensive, is_assistance;
  uint32_t mod_off, mod_cnt, name_off, name_len;
  uint32_t all_item_domain;  // every modifier has domain Item (used by structure skill filter)
  uint32_t pad;
};
struct GroupRec {
  uint32_t id, category, name_off, name_len;
};
struct DbuffRec {
  uint32_t id;
  int32_t op;
  uint32_t aggregate_min;
  uint32_t item_off, item_cnt, loc_off, loc_cnt, lgrp_off, lgrp_cnt, lskill_off, lskill_cnt;  // pairs count as 2 u32
  uint32_t pad;
};
struct MutaAttr {
  uint32_t attr, pad;
  double lo, hi;
};
struct MutaRec {
  uint32_t id, off, cnt, map_off, map_cnt, pad;  // map: pool u32s [output, n, inputs...]*
};
struct NameIdx {
  uint32_t off, len, id, pad;
};

enum Section : uint32_t {
  S_STR, S_ATTRS, S_ATTR_IDX, S_TYPES, S_TYPE_IDX, S_TATTRS, S_TEFFS, S_EFFECTS, S_EFF_IDX, S_MODS, S_GROUPS,
  S_GROUP_IDX, S_DBUFFS, S_U32POOL, S_MUTAS, S_MUTA_ATTRS, S_ATTR_NAMES, S_EFF_NAMES, S_TYPE_NAMES, S_SKILLS,
  S_MODES, S_ZH, S_CATS, S_SKREL_NS, S_SKREL_ST, S_SKREL_G, S_SKREL_N, S_COUNT
};

struct SecEnt {
  uint64_t off, bytes;
};
struct Header {
  char magic[8];
  uint32_t version, pad;
  uint64_t src_size;
  int64_t src_mtime_ns;
  uint64_t src_hash;
  uint64_t build;
  char sha256[64];
  char release_date[32];
  uint64_t total_bytes;
  SecEnt sec[S_COUNT];
};

constexpr uint32_t CACHE_VERSION = 7;

class Dataset {
 public:
  // Load: binary cache if valid (else build it from the gz JSON and try to write it).
  static Dataset* open(const std::string& path, const std::string& cache_path, bool use_cache, std::string& err);
  // Build the binary image from raw (possibly gzip) JSON bytes.
  static bool build_image(const std::vector<uint8_t>& src, std::vector<uint8_t>& out, std::string& err);

  uint64_t build = 0;
  std::string sha256, release_date;

  std::span<const AttrRec> attrs;
  std::span<const TypeRec> types;
  std::span<const EffRec> effects;
  std::span<const GroupRec> groups;
  std::span<const GroupRec> cats;  // categories (id, -, name)
  std::span<const DbuffRec> dbuffs;
  std::span<const MutaRec> mutas;
  std::span<const uint32_t> skills;  // published skill ids, sorted
  // skill-pruning relevance table (built into the image; see compute_skill_rel in dataset.cpp)
  std::span<const uint8_t> skrel_always_ns, skrel_always_st;
  std::span<const uint64_t> skrel_by_group, skrel_by_need;
  std::span<const uint32_t> modes;   // tactical destroyer mode type ids (group 1306), sorted

  const TypeRec* type(uint32_t id) const {
    if (id >= type_idx_.size()) return nullptr;
    int32_t i = type_idx_[id];
    return i < 0 ? nullptr : &types[i];
  }
  const EffRec* effect(uint32_t id) const {
    if (id >= eff_idx_.size()) return nullptr;
    int32_t i = eff_idx_[id];
    return i < 0 ? nullptr : &effects[i];
  }
  const AttrRec* attr(uint32_t id) const {
    if (id >= attr_idx_.size()) return nullptr;
    int32_t i = attr_idx_[id];
    return i < 0 ? nullptr : &attrs[i];
  }
  const GroupRec* group(uint32_t id) const {
    if (id >= group_idx_.size()) return nullptr;
    int32_t i = group_idx_[id];
    return i < 0 ? nullptr : &groups[i];
  }
  const DbuffRec* dbuff(uint32_t id) const;
  std::string_view category_name(uint32_t id) const;
  const MutaRec* muta(uint32_t id) const;
  // mutated output type for base + mutaplasmid (0 if no mapping)
  uint32_t muta_output(uint32_t muta_id, uint32_t base) const;
  double attr_default(uint32_t id) const {
    const AttrRec* a = attr(id);
    return a ? a->def : 0.0;
  }
  std::span<const TAttr> type_attrs(const TypeRec& t) const { return {tattrs_.data() + t.attr_off, t.attr_cnt}; }
  std::span<const TEff> type_effects(const TypeRec& t) const { return {teffs_.data() + t.eff_off, t.eff_cnt}; }
  std::span<const ModRec> effect_mods(const EffRec& e) const { return {mods_.data() + e.mod_off, e.mod_cnt}; }
  std::span<const uint32_t> pool(uint32_t off, uint32_t cnt) const { return {u32pool_.data() + off, cnt}; }
  std::span<const MutaAttr> muta_attrs(const MutaRec& m) const { return {muta_attrs_.data() + m.off, m.cnt}; }
  // raw attribute value from the type's attribute list (sorted by id)
  static const TAttr* find_tattr(std::span<const TAttr> a, uint32_t id) {
    // branchless lower_bound
    const TAttr* base = a.data();
    size_t n = a.size();
    if (n == 0) return nullptr;
    while (n > 1) {
      size_t half = n >> 1;
      base = (base[half].id <= id) ? base + half : base;
      n -= half;
    }
    return base->id == id ? base : nullptr;
  }
  bool type_attr(const TypeRec& t, uint32_t id, double& out) const {
    const TAttr* p = find_tattr(type_attrs(t), id);
    if (!p) return false;
    out = p->v;
    return true;
  }
  std::string_view str(uint32_t off, uint32_t len) const { return {strs_.data() + off, len}; }
  std::string_view type_name(const TypeRec& t) const { return str(t.name_off, t.name_len); }
  std::string_view effect_name(const EffRec& e) const { return str(e.name_off, e.name_len); }
  std::string_view attr_name(const AttrRec& a) const { return str(a.name_off, a.name_len); }
  std::string_view group_name(const GroupRec& g) const { return str(g.name_off, g.name_len); }
  std::string_view zh_name(uint32_t type_id) const;

  uint32_t attr_id(std::string_view name) const { return lookup(attr_names_, name); }
  uint32_t effect_id(std::string_view name) const { return lookup(eff_names_, name); }
  // case-insensitive (lowercased, trimmed) name lookup; 0 if absent
  uint32_t type_by_name(std::string_view name) const;
  size_t type_count() const { return types.size(); }

  ~Dataset();

 private:
  uint32_t lookup(std::span<const NameIdx> idx, std::string_view name) const;
  bool attach(const uint8_t* base, size_t size, std::string& err);

  std::span<const char> strs_;
  std::span<const int32_t> attr_idx_, type_idx_, eff_idx_, group_idx_;
  std::span<const TAttr> tattrs_;
  std::span<const TEff> teffs_;
  std::span<const ModRec> mods_;
  std::span<const uint32_t> u32pool_;
  std::span<const MutaAttr> muta_attrs_;
  std::span<const NameIdx> attr_names_, eff_names_, type_names_, zh_;
  void* map_ = nullptr;
  size_t map_size_ = 0;
  std::vector<uint8_t> owned_;
};

std::string sha256_hex(const uint8_t* data, size_t n);

}  // namespace evej
