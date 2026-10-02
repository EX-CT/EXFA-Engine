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
  indexes for `search`/name lookup.
* The cache file is keyed by the dataset name and validated by source size plus a fast hash. The full SHA-256
  of the JSON is stored in the image and reported in `meta`. The file is written to a temp file and atomically
  renamed, so concurrent first runs are safe.
* At start-up the image is `mmap`ped read-only. No parsing or allocation happens, and pages are shared between
  processes and threads. This is why cold start is about 4 ms, against about 150–200 ms for engines that parse
  the JSON at start.
* Attribute lookups use binary search over a type's sorted (attr id, value) slice. These slices are short and
  cache-resident.

### Engine (src/engine.*)
* A direct port of eve-dogma-rs's lazy, memoised modifier graph. Items are the ship, character, skills,
  modules, charges, drones, fighters, implants, boosters, subsystems, the mode, environment and projected
  sources. Each fit has a local open-addressing hash table `(item, attr) → LAttr` that holds the base value,
  the memoised value, a dirty flag and the head of an intrusive linked list of modifiers (all in one arena).
* Values are computed on demand, with a cycle guard, Pyfa's operator order (PreAssign … PostAssign) and stacking
  penalties. An attribute the fit never touches falls back to the type's raw value without being materialised.
* The same feature set as the reference: projected modules, drones and whole fits (frozen into base values),
  fleet bursts, booster fits (strongest |value| per buff id), environment effects, the Reactive Armor
  Hardener simulation, damage patterns, reload, incoming remote reps, neuts, nos and cap transfers.
* All floating-point arithmetic follows the reference expression order, including Rust's
  `Iterator::sum` starting from −0.0 and `min_by`/`max_by` tie-breaking. This keeps outputs byte-identical.

### Capacitor simulation (src/capsim.*)
A port of the reference's event-driven Pyfa capsim. It uses a binary min-heap with the same ordering and tie
rules, plus the same stagger and clip semantics.

### Stats and output (src/stats.*, src/jsonw.hpp)
* Stats are computed in reference order, then emitted with a streaming writer that writes keys in sorted
  (BTreeMap) order. There is no DOM.
* Numbers are formatted like serde_json/ryu (shortest round-trip, `1.0` style for integral values) after the
  same `round6` step.
* Each thread has a `Worker` holding its own simdjson parser and output buffer. These are reused across
  requests, so a warmed-up calc does almost no allocation besides the Fit itself.

## Batch / threading
* `batch` reads all currently available stdin (blocking for the first chunk, then non-blocking while data is
  ready, up to 64 MiB). It splits the chunk into lines, then N worker threads pull line indices from an atomic
  counter. Results go into per-line slots and are written in input order, so the output is deterministic and
  identical to `--threads 1`.
* stdout is flushed whenever stdin has no more data waiting. Interactive or pipelined use (one request,
  wait for the answer) therefore works with no deadlock, and the full-throughput path only flushes per chunk.
* Default threads = `hardware_concurrency()`; `--threads N` overrides.
* The dataset is read-only and shared, and all mutable state is per Worker or per Fit. No locks are taken
  on the hot path.

## Trade-offs
* Byte-for-byte reference fidelity was chosen over independent re-derivation from Pyfa. Every value matches
  Pyfa wherever the reference does (all 13 812 bench values today). The cost is that J inherits any
  divergence the reference has, and that new reference features must be ported (done up to eve-dogma-rs
  0e5a1ce).
* The binary cache costs about 110 ms once per dataset and ~20 MB of disk. It can be disabled.
* The lazy evaluator only computes what the stats need. A full attribute dump (`type`) goes through the same
  path.
* EFT import/export commands are not implemented. The contract's request/response JSON is the interface.

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
