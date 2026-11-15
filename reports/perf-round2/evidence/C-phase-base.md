| phase (min of runs, ms, PRoot) | small-files | solution | dense-file | many-callables | deep-nesting |
|---|---|---|---|---|---|
| snapshot_scan | 1100.44 | 1065.87 | 1081.58 | 635.04 | 152.71 |
| source_reread | 353.75 | 158.36 | 1.88 | 0.76 | 0.29 |
| verify_after_snapshot | 290.14 | 237.99 | 1.67 | 1.16 | 0.37 |
| tempdir | 0.25 | 0.25 | 0.4 | 0.28 | 0.19 |
| child_process | 28.52 | 256.5 | 48.88 | 58.83 | 37.21 |
| verify_after_command | 463.22 | 233.13 | 1.4 | 1.01 | 0.79 |
| find_reports | 1.19 | 47.87 | 0.66 | 0.82 | 0.45 |
| cobertura_import | 3478.17 | 2432.46 | 114.51 | 106.19 | 26.58 |
| &nbsp;&nbsp;cob_read | 2.42 | 74.57 | 16.09 | 10.16 | 2.6 |
| &nbsp;&nbsp;cob_lowercase | 1.44 | 2.11 | 7.31 | 4.19 | 1.34 |
| &nbsp;&nbsp;cob_dom | 15.51 | 23.51 | 54.9 | 51.87 | 15.81 |
| &nbsp;&nbsp;cob_classes | 3441.74 | 2298.96 | 15.16 | 26.33 | 3.92 |
| evidence_write | 36.46 | 43.79 | 169.03 | 91.94 | 57.29 |
| evidence_fsync | 2.58 | 2.89 | 10.12 | 7.09 | 5.55 |
| verify_scan | 2051.57 | 872.32 | 662.07 | 453.85 | 112.8 |
| verify_before_publish | 501.04 | 254.81 | 2.91 | 0.98 | 0.42 |
| publish | 0.41 | 0.61 | 0.44 | 0.57 | 0.25 |
| total | 11134.38 | 6407.04 | 2174.61 | 1457.79 | 439.42 |
| verify_sources(all) | 3724.42 | 1293.39 | 17.31 | 6.55 | 2.72 |
