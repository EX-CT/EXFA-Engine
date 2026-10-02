#!/usr/bin/env python3
"""Cross-check variant J against the reference engine (eve-dogma-rs, variant A) on every JSON case.

  python3 tools/compare_ref.py [--ref BIN] [--j BIN] [--dataset PATH] [case globs...]

Both engines run in batch mode; responses are compared (1) byte-for-byte after normalising meta.engine and
(2) value-by-value with the bench tolerance (|d| <= max(1e-3, 1e-4*|ref|)). Prints a summary + first diffs."""
import argparse, glob, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ap = argparse.ArgumentParser()
ap.add_argument("--ref", default="/workspace/exct-eve/eve-dogma-rs/target/release/eve-dogma")
ap.add_argument("--j", default=os.path.join(HERE, "..", "build", "eve-dogma-j"))
ap.add_argument("--dataset", default="/workspace/exct-eve/data/dataset-3569502.json.gz")
ap.add_argument("--show", type=int, default=30)
ap.add_argument("--json-out")
ap.add_argument("globs", nargs="*", default=["/workspace/exct-eve/eve-dogma-bench/cases/*.json",
                                             "/workspace/exct-eve/eve-dogma-rs/tests/cases/*.json"])
a = ap.parse_args()

files = sorted({f for g in a.globs for f in glob.glob(g)})
names, lines = [], []
for f in files:
    try:
        lines.append(json.dumps(json.loads(open(f).read())))
        names.append(f)
    except Exception as e:  # noqa
        print("skip", f, e)
inp = "".join(l + "\n" for l in lines)

def run(cmd):
    r = subprocess.run(cmd, input=inp, capture_output=True, text=True)
    out = r.stdout.splitlines()
    if len(out) != len(lines):
        sys.exit(f"{cmd[0]}: {len(out)} responses for {len(lines)} requests; stderr: {r.stderr[-500:]}")
    return out

ref = run([a.ref, "--dataset", a.dataset, "batch"])
jj = run([a.j, "--dataset", a.dataset, "batch"])

def norm(s):
    d = json.loads(s)
    if isinstance(d.get("meta"), dict):
        d["meta"]["engine"] = "*"
    return d

def walk(x, y, path, out):
    if isinstance(x, dict) and isinstance(y, dict):
        for k in sorted(set(x) | set(y)):
            if k not in x or k not in y:
                out.append((path + "/" + k, x.get(k, "<missing>"), y.get(k, "<missing>")))
            else:
                walk(x[k], y[k], path + "/" + k, out)
    elif isinstance(x, list) and isinstance(y, list):
        if len(x) != len(y):
            out.append((path + "[len]", len(x), len(y)))
        for i, (p, q) in enumerate(zip(x, y)):
            walk(p, q, f"{path}/{i}", out)
    elif isinstance(x, bool) or isinstance(y, bool) or not isinstance(x, (int, float)) or not isinstance(y, (int, float)):
        if x != y:
            out.append((path, x, y))
    else:
        if abs(x - y) > max(1e-3, 1e-4 * abs(x)):
            out.append((path, x, y))

identical = tol_ok = 0
values = values_bad = 0
report = []
for n, r, j in zip(names, ref, jj):
    rn, jn = norm(r), norm(j)
    if json.dumps(rn, sort_keys=True) == json.dumps(jn, sort_keys=True) and r.replace('"engine":"eve-dogma-rs 0.1.0"', '"engine":"*"') == j.replace('"engine":"eve-dogma-j 0.1.0"', '"engine":"*"'):
        identical += 1
    diffs = []
    walk(rn, jn, "", diffs)
    def count(x):
        if isinstance(x, dict): return sum(count(v) for v in x.values())
        if isinstance(x, list): return sum(count(v) for v in x)
        return 1
    values += count(rn)
    values_bad += len(diffs)
    if not diffs:
        tol_ok += 1
    else:
        report.append((os.path.basename(n), diffs))
print(f"cases: {len(names)}  byte-identical (engine name normalised): {identical}  within tolerance: {tol_ok}")
print(f"leaf values: {values}  differing beyond tolerance: {values_bad}")
for n, d in report[: a.show]:
    print(f"- {n}: {len(d)} diffs: " + "; ".join(f"{p}: ref={x!r} j={y!r}" for p, x, y in d[:6]))
if a.json_out:
    json.dump({"cases": len(names), "identical": identical, "within_tolerance": tol_ok, "values": values,
               "values_differing": values_bad, "diffs": {n: d for n, d in report}}, open(a.json_out, "w"), indent=1, default=str)
