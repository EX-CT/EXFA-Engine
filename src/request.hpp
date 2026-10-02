// FitRequest v1 (contract.md) parsed with simdjson.
#pragma once
#include <cstdint>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

namespace simdjson::dom { class element; }

namespace evej {

enum class State : uint8_t { Offline = 0, Online = 1, Active = 2, Overheated = 3 };
enum class Slot : int8_t { None = -1, High = 0, Mid, Low, Rig, Subsystem, Service };
enum class SpoolType : uint8_t { SpoolScale, CycleScale, Time, Cycles };

struct Spool {
  SpoolType kind = SpoolType::SpoolScale;
  double amount = 1.0;
};
struct Mutation {
  uint32_t base_type_id = 0;
  std::optional<uint32_t> mutaplasmid_type_id;
  std::vector<std::pair<uint32_t, double>> attributes;  // parsed keys (BTreeMap<String,_> order)
};
struct ModuleReq {
  uint32_t type_id = 0;
  Slot slot = Slot::None;
  std::optional<State> state;
  std::optional<uint32_t> charge_type_id;
  std::optional<Mutation> mutation;
  std::optional<Spool> spool;
};
struct DroneReq {
  uint32_t type_id = 0, quantity = 1;
  std::optional<uint32_t> active;
  std::optional<Mutation> mutation;
};
struct FighterReq {
  uint32_t type_id = 0;
  std::optional<uint32_t> quantity;
  bool active = true;
  std::optional<std::vector<uint32_t>> abilities;
};
struct BoosterReq {
  uint32_t type_id = 0;
  std::vector<uint32_t> side_effects;
};
struct CargoReq {
  uint32_t type_id = 0, quantity = 1;
};
struct Buff {
  uint32_t buff_id = 0;
  double value = 0;
};
struct FitRequest;
struct Projected {
  std::string kind;
  std::optional<ModuleReq> module;
  std::optional<DroneReq> drone;
  std::shared_ptr<FitRequest> fit;
  uint32_t amount = 1;
  std::optional<double> distance_m;
};
struct Resists {
  double em = 0, thermal = 0, kinetic = 0, explosive = 0;
};
struct TargetProfile {
  double em = 0, thermal = 0, kinetic = 0, explosive = 0;
  std::optional<double> signature_radius, max_velocity, radius;
};
struct Override {
  uint32_t type_id = 0, attribute_id = 0;
  double value = 0;
};
struct FitRequest {
  uint32_t ship_type_id = 0;
  std::optional<uint32_t> mode_type_id;
  std::optional<uint8_t> default_level;
  std::vector<std::pair<std::string, uint8_t>> skill_levels;  // sorted by key (string order)
  std::optional<double> security_status;
  std::vector<ModuleReq> modules;
  std::vector<DroneReq> drones;
  std::vector<FighterReq> fighters;
  std::vector<uint32_t> implants;
  std::vector<BoosterReq> boosters;
  std::vector<CargoReq> cargo;
  std::vector<Buff> buffs;
  std::vector<FitRequest> booster_fits;
  std::vector<Projected> projected;
  std::vector<uint32_t> env_effects;
  std::optional<std::string> system_security;
  std::optional<Resists> damage_pattern;
  std::optional<TargetProfile> target_profile;
  std::vector<Override> overrides;
  // options
  bool nos_no_target_cap = false, factor_reload = false, sources = false, validate = true;
  std::optional<Spool> default_spool;
  std::optional<std::string> rah, include_attributes;
  bool cs_reload = false, cs_stagger = false;
  std::optional<double> cs_max_time_s;
};

// Returns empty string on success, else an error message (BAD_REQUEST).
std::string parse_request(const simdjson::dom::element& root, FitRequest& out);

}  // namespace evej
