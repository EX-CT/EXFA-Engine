// Usage: node crates/eve-wasm/examples/node-calc.mjs target/wasm32-unknown-unknown/release-small/eve_wasm.wasm < request.json
import { readFileSync } from "node:fs";
const bytes = readFileSync(process.argv[2]);
const { instance } = await WebAssembly.instantiate(bytes, {});
const x = instance.exports;
function call(fn, str) {
  const inp = new TextEncoder().encode(str);
  const p = x.alloc(inp.length);
  new Uint8Array(x.memory.buffer, p, inp.length).set(inp);
  const r = x[fn](p, inp.length);           // u64 → BigInt: ptr << 32 | len
  x.dealloc(p, inp.length);
  const op = Number(r >> 32n), ol = Number(r & 0xffffffffn);
  const out = new TextDecoder().decode(new Uint8Array(x.memory.buffer, op, ol));
  x.dealloc(op, ol);
  return out;
}
process.stdout.write(call("calc", readFileSync(0, "utf8")) + "\n");
