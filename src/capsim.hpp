// Event-driven capacitor simulator (behaviour-compatible with Pyfa eos/capSim.py, LGPL; port of eve-dogma-rs capsim.rs).
#pragma once
#include <cstdint>
#include <vector>

namespace evej {
struct Drain {
  double duration, cap_need;
  uint32_t clip_size;
  double reload_ms;
  bool is_injector, disable_stagger;
};
struct CapResult {
  bool stable;
  double stable_low, stable_high, t_s, eve_stable;
  uint64_t iterations;
};
// testing hook: force the general (3-key) event layout instead of the compact one
extern bool g_capsim_force_general;
CapResult simulate(double capacity, double recharge_ms, const std::vector<Drain>& drains, double start_frac, bool reload,
                   bool stagger, double t_max_ms);
}  // namespace evej
