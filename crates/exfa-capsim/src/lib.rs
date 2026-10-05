//! Event-driven capacitor simulator. Behaviour-compatible with Pyfa eos/capSim.py; taken from eve-dogma-rs (LGPL-3.0-or-later).
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
pub struct Drain {
    /// cycle duration in ms
    pub duration: f64,
    /// cap used per cycle (negative = cap injected)
    pub cap_need: f64,
    /// shots before reload (0 = infinite)
    pub clip_size: u32,
    pub reload_ms: f64,
    pub is_injector: bool,
    pub disable_stagger: bool,
}

#[derive(Debug, Clone)]
pub struct CapResult {
    pub stable: bool,
    /// lowest cap fraction reached while stable (0..1)
    pub stable_low: f64,
    pub stable_high: f64,
    /// time (s) at which the cap ran out (unstable) or simulation end
    pub t_s: f64,
    pub depletes_in_s: Option<f64>,
    pub eve_stable: f64,
    pub iterations: u64,
}

#[derive(Debug, Clone, Copy)]
struct Ev {
    t: f64,
    duration: f64,
    cap_need: f64,
    shot: u32,
    clip: u32,
    reload: f64,
    inj: bool,
    seq: u64,
}
impl PartialEq for Ev {
    fn eq(&self, o: &Self) -> bool {
        self.cmp(o) == Ordering::Equal
    }
}
impl Eq for Ev {}
impl PartialOrd for Ev {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Ev {
    fn cmp(&self, o: &Self) -> Ordering {
        // min-heap with Python-list ordering like Pyfa's heapq of
        // [t, duration, capNeed, shot, clipSize, reloadTime, isInjector], then insertion order
        // lazy chain: almost every comparison is decided by `t`
        #[inline(always)]
        fn f(a: f64, b: f64) -> Ordering {
            b.partial_cmp(&a).unwrap_or(Ordering::Equal)
        }
        f(self.t, o.t)
            .then_with(|| f(self.duration, o.duration))
            .then_with(|| f(self.cap_need, o.cap_need))
            .then_with(|| o.shot.cmp(&self.shot))
            .then_with(|| o.clip.cmp(&self.clip))
            .then_with(|| f(self.reload, o.reload))
            .then_with(|| o.inj.cmp(&self.inj))
            .then_with(|| o.seq.cmp(&self.seq))
    }
}

/// Min-heap of events ordered like Pyfa's heapq (see `Ord for Ev`). Entries are (t, slab index) so sifting
/// moves 16 bytes; the full lexicographic comparison only runs on equal times.
struct EvHeap {
    h: Vec<(f64, u32)>,
    slab: Vec<Ev>,
    free: Vec<u32>,
}

impl EvHeap {
    fn with_capacity(n: usize) -> Self {
        EvHeap { h: Vec::with_capacity(n), slab: Vec::with_capacity(n), free: Vec::with_capacity(n) }
    }
    #[inline(always)]
    fn less(&self, a: (f64, u32), b: (f64, u32)) -> bool {
        if a.0 < b.0 {
            true
        } else if a.0 > b.0 {
            false
        } else {
            // `Ord for Ev` is reversed (max-heap form): "greater" = earlier
            self.slab[a.1 as usize].cmp(&self.slab[b.1 as usize]) == Ordering::Greater
        }
    }
    fn push(&mut self, ev: Ev) {
        let idx = match self.free.pop() {
            Some(i) => {
                self.slab[i as usize] = ev;
                i
            }
            None => {
                self.slab.push(ev);
                (self.slab.len() - 1) as u32
            }
        };
        let item = (ev.t, idx);
        let mut pos = self.h.len();
        self.h.push(item);
        while pos > 0 {
            let parent = (pos - 1) / 2;
            if self.less(item, self.h[parent]) {
                self.h[pos] = self.h[parent];
                pos = parent;
            } else {
                break;
            }
        }
        self.h[pos] = item;
    }
    fn pop(&mut self) -> Option<Ev> {
        let top = *self.h.first()?;
        let last = self.h.pop().unwrap();
        let n = self.h.len();
        if n > 0 {
            let mut pos = 0;
            loop {
                let l = 2 * pos + 1;
                if l >= n {
                    break;
                }
                let r = l + 1;
                let c = if r < n && self.less(self.h[r], self.h[l]) { r } else { l };
                if self.less(self.h[c], last) {
                    self.h[pos] = self.h[c];
                    pos = c;
                } else {
                    break;
                }
            }
            self.h[pos] = last;
        }
        self.free.push(top.1);
        Some(self.slab[top.1 as usize])
    }
    fn into_vec(self) -> Vec<Ev> {
        self.h.iter().map(|&(_, i)| self.slab[i as usize]).collect()
    }
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub fn simulate(capacity: f64, recharge_ms: f64, drains: &[Drain], start_frac: f64, reload: bool, stagger: bool, t_max_ms: f64) -> CapResult {
    simulate_ex(capacity, recharge_ms, drains, start_frac, reload, stagger, t_max_ms, true, None)
}

/// `simulate` with the repeat optimisation switchable and an optional history of (t ms, cap after the changes at t)
/// (Pyfa `saved_changes`, used by the capacitor graph).
#[allow(clippy::too_many_arguments)]
pub fn simulate_ex(
    capacity: f64, recharge_ms: f64, drains: &[Drain], start_frac: f64, reload: bool, stagger: bool, t_max_ms: f64, optimize: bool,
    mut hist: Option<&mut std::collections::BTreeMap<u64, f64>>,
) -> CapResult {
    let mut rec = |t: f64, c: f64| {
        if let Some(h) = hist.as_deref_mut() {
            h.insert(t.to_bits(), c);
        }
    };
    let tau = recharge_ms / 5.0;
    let mut heap = EvHeap::with_capacity(64);
    let mut seq = 0u64;
    let mut period: u64 = 1;
    let mut disable_period = false;
    // group identical modules
    let mut groups: Vec<(Drain, u32)> = Vec::new();
    for d in drains {
        let mut d = *d;
        if !reload && !d.is_injector {
            d.clip_size = 0;
            d.reload_ms = 0.0;
        }
        if d.duration <= 0.0 {
            continue;
        }
        if let Some(g) = groups.iter_mut().find(|(x, _)| {
            x.duration == d.duration && x.cap_need == d.cap_need && x.clip_size == d.clip_size && x.reload_ms == d.reload_ms
                && x.is_injector == d.is_injector && x.disable_stagger == d.disable_stagger
        }) {
            g.1 += 1;
        } else {
            groups.push((d, 1));
        }
    }
    for (d, n) in &groups {
        let mut d = *d;
        if d.clip_size > 0 {
            disable_period = true;
        }
        if d.is_injector {
            for _ in 0..*n {
                heap.push(Ev { t: 0.0, duration: d.duration, cap_need: d.cap_need, shot: 0, clip: d.clip_size, reload: d.reload_ms, inj: true, seq });
                seq += 1;
            }
            continue;
        }
        if stagger && !d.disable_stagger {
            if d.clip_size == 0 {
                d.duration = (d.duration / *n as f64).floor();
            } else {
                let st = (d.duration * d.clip_size as f64 + d.reload_ms) / (*n as f64 * d.clip_size as f64);
                for i in 1..*n {
                    heap.push(Ev { t: i as f64 * st, duration: d.duration, cap_need: d.cap_need, shot: 0, clip: d.clip_size, reload: d.reload_ms, inj: false, seq });
                    seq += 1;
                }
            }
        } else {
            d.cap_need *= *n as f64;
        }
        let dur = d.duration.round().max(1.0) as u64;
        period = period / gcd(period, dur) * dur;
        heap.push(Ev { t: 0.0, duration: d.duration, cap_need: d.cap_need, shot: 0, clip: d.clip_size, reload: d.reload_ms, inj: false, seq });
        seq += 1;
    }
    let period = if disable_period || period as f64 > t_max_ms { t_max_ms } else { period as f64 };

    let cap_max = capacity;
    let mut cap = capacity * start_frac;
    let mut cap_wrap = cap;
    let mut cap_lowest = cap;
    let mut cap_lowest_pre = cap;
    let mut t_wrap = period;
    let mut t_last = 0.0f64;
    let mut iterations = 0u64;
    let mut awaiting: Vec<Ev> = Vec::with_capacity(64);
    let mut awaiting_wrap: Vec<(u64, u64)> = Vec::new();
    let mut ran_out = false;
    let key = |v: &Vec<Ev>| {
        let mut k: Vec<(u64, u64)> = v.iter().map(|e| (e.duration.to_bits(), e.cap_need.to_bits())).collect();
        k.sort();
        k
    };
    let mut last_ev: Option<Ev> = None;
    let mut exp_memo = [(u64::MAX, 0.0f64); 2];
    while let Some(mut ev) = heap.pop() {
        let t_now = ev.t;
        if t_now >= t_max_ms {
            last_ev = Some(ev);
            break;
        }
        if t_now > t_last && cap_max > 0.0 && tau > 0.0 {
            let x = (cap / cap_max).max(0.0).sqrt();
            // the same few event spacings recur all the time: memoise exp() (bit-identical results)
            let arg = (t_last - t_now) / tau;
            let e = if arg.to_bits() == exp_memo[0].0 {
                exp_memo[0].1
            } else if arg.to_bits() == exp_memo[1].0 {
                exp_memo.swap(0, 1);
                exp_memo[0].1
            } else {
                let e = arg.exp();
                exp_memo[1] = exp_memo[0];
                exp_memo[0] = (arg.to_bits(), e);
                e
            };
            cap = (1.0 + (x - 1.0) * e).powi(2) * cap_max;
        }
        if t_now != t_last {
            if cap < cap_lowest_pre {
                cap_lowest_pre = cap;
            }
            if t_now == t_wrap {
                let k = key(&awaiting);
                if optimize && cap >= cap_wrap && k == awaiting_wrap {
                    last_ev = Some(ev);
                    break;
                }
                cap_wrap = py_round(cap, 1);
                awaiting_wrap = k;
                t_wrap += period;
            }
        }
        t_last = t_now;
        iterations += 1;
        if iterations > 5_000_000 {
            last_ev = Some(ev);
            break;
        }
        if ev.inj && cap - ev.cap_need > cap_max {
            awaiting.push(ev);
            continue;
        }
        if ev.cap_need > cap && cap < cap_max {
            while !awaiting.is_empty() && ev.cap_need > cap && cap_max > cap {
                let need = (ev.cap_need - cap).min(cap_max - cap);
                let good: Vec<usize> = (0..awaiting.len()).filter(|&i| -awaiting[i].cap_need >= need).collect();
                let pick = if !good.is_empty() {
                    *good.iter().min_by(|&&a, &&b| (-awaiting[a].cap_need).partial_cmp(&-awaiting[b].cap_need).unwrap()).unwrap()
                } else {
                    (0..awaiting.len()).max_by(|&a, &b| (-awaiting[a].cap_need).partial_cmp(&-awaiting[b].cap_need).unwrap()).unwrap()
                };
                let mut inj = awaiting.remove(pick);
                cap = (cap - inj.cap_need).min(cap_max);
                rec(t_now, cap);
                inj.t = t_now + inj.duration;
                inj.shot += 1;
                if inj.clip > 0 && inj.shot % inj.clip == 0 {
                    inj.shot = 0;
                    inj.t += inj.reload;
                }
                inj.seq = seq;
                seq += 1;
                heap.push(inj);
            }
        }
        cap = (cap - ev.cap_need).min(cap_max);
        rec(t_now, cap);
        if cap < cap_lowest {
            if cap < 0.0 {
                ran_out = true;
                last_ev = Some(ev);
                break;
            }
            cap_lowest = cap;
        }
        while !awaiting.is_empty() && cap < cap_max {
            let need = cap_max - cap;
            let good: Vec<usize> = (0..awaiting.len()).filter(|&i| -awaiting[i].cap_need <= need).collect();
            if good.is_empty() {
                break;
            }
            let pick = *good.iter().max_by(|&&a, &&b| (-awaiting[a].cap_need).partial_cmp(&-awaiting[b].cap_need).unwrap()).unwrap();
            let mut inj = awaiting.remove(pick);
            cap = (cap - inj.cap_need).min(cap_max);
            rec(t_now, cap);
            inj.t = t_now + inj.duration;
            inj.shot += 1;
            if inj.clip > 0 && inj.shot % inj.clip == 0 {
                inj.shot = 0;
                inj.t += inj.reload;
            }
            inj.seq = seq;
            seq += 1;
            heap.push(inj);
        }
        ev.t = t_now + ev.duration;
        ev.shot += 1;
        if ev.clip > 0 && ev.shot % ev.clip == 0 {
            ev.shot = 0;
            ev.t += ev.reload;
        }
        ev.seq = seq;
        seq += 1;
        heap.push(ev);
    }
    // EVE's own stability estimate
    let mut all: Vec<Ev> = heap.into_vec();
    if let Some(e) = last_ev {
        all.push(e);
    }
    let avg_drain: f64 = all.iter().map(|e| e.cap_need / e.duration).sum();
    let inner = -(2.0 * avg_drain * tau - cap_max) / cap_max;
    let eve_stable = if inner >= 0.0 && cap_max > 0.0 { 0.25 * (1.0 + inner.sqrt()).powi(2) } else { 0.0 };
    let stable = !ran_out;
    CapResult {
        stable,
        stable_low: if stable && cap_max > 0.0 { cap_lowest / cap_max } else { 0.0 },
        stable_high: if stable && cap_max > 0.0 { cap_lowest_pre / cap_max } else { 0.0 },
        t_s: t_last / 1000.0,
        depletes_in_s: if stable { None } else { Some(t_last / 1000.0) },
        eve_stable,
        iterations,
    }
}

/// Python `round(x, nd)`: correctly rounded on the exact binary value, ties to even.
/// Fast path for non-ties; near-ties fall back to exact decimal formatting.
#[inline]
pub fn py_round(x: f64, nd: i32) -> f64 {
    if !x.is_finite() {
        return x;
    }
    let p = 10f64.powi(nd);
    let y = x * p;
    let fr = (y - y.trunc()).abs();
    if nd < 0 || (fr - 0.5).abs() > 1e-6 {
        return y.round() / p;
    }
    format!("{:.*}", nd as usize, x).parse().unwrap_or(y.round() / p)
}
