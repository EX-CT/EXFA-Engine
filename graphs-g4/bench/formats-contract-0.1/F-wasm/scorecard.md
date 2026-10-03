# Formats scorecard: F-wasm

- contract: CONTRACT-FORMATS 0.1 (rulings 2026-10-03) (DRAFT)
- **score (4 groups x 25 %): 98.46 %**
- scored rows passed: **4773/4779** (99.87 %) - gate not met
- error codes matching (informational): 0/34
- report-only rows (not scored) agreeing with Pyfa: 9/14
- wall time: 1.28 s

| group | weight | pass | scored rows | % | report-only rows |
|---|---|---|---|---|---|
| export | 25 % | 3257 | 3257 | 100.00 | 3 |
| import | 25 % | 1304 | 1304 | 100.00 | 0 |
| edge_export | 25 % | 124 | 125 | 99.20 | 0 |
| edge | 25 % | 88 | 93 | 94.62 | 11 |

| category | pass | total | legal-fit pass | error code ok | notes |
|---|---|---|---|---|---|
| edge:autodetect | 6 | 6 |  | 0/6 |  |
| edge:dna | 9 | 9 |  | 0/2 | unscored_agree 3, unscored_rows 3 |
| edge:eft | 27 | 29 |  | 0/5 | diff_expected_error 2, unscored_agree 0, unscored_rows 1 |
| edge:eftcfg | 2 | 2 |  | 0/2 |  |
| edge:esi | 10 | 10 |  | 0/4 | unscored_agree 0, unscored_rows 1 |
| edge:forced | 13 | 14 |  | 0/6 | diff_expected_error 1, info_notes 1, unscored_agree 3, unscored_rows 3 |
| edge:items | 8 | 8 |  |  |  |
| edge:multi | 5 | 5 |  |  | info_notes 1, unscored_agree 2, unscored_rows 2 |
| edge:mutated | 4 | 4 |  |  |  |
| edge:xml | 4 | 6 |  | 0/3 | diff_expected_error 2, unscored_agree 0, unscored_rows 1 |
| edge_export:dna | 9 | 9 |  |  |  |
| edge_export:dna_formatted | 9 | 9 |  |  |  |
| edge_export:eft | 9 | 9 |  |  |  |
| edge_export:eft_min | 9 | 9 |  |  |  |
| edge_export:esi | 9 | 9 |  | 0/1 |  |
| edge_export:esi_min | 9 | 9 |  | 0/1 |  |
| edge_export:import_dna | 9 | 9 |  |  |  |
| edge_export:import_eft | 9 | 9 |  | 0/1 |  |
| edge_export:import_esi | 8 | 8 |  |  |  |
| edge_export:import_xml | 8 | 9 |  |  | diff_name 1 |
| edge_export:multibuy | 9 | 9 |  |  |  |
| edge_export:multibuy_min | 9 | 9 |  |  |  |
| edge_export:shipstats | 9 | 9 |  |  |  |
| edge_export:xml | 9 | 9 |  |  |  |
| export:dna | 326 | 326 |  |  |  |
| export:dna_formatted | 326 | 326 |  |  |  |
| export:eft | 326 | 326 |  |  | info_known_divergence_subsystem_slot 21 |
| export:eft_min | 326 | 326 |  |  | info_known_divergence_subsystem_slot 21 |
| export:esi | 326 | 326 |  |  |  |
| export:esi_min | 326 | 326 |  | 0/2 |  |
| export:multibuy | 326 | 326 |  |  |  |
| export:multibuy_min | 326 | 326 |  |  |  |
| export:shipstats | 323 | 323 |  |  | unscored_agree 1, unscored_rows 3 |
| export:xml | 326 | 326 |  |  |  |
| import:dna | 326 | 326 | 194/194 | 0/1 |  |
| import:eft | 326 | 326 | 194/194 |  |  |
| import:esi | 326 | 326 | 194/194 |  |  |
| import:xml | 326 | 326 | 194/194 |  |  |
