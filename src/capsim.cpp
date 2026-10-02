#include "capsim.hpp"

#include <algorithm>
#include <charconv>
#include <cmath>
#include <numeric>
#include <cstring>

namespace evej {
namespace {
// A capacitor source: the static part of Pyfa's heapq entry [t, duration, capNeed, shot, clipSize, reloadTime,
// isInjector] (+ insertion order). As in eve-dogma-rs (since 60de0b9) the static fields are replaced by their ranks
// (r1 = (duration, capNeed), r2 = (clip, reload, inj)), which gives exactly the same order, and the order is packed
// into integer keys: k0 = bits of t (t >= 0 so the IEEE pattern orders like the value), then (r1, shot, r2, seq).
struct Source {
  double duration, cap_need;
  uint32_t clip;
  double reload;
  bool inj;
};
// general layout: k1 = r1 << 32 | shot, k2 = r2 << 40 | seq
struct EvG {
  uint64_t k0, k1, k2;
  uint32_t src;
  static EvG make(double t, uint32_t r1, uint32_t shot, uint32_t r2, uint32_t src, uint64_t seq) {
    return EvG{tbits(t), (uint64_t)r1 << 32 | shot, (uint64_t)r2 << 40 | seq, src};
  }
  static uint64_t tbits(double t) {
    t += 0.0;
    uint64_t u;
    memcpy(&u, &t, 8);
    return u;
  }
  double t() const {
    double d;
    memcpy(&d, &k0, 8);
    return d;
  }
  uint32_t shot() const { return (uint32_t)k1; }
  void reschedule(double t, uint32_t shot, uint64_t seq) {
    k0 = tbits(t);
    k1 = (k1 & 0xffffffff00000000ull) | shot;
    k2 = (k2 & ~((1ull << 40) - 1)) | seq;
  }
  bool lt(const EvG& o) const {
    if (k0 != o.k0) return k0 < o.k0;
    if (k1 != o.k1) return k1 < o.k1;
    return k2 < o.k2;
  }
};
// compact layout for <= 256 sources (shots and sequence numbers stay < 2^24 within the 5 M iteration limit):
// k0 = bits of t, k1 = r1 << 56 | shot << 32 | r2 << 24 | seq
struct EvC {
  uint64_t k0, k1;
  uint32_t src;
  static EvC make(double t, uint32_t r1, uint32_t shot, uint32_t r2, uint32_t src, uint64_t seq) {
    return EvC{EvG::tbits(t), (uint64_t)r1 << 56 | (uint64_t)shot << 32 | (uint64_t)r2 << 24 | seq, src};
  }
  double t() const {
    double d;
    memcpy(&d, &k0, 8);
    return d;
  }
  uint32_t shot() const { return (uint32_t)(k1 >> 32) & 0xffffffu; }
  void reschedule(double t, uint32_t shot, uint64_t seq) {
    k0 = EvG::tbits(t);
    k1 = (k1 & 0xff000000ff000000ull) | (uint64_t)shot << 32 | seq;
  }
  bool lt(const EvC& o) const { return k0 != o.k0 ? k0 < o.k0 : k1 < o.k1; }
};
// binary min-heap with Rust BinaryHeap's sift rules (classic sift-up on push, sift-down on replace/pop), so the
// final element layout (used by the avg-drain sum) is identical to the reference
template <class E>
struct Heap {
  std::vector<E> v;
  void push(E e) {
    size_t i = v.size();
    v.push_back(e);
    while (i > 0) {
      size_t p = (i - 1) / 2;
      if (!e.lt(v[p])) break;
      v[i] = v[p];
      i = p;
    }
    v[i] = e;
  }
  void sift_down(size_t i, E e) {
    const size_t n = v.size();
    while (true) {
      size_t l = 2 * i + 1;
      if (l >= n) break;
      size_t m = (l + 1 < n && v[l + 1].lt(v[l])) ? l + 1 : l;
      if (!v[m].lt(e)) break;
      v[i] = v[m];
      i = m;
    }
    v[i] = e;
  }
  void replace_top(E e) { sift_down(0, e); }
  void pop() {
    E last = v.back();
    v.pop_back();
    if (!v.empty()) sift_down(0, last);
  }
};
// memo of exp(dt / tau) for recurring time steps (exact: same input bits -> same result)
struct ExpMemo {
  uint64_t k[256];
  double r[256];
  bool used[256] = {};
  double get(double a) {
    uint64_t b;
    memcpy(&b, &a, 8);
    unsigned h = (unsigned)(((b * 0x9E3779B97F4A7C15ull) >> 56) & 255);
    if (used[h] && k[h] == b) return r[h];
    double e = std::exp(a);
    used[h] = true;
    k[h] = b;
    r[h] = e;
    return e;
  }
};
// Python round(v, 1) (eve-dogma-rs py_round1): correctly rounded, exact ties to even. Fast exact path as
// py_round2 in engine.cpp (nearest integer to the exact product v*10 unless within a hair of a tie).
double py_round1(double v) {
  if (!std::isfinite(v)) return v;
  if (std::fabs(v) < 1e12) {
    const double t = v * 10.0;
    const double e = std::fma(v, 10.0, -t);
    const double n = std::nearbyint(t);
    const double d = (t - n) + e;
    if (std::fabs(d) < 0.4999999) return n / 10.0;
  }
  char buf[400];
  auto r = std::to_chars(buf, buf + sizeof buf, v, std::chars_format::fixed, 1);
  if (r.ec != std::errc()) return v;
  double o = v;
  std::from_chars(buf, r.ptr, o);
  return o;
}
uint64_t gcd(uint64_t a, uint64_t b) { return b == 0 ? a : gcd(b, a % b); }

using Key = std::vector<std::pair<uint64_t, uint64_t>>;
inline uint64_t bits(double d) {
  uint64_t u;
  memcpy(&u, &d, 8);
  return u;
}
}  // namespace

namespace {
template <class E>
CapResult run(const std::vector<Source>& sources, const std::vector<std::pair<uint32_t, double>>& initial,
              const std::vector<uint32_t>& r1, const std::vector<uint32_t>& r2, double periodf, double capacity,
              double start_frac, double tau, double t_max_ms) {
  Heap<E> heap;
  heap.v.reserve(initial.size() + 4);
  uint64_t seq = 0;
  for (auto& [si, t] : initial) heap.push(E::make(t, r1[si], 0, r2[si], si, seq++));
  ExpMemo em;
  const double cap_max = capacity;
  double cap = capacity * start_frac;
  double cap_wrap = cap, cap_lowest = cap, cap_lowest_pre = cap;
  double t_wrap = periodf, t_last = 0.0;
  uint64_t iterations = 0;
  std::vector<E> awaiting;
  Key awaiting_wrap;
  bool ran_out = false;
  auto key = [&](const std::vector<E>& v) {
    Key k;
    k.reserve(v.size());
    for (auto& e : v) k.push_back({bits(sources[e.src].duration), bits(sources[e.src].cap_need)});
    std::sort(k.begin(), k.end());
    return k;
  };
  bool has_last = false;
  E last_ev{};
  auto refire = [&](E inj, double t_now) {
    const Source& is = sources[inj.src];
    double nt = t_now + is.duration;
    uint32_t shot = inj.shot() + 1;
    if (is.clip > 0 && shot % is.clip == 0) {
      shot = 0;
      nt += is.reload;
    }
    inj.reschedule(nt, shot, seq++);
    heap.push(inj);
  };
  auto cn = [&](const E& e) { return sources[e.src].cap_need; };
  // The pop order depends only on the set of entries (the order is total: seq is unique), so the current event
  // stays in the heap and is updated in place (one sift-down) unless something else must be pushed first or it
  // leaves the simulation (as the reference does; this also fixes the heap layout the avg-drain sum iterates).
  while (!heap.v.empty()) {
    E ev = heap.v[0];
    bool in_heap = true;
    auto take = [&]() {
      if (in_heap) {
        heap.pop();
        in_heap = false;
      }
    };
    const Source& sv = sources[ev.src];
    const double t_now = ev.t();
    if (t_now >= t_max_ms) {
      take();
      last_ev = ev;
      has_last = true;
      break;
    }
    if (t_now > t_last && cap_max > 0.0 && tau > 0.0) {
      double x = std::sqrt(std::max(cap / cap_max, 0.0));
      double y = 1.0 + (x - 1.0) * em.get((t_last - t_now) / tau);
      cap = y * y * cap_max;
    }
    if (t_now != t_last) {
      if (cap < cap_lowest_pre) cap_lowest_pre = cap;
      if (t_now == t_wrap) {
        Key k = key(awaiting);
        if (cap >= cap_wrap && k == awaiting_wrap) {
          take();
          last_ev = ev;
          has_last = true;
          break;
        }
        cap_wrap = py_round1(cap);  // Python round(cap, 1)
        awaiting_wrap = std::move(k);
        t_wrap += periodf;
      }
    }
    t_last = t_now;
    iterations++;
    if (iterations > 5000000) {
      take();
      last_ev = ev;
      has_last = true;
      break;
    }
    if (sv.inj && cap - sv.cap_need > cap_max) {
      take();
      awaiting.push_back(ev);
      continue;
    }
    if (sv.cap_need > cap && cap < cap_max) {
      while (!awaiting.empty() && sv.cap_need > cap && cap_max > cap) {
        double need = std::min(sv.cap_need - cap, cap_max - cap);
        // smallest injection that covers the need (first minimum), else the largest (last maximum)
        long pick = -1;
        for (size_t i = 0; i < awaiting.size(); i++)
          if (-cn(awaiting[i]) >= need && (pick < 0 || -cn(awaiting[i]) < -cn(awaiting[pick]))) pick = (long)i;
        if (pick < 0) {
          pick = 0;
          for (size_t i = 1; i < awaiting.size(); i++)
            if (-cn(awaiting[i]) >= -cn(awaiting[pick])) pick = (long)i;
        }
        take();
        E inj = awaiting[pick];
        awaiting.erase(awaiting.begin() + pick);
        cap = std::min(cap - cn(inj), cap_max);
        refire(inj, t_now);
      }
    }
    cap = std::min(cap - sv.cap_need, cap_max);
    if (cap < cap_lowest) {
      if (cap < 0.0) {
        take();
        ran_out = true;
        last_ev = ev;
        has_last = true;
        break;
      }
      cap_lowest = cap;
    }
    while (!awaiting.empty() && cap < cap_max) {
      double need = cap_max - cap;
      long pick = -1;
      for (size_t i = 0; i < awaiting.size(); i++)
        if (-cn(awaiting[i]) <= need && (pick < 0 || -cn(awaiting[i]) >= -cn(awaiting[pick]))) pick = (long)i;
      if (pick < 0) break;
      take();
      E inj = awaiting[pick];
      awaiting.erase(awaiting.begin() + pick);
      cap = std::min(cap - cn(inj), cap_max);
      refire(inj, t_now);
    }
    double nt = t_now + sv.duration;
    uint32_t shot = ev.shot() + 1;
    if (sv.clip > 0 && shot % sv.clip == 0) {
      shot = 0;
      nt += sv.reload;
    }
    ev.reschedule(nt, shot, seq++);
    if (in_heap) heap.replace_top(ev);
    else heap.push(ev);
  }
  double avg_drain = -0.0;
  for (auto& e : heap.v) avg_drain += sources[e.src].cap_need / sources[e.src].duration;
  if (has_last) avg_drain += sources[last_ev.src].cap_need / sources[last_ev.src].duration;
  double inner = -(2.0 * avg_drain * tau - cap_max) / cap_max;
  double eve_stable = 0.0;
  if (inner >= 0.0 && cap_max > 0.0) {
    double q = 1.0 + std::sqrt(inner);
    eve_stable = 0.25 * q * q;
  }
  CapResult r;
  r.stable = !ran_out;
  r.stable_low = r.stable && cap_max > 0.0 ? cap_lowest / cap_max : 0.0;
  r.stable_high = r.stable && cap_max > 0.0 ? cap_lowest_pre / cap_max : 0.0;
  r.t_s = t_last / 1000.0;
  r.eve_stable = eve_stable;
  r.iterations = iterations;
  return r;
}
// partial_cmp(..).unwrap_or(Equal)
inline int fcmp(double a, double b) { return a < b ? -1 : (a > b ? 1 : 0); }
}  // namespace

bool g_capsim_force_general = false;

CapResult simulate(double capacity, double recharge_ms, const std::vector<Drain>& drains, double start_frac, bool reload,
                   bool stagger, double t_max_ms) {
  const double tau = recharge_ms / 5.0;
  std::vector<Source> sources;
  std::vector<std::pair<uint32_t, double>> initial;  // (source, initial t) in insertion order
  sources.reserve(drains.size() + 1);
  initial.reserve(drains.size() + 1);
  uint64_t period = 1;
  bool disable_period = false;
  std::vector<std::pair<Drain, uint32_t>> groups;
  groups.reserve(drains.size());
  for (Drain d : drains) {
    if (!reload && !d.is_injector) {
      d.clip_size = 0;
      d.reload_ms = 0.0;
    }
    if (d.duration <= 0.0) continue;
    bool found = false;
    for (auto& g : groups) {
      const Drain& x = g.first;
      if (x.duration == d.duration && x.cap_need == d.cap_need && x.clip_size == d.clip_size && x.reload_ms == d.reload_ms &&
          x.is_injector == d.is_injector && x.disable_stagger == d.disable_stagger) {
        g.second++;
        found = true;
        break;
      }
    }
    if (!found) groups.push_back({d, 1});
  }
  for (auto& [d0, n] : groups) {
    Drain d = d0;
    if (d.clip_size > 0) disable_period = true;
    if (d.is_injector) {
      sources.push_back(Source{d.duration, d.cap_need, d.clip_size, d.reload_ms, true});
      for (uint32_t k = 0; k < n; k++) initial.push_back({(uint32_t)sources.size() - 1, 0.0});
      continue;
    }
    if (stagger && !d.disable_stagger) {
      if (d.clip_size == 0) {
        d.duration = std::floor(d.duration / (double)n);
      } else {
        double st = (d.duration * d.clip_size + d.reload_ms) / ((double)n * d.clip_size);
        sources.push_back(Source{d.duration, d.cap_need, d.clip_size, d.reload_ms, false});
        for (uint32_t i = 1; i < n; i++) initial.push_back({(uint32_t)sources.size() - 1, i * st});
      }
    } else {
      d.cap_need *= (double)n;
    }
    uint64_t dur = (uint64_t)std::max(std::round(d.duration), 1.0);
    period = period / gcd(period, dur) * dur;
    sources.push_back(Source{d.duration, d.cap_need, d.clip_size, d.reload_ms, false});
    initial.push_back({(uint32_t)sources.size() - 1, 0.0});
  }
  // ranks of the static tie-break tuples (equal tuples share a rank)
  auto rank = [&](auto cmp) {
    std::vector<uint32_t> idx(sources.size());
    std::iota(idx.begin(), idx.end(), 0u);
    std::stable_sort(idx.begin(), idx.end(), [&](uint32_t a, uint32_t b) { return cmp(sources[a], sources[b]) < 0; });
    std::vector<uint32_t> r(sources.size());
    uint32_t cur = 0;
    for (size_t k = 0; k < idx.size(); k++) {
      if (k > 0 && cmp(sources[idx[k - 1]], sources[idx[k]]) != 0) cur++;
      r[idx[k]] = cur;
    }
    return r;
  };
  auto r1 = rank([](const Source& a, const Source& b) {
    int c = fcmp(a.duration, b.duration);
    return c ? c : fcmp(a.cap_need, b.cap_need);
  });
  auto r2 = rank([](const Source& a, const Source& b) {
    if (a.clip != b.clip) return a.clip < b.clip ? -1 : 1;
    int c = fcmp(a.reload, b.reload);
    if (c) return c;
    return (int)a.inj - (int)b.inj;
  });
  const double periodf = (disable_period || (double)period > t_max_ms) ? t_max_ms : (double)period;
  if (sources.size() <= 256 && !g_capsim_force_general)
    return run<EvC>(sources, initial, r1, r2, periodf, capacity, start_frac, tau, t_max_ms);
  return run<EvG>(sources, initial, r1, r2, periodf, capacity, start_frac, tau, t_max_ms);
}

}  // namespace evej
