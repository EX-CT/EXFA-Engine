#!/usr/bin/env python3
"""Check the explicit EFT gates against ci/gate.json (minimum scores). Exit 1 on any regression.

Suite coverage itself is enforced by check_no_regress.py against the EXFA-Bench baseline; this script
only verifies the round-1 EFT export (326/326) and mutated EFT export/import (93/99) results, which are
produced by dedicated tools rather than scorecards. Usage: gate.py MODE OUTDIR"""
import json, pathlib, re, sys
name, OUT = sys.argv[1], pathlib.Path(sys.argv[2])
gate = json.load(open(pathlib.Path(__file__).with_name("gate.json")))
def frac(s):
    a, b = str(s).split("/"); return int(a), int(b)
got = {}
try:
    m = re.search(r"eft_export: (\d+)/(\d+)", (OUT / "eft18.log").read_text()); got["eft-export-1.8.0"] = (int(m[1]), int(m[2]))
except Exception as e: got["eft-export-1.8.0"] = (0, repr(e))
try:
    e = json.load(open(OUT / "mut-eft.json")); got["mutated-eft-export"] = frac(e["export"]); got["mutated-eft-import"] = frac(e["import"])
except Exception as e: got["mutated-eft-export"] = got["mutated-eft-import"] = (0, repr(e))
bad = 0
print(f"| gate ({name}) | score | required |\n|---|---|---|")
for k, need in gate.items():
    have = got.get(k, (0, "missing"))
    ok = isinstance(have[1], int) and have[0] >= need[0] and have[1] == need[1]
    bad += not ok
    print(f"| {k} | {have[0]}/{have[1]} | ≥ {need[0]}/{need[1]} {'ok' if ok else '**FAIL**'} |")
sys.exit(1 if bad else 0)
