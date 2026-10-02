#!/usr/bin/env python3
# Structured random-fit generator: random published ship + random slot-correct modules (states, charges of a
# matching group/size, spool, mutations), drones, fighters, implants, boosters, cargo, modes, skills, projected
# items, fleet buffs/boosters, environment beacons, damage pattern, target profile and options. Every request is
# run through J and the eve-dogma-rs reference batch; outputs must be byte-identical (engine name normalised).
# usage: randfit_ref.py SEED [N] [--out FILE]   (writes the requests to FILE when given, e.g. for the Pyfa oracle)
import gzip, json, random, subprocess, sys
args = [a for a in sys.argv[1:] if not a.startswith('--')]
seed = int(args[0]) if args else 1
N = int(args[1]) if len(args) > 1 else 500
out_file = sys.argv[sys.argv.index('--out') + 1] if '--out' in sys.argv else None
rnd = random.Random(seed)
D = '/workspace/exct-eve/data/dataset-3569502.json.gz'
J = '/workspace/exct-eve/lab-j/variant-j/build/eve-dogma-j'
A = '/tmp/refhead/target/release/eve-dogma'
ds = json.load(gzip.open(D))
T = ds['types']; G = ds['groups']; E = ds['effects']
eid = {v['name']: int(k) for k, v in E.items()}
def pub(t): return t.get('published', True)
def effs(t): return {e[0] for e in t.get('effects', [])}
SLOT = {'high': eid['hiPower'], 'mid': eid['medPower'], 'low': eid['loPower'], 'rig': eid['rigSlot'], 'subsystem': eid['subSystem']}
if 'serviceSlot' in eid: SLOT['service'] = eid['serviceSlot']
cat = lambda t: G[str(t['group'])]['category']
ships = [int(k) for k, t in T.items() if cat(t) == 6 and pub(t)]
structures = [int(k) for k, t in T.items() if cat(t) == 65 and pub(t)]
mods = {s: [int(k) for k, t in T.items() if cat(t) in (7, 32, 66) and pub(t) and e in effs(t)] for s, e in SLOT.items()}
charges = [int(k) for k, t in T.items() if cat(t) == 8 and pub(t)]
by_group = {}
for c in charges: by_group.setdefault(T[str(c)]['group'], []).append(c)
drones = [int(k) for k, t in T.items() if cat(t) == 18 and pub(t)]
fighters = [int(k) for k, t in T.items() if cat(t) == 87 and pub(t)]
implants = [int(k) for k, t in T.items() if cat(t) == 20 and pub(t) and '331' in t['attrs']]
boosters = [int(k) for k, t in T.items() if cat(t) == 20 and pub(t) and '1087' in t['attrs']]
skills = [int(k) for k, t in T.items() if cat(t) == 16 and pub(t)]
beacons = [int(k) for k, t in T.items() if G[str(t['group'])]['name'] in ('Effect Beacon', 'Cloud') ]
modes = [int(k) for k, t in T.items() if G[str(t['group'])]['name'] == 'Ship Modifiers']
dbuffs = [int(k) for k in ds['dbuffs']]
muta = ds['mutaplasmids']
muta_for = {}
for mid, m in muta.items():
    for mp in m['mapping']:
        for i in mp['inputs']: muta_for.setdefault(i, []).append(int(mid))
CG = ['604', '605', '606', '609', '610', '2076', '2077', '2078', '2079', '2080']
def charge_for(tid):
    t = T[str(tid)]
    gs = [int(t['attrs'][a]) for a in CG if a in t['attrs'] and t['attrs'][a]]
    cs = [c for g in gs for c in by_group.get(g, [])]
    size = t['attrs'].get('128')
    if size is not None and rnd.random() < 0.8:
        cs2 = [c for c in cs if T[str(c)]['attrs'].get('128') == size]
        cs = cs2 or cs
    return rnd.choice(cs) if cs else None
STATES = ['offline', 'online', 'active', 'active', 'active', 'overheated']
def module(slot):
    tid = rnd.choice(mods[slot])
    m = {'type_id': tid, 'slot': slot, 'state': rnd.choice(STATES) if slot not in ('rig', 'subsystem') else 'online'}
    if rnd.random() < 0.7:
        c = charge_for(tid)
        if c: m['charge_type_id'] = c
    if rnd.random() < 0.1: m['spool'] = rnd.choice([{'type': 'spool_scale', 'amount': rnd.random()}, {'type': 'cycle_scale', 'amount': rnd.random()}, {'type': 'cycles', 'amount': rnd.randrange(0, 40)}, {'type': 'time', 'amount': rnd.random() * 100}])
    if tid in muta_for and rnd.random() < 0.3:
        mid = rnd.choice(muta_for[tid])
        bt = T[str(tid)]['attrs']
        m['mutation'] = {'base_type_id': tid, 'mutaplasmid_type_id': mid, 'attributes': {a: round(bt.get(a, 1.0) * rnd.uniform(lo, hi), 4) for a, (lo, hi) in muta[str(mid)]['attrs'].items() if rnd.random() < 0.7}}
        m['type_id'] = next(mp['output'] for mp in muta[str(mid)]['mapping'] if tid in mp['inputs'])
    return m
def fit(depth=0):
    ship = rnd.choice(structures if rnd.random() < 0.05 else ships)
    st = T[str(ship)]['attrs']
    req = {'schema_version': 1, 'ship': {'type_id': ship}}
    if modes and rnd.random() < 0.1: req['ship']['mode_type_id'] = rnd.choice(modes)
    ms = []
    for slot, a in (('high', '14'), ('mid', '13'), ('low', '12'), ('rig', '1137'), ('subsystem', '1367'), ('service', '2056')):
        if slot not in mods or not mods[slot]: continue
        n = int(st.get(a, 0))
        if slot == 'subsystem' and n: n = rnd.randint(0, 4)
        n = max(0, n + (rnd.choice([-1, 0, 0, 0, 1]) if n else (1 if rnd.random() < 0.03 else 0)))
        for _ in range(n): ms.append(module(slot))
    req['modules'] = ms
    if drones and rnd.random() < 0.5:
        req['drones'] = [{'type_id': rnd.choice(drones), 'quantity': rnd.randint(1, 5), 'active': rnd.randint(0, 5)} for _ in range(rnd.randint(1, 3))]
    if fighters and rnd.random() < 0.08:
        req['fighters'] = [{'type_id': rnd.choice(fighters), 'quantity': rnd.randint(1, 9), 'active': rnd.random() < 0.7} for _ in range(rnd.randint(1, 3))]
    if rnd.random() < 0.4: req['implants'] = rnd.sample(implants, rnd.randint(1, 4))
    if rnd.random() < 0.2: req['boosters'] = [{'type_id': b} for b in rnd.sample(boosters, rnd.randint(1, 3))]
    if rnd.random() < 0.2: req['cargo'] = [{'type_id': rnd.choice(charges), 'quantity': rnd.randint(1, 1000)}]
    r = rnd.random()
    if r < 0.6: req['character'] = {'skills': {'default_level': 5}}
    elif r < 0.8: req['character'] = {'skills': {'default_level': rnd.randint(0, 5), 'levels': {str(s): rnd.randint(0, 5) for s in rnd.sample(skills, 30)}}}
    elif r < 0.9: req['character'] = {'skills': {'default_level': 0}, 'security_status': rnd.uniform(-10, 5)}
    if rnd.random() < 0.15:
        pr = []
        for _ in range(rnd.randint(1, 3)):
            r2 = rnd.random()
            if r2 < 0.6:
                mm = {'type_id': rnd.choice(mods[rnd.choice(['high', 'mid', 'low'])]), 'state': 'active'}
                c = charge_for(mm['type_id'])
                if c and rnd.random() < 0.5: mm['charge_type_id'] = c
                e = {'kind': 'module', 'module': mm}
            elif r2 < 0.85: e = {'kind': 'drone', 'drone': {'type_id': rnd.choice(drones), 'quantity': rnd.randint(1, 5)}}
            elif depth == 0: e = {'kind': 'fit', 'fit': fit(1)}
            else: continue
            e['amount'] = rnd.randint(1, 3)
            if rnd.random() < 0.7: e['distance_m'] = rnd.uniform(0, 80000)
            pr.append(e)
        req['projected'] = pr
    if rnd.random() < 0.1:
        req['fleet'] = {'buffs': [{'buff_id': rnd.choice(dbuffs), 'value': rnd.uniform(-30, 30)} for _ in range(rnd.randint(1, 3))]}
        if depth == 0 and rnd.random() < 0.3: req['fleet']['booster_fits'] = [fit(1)]
    if beacons and rnd.random() < 0.08: req['environment'] = {'effect_type_ids': [rnd.choice(beacons)], 'system_security': rnd.choice(['hisec', 'lowsec', 'nullsec', 'wspace'])}
    if rnd.random() < 0.2: req['damage_pattern'] = {k: rnd.uniform(0, 100) for k in ('em', 'thermal', 'kinetic', 'explosive')}
    if rnd.random() < 0.1: req['target_profile'] = {'em': rnd.random(), 'thermal': rnd.random(), 'kinetic': rnd.random(), 'explosive': rnd.random(), 'signature_radius': rnd.uniform(10, 5000), 'max_velocity': rnd.uniform(0, 3000)}
    if rnd.random() < 0.05: req['boosters'] = [{'type_id': b} for b in rnd.sample(boosters, 1)]
    if rnd.random() < 0.05: req['overrides'] = [{'type_id': ship, 'attribute_id': rnd.choice([37, 9, 263, 265, 482, 48, 11]), 'value': rnd.uniform(0, 5000)}]
    o = {}
    if rnd.random() < 0.15: o['factor_reload'] = True
    if rnd.random() < 0.05: o['include_attributes'] = 'all'
    if rnd.random() < 0.05: o['rah'] = 'disable'
    if rnd.random() < 0.05: o['nos_no_target_cap'] = True
    if rnd.random() < 0.05: o['sources'] = True
    if rnd.random() < 0.1: o['default_spool'] = {'type': 'spool_scale', 'amount': rnd.random()}
    if rnd.random() < 0.15: o['cap_sim'] = {'reload': rnd.random() < 0.5, 'stagger': rnd.random() < 0.5}
    if o: req['options'] = o
    return req
reqs = [fit() for _ in range(N)]
inp = ''.join(json.dumps(r) + '\n' for r in reqs)
if out_file: open(out_file, 'w').write(inp)
jo = subprocess.run([J, '--dataset', D, 'batch'], input=inp.encode(), capture_output=True, timeout=900)
ao = subprocess.run([A, '--dataset', D, 'batch'], input=inp.encode(), capture_output=True, timeout=1800)
jl = [l.replace('eve-dogma-j 0.1.0', 'E') for l in jo.stdout.decode().splitlines()]
al = [l.replace('eve-dogma-rs 0.1.0', 'E') for l in ao.stdout.decode().splitlines()]
ok = sum(1 for l in al if not l.startswith('{"error"'))
same = errc = 0; diffs = []
for k, (a, j) in enumerate(zip(al, jl)):
    if a == j: same += 1; continue
    if a.startswith('{"error"') and j.startswith('{"error"') and json.loads(a)['error']['code'] == json.loads(j)['error']['code']: errc += 1; continue
    diffs.append(k)
print(f'seed {seed} requests {N} J rc {jo.returncode} lines {len(jl)} ref rc {ao.returncode} lines {len(al)} computed {ok} same {same} err-code-match {errc} diff {len(diffs) + abs(len(jl) - len(al))}')
for k in diffs[:3]:
    print('REQ', json.dumps(reqs[k])[:600])
    a, j = json.loads(al[k]), json.loads(jl[k])
    def walk(x, y, p=''):
        if type(x) != type(y): print(' ', p, x if not isinstance(x, (dict, list)) else '...', '|', y if not isinstance(y, (dict, list)) else '...'); return
        if isinstance(x, dict):
            for kk in sorted(set(x) | set(y)): walk(x.get(kk), y.get(kk), p + '.' + kk)
        elif isinstance(x, list):
            if len(x) != len(y): print(' ', p, 'len', len(x), len(y))
            for i, (u, v) in enumerate(zip(x, y)): walk(u, v, f'{p}[{i}]')
        elif x != y: print(' ', p, 'ref', x, 'J', y)
    walk(a, j)
