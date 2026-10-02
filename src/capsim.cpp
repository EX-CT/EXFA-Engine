#include "capsim.hpp"

#include <algorithm>
#include <cmath>
#include <numeric>
#include <cstring>

namespace evej {
namespace {
struct Ev {
  double t, duration, cap_need;
  uint32_t shot, clip;
  double reload;
  bool inj;
  uint64_t seq;
};
// Python-list ordering of [t, duration, capNeed, shot, clipSize, reloadTime, isInjector], then insertion order
inline bool ev_less(const Ev& a, const Ev& b) {
  if (a.t != b.t) return a.t < b.t;
  if (a.duration != b.duration) return a.duration < b.duration;
  if (a.cap_need != b.cap_need) return a.cap_need < b.cap_need;
  if (a.shot != b.shot) return a.shot < b.shot;
  if (a.clip != b.clip) return a.clip < b.clip;
  if (a.reload != b.reload) return a.reload < b.reload;
  if (a.inj != b.inj) return a.inj < b.inj;
  return a.seq < b.seq;
}
// binary min-heap over indices into a slot array. Same sift rules as Rust's BinaryHeap (classic sift-up on
// push; pop's sift-down-to-bottom + sift-up gives the same layout as classic sift-down for a total order), so
// the final element order (used by the avg-drain sum) is identical to the reference.
struct Heap {
  struct HE {
    double t;
    uint32_t id;
  };
  std::vector<Ev> slots;
  std::vector<uint32_t> free_;
  std::vector<HE> v;  // heap entries carry t inline; ties fall back to the full ordering
  bool less(const HE& a, const HE& b) const {
    if (a.t != b.t) return a.t < b.t;
    return ev_less(slots[a.id], slots[b.id]);
  }
  void push(const Ev& e) {
    uint32_t id;
    if (!free_.empty()) {
      id = free_.back();
      free_.pop_back();
      slots[id] = e;
    } else {
      id = (uint32_t)slots.size();
      slots.push_back(e);
    }
    HE h{e.t, id};
    size_t i = v.size();
    v.push_back(h);
    while (i > 0) {
      size_t p = (i - 1) / 2;
      if (!less(h, v[p])) break;
      v[i] = v[p];
      i = p;
    }
    v[i] = h;
  }
  bool pop(Ev& out) {
    if (v.empty()) return false;
    uint32_t top = v[0].id;
    out = slots[top];
    free_.push_back(top);
    HE last = v.back();
    v.pop_back();
    size_t n = v.size();
    if (n == 0) return true;
    size_t i = 0;
    while (true) {
      size_t l = 2 * i + 1, r = l + 1, m;
      if (l >= n) break;
      m = (r < n && less(v[r], v[l])) ? r : l;
      if (!less(v[m], last)) break;
      v[i] = v[m];
      i = m;
    }
    v[i] = last;
    return true;
  }
  std::vector<Ev> into_vec() const {
    std::vector<Ev> o;
    o.reserve(v.size() + 1);
    for (auto& h : v) o.push_back(slots[h.id]);
    return o;
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
uint64_t gcd(uint64_t a, uint64_t b) { return b == 0 ? a : gcd(b, a % b); }

using Key = std::vector<std::pair<uint64_t, uint64_t>>;
inline uint64_t bits(double d) {
  uint64_t u;
  memcpy(&u, &d, 8);
  return u;
}
}  // namespace

CapResult simulate(double capacity, double recharge_ms, const std::vector<Drain>& drains, double start_frac, bool reload,
                   bool stagger, double t_max_ms) {
  const double tau = recharge_ms / 5.0;
  Heap heap;
  heap.slots.reserve(64);
  heap.v.reserve(64);
  heap.free_.reserve(64);
  ExpMemo em;
  uint64_t seq = 0, period = 1;
  bool disable_period = false;
  std::vector<std::pair<Drain, uint32_t>> groups;
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
      for (uint32_t k = 0; k < n; k++) heap.push(Ev{0.0, d.duration, d.cap_need, 0, d.clip_size, d.reload_ms, true, seq++});
      continue;
    }
    if (stagger && !d.disable_stagger) {
      if (d.clip_size == 0) {
        d.duration = std::floor(d.duration / (double)n);
      } else {
        double st = (d.duration * d.clip_size + d.reload_ms) / ((double)n * d.clip_size);
        for (uint32_t i = 1; i < n; i++) heap.push(Ev{i * st, d.duration, d.cap_need, 0, d.clip_size, d.reload_ms, false, seq++});
      }
    } else {
      d.cap_need *= (double)n;
    }
    uint64_t dur = (uint64_t)std::max(std::round(d.duration), 1.0);
    period = period / gcd(period, dur) * dur;
    heap.push(Ev{0.0, d.duration, d.cap_need, 0, d.clip_size, d.reload_ms, false, seq++});
  }
  const double periodf = (disable_period || (double)period > t_max_ms) ? t_max_ms : (double)period;

  const double cap_max = capacity;
  double cap = capacity * start_frac;
  double cap_wrap = cap, cap_lowest = cap, cap_lowest_pre = cap;
  double t_wrap = periodf, t_last = 0.0;
  uint64_t iterations = 0;
  std::vector<Ev> awaiting;
  Key awaiting_wrap;
  bool ran_out = false;
  auto key = [](const std::vector<Ev>& v) {
    Key k;
    k.reserve(v.size());
    for (auto& e : v) k.push_back({bits(e.duration), bits(e.cap_need)});
    std::sort(k.begin(), k.end());
    return k;
  };
  bool has_last = false;
  Ev last_ev{};
  auto refire = [&](Ev& inj, double t_now) {
    inj.t = t_now + inj.duration;
    inj.shot += 1;
    if (inj.clip > 0 && inj.shot % inj.clip == 0) {
      inj.shot = 0;
      inj.t += inj.reload;
    }
    inj.seq = seq++;
    heap.push(inj);
  };
  Ev ev;
  while (heap.pop(ev)) {
    const double t_now = ev.t;
    if (t_now >= t_max_ms) {
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
          last_ev = ev;
          has_last = true;
          break;
        }
        cap_wrap = std::round(cap * 10.0) / 10.0;
        awaiting_wrap = std::move(k);
        t_wrap += periodf;
      }
    }
    t_last = t_now;
    iterations++;
    if (iterations > 5000000) {
      last_ev = ev;
      has_last = true;
      break;
    }
    if (ev.inj && cap - ev.cap_need > cap_max) {
      awaiting.push_back(ev);
      continue;
    }
    if (ev.cap_need > cap && cap < cap_max) {
      while (!awaiting.empty() && ev.cap_need > cap && cap_max > cap) {
        double need = std::min(ev.cap_need - cap, cap_max - cap);
        // smallest injection that covers the need (first minimum), else the largest (last maximum)
        long pick = -1;
        for (size_t i = 0; i < awaiting.size(); i++)
          if (-awaiting[i].cap_need >= need && (pick < 0 || -awaiting[i].cap_need < -awaiting[pick].cap_need)) pick = (long)i;
        if (pick < 0) {
          pick = 0;
          for (size_t i = 1; i < awaiting.size(); i++)
            if (-awaiting[i].cap_need >= -awaiting[pick].cap_need) pick = (long)i;
        }
        Ev inj = awaiting[pick];
        awaiting.erase(awaiting.begin() + pick);
        cap = std::min(cap - inj.cap_need, cap_max);
        refire(inj, t_now);
      }
    }
    cap = std::min(cap - ev.cap_need, cap_max);
    if (cap < cap_lowest) {
      if (cap < 0.0) {
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
        if (-awaiting[i].cap_need <= need && (pick < 0 || -awaiting[i].cap_need >= -awaiting[pick].cap_need)) pick = (long)i;
      if (pick < 0) break;
      Ev inj = awaiting[pick];
      awaiting.erase(awaiting.begin() + pick);
      cap = std::min(cap - inj.cap_need, cap_max);
      refire(inj, t_now);
    }
    refire(ev, t_now);
  }
  std::vector<Ev> all = heap.into_vec();
  if (has_last) all.push_back(last_ev);
  double avg_drain = -0.0;
  for (auto& e : all) avg_drain += e.cap_need / e.duration;
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

}  // namespace evej
