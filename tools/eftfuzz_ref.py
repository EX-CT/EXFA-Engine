#!/usr/bin/env python3
# EFT text fuzzer: exports the bench corpus to EFT (through J's eft_export), mutates the text (dropped/duplicated/
# swapped lines, case and whitespace changes, quantities, /OFFLINE, unknown names, ship line damage, CRLF, empty
# slots, cargo/drone sections, truncation) and compares J's and the reference's eft_parse responses byte for byte.
# usage: eftfuzz_ref.py SEED [N]
import json, random, subprocess, sys
seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
N = int(sys.argv[2]) if len(sys.argv) > 2 else 1500
rnd = random.Random(seed)
D = '/workspace/exct-eve/data/dataset-3569502.json.gz'
J = '/workspace/exct-eve/lab-j/variant-j/build/eve-dogma-j'
A = '/tmp/refhead/target/release/eve-dogma'
reqs = [json.loads(l) for l in open('/tmp/jv/all.jsonl')]
inp = ''.join(json.dumps({"id": i, "method": "eft_export", "params": {"fit": r, "name": "F%d" % i}}) + '\n' for i, r in enumerate(reqs))
texts = [json.loads(l)['result']['text'] for l in subprocess.run([J, '--dataset', D, 'serve-stdio'], input=inp.encode(), capture_output=True).stdout.decode().splitlines()]
def mutate(t):
    L = t.split('\n')
    for _ in range(rnd.randint(1, 4)):
        r = rnd.random(); i = rnd.randrange(len(L))
        if r < 0.12: del L[i]
        elif r < 0.22: L.insert(i, L[i])
        elif r < 0.30: j = rnd.randrange(len(L)); L[i], L[j] = L[j], L[i]
        elif r < 0.40: L[i] = L[i].upper() if rnd.random() < 0.5 else L[i].lower()
        elif r < 0.50: L[i] = rnd.choice(['  ', '\t', ' ']) + L[i] + rnd.choice(['', ' ', '  '])
        elif r < 0.58: L[i] = L[i] + rnd.choice([' x2', ' x0', ' x-1', ' x99999999999', 'x3', ' x 3'])
        elif r < 0.64: L[i] = L[i] + rnd.choice([' /OFFLINE', '/offline', ' /OFFLINE ', ' [Empty High slot]'])
        elif r < 0.70: L[i] = rnd.choice(['[Empty High slot]', '[Empty Low slot]', '[empty mid slot]', 'Nonexistent Module', 'Rifter', ''])
        elif r < 0.76 and i == 0: L[0] = rnd.choice(['[Rifter]', '[, x]', '[Nope, y]', 'Rifter, x', '[Rifter, ]', '[ rifter , a,b]'])
        elif r < 0.82: L[i] = L[i].replace(', ', ',', 1) if rnd.random() < 0.5 else L[i].replace(',', ', ,', 1)
        elif r < 0.88: L[i] = L[i][:rnd.randrange(len(L[i]) + 1)]
        elif r < 0.94: L.insert(i, rnd.choice(['', '', 'Hobgoblin I x5', 'Antimatter Charge S x100', 'Rifter x1']))
    s = '\n'.join(L)
    if rnd.random() < 0.1: s = s.replace('\n', '\r\n')
    if rnd.random() < 0.05: s = s[:rnd.randrange(len(s) + 1)]
    return s
lines = [json.dumps({"id": k, "method": "eft_parse", "params": {"text": mutate(rnd.choice(texts))}}, ensure_ascii=False) for k in range(N)]
inp = '\n'.join(lines) + '\n'
o = {n: subprocess.run([b, '--dataset', D, 'serve-stdio'], input=inp.encode(), capture_output=True).stdout.decode().splitlines() for n, b in (('A', A), ('J', J))}
d = [k for k in range(N) if k >= len(o['A']) or k >= len(o['J']) or o['A'][k] != o['J'][k]]
ok = sum(1 for x in o['J'] if '"error"' not in x[:40])
print('seed', seed, 'requests', N, 'lines', len(o['A']), len(o['J']), 'parsed-ok', ok, 'diff', len(d))
for k in d[:3]: print(lines[k][:400]); print('A', o['A'][k][:300] if k < len(o['A']) else None); print('J', o['J'][k][:300] if k < len(o['J']) else None)
