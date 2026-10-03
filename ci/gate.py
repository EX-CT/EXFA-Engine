#!/usr/bin/env python3
"""Check suite scorecards against ci/gate.json (minimum scores). Exit 1 on any regression."""
import json, pathlib, re, sys
name, T, OUT = sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3])
gate = json.load(open(pathlib.Path(__file__).with_name("gate.json")))
def frac(s):
    a, b = str(s).split("/"); return int(a), int(b)
got = {}
try:
    sc = json.load(open(T / "b19/results" / name / "scorecard.json")); got["bench-1.9.0"] = (sc["cases_fully_correct"], sc["cases"])
except Exception as e: got["bench-1.9.0"] = (0, repr(e))
try:
    m = re.search(r"eft_export: (\d+)/(\d+)", (OUT / "eft18.log").read_text()); got["eft-export-1.8.0"] = (int(m[1]), int(m[2]))
except Exception as e: got["eft-export-1.8.0"] = (0, repr(e))
try:
    sc = json.load(open(T / "cap/cap/results" / f"{name}.json")); got["cap-suite"] = (sc["cases_ok"], sc["cases"])
except Exception as e: got["cap-suite"] = (0, repr(e))
try:
    sc = json.load(open(T / "mut/mutated/results" / name / "scorecard.json")); got["mutated-suite"] = (sc["cases_fully_correct"], sc["cases"])
    e = json.load(open(OUT / "mut-eft.json")); got["mutated-eft-export"] = frac(e["export"]); got["mutated-eft-import"] = frac(e["import"])
except Exception as e: got.setdefault("mutated-suite", (0, repr(e)))
try:
    sc = json.load(open(OUT / "fmt/scorecard.json")); got["formats-suite"] = (sc["rows_passed"], sc["rows"])
except Exception as e: got["formats-suite"] = (0, repr(e))
try:
    sc = json.load(open(T / "gr/results" / f"graphs-{name}" / "scorecard.json")); got["graphs-0.2"] = (sc["cases_fully_correct"], sc["cases"])
except Exception as e: got["graphs-0.2"] = (0, repr(e))
try:
    sc = json.load(open(T / "b110/results" / name / "scorecard.json")); got["bench-1.10.0"] = (sc["cases_fully_correct"], sc["cases"])
except Exception as e: got["bench-1.10.0"] = (0, repr(e))
for k, f in (("effects-1.10.0", "effects.json"), ("ext-1.10.0", "ext.json")):
    try:
        sc = json.load(open(OUT / f)); got[k] = (sc["pass"], sc["total"])
    except Exception as e: got[k] = (0, repr(e))
bad = 0
print(f"| suite ({name}) | score | gate |\n|---|---|---|")
for k, need in gate.items():
    have = got.get(k, (0, "missing"))
    ok = isinstance(have[1], int) and have[0] >= need[0] and have[1] == need[1]
    bad += not ok
    print(f"| {k} | {have[0]}/{have[1]} | ≥ {need[0]}/{need[1]} {'ok' if ok else '**FAIL**'} |")
sys.exit(1 if bad else 0)
