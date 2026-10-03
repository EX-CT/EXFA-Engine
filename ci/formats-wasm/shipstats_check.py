#!/usr/bin/env python3
"""Ship-stats text through the formats WASM (engine stats passed as params.stats_json) must equal eve-fit's
(formats + linked engine) text byte for byte.
usage: ci/formats-wasm/shipstats_check.py EVE_FIT FORMATS_WASM"""
import json, pathlib, subprocess, sys
fit_bin, wasm = sys.argv[1], sys.argv[2]
here = pathlib.Path(__file__).parent
reqs = [json.loads(l) for l in open(here / "shipstats_requests.jsonl")]
calc = []
for r in reqs:  # eve_fit_formats::shipstats_request
    f = json.loads(json.dumps(r["params"]["fit"]))
    f.setdefault("options", {}).update(include_attributes="all", default_spool={"type": "spool_scale", "amount": 0.0}, full_precision=True)
    for m in f.get("modules", []):
        m.pop("spool", None)
    calc.append(json.dumps(f))
stats = subprocess.run([fit_bin, "batch"], input="\n".join(calc) + "\n", capture_output=True, text=True, check=True).stdout.splitlines()
lines = "".join(json.dumps({**r, "params": {**r["params"], "stats_json": s}}) + "\n" for r, s in zip(reqs, stats))
a = subprocess.run(["node", str(here.parent.parent / "crates/eve-fit-formats-wasm/examples/node-formats.mjs"), wasm], input=lines,
                   capture_output=True, text=True, check=True).stdout.splitlines()
b = subprocess.run([fit_bin, "serve-stdio"], input="".join(json.dumps(r) + "\n" for r in reqs), capture_output=True, text=True,
                   check=True).stdout.splitlines()
ok = sum(x == y and '"text"' in x for x, y in zip(a, b))
print(f"- formats wasm shipstats via stats_json: {ok}/{len(reqs)} byte-identical to eve-fit")
sys.exit(0 if ok == len(reqs) else 1)
