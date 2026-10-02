// Run the golden regression set (test/golden) through the WASM build in Node:
//   node test/wasm_golden.mjs build-wasm/evej.mjs /path/to/dataset.json.gz
// Prints "N passed, M failed"; exit code 1 on any mismatch.
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const [modPath, dsPath] = [process.argv[2] ?? join(here, "../build-wasm/evej.mjs"), process.argv[3] ?? process.env.EVE_DOGMA_DATASET];
if (!dsPath) throw new Error("dataset path: argv[3] or $EVE_DOGMA_DATASET");
const { default: createEvej } = await import(pathToFileURL(resolve(modPath)).href);
const M = await createEvej();
M.FS.writeFile("/dataset.json.gz", readFileSync(dsPath));
const open = M.cwrap("evej_open", "string", ["string"]);
const calc = M.cwrap("evej_calc", "string", ["string"]);
const rpc = M.cwrap("evej_rpc", "string", ["string"]);
const err = open("/dataset.json.gz");
if (err) throw new Error("evej_open: " + err);

const gold = join(here, "golden");
let pass = 0, fail = 0;
for (const f of readdirSync(gold).filter((x) => x.endsWith(".req.json")).sort()) {
  const req = readFileSync(join(gold, f), "utf8").trim();
  const exp = readFileSync(join(gold, f.replace(".req.json", ".out.json")), "utf8").trim();
  const got = calc(req).trim();
  if (got === exp) pass++;
  else { fail++; console.log("FAIL", f, got.slice(0, 200)); }
}
const lines = readFileSync(join(gold, "rpc_session.rpc.jsonl"), "utf8").split("\n").filter((l) => l.trim());
const expLines = readFileSync(join(gold, "rpc_session.out.jsonl"), "utf8").split("\n").filter((l) => l.trim());
const got = lines.map((l) => rpc(l));
if (got.length === expLines.length && got.every((g, i) => g === expLines[i])) pass++;
else { fail++; const i = got.findIndex((g, k) => g !== expLines[k]); console.log("FAIL rpc_session line", i, (got[i] ?? "").slice(0, 200)); }
console.log(`${pass} passed, ${fail} failed`);
process.exit(fail ? 1 : 0);
