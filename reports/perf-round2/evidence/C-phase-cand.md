| phase (min of runs, ms, PRoot) | small-files | solution | dense-file | many-callables | deep-nesting |
|---|---|---|---|---|---|
| snapshot_scan | 938.56 | 1122.63 | 727.55 | 449.1 | 188.28 |
| source_reread | 0.57 | 0.63 | 0.01 | 0.01 | 0.01 |
| verify_after_snapshot | - | - | - | - | - |
| tempdir | 0.36 | 0.42 | 0.23 | 0.21 | 0.21 |
| child_process | 34.81 | 321.31 | 54.89 | 56.52 | 38.72 |
| verify_after_command | 274.17 | 286.39 | 1.3 | 1.57 | 1.08 |
| find_reports | 0.96 | 45.15 | 0.49 | 0.88 | 1.33 |
| cobertura_import | 1008.86 | 2351.43 | 100.47 | 104.53 | 37.74 |
| &nbsp;&nbsp;cob_read | 3.0 | 86.67 | 14.46 | 12.81 | 3.54 |
| &nbsp;&nbsp;cob_lowercase | 0.64 | 1.88 | 2.79 | 2.7 | 1.03 |
| &nbsp;&nbsp;cob_dom | 17.72 | 19.95 | 54.53 | 51.61 | 22.01 |
| &nbsp;&nbsp;cob_classes | 981.45 | 2233.78 | 8.16 | 22.27 | 5.51 |
| evidence_write | 2.56 | 1.59 | 3.91 | 6.09 | 3.0 |
| evidence_fsync | 7.16 | 8.42 | 13.05 | 11.51 | 6.88 |
| verify_scan | 3.35 | 3.41 | 13.57 | 11.15 | 7.56 |
| verify_before_publish | 278.79 | 419.71 | 1.34 | 1.22 | 0.86 |
| publish | 0.4 | 0.45 | 0.62 | 0.97 | 0.41 |
| total | 2821.27 | 5071.54 | 958.77 | 731.77 | 323.94 |
| verify_sources(all) | 1057.97 | 1096.12 | 4.13 | 5.64 | 3.52 |
