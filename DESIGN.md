# Variant J — design

## Goals

1. Correctness against Pyfa, via the contract's FitStats fields. Fidelity to the reference engine (eve-dogma-rs)
   is byte-level, so every behaviour the reference matches Pyfa on is inherited exactly.
2. Extreme single-fit latency: one `calc` is about 60 µs in process, and a cold process is about 4 ms.
3. Batch throughput: `batch` spreads the JSONL across all cores and keeps the output order.

## Pipeline

```
gz JSON dataset --libdeflate--> simdjson DOM --> flat POD image (.bin, cached, mmapped)
FitRequest JSON --simdjson ondemand/DOM--> FitRequest --build--> Fit (items + attr table) --stats--> streaming JSON writer
```

### Dataset image (src/dataset.*)
* Built once per dataset: gunzip (libdeflate), parse (simdjson), then write a single relocatable blob. The blob
  holds fixed-size records for types, attributes, effects and modifiers, plus flat index arrays (type → attrs
  sorted by id, type → effects, effect → modifiers, group/skill membership), a string pool, and sorted name
  indexes for `search`/name lookup. Derived tables are computed at build time and stored too: the skill-pruning
  relevance index (always / by group / by required skill), so a cold process does no per-process precompute.
* The cache file is keyed by the dataset name and the image format version (`-v6.bin`), and validated by source size plus a fast hash. The full SHA-256
  of the JSON is stored in the image and reported in `meta`. The file is written to a temp file and atomically
  renamed, so concurrent first runs are safe.
* At start-up the image is `mmap`ped read-only. No parsing or allocation happens, and pages are shared between
  processes and threads. This is why cold start is about 1–2 ms, against about 150–200 ms for engines that parse
  the JSON at start.
* Attribute lookups use binary search over a type's sorted (attr id, value) slice. These slices are short and
  cache-resident.

### Engine (src/engine.*)
* A direct port of eve-dogma-rs's lazy, memoised modifier graph. Items are the ship, character, skills,
  modules, charges, drones, fighters, implants, boosters, subsystems, the mode, environment and projected
  sources. Each fit has a local open-addressing hash table `(item, attr) → LAttr` that holds the base value,
  the memoised value, a dirty flag and the head of an intrusive linked list of modifiers (all in one arena).
  Slots carry a generation stamp, so reusing a worker's fit for the next request bumps the generation instead
  of clearing the table (-2.5 % instructions per rifter calc). Skill pruning uses an inverted relevance index
  (group → skills, required skill → skills) built once, so each fit marks its relevant skills in one pass.
* Values are computed on demand, with a cycle guard, Pyfa's operator order (PreAssign … PostAssign) and stacking
  penalties. An attribute the fit never touches falls back to the type's raw value without being materialised.
* The same feature set as the reference: projected modules, drones and whole fits (frozen into base values),
  fleet bursts, booster fits (strongest |value| per buff id), environment effects, the Reactive Armor
  Hardener simulation, damage patterns, reload, incoming remote reps, neuts, nos and cap transfers, projected
  tracking/guidance disruptors and remote tracking computers (Pyfa Effect6424/6423/shipModuleRemoteTrackingComputer:
  the target's Gunnery modules / Missile Launcher Operation charges, with range factor and resistance; RTCs gated by
  the target's disallowAssistance), abyssal weather / AoE cloud beacon buffs (including drones and Pyfa's unpenalised resist/HP/velocity buffs), incursion system effects, burst projectors (web/paint/damp/track/neut/ECM at full strength), the Standup weapon disruptor, Breach Control, and the contract 1.4.3 semantics (projected `amount`, fleet-buff
  precedence, use/injected/delta GJ/s).
* All floating-point arithmetic follows the reference expression order, including Rust's
  `Iterator::sum` starting from −0.0 and `min_by`/`max_by` tie-breaking. This keeps outputs byte-identical.

### Capacitor simulation (src/capsim.*)
A port of the reference's event-driven Pyfa capsim. It uses a binary min-heap with the same ordering and tie
rules, plus the same stagger and clip semantics. As in eve-dogma-rs 60de0b9, the static tie-break fields are
replaced by their ranks and each event's order is packed into integer keys (two u64 for <= 256 sources, a general
three-key layout otherwise; a unit test checks both layouts give bit-identical results). `cap_wrap` uses
Python's `round(cap, 1)` (exact, ties to even).

### Stats and output (src/stats.*, src/jsonw.hpp)
* Stats are computed in reference order, then emitted with a streaming writer that writes keys in sorted
  (BTreeMap) order. There is no DOM.
* Numbers are formatted like serde_json/ryu (shortest round-trip, `1.0` style for integral values) after the
  same `round6` step.
* Each thread has a `Worker` holding its own simdjson parser and output buffer. These are reused across
  requests, so a warmed-up calc does almost no allocation besides the Fit itself.

## Batch / threading
* With `--threads N` (N > 1, default `hardware_concurrency()`), `batch` runs as a streaming pipeline:
  * The main thread reads stdin and splits it into lines.
  * N workers, each with its own simdjson parser, reused `Fit` arena and output buffer, pull jobs from a queue.
  * A writer thread emits results strictly in input order.
  * There are no per-chunk barriers, so computing overlaps with reading and writing.
* When a single request arrives with nothing in flight (interactive request/response use), the reader computes it
  inline and answers right away. This avoids two thread hand-offs, which cost milliseconds on a loaded box.
* stdout is flushed whenever the writer has caught up with the finished results, so pipelined clients never stall.
* Output is byte-identical to `--threads 1`, which uses a simple serial loop.
* The dataset is read-only and shared. All mutable state is per worker or per fit, so the hot path takes no
  locks; only the job queue and result slots take a mutex, once per line.

## Start-up path
* The executable is linked statically by default (`EVEJ_STATIC`). This saves about 0.6 ms of dynamic loading per
  process.
* Cache freshness is checked by source size + mtime (stored in the image header). Only on a mismatch is the source
  read and its content hash compared, so a warm start does not read the 0.7 MB gzip.
* Resolved attribute and effect ids (`Ids`, about 400 name lookups) are cached in `<cache>.ids`. The file is keyed
  by the dataset sha256 and by the executable's size and mtime, so a rebuilt engine never reuses stale ids.
* Typical warm-start phases: open 0.03 ms, ids 0.02 ms, first calc 0.4–0.8 ms (cold CPU caches and page faults
  on the image), then about 0.05 ms per calc.

## Hot-path notes (measured with callgrind, instructions per rifter calc ≈ 0.95 M)
* Skill pruning: a skill is instantiated only if one of its modifiers can reach an item other than itself
  (ship / char targets, location-group targets present in the fit, required-skill targets the fit's items need)
  or a fit item requires it. A skill's own attributes are only read by its own modifiers, so self-, "other"- and
  target-domain modifiers do not make it relevant. This is stricter than eve-dogma-rs's rule (which keeps any
  skill with an item modifier) and removes ~200 of ~500 skills for a typical fit: -19 % instructions on the
  rifter, -16 % on the corpus, output unchanged (checked against `EVEJ_NO_PRUNE=1` and the reference).
* Modifier targets filtered by location group or required skill come from per-fit sorted `(key, item)` indexes,
  instead of scanning all skills for every modifier.
* Attribute entries are created without a base value. The type's base or default is filled lazily on first read,
  and one probe both finds and inserts.
* Six-decimal numbers (almost all output) use a fast fixed-point formatter, which is proven equal to the shortest
  round-trip path for 1e-5 ≤ |v| < 1e9 and checked against it in `test/fmt_test.cpp` over 5 M random values.
  Everything else goes through `std::to_chars`.
* Capsim: rank-keyed 24-byte heap entries updated in place (one sift-down per event) and a memo of
  `exp(dt/tau)` for recurring time steps. The heap layout matches the reference's `BinaryHeap`, so the final
  avg-drain summation order is identical.

## Trade-offs
* Byte-for-byte reference fidelity was chosen over independent re-derivation from Pyfa. Every value matches
  Pyfa wherever the reference does (all bench 1.8.0 values today, 326/326 cases). The cost is that J inherits any
  divergence the reference has, and that new reference features must be ported (done up to eve-dogma-rs
  60de0b9, contract revision 1.4.3: incl. overheat-before-module application order and breacher pods, which are
  pending for bench 1.9.0).
* The binary cache costs about 110 ms once per dataset and ~20 MB of disk. It can be disabled.
* The lazy evaluator only computes what the stats need. A full attribute dump (`type`) goes through the same
  path.
* EFT: `eft FILE [--calc] [--skills N]`, rpc `eft_parse` / `eft_export`. Export follows Pyfa's exporter layout
  (contract 1.4.1 ruling 4; bench check 289/289); slot fillers use the built fit's slot totals after modifiers.
* `type` lists the type-level mass/capacity/volume/radius among the attributes (a non-zero field overrides the
  attribute; always present), like eve-dogma-rs since 1d09341.
* `search` follows the interim 1.4.1 spec (kinds, exact > prefix > substring, typeID ties, limit 20).
  Lowercasing covers ASCII, Latin-1/Ext-A, Greek, Cyrillic and fullwidth letters (Rust uses full Unicode).

## Number semantics
* Input: eve-dogma-rs parses JSON with serde_json without `float_roundtrip`, which does not always round
  decimals correctly (19-digit significand, then one multiply/divide by a power of ten). `serdenum.cpp` scans the
  raw request text once and rewrites only the number tokens whose serde value differs from the correctly
  rounded one (>= 16 significant digits or large exponents), plus `-0` (a float -0.0 for serde), so simdjson's
  exact parse yields serde's double. The common request has no such token and is not copied.
* Output: shortest round-trip like serde_json 1.0.151 (zmij): `1.0` for integral values, exponents as `1e+16` /
  `1e-7`. Lock time uses Rust's `asinh` formulation (log1p/hypot), so the last ulp matches.

## Tests and builds
* `ctest --test-dir build` (README "Test"): 7 unit tests and 116 golden regression tests whose stored outputs
  were checked against eve-dogma-rs when recorded. Wider parity tooling lives in `tools/` (compare_ref,
  fuzz_ref, randfit_ref, arrconv_ref) and is summarised in `results/compare_ref.txt`.
* WebAssembly: the same sources build with Emscripten (`src/wasm.cpp`, `wasm/README.md`). simdjson and libdeflate
  are built from source via FetchContent, and C++ exceptions are enabled (request errors use them). The output is
  byte-identical to the native build (bench corpus and golden set in Node).

## Provenance
The dogma algorithms (modifier graph semantics, stacking, capsim, RAH, stats formulas) were ported from
eve-dogma-rs (LGPL-3.0-or-later), which derives them from Pyfa's behaviour. J is therefore a derivative work
and is licensed LGPL-3.0-or-later. No Pyfa source was copied. Pyfa was used only as the black-box oracle
behind the bench's expected values.

## Contract notes / open questions
* `type_by_name` with duplicate names: J picks the smallest published type id. The reference's choice among
  duplicates is unspecified (hash-order dependent).
* BAD_REQUEST message texts differ from serde's (for example, no "at line 1 column N"). The error codes are
  identical.
* `meta.engine` = `eve-dogma-j 0.1.0`.
* Batch: the contract only requires in-order JSONL. J's parallel execution keeps that order.
* `options` omitted → `validate` true (contract 1.4.1 ruling 2; J always behaved this way).
* eve-dogma-rs is developed with uncommitted WIP in its working tree; J's parity checks use binaries built
  from the committed HEAD (`git archive`), not the live `target/release` binary.
* Request typing follows serde: integer fields reject floats (`2.0`), explicit `null` is accepted only for
  `Option` fields, `schema_version` must be a u32 or null. Fuzzing (tools/fuzz_ref.py, 3 × 3000 mutated corpus
  requests) gives identical outcomes to eve-dogma-rs (same output or same error code), including structs written
  as JSON arrays (serde's positional form: fields in declaration order, missing trailing fields take their default,
  a missing required field or extra elements is an error). J rewrites such requests to the object form only after
  the normal parse failed, so the common path pays nothing (tools/arrconv_ref.py: 612 converted corpus requests
  byte-identical). No crashes.
