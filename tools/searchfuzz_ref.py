#!/usr/bin/env python3
# search/type fuzzer: random queries (name fragments, case/whitespace changes, Chinese names, punctuation,
# non-ASCII letters), limits and kinds filters, and random type lookups by id/name; J vs the reference over
# serve-stdio, byte for byte (engine name aside). usage: searchfuzz_ref.py SEED [N]
import gzip, json, random, subprocess, sys
seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
N = int(sys.argv[2]) if len(sys.argv) > 2 else 2000
rnd = random.Random(seed)
D = '/workspace/exct-eve/data/dataset-3569502.json.gz'
J = '/workspace/exct-eve/lab-j/variant-j/build/eve-dogma-j'
A = '/tmp/refhead/target/release/eve-dogma'
ds = json.load(gzip.open(D))
names = [t.get('name', '') for t in ds['types'].values() if t.get('name')]
zh = [v for v in (ds.get('names_zh') or {}).values()] if isinstance(ds.get('names_zh'), dict) else []
ids = [int(k) for k in ds['types']]
KINDS = ['ship', 'module', 'charge', 'drone', 'fighter', 'implant', 'booster', 'skill', 'structure', 'bogus']
def q():
    s = rnd.choice(zh) if zh and rnd.random() < 0.15 else rnd.choice(names)
    r = rnd.random()
    if r < 0.4: a = rnd.randrange(len(s)); s = s[a:a + rnd.randint(1, 8)]
    elif r < 0.5: s = s.upper()
    elif r < 0.6: s = '  ' + s.lower() + ' '
    elif r < 0.65: s = rnd.choice(['', ' ', '-', "'", 'ä', 'Ω', 'ｒｉｆｔｅｒ', 'İ', 'ß', '\u0130stanbul', 'x' * 300])
    return s
L = []
for k in range(N):
    r = rnd.random()
    if r < 0.6:
        p = {"query": q()}
        if rnd.random() < 0.5: p["limit"] = rnd.choice([0, 1, 5, 20, 100, 1000, -1, 2.5, "3"])
        if rnd.random() < 0.3: p["kinds"] = rnd.sample(KINDS, rnd.randint(0, 3))
        L.append({"id": k, "method": "search", "params": p})
    else:
        x = rnd.choice(ids) if rnd.random() < 0.5 else (q() if rnd.random() < 0.7 else rnd.choice([0, -5, 2**33, 1.5, None, [], "587"]))
        L.append({"id": k, "method": "type", "params": {"id": x}})
inp = ''.join(json.dumps(x, ensure_ascii=False) + '\n' for x in L)
o = {n: subprocess.run([b, '--dataset', D, 'serve-stdio'], input=inp.encode(), capture_output=True).stdout.decode()
     .replace('eve-dogma-rs 0.1.0', 'E').replace('eve-dogma-j 0.1.0', 'E').splitlines() for n, b in (('A', A), ('J', J))}
d = [k for k in range(N) if k >= len(o['A']) or k >= len(o['J']) or o['A'][k] != o['J'][k]]
print('seed', seed, 'requests', N, 'lines', len(o['A']), len(o['J']), 'diff', len(d))
for k in d[:4]: print(json.dumps(L[k], ensure_ascii=False)[:300]); print('A', o['A'][k][:300]); print('J', o['J'][k][:300])
