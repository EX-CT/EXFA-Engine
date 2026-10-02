#!/usr/bin/env python3
# Numeric-perturbation fuzzer: bench-corpus requests (/tmp/jv/all.jsonl) with numeric leaves nudged (ulp-level
# and percent-level changes, 17-digit decimals, sign flips, nearby integers, range extremes), run through J and
# the reference batch; computed outputs must be byte-identical, errors must have the same code.
# usage: numfuzz_ref.py SEED [N]
import json, math, random, subprocess, sys
seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
N = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
rnd = random.Random(seed)
reqs = [json.loads(l) for l in open('/tmp/jv/all.jsonl')]
def nudge(v):
    r = rnd.random()
    if isinstance(v, bool): return v
    if isinstance(v, int):
        if abs(v) > 10: return v  # type ids, counts: keep the fit valid
        if r < 0.5: return v + rnd.choice([-1, 1])
        if r < 0.7: return rnd.choice([0, 1, 5, 6, 255])
        return float(v) if r < 0.85 else v
    if r < 0.3: return math.nextafter(v, math.inf if rnd.random() < 0.5 else -math.inf)
    if r < 0.6: return v * (1 + rnd.uniform(-0.05, 0.05))
    if r < 0.75: return float('%.17g' % (v * rnd.uniform(0.5, 2)))
    if r < 0.85: return -v
    return rnd.choice([0.0, 1e-9, 1e9, 0.5, 1.0, 100.0])
def walk(o):
    if isinstance(o, dict): return {k: walk(v) for k, v in o.items()}
    if isinstance(o, list): return [walk(v) for v in o]
    if isinstance(o, (int, float)) and not isinstance(o, bool) and rnd.random() < 0.25: return nudge(o)
    return o
L = [json.dumps(walk(rnd.choice(reqs))) for _ in range(N)]
inp = '\n'.join(L) + '\n'
D = '/workspace/exct-eve/data/dataset-3569502.json.gz'
J = '/workspace/exct-eve/lab-j/variant-j/build/eve-dogma-j'
A = '/tmp/refhead/target/release/eve-dogma'
jo = subprocess.run([J, '--dataset', D, 'batch'], input=inp.encode(), capture_output=True, timeout=900).stdout.decode().replace('eve-dogma-j 0.1.0', 'E').splitlines()
ao = subprocess.run([A, '--dataset', D, 'batch'], input=inp.encode(), capture_output=True, timeout=1800).stdout.decode().replace('eve-dogma-rs 0.1.0', 'E').splitlines()
same = errc = diff = 0; ex = []
for k, (a, j) in enumerate(zip(ao, jo)):
    if a == j: same += 1; continue
    if a.startswith('{"error"') and j.startswith('{"error"') and json.loads(a)['error']['code'] == json.loads(j)['error']['code']:
        errc += 1; continue
    diff += 1; ex.append(k)
print('seed', seed, 'lines', len(jo), len(ao), 'same', same, 'err-code-match', errc, 'diff', diff + abs(len(jo) - len(ao)))
for k in ex[:3]: print(L[k][:400]); print('A', ao[k][:300]); print('J', jo[k][:300])
