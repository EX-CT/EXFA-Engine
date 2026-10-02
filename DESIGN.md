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

## Hot-path notes (measured with callgrind, instructions per rifter calc ≈ 1.2 M)
* Modifier targets filtered by location group or required skill come from per-fit sorted `(key, item)` indexes,
  instead of scanning all skills for every modifier.
* Attribute entries are created without a base value. The type's base or default is filled lazily on first read,
  and one probe both finds and inserts.
* Six-decimal numbers (almost all output) use a fast fixed-point formatter, which is proven equal to the shortest
  round-trip path for 1e-5 ≤ |v| < 1e9 and checked against it in `test/fmt_test.cpp` over 5 M random values.
  Everything else goes through `std::to_chars`.
* Capsim: an index heap with `t` inline, in-place event slots (no 64-byte event copies) and a memo of
  `exp(dt/tau)` for recurring time steps. The heap layout matches the reference's `BinaryHeap`, so the final
  avg-drain summation order is identical.

## Trade-offs
* Byte-for-byte reference fidelity was chosen over independent re-derivation from Pyfa. Every value matches
  Pyfa wherever the reference does (all 13 812 bench values today). The cost is that J inherits any
  divergence the reference has, and that new reference features must be ported (done up to eve-dogma-rs
  ae4bfb0).
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
