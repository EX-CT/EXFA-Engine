# contract/ — engine interface contracts and JSON schemas

The engine owns its interface definitions. This directory collects the contract documents and JSON
schemas that previously lived in three different repositories; contents are copied verbatim.

| file | source | revision |
|---|---|---|
| `CONTRACT.md` | `EX-CT/eve-dogma-bench` branch `pending-1.11` @ `01e6724` (`CONTRACT.md`) | v1, rev 1.4.5 |
| `contract-v1.md` | `EX-CT/eve-dogma-rs` @ `d6043a7` (`docs/contract.md`) | v1, rev 1.4.3 |
| `CONTRACT-GRAPHS.md` | `EX-CT/eve-dogma-bench` branch `graphs-round2` @ `db81b8c` (`graphs/CONTRACT-GRAPHS.md`) | round 2, rev 0.3 |
| `schema/fit-request.schema.json` | `EX-CT/eve-fit-docs` @ `7fa6adb` (`schema/`) | — |
| `schema/fit-stats.schema.json` | `EX-CT/eve-fit-docs` @ `7fa6adb` (`schema/`) | — |

`CONTRACT.md` (the bench contract, latest revision) is the current authority for `calc`/`batch`/RPC
behaviour; `contract-v1.md` is the reference-engine revision it was based on; `CONTRACT-GRAPHS.md`
covers the `graph`/`graph_specs` methods. **The three contract documents are pending merge into a
single contract** (EXFA-Docs `docs/00-architecture-plan.md` §4.1); until then all three are kept.
