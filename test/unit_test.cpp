// Unit tests for Variant J (no dataset needed). Usage: evej-unit <name> | all   (registered with CTest)
#include <cmath>
#include <cstdio>
#include <cstring>
#include <functional>
#include <random>
#include <string>
#include <vector>

#include "capsim.hpp"
#include "engine.hpp"
#include "jsonw.hpp"
#include "serdenum.hpp"
#include "stats.hpp"

namespace {
int fails = 0;
#define CHECK(c)                                                       \
  do {                                                                 \
    if (!(c)) {                                                        \
      std::printf("  CHECK failed %s:%d: %s\n", __FILE__, __LINE__, #c); \
      fails++;                                                         \
    }                                                                  \
  } while (0)

// JW::num fast path == generic shortest-repr path for round6'd values
void test_num_fast_path() {
  std::mt19937_64 rng(42);
  std::uniform_real_distribution<double> ex(-8, 10), u(0, 1);
  for (long i = 0; i < 300000; i++) {
    double v = std::pow(10.0, ex(rng)) * (u(rng) < 0.5 ? -1 : 1);
    if (i % 3 == 0) v = std::round(v * 1000) / 1000;
    if (i % 7 == 0) v = (double)(long)v;
    evej::JW a, b;
    a.num(v);
    b.num_raw(evej::round6(v));
    if (a.s != b.s) {
      std::printf("  %.17g: %s vs %s\n", v, a.s.c_str(), b.s.c_str());
      fails++;
      return;
    }
  }
}

// serde_json-compatible number formatting (ryu/zmij shortest repr, "e+NN" / "e-NN", ".0" on integral floats)
void test_num_format() {
  auto f = [](double v) {
    evej::JW w;
    w.num_raw(v);
    return w.s;
  };
  CHECK(f(0.0) == "0.0");
  CHECK(f(-0.0) == "-0.0");
  CHECK(f(1.5) == "1.5");
  CHECK(f(100.0) == "100.0");
  CHECK(f(1e16) == "1e+16");
  CHECK(f(1e-7) == "1e-7");
  CHECK(f(0.1) == "0.1");
}

// JSON number tokens are re-rounded the way serde_json (no float_roundtrip) parses them; strings untouched
void test_serde_numbers() {
  auto fix = [](const char* in) {
    std::string out;
    return evej::serde_fix_numbers(in, out) ? out : std::string(in);
  };
  CHECK(fix(R"({"a":0.1,"b":[1,2.5,-3e2]})") == R"({"a":0.1,"b":[1,2.5,-3e2]})");
  CHECK(fix(R"({"a":-0})") == R"({"a":-0.0})");
  CHECK(fix(R"({"a":"-0","b":"9007199254740993.0"})") == R"({"a":"-0","b":"9007199254740993.0"})");
  CHECK(fix(R"({"a":2.2250738585072011e-308})") == R"({"a":2.2250738585072014e-308})");
  CHECK(fix(R"({"a":9007199254740993.0})") == R"({"a":9007199254740994.0})");
  CHECK(fix(R"({"a":123456789012345678901234567890})") == R"({"a":1.2345678901234568e+29})");
}

// Rust's str::parse::<u32>
void test_parse_u32() {
  uint32_t v = 0;
  CHECK(evej::parse_u32_rust("0", v) && v == 0);
  CHECK(evej::parse_u32_rust("+5", v) && v == 5);
  CHECK(evej::parse_u32_rust("4294967295", v) && v == 4294967295u);
  CHECK(!evej::parse_u32_rust("4294967296", v));
  CHECK(!evej::parse_u32_rust("-1", v));
  CHECK(!evej::parse_u32_rust("", v));
  CHECK(!evej::parse_u32_rust(" 1", v));
  CHECK(!evej::parse_u32_rust("1.0", v));
}

bool same(const evej::CapResult& a, const evej::CapResult& b) {
  return a.stable == b.stable && std::memcmp(&a.stable_low, &b.stable_low, 8) == 0 &&
         std::memcmp(&a.stable_high, &b.stable_high, 8) == 0 && std::memcmp(&a.t_s, &b.t_s, 8) == 0 &&
         std::memcmp(&a.eve_stable, &b.eve_stable, 8) == 0 && a.iterations == b.iterations;
}

// capacitor simulator: compact and general event-key layouts give bit-identical results
void test_capsim_layouts() {
  std::mt19937_64 rng(7);
  std::uniform_int_distribution<int> nd(0, 9), clip(0, 3);
  std::uniform_real_distribution<double> dur(1000, 12000), need(1, 300), u(0, 1);
  for (int it = 0; it < 300; it++) {
    std::vector<evej::Drain> ds;
    int n = nd(rng);
    for (int k = 0; k < n; k++) {
      evej::Drain d{};
      d.duration = std::round(dur(rng));
      d.cap_need = std::round(need(rng));
      d.is_injector = u(rng) < 0.15;
      if (d.is_injector) {
        d.cap_need = -d.cap_need * 3;
        d.clip_size = 1 + clip(rng);
        d.reload_ms = 10000;
      } else if (u(rng) < 0.2) {
        d.clip_size = 1 + clip(rng);
        d.reload_ms = 5000;
      }
      d.disable_stagger = u(rng) < 0.1;
      ds.push_back(d);
      if (u(rng) < 0.4) ds.push_back(d);  // identical modules are grouped / staggered
    }
    double cap = 500 + 3000 * u(rng), rech = 100000 + 300000 * u(rng);
    bool reload = u(rng) < 0.5;
    evej::g_capsim_force_general = false;
    auto a = evej::simulate(cap, rech, ds, 1.0, reload, true, 6.0 * 3600 * 1000);
    evej::g_capsim_force_general = true;
    auto b = evej::simulate(cap, rech, ds, 1.0, reload, true, 6.0 * 3600 * 1000);
    evej::g_capsim_force_general = false;
    if (!same(a, b)) {
      std::printf("  layout mismatch at iteration %d\n", it);
      fails++;
      return;
    }
  }
}

// capacitor simulator: basic behaviour
void test_capsim_basic() {
  auto none = evej::simulate(1000, 250000, {}, 1.0, false, true, 6.0 * 3600 * 1000);
  CHECK(none.stable && none.stable_low == 1.0);
  evej::Drain heavy{1000, 500, 0, 0, false, false};
  auto out = evej::simulate(1000, 250000, {heavy}, 1.0, false, true, 6.0 * 3600 * 1000);
  CHECK(!out.stable && out.t_s > 0 && out.t_s < 10);
  evej::Drain light{10000, 10, 0, 0, false, false};
  auto st = evej::simulate(1000, 250000, {light}, 1.0, false, true, 6.0 * 3600 * 1000);
  CHECK(st.stable && st.stable_low > 0.9 && st.stable_low < 1.0);
}

// range factor: inside optimal -> 1, one falloff out -> 0.5
void test_range_factor() {
  CHECK(evej::range_factor(10000, 5000, 5000.0, false) == 1.0);
  CHECK(std::fabs(evej::range_factor(10000, 5000, 15000.0, false) - 0.5) < 1e-12);
  CHECK(evej::range_factor(10000, 5000, std::nullopt, false) == 1.0);
}

struct T {
  const char* name;
  void (*fn)();
};
const T tests[] = {{"num_fast_path", test_num_fast_path}, {"num_format", test_num_format},
                   {"serde_numbers", test_serde_numbers}, {"parse_u32", test_parse_u32},
                   {"capsim_layouts", test_capsim_layouts}, {"capsim_basic", test_capsim_basic},
                   {"range_factor", test_range_factor}};
}  // namespace

int main(int argc, char** argv) {
  std::string want = argc > 1 ? argv[1] : "all";
  int run = 0, failed = 0;
  for (const T& t : tests) {
    if (want != "all" && want != t.name) continue;
    int before = fails;
    t.fn();
    run++;
    bool ok = fails == before;
    failed += !ok;
    std::printf("%s %s\n", ok ? "PASS" : "FAIL", t.name);
  }
  if (run == 0) {
    std::printf("unknown test %s\n", want.c_str());
    return 2;
  }
  std::printf("%d passed, %d failed\n", run - failed, failed);
  return failed ? 1 : 0;
}
