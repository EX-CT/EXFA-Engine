// Data-driven dogma engine: object graph per request, flat modifier lists, lazy memoised evaluation.
#pragma once
#include <cstdint>
#include <memory>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "dataset.hpp"
#include "request.hpp"

namespace evej {

enum class Kind : uint8_t { Ship, Char, Skill, Module, Charge, Drone, Fighter, Implant, Booster, Mode, Beacon, Projected };
enum class Loc : uint8_t { Ship, Char, Space, Nowhere };

// Attribute / effect ids resolved once per dataset.
struct Ids {
  explicit Ids(const Dataset& ds);
  // engine
  uint32_t pilotSecurityStatus, fighterSquadronMaxSize, hiSecModifier, lowSecModifier, nullSecModifier, securityModifier;
  uint32_t massAddition, speedFactor, speedBoostFactor, maxVelocity, signatureRadiusBonus, signatureRadius,
      signatureRadiusBonusPercent, hiSlots, medSlots, lowSlots, hiSlotModifier, medSlotModifier, lowSlotModifier,
      turretSlotsLeft, launcherSlotsLeft, turretHardPointModifier, launcherHardPointModifier, remoteResistanceID,
      maxTargetRange, maxTargetRangeBonus, scanResolution, scanResolutionBonus, resistanceShiftAmount;
  uint32_t warfareBuffID[4], warfareBuffValue[4], armorRes[4];
  uint32_t e_ab, e_mwd, e_slot, e_hp, e_mjd, e_bastion, e_rah, structure_ok[5];
  // stats
  uint32_t cpu, power, cpuOutput, powerOutput, upgradeCost, upgradeCapacity, speed, duration, capacitorNeed, reloadTime,
      moduleReactivationDelay, chargeRate, damageMultiplier, dmg[4], cycle_extra[5], crystalsGetDamaged,
      crystalVolatilityChance, crystalVolatilityDamage, missileDamageMultiplier, droneBandwidthUsed, droneBandwidth,
      droneCapacity, fighterCapacity, rigSlots, maxSubSystems, serviceSlots, fighterSquadronIsHeavy,
      fighterSquadronIsSupport, fighterTubes, fighterLightSlots, fighterSupportSlots, fighterHeavySlots,
      damageMultiplierBonusMax, damageMultiplierBonusPerCycle, maxRange, falloff, trackingSpeed, explosionDelay,
      aoeCloudSize, aoeVelocity, empFieldRange, fam[6], fmi[6], shieldRes[4], hullRes[4], shieldCapacity, armorHP,
      shieldBonus, armorDamageAmount, structureDamageAmount, shieldRechargeRate, capacitorCapacity, rechargeRate,
      capacitorBonus, powerTransferAmount, speedLimit, agility, baseWarpSpeed, warpSpeedMultiplier, warpCapacitorNeed,
      warpScrambleStatus, scanStrength[4], maxLockedTargets, maxActiveDrones, droneControlDistance, rigSize,
      canFitShipGroup[20], canFitShipType[11], maxGroupFitted, maxTypeFitted, maxGroupOnline, maxGroupActive,
      chargeGroup[5], chargeSize, requiredSkill[6], requiredSkillLevel[6];
  uint32_t n_cfg = 0, n_cft = 0;
  uint32_t e_turret, e_launcher, e_empwave, e_chain, e_shieldBoosting, e_fueledShieldBoosting, e_armorRepair,
      e_fueledArmorRepair, e_structureRepair, e_nos, e_fam, e_fmi;
  uint32_t g_cap_booster_group_ok;  // unused placeholder
  // projected specials / missile range
  uint32_t falloffEffectiveness, disallowAssistance, energyNeutralizerAmount, energyNeutralizerDuration,
      energyNeutralizerRangeOptimal, energyNeutralizerSignatureResolution, radius, mass, agilityA, maxFOFTargetRange,
      scanStrengthPercent[4], scanStrengthG[4];
  uint32_t e_fof;
};

struct Src {
  enum K : uint8_t { Attr, Const, Prop, Proj } k = Const;
  uint8_t mul = 0;
  uint32_t a = 0, b = 0, c = 0, d = 0, e = 0;
  double f = 0;
};

struct AMod {
  uint32_t next;
  int8_t op;
  uint8_t pen;
  Src src;
};

struct LAttr {
  double base, val;
  uint32_t head, tail;
  uint32_t item, attr;
  uint8_t st;  // 0 = not evaluated, 1 = busy, 2 = cached
};

struct Item {
  const TypeRec* t;
  uint32_t type_id, group, category;
  Kind kind;
  State state = State::Online;
  Loc loc;
  bool owned;
  Slot slot = Slot::None;
  int32_t parent = -1, charge = -1, req_index = -1;
  uint32_t quantity = 1, active_count = 0;
  uint32_t req_skills[6];
  uint32_t n_req = 0;
  std::span<const TEff> effs;
  const std::vector<uint32_t>* fighter_abilities = nullptr;
  const std::vector<uint32_t>* booster_side_effects = nullptr;
  const Spool* spool = nullptr;
  bool has_distance = false;
  double distance = 0;
  bool has_effect(uint32_t e) const {
    for (auto& x : effs)
      if (x.id == e) return true;
    return false;
  }
  bool needs_skill(uint32_t s) const {
    for (uint32_t i = 0; i < n_req; i++)
      if (req_skills[i] == s) return true;
    return false;
  }
};

// A projected effect that feeds tank or capacitor stats (Pyfa: fit._armorRr, addDrain).
struct ProjSpecial {
  bool rep;  // true: remote repair, false: capacitor drain/fill
  uint8_t layer;
  uint32_t item, amount, duration, resist;
  double mult, factor, sign;
};

struct EngineError {
  const char* code;
  std::string message, path;
};

class Fit {
 public:
  Fit(const Dataset& ds, const Ids& ids) : ds(ds), K(ids) {}
  // Build the graph and register all modifiers. Returns false on error (err filled).
  bool build(const FitRequest& req, EngineError& err, bool no_projected = false, bool no_boosters = false);

  double get(uint32_t item, uint32_t attr);
  bool get_opt(uint32_t item, uint32_t attr, double& out);
  bool has(uint32_t item, uint32_t attr) const;
  double base(uint32_t item, uint32_t attr) const;
  // all attribute ids present on an item (sorted)
  std::vector<uint32_t> attr_keys(uint32_t item) const;

  const Dataset& ds;
  const Ids& K;
  std::vector<Item> items;
  uint32_t ship = 0, chr = 0;
  bool is_structure = false;
  std::vector<std::string> warnings;
  std::vector<std::pair<uint32_t, uint8_t>> skill_levels;  // (skill id, level) of skill items
  std::vector<ProjSpecial> proj_special;

 private:
  int32_t new_item(uint32_t type_id, Kind kind, Loc loc, const char* path_fmt, long idx, EngineError& err);
  void apply_mutation(uint32_t idx, const Mutation& m);
  bool add_module(uint32_t i, const ModuleReq& m, EngineError& err);
  void register_all(const FitRequest& req, bool no_boosters);
  void register_projected(uint32_t i);
  bool proj_special_for(uint32_t i, std::string_view name, uint32_t resist, std::vector<ProjSpecial>& out);
  void register_buffs(const FitRequest& req, bool no_boosters);
  void apply_buff(uint32_t id, const Src& src, uint32_t source_item);
  void apply_rah(const FitRequest& req);
  void push_mod(uint32_t target, uint32_t attr, int op, const Src& src, uint32_t source_cat);
  template <class F>
  void for_targets(uint32_t src, int func, int domain, uint32_t extra, F&& f);
  State effective_state(uint32_t i) const;
  void clear_cache();

  // fit-local attribute table: open addressing (item,attr) -> LAttr index
  int32_t find(uint32_t item, uint32_t attr) const;
  uint32_t ensure(uint32_t item, uint32_t attr);           // creates with type base / default
  void set_base(uint32_t item, uint32_t attr, double v);   // replace (like HashMap::insert(Attr::new(v)))
  bool type_base(uint32_t item, uint32_t attr, double& v) const;
  double eval(uint32_t idx);
  double src_value(const Src& s);
  double caps(uint32_t item, const AttrRec* info, double v);
  void grow();

  std::vector<uint64_t> hkeys_;
  std::vector<uint32_t> hvals_;
  uint64_t hmask_ = 0;
  uint32_t hcount_ = 0;
  std::vector<LAttr> la_;
  std::vector<AMod> mods_;
  std::vector<std::unique_ptr<std::vector<TEff>>> eff_store_;
  std::vector<std::unique_ptr<std::vector<uint32_t>>> list_store_;
  std::vector<std::unique_ptr<Spool>> spool_store_;
  std::vector<uint32_t> ship_loc_, char_loc_, owned_, char_skill_tgt_;
};

}  // namespace evej
