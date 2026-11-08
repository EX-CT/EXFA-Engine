#!/usr/bin/env python3
"""Adopt a new EXFA-Data SDE release into the engine -- run locally, by hand (or by an agent).

Replaces the old `sde-update` GitHub workflow (6-hourly poll + auto-release), which could only ever
open an issue when the gate failed. Adopting an SDE needs a human/agent decision (data change vs bench
fixture vs engine bug), so it is a local, reviewed step. The PR that this prepares is still gated by
the normal `ci` workflow (native + wasm suites on the new dataset).

    python ci/sde_update.py                       # latest sde-* release, gate, update, commit on branch sde/<tag>
    python ci/sde_update.py --tag sde-3670638-r7
    python ci/sde_update.py --bench-sha <sha>     # gate against another EXFA-Bench commit (written to bench.lock)
    python ci/sde_update.py --gate-only           # download + build + test + suites, change nothing in the repo
    python ci/sde_update.py --no-commit           # update files but leave them uncommitted

Steps: download dataset + pyfa presets -> pin _bench (bench.lock) -> cargo build --locked + cargo test
-> ci/run_suites.sh native (skip-round1: new data legitimately changes those bytes) -> embed the newest
prices-* snapshot -> sde.lock + patch version bump -> rebuild (Cargo.lock) -> regenerate the two round-1
shas -> commit "Update SDE to <tag>".  Afterwards: push the branch, open a PR, merge when CI is green,
then `git tag v<version> && git push origin v<version>` (release.yml builds + publishes the binaries).
"""
import argparse, gzip, json, os, pathlib, re, shutil, subprocess, sys, hashlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
os.chdir(ROOT)
DATA_REPO = "EX-CT/EXFA-Data"


def run(cmd, check=True, capture=False, env=None, **kw):
    print("+ " + " ".join(str(c) for c in cmd), flush=True)
    r = subprocess.run([str(c) for c in cmd], check=False, capture_output=capture, env=env, **kw)
    if check and r.returncode != 0:
        if capture:
            sys.stderr.write((r.stdout or b"").decode(errors="replace") + (r.stderr or b"").decode(errors="replace"))
        sys.exit(f"FAILED ({r.returncode}): {' '.join(str(c) for c in cmd)}")
    return r


def out(cmd):
    return run(cmd, capture=True).stdout.decode().strip()


def lock(name):
    return dict(l.strip().split("=", 1) for l in pathlib.Path(name).read_text(encoding="utf-8").splitlines() if "=" in l)


def posix(p):
    # forward slashes work for python, cargo and Git Bash alike (C:/Users/...)
    return str(pathlib.Path(p).resolve()).replace("\\", "/")


def find_bash():
    # a bare `bash` on Windows may be the WSL launcher; prefer Git for Windows' bash
    for c in (os.environ.get("EXFA_BASH"), r"C:\Program Files\Git\bin\bash.exe", r"C:\Program Files (x86)\Git\bin\bash.exe"):
        if c and pathlib.Path(c).exists():
            return c
    return shutil.which("bash") or sys.exit("no bash found (install Git for Windows or set EXFA_BASH)")


def latest_release(prefix):
    tags = json.loads(out(["gh", "release", "list", "-R", DATA_REPO, "--limit", "100", "--json", "tagName,createdAt"]))
    tags = [t for t in tags if t["tagName"].startswith(prefix)]
    if not tags:
        sys.exit(f"no {prefix}* release in {DATA_REPO}")
    # sde-<build>-r<rev>: highest build wins; prices-jita44-<time>: newest time wins (both sort lexically/numerically)
    if prefix == "sde-":
        key = lambda t: tuple(int(x) for x in re.findall(r"\d+", t["tagName"]))
    else:
        key = lambda t: t["tagName"]
    return max(tags, key=key)["tagName"]


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--tag", help="EXFA-Data SDE release tag (default: latest sde-*)")
    ap.add_argument("--bench-sha", help="EXFA-Bench commit to gate against (default: bench.lock); written to bench.lock")
    ap.add_argument("--gate-only", action="store_true", help="run the gate, do not touch the repo files")
    ap.add_argument("--no-commit", action="store_true", help="update files, leave them uncommitted")
    ap.add_argument("--skip-tests", action="store_true", help="skip `cargo test --workspace` (suites still run)")
    ap.add_argument("--allow-dirty", action="store_true")
    a = ap.parse_args()

    if not a.gate_only and not a.allow_dirty and out(["git", "status", "--porcelain"]):
        sys.exit("working tree is not clean (commit/stash first, or use --allow-dirty / --gate-only)")

    cur = lock("sde.lock")["SDE_TAG"]
    tag = a.tag or latest_release("sde-")
    print(f"sde.lock pins {cur}; incoming {tag}")
    if tag == cur and not a.gate_only:
        sys.exit("sde.lock already at that tag - nothing to do")
    build = re.match(r"sde-(\d+)-", tag).group(1)

    # --- inputs --------------------------------------------------------------------------------
    d = ROOT / "_data" / tag
    if not list(d.glob("dataset-*.json.gz")):
        run(["gh", "release", "download", tag, "-R", DATA_REPO, "--pattern", "dataset-*.json.gz", "-D", d])
    dataset = next(d.glob("dataset-*.json.gz"))
    presets_tag = re.search(r"presets-pyfa-\d+-r\d+", (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")).group(0)
    pd = ROOT / "_data" / presets_tag
    if not list(pd.glob("presets-pyfa-*.json")):
        run(["gh", "release", "download", presets_tag, "-R", "EX-CT/EXFA-Bench", "--pattern", "presets-pyfa-*", "-D", pd])
    pyfa = next(pd.glob("presets-pyfa-*.json"))

    bl = lock("bench.lock")
    bench_sha = a.bench_sha or bl["BENCH_SHA"]
    bench = ROOT / "_bench"
    if not bench.exists():
        run(["git", "clone", "-q", f"https://github.com/{bl['BENCH_REPO']}", bench])
    run(["git", "-C", bench, "fetch", "-q", "origin"])
    run(["git", "-C", bench, "checkout", "-q", bench_sha])
    print(f"bench @ {bench_sha}")

    env = dict(os.environ, EXFA_DATASET=posix(dataset), EXFA_DATASET_R5=posix(dataset), EXFA_PYFA_DATA=posix(pyfa),
               BENCH=posix(bench), PYTHONUTF8="1")
    # absolute: Windows CreateProcess does not resolve a relative path with forward slashes
    exe = posix(ROOT / "target/release/exfa") + (".exe" if os.name == "nt" else "")

    # --- gate ----------------------------------------------------------------------------------
    run(["cargo", "build", "--release", "--locked", "-p", "exfa-cli"], env=env)
    if not a.skip_tests:
        run(["cargo", "test", "--workspace", "--locked"], env=env)
    shutil.rmtree(ROOT / "ci-results" / "native", ignore_errors=True)
    r = run([find_bash(), "ci/run_suites.sh", "native", exe, "skip-round1"], check=False, env=env)
    if r.returncode != 0:
        sys.exit(f"GATE FAILED on {tag}: see ci-results/native/nrg.txt and summary.md.\n"
                 "Classify each regression: SDE data change / stale bench fixture / engine bug. Do not update sde.lock until resolved.")
    print(f"\nGATE OK on {tag} (wasm suites run in the PR's CI)")
    if a.gate_only:
        return

    # --- update --------------------------------------------------------------------------------
    branch = f"sde/{tag}"
    if out(["git", "rev-parse", "--abbrev-ref", "HEAD"]) == "main":
        run(["git", "checkout", "-b", branch])

    # newest price snapshot -> embedded in exfa-core
    ptag = latest_release("prices-")
    pdir = ROOT / "_data" / ptag
    if not list(pdir.glob("prices-*.json.gz")):
        run(["gh", "release", "download", ptag, "-R", DATA_REPO, "-D", pdir, "-p", "prices-*.json.gz"])
    f = next(pdir.glob("prices-*.json.gz"))
    s = json.loads(gzip.decompress(f.read_bytes()))
    sid, mtime, chash, ntypes = s["snapshot_id"], s["market_time"], s["content_hash"], s["type_count"]
    data = ROOT / "crates/exfa-core/data"
    for old in data.glob("prices-*.json.gz"):
        old.unlink()
    dest = data / f"prices-{sid}.json.gz"
    dest.write_bytes(f.read_bytes())
    p = ROOT / "crates/exfa-core/src/price.rs"
    t = p.read_bytes().decode("utf-8")
    t = re.sub(r'include_bytes!\("\.\./data/prices-[^"]+"\)', f'include_bytes!("../data/{dest.name}")', t)
    t = re.sub(r'EMBEDDED_ID: &str = "[^"]+"', f'EMBEDDED_ID: &str = "{sid}"', t)
    t = re.sub(r'EMBEDDED_TIME: &str = "[^"]+"', f'EMBEDDED_TIME: &str = "{mtime}"', t)
    t = re.sub(r'EMBEDDED_HASH: &str = "[^"]+"', f'EMBEDDED_HASH: &str = "{chash}"', t)
    t = re.sub(r'EMBEDDED_TYPES: usize = \d+', f'EMBEDDED_TYPES: usize = {ntypes}', t)
    p.write_bytes(t.encode("utf-8"))
    print(f"embedded snapshot -> {sid} ({ntypes} types, sde_build {s.get('sde_build')})")
    if str(s.get("sde_build")) != build:
        print(f"WARNING: snapshot sde_build {s.get('sde_build')} != dataset build {build} (embedded anyway; engine warns at runtime)")

    # sde.lock / bench.lock
    for name, key, val in (("sde.lock", "SDE_TAG", tag), ("bench.lock", "BENCH_SHA", bench_sha)):
        pth = ROOT / name
        pth.write_bytes(re.sub(rf"(?m)^{key}=.*$", f"{key}={val}", pth.read_bytes().decode()).encode())

    # patch version bump
    pth = ROOT / "Cargo.toml"
    t = pth.read_bytes().decode()
    m = re.search(r'(?ms)^\[workspace\.package\].*?^version\s*=\s*"(\d+)\.(\d+)\.(\d+)"', t)
    maj, mi, pa = map(int, m.groups())
    new_ver = f"{maj}.{mi}.{pa + 1}"
    span = t[m.start():m.end()]
    pth.write_bytes((t[:m.start()] + span.replace(f"{maj}.{mi}.{pa}", new_ver, 1) + t[m.end():]).encode())
    print(f"version {maj}.{mi}.{pa} -> {new_ver}")
    run(["cargo", "build", "--release", "-p", "exfa-cli"], env=env)   # no --locked: Cargo.lock picks up the version

    # round-1 shas on the new data (same recipe as ci/run_suites.sh)
    cases = sorted((bench / "suites/round1/cases").glob("*.json"))
    jsonl = b"".join(json.dumps(json.load(open(c, encoding="utf-8")), separators=(", ", ": ")).encode() + b"\n" for c in cases)
    res = run([exe, "batch"], capture=True, env=env, input=jsonl)
    sha = hashlib.sha256(res.stdout).hexdigest()
    keys = [l for l in (ROOT / "ci/round1-new-keys.txt").read_text(encoding="utf-8").splitlines() if l and not l.startswith("#")]
    strip = run([sys.executable, "ci/strip_keys.py", *keys], capture=True, input=res.stdout)
    # strip_keys.py writes text-mode stdout: on Windows that turns \n into \r\n; CI (Linux) hashes plain \n
    base = hashlib.sha256(strip.stdout.replace(b"\r\n", b"\n")).hexdigest()
    (ROOT / "ci/round1.sha256").write_bytes((sha + "\n").encode())
    (ROOT / "ci/round1-base.sha256").write_bytes((base + "\n").encode())
    print(f"round1.sha256 = {sha}\nround1-base.sha256 = {base}")

    if a.no_commit:
        print("files updated, not committed (--no-commit)")
        return
    run(["git", "add", "sde.lock", "bench.lock", "Cargo.toml", "Cargo.lock", "ci/round1.sha256", "ci/round1-base.sha256",
         "crates/exfa-core/data", "crates/exfa-core/src/price.rs"])
    run(["git", "commit", "-m", f"Update SDE to {tag}"])
    print(f"\ncommitted on {out(['git', 'rev-parse', '--abbrev-ref', 'HEAD'])}. Next:\n"
          f"  git push -u origin HEAD && gh pr create --fill     # CI gates it on native + wasm\n"
          f"  (after merge)  git tag v{new_ver} && git push origin v{new_ver}   # release.yml publishes the binaries")


if __name__ == "__main__":
    main()
