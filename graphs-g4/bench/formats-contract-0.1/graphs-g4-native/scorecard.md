# Formats scorecard: G4

- contract: CONTRACT-FORMATS 0.1 (DRAFT)
- rows passed: **4781/4792** (99.77 %)
- error codes matching (informational): 0/42
- rows where Pyfa crashes (flagged, scored in 0.1): 6/8 passed
- wall time: 1.03 s

| group | pass | total | % |
|---|---|---|---|
| export | 3258 | 3260 | 99.9 |
| import | 1304 | 1304 | 100.0 |
| edge_export | 124 | 125 | 99.2 |
| edge | 95 | 103 | 92.2 |

| category | pass | total | legal-fit pass | error code ok | notes |
|---|---|---|---|---|---|
| edge:autodetect | 6 | 6 |  | 0/6 |  |
| edge:dna | 12 | 12 |  | 0/5 | pyfa_crash_pass 3, pyfa_crash_rows 3 |
| edge:eft | 26 | 29 |  | 0/5 | diff_expected_error 2, diff_modules 1 |
| edge:eftcfg | 2 | 2 |  | 0/2 |  |
| edge:esi | 10 | 11 |  | 0/5 | diff_expected_error 1, pyfa_crash_pass 0, pyfa_crash_rows 1 |
| edge:forced | 16 | 17 |  | 0/9 | diff_expected_error 1, info_notes 1, pyfa_crash_pass 3, pyfa_crash_rows 3 |
| edge:items | 8 | 8 |  |  |  |
| edge:multi | 7 | 7 |  |  | info_notes 1 |
| edge:mutated | 4 | 4 |  |  |  |
| edge:xml | 4 | 7 |  | 0/4 | diff_expected_error 3, pyfa_crash_pass 0, pyfa_crash_rows 1 |
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
| export:shipstats | 324 | 326 |  |  | diff_text 2 |
| export:xml | 326 | 326 |  |  |  |
| import:dna | 326 | 326 | 194/194 | 0/1 |  |
| import:eft | 326 | 326 | 194/194 |  |  |
| import:esi | 326 | 326 | 194/194 |  |  |
| import:xml | 326 | 326 | 194/194 |  |  |
