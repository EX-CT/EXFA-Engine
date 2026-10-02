// Randomised check: JW::num fast path == generic shortest-repr path for round6'd values.
#include <cstdio>
#include <random>
#include "../src/jsonw.hpp"
int main() {
  std::mt19937_64 rng(42);
  std::uniform_real_distribution<double> ex(-8, 10), u(0, 1);
  long bad = 0, n = 0;
  for (long i = 0; i < 5000000; i++) {
    double v = std::pow(10.0, ex(rng)) * (u(rng) < 0.5 ? -1 : 1);
    if (i % 3 == 0) v = std::round(v * 1000) / 1000;
    if (i % 7 == 0) v = (double)(long)v;
    evej::JW a, b;
    a.num(v);
    b.num_raw(evej::round6(v));
    n++;
    if (a.s != b.s && bad++ < 10) printf("%.17g: %s vs %s\n", v, a.s.c_str(), b.s.c_str());
  }
  printf("checked %ld, mismatches %ld\n", n, bad);
  return bad != 0;
}
