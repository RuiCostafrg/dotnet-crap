## per-stage (cumulative keep-alive) — RSS/HWM from unhooked run (MiB), Rust live (MB, counting allocator), C live (MB, tree-sitter allocator)

### dense-file-1x  INFO files=1 items=4000 src_bytes=556000 nodes_total=287001 nodes_named=177501 max_depth=8
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 4.2 | 4.2 | 1.1 | 1.1 | 0.0 | 0.0 | 18 |
| 3-parse | 56.4 | 56.4 | 1.1 | 1.1 | 41.1 | 41.7 | 456544 |
| 3b-coverage-index | 56.4 | 56.4 | 1.1 | 1.1 | 41.1 | 41.7 | 456544 |
| 4-collect-items | 57.9 | 57.9 | 1.8 | 1.8 | 41.1 | 138.3 | 834046 |
| 5-report+verify | 58.5 | 58.0 | 1.8 | 2.3 | 41.1 | 138.3 | 834046 |
| 6a-json-string | 59.2 | 59.2 | 2.8 | 2.8 | 41.1 | 138.3 | 834046 |
| 6b-format-copy | 61.0 | 60.2 | 4.6 | 4.6 | 41.1 | 138.3 | 834046 |
| 7-write | 61.0 | 60.2 | 4.6 | 4.6 | 41.1 | 138.3 | 834046 |
| 8-drop-json | 61.0 | 58.5 | 1.8 | 4.6 | 41.1 | 138.3 | 834046 |
| 9-parser-dropped | 61.0 | 58.5 | 1.8 | 4.6 | 41.1 | 138.3 | 834050 |
| 10-trees-dropped | 61.0 | 59.4 | 0.7 | 4.6 | 0.0 | 138.3 | 834053 |
| 11-report-dropped | 61.0 | 59.4 | 0.0 | 4.6 | 0.0 | 138.3 | 834053 |

### dense-file-2x  INFO files=1 items=8000 src_bytes=1112000 nodes_total=574001 nodes_named=355001 max_depth=8
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 5.3 | 5.3 | 2.2 | 2.2 | 0.0 | 0.0 | 18 |
| 3-parse | 107.3 | 107.3 | 2.2 | 2.2 | 82.1 | 83.4 | 913045 |
| 3b-coverage-index | 107.3 | 107.3 | 2.2 | 2.2 | 82.1 | 83.4 | 913045 |
| 4-collect-items | 110.3 | 110.3 | 3.5 | 3.6 | 82.1 | 276.7 | 1668047 |
| 5-report+verify | 111.4 | 110.4 | 3.5 | 4.6 | 82.1 | 276.7 | 1668047 |
| 6a-json-string | 112.1 | 111.9 | 5.6 | 5.6 | 82.1 | 276.7 | 1668047 |
| 6b-format-copy | 115.4 | 113.9 | 9.3 | 9.3 | 82.1 | 276.7 | 1668047 |
| 7-write | 115.4 | 113.9 | 9.3 | 9.3 | 82.1 | 276.7 | 1668047 |
| 8-drop-json | 115.4 | 110.4 | 3.5 | 9.3 | 82.1 | 276.7 | 1668047 |
| 9-parser-dropped | 115.4 | 110.4 | 3.5 | 9.3 | 82.1 | 276.7 | 1668051 |
| 10-trees-dropped | 115.4 | 20.8 | 1.3 | 9.3 | 0.0 | 276.7 | 1668054 |
| 11-report-dropped | 115.4 | 19.4 | 0.0 | 9.3 | 0.0 | 276.7 | 1668054 |

### dense-file-4x  INFO files=1 items=16000 src_bytes=2224000 nodes_total=1148001 nodes_named=710001 max_depth=8
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 7.4 | 7.4 | 4.4 | 4.5 | 0.0 | 0.0 | 18 |
| 3-parse | 209.1 | 209.1 | 4.4 | 4.5 | 164.2 | 166.8 | 1826046 |
| 3b-coverage-index | 209.1 | 209.1 | 4.4 | 4.5 | 164.2 | 166.8 | 1826046 |
| 4-collect-items | 214.0 | 214.0 | 7.1 | 7.2 | 164.2 | 553.4 | 3336049 |
| 5-report+verify | 216.2 | 214.1 | 7.1 | 9.3 | 164.2 | 553.4 | 3336049 |
| 6a-json-string | 217.9 | 217.4 | 11.3 | 11.3 | 164.2 | 553.4 | 3336049 |
| 6b-format-copy | 224.5 | 221.1 | 18.6 | 18.6 | 164.2 | 553.4 | 3336049 |
| 7-write | 224.5 | 221.1 | 18.6 | 18.6 | 164.2 | 553.4 | 3336049 |
| 8-drop-json | 224.5 | 214.1 | 7.1 | 18.6 | 164.2 | 553.4 | 3336049 |
| 9-parser-dropped | 224.5 | 214.1 | 7.1 | 18.6 | 164.2 | 553.4 | 3336053 |
| 10-trees-dropped | 224.5 | 35.3 | 2.6 | 18.6 | 0.0 | 553.4 | 3336056 |
| 11-report-dropped | 224.5 | 32.1 | 0.0 | 18.6 | 0.0 | 553.4 | 3336056 |

### many-callables-1x  INFO files=1 items=8000 src_bytes=372890 nodes_total=171001 nodes_named=104001 max_depth=11
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 3.9 | 3.9 | 0.7 | 0.7 | 0.0 | 0.0 | 18 |
| 3-parse | 34.1 | 34.1 | 0.7 | 0.7 | 22.7 | 24.5 | 268049 |
| 3b-coverage-index | 34.1 | 34.1 | 0.7 | 0.7 | 22.7 | 24.5 | 268049 |
| 4-collect-items | 37.1 | 37.1 | 2.1 | 2.1 | 22.7 | 76.2 | 470051 |
| 5-report+verify | 37.4 | 37.4 | 2.1 | 2.5 | 22.7 | 76.2 | 470051 |
| 6a-json-string | 39.3 | 39.0 | 4.2 | 4.2 | 22.7 | 76.2 | 470051 |
| 6b-format-copy | 42.6 | 41.0 | 7.9 | 7.9 | 22.7 | 76.2 | 470051 |
| 7-write | 42.6 | 41.0 | 7.9 | 7.9 | 22.7 | 76.2 | 470051 |
| 8-drop-json | 42.6 | 37.5 | 2.1 | 7.9 | 22.7 | 76.2 | 470051 |
| 9-parser-dropped | 42.6 | 37.5 | 2.1 | 7.9 | 22.7 | 76.2 | 470055 |
| 10-trees-dropped | 42.6 | 38.5 | 1.3 | 7.9 | 0.0 | 76.2 | 470058 |
| 11-report-dropped | 42.6 | 37.8 | 0.0 | 7.9 | 0.0 | 76.2 | 470058 |

### many-callables-2x  INFO files=1 items=16000 src_bytes=746890 nodes_total=342001 nodes_named=208001 max_depth=11
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 4.7 | 4.7 | 1.5 | 1.5 | 0.0 | 0.0 | 18 |
| 3-parse | 62.0 | 62.0 | 1.5 | 1.5 | 45.5 | 49.0 | 536050 |
| 3b-coverage-index | 62.0 | 62.0 | 1.5 | 1.5 | 45.5 | 49.0 | 536050 |
| 4-collect-items | 67.0 | 67.0 | 4.2 | 4.3 | 45.5 | 152.5 | 940053 |
| 5-report+verify | 67.7 | 67.2 | 4.2 | 4.9 | 45.5 | 152.5 | 940053 |
| 6a-json-string | 71.0 | 70.5 | 8.4 | 8.4 | 45.5 | 152.5 | 940053 |
| 6b-format-copy | 77.7 | 74.3 | 15.8 | 15.8 | 45.5 | 152.5 | 940053 |
| 7-write | 77.7 | 74.3 | 15.8 | 15.8 | 45.5 | 152.5 | 940053 |
| 8-drop-json | 77.7 | 67.2 | 4.2 | 15.8 | 45.5 | 152.5 | 940053 |
| 9-parser-dropped | 77.7 | 67.2 | 4.2 | 15.8 | 45.5 | 152.5 | 940057 |
| 10-trees-dropped | 77.7 | 17.7 | 2.7 | 15.8 | 0.0 | 152.5 | 940060 |
| 11-report-dropped | 77.7 | 14.4 | 0.0 | 15.8 | 0.0 | 152.5 | 940060 |

### many-callables-4x  INFO files=1 items=32000 src_bytes=1494890 nodes_total=684001 nodes_named=416001 max_depth=11
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.2 | 2.2 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 6.0 | 6.0 | 3.0 | 3.0 | 0.0 | 0.0 | 18 |
| 3-parse | 118.4 | 118.4 | 3.0 | 3.0 | 91.0 | 98.1 | 1072051 |
| 3b-coverage-index | 118.4 | 118.4 | 3.0 | 3.0 | 91.0 | 98.1 | 1072051 |
| 4-collect-items | 127.3 | 127.3 | 8.3 | 8.6 | 91.0 | 304.9 | 1880054 |
| 5-report+verify | 128.7 | 127.4 | 8.3 | 9.8 | 91.0 | 304.9 | 1880054 |
| 6a-json-string | 135.3 | 134.2 | 16.7 | 16.7 | 91.0 | 304.9 | 1880054 |
| 6b-format-copy | 148.5 | 141.6 | 31.6 | 31.6 | 91.0 | 304.9 | 1880054 |
| 7-write | 148.5 | 141.6 | 31.6 | 31.6 | 91.0 | 304.9 | 1880054 |
| 8-drop-json | 148.5 | 127.4 | 8.3 | 31.6 | 91.0 | 304.9 | 1880054 |
| 9-parser-dropped | 148.5 | 127.4 | 8.3 | 31.6 | 90.9 | 304.9 | 1880058 |
| 10-trees-dropped | 148.5 | 28.4 | 5.3 | 31.6 | 0.0 | 304.9 | 1880061 |
| 11-report-dropped | 148.5 | 21.7 | 0.0 | 31.6 | 0.0 | 304.9 | 1880061 |

### deep-nesting-1x  INFO files=1 items=3001 src_bytes=158863 nodes_total=66030 nodes_named=38018 max_depth=15007
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 3.5 | 3.5 | 0.3 | 0.3 | 0.0 | 0.0 | 18 |
| 3-parse | 20.6 | 20.6 | 0.3 | 0.3 | 8.1 | 13.2 | 114082 |
| 3b-coverage-index | 20.6 | 20.6 | 0.3 | 0.3 | 8.1 | 13.2 | 114082 |
| 4-collect-items | 22.2 | 22.2 | 1.0 | 1.1 | 8.1 | 33.2 | 192116 |
| 5-report+verify | 22.2 | 22.2 | 1.0 | 1.1 | 8.1 | 33.2 | 192116 |
| 6a-json-string | 23.3 | 23.3 | 2.0 | 2.0 | 8.1 | 33.2 | 192116 |
| 6b-format-copy | 24.7 | 24.2 | 3.4 | 3.4 | 8.1 | 33.2 | 192116 |
| 7-write | 24.7 | 24.2 | 3.4 | 3.4 | 8.1 | 33.2 | 192116 |
| 8-drop-json | 24.7 | 22.8 | 1.0 | 3.4 | 8.1 | 33.2 | 192116 |
| 9-parser-dropped | 24.7 | 22.8 | 1.0 | 3.4 | 8.0 | 35.3 | 192129 |
| 10-trees-dropped | 24.7 | 22.9 | 0.6 | 3.4 | 0.0 | 35.5 | 192141 |
| 11-report-dropped | 24.7 | 23.2 | 0.0 | 3.4 | 0.0 | 35.5 | 192141 |

### deep-nesting-2x  INFO files=1 items=6001 src_bytes=281863 nodes_total=120030 nodes_named=68018 max_depth=30007
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.4 | 2.4 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.4 | 2.4 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.4 | 2.4 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 4.0 | 4.0 | 0.6 | 0.6 | 0.0 | 0.0 | 18 |
| 3-parse | 33.4 | 33.4 | 0.6 | 0.6 | 14.1 | 24.2 | 204083 |
| 3b-coverage-index | 33.4 | 33.4 | 0.6 | 0.6 | 14.1 | 24.2 | 204083 |
| 4-collect-items | 36.0 | 36.0 | 1.9 | 2.0 | 14.1 | 59.6 | 342117 |
| 5-report+verify | 36.4 | 36.4 | 1.9 | 2.1 | 14.1 | 59.6 | 342117 |
| 6a-json-string | 38.3 | 37.8 | 4.0 | 4.0 | 14.1 | 59.6 | 342117 |
| 6b-format-copy | 40.5 | 39.3 | 6.8 | 6.8 | 14.1 | 59.6 | 342117 |
| 7-write | 40.5 | 39.3 | 6.8 | 6.8 | 14.1 | 59.6 | 342117 |
| 8-drop-json | 40.5 | 36.5 | 1.9 | 6.8 | 14.1 | 59.6 | 342117 |
| 9-parser-dropped | 40.5 | 36.5 | 1.9 | 6.8 | 13.8 | 63.7 | 342131 |
| 10-trees-dropped | 40.5 | 37.1 | 1.3 | 6.8 | 0.0 | 64.3 | 342144 |
| 11-report-dropped | 40.5 | 36.6 | 0.0 | 6.8 | 0.0 | 64.3 | 342144 |

### deep-nesting-4x  INFO files=1 items=12001 src_bytes=529864 nodes_total=228030 nodes_named=128018 max_depth=60007
| stage | HWM MiB | RSS MiB | Rust live MB | Rust peak MB | C live MB | C total-alloc MB | C allocs |
|---|---|---|---|---|---|---|---|
| 0-start | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1-coverage-json-read | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 0 |
| 1b-parser-created | 2.3 | 2.3 | 0.0 | 0.0 | 0.0 | 0.0 | 10 |
| 2-read+hash+decode | 4.2 | 4.2 | 1.1 | 1.1 | 0.0 | 0.0 | 18 |
| 3-parse | 58.5 | 58.5 | 1.1 | 1.1 | 26.0 | 46.2 | 384084 |
| 3b-coverage-index | 58.5 | 58.5 | 1.1 | 1.1 | 26.0 | 46.2 | 384084 |
| 4-collect-items | 62.2 | 62.2 | 3.7 | 3.8 | 26.0 | 112.3 | 642118 |
| 5-report+verify | 62.5 | 62.3 | 3.7 | 4.2 | 26.0 | 112.3 | 642118 |
| 6a-json-string | 66.1 | 64.8 | 7.8 | 7.8 | 26.0 | 112.3 | 642118 |
| 6b-format-copy | 70.3 | 67.8 | 13.6 | 13.6 | 26.0 | 112.3 | 642118 |
| 7-write | 70.3 | 67.8 | 13.6 | 13.6 | 26.0 | 112.3 | 642118 |
| 8-drop-json | 70.3 | 62.3 | 3.7 | 13.6 | 26.0 | 112.3 | 642118 |
| 9-parser-dropped | 70.3 | 42.6 | 3.7 | 13.6 | 25.4 | 120.7 | 642133 |
| 10-trees-dropped | 70.3 | 14.4 | 2.6 | 13.6 | 0.0 | 121.7 | 642147 |
| 11-report-dropped | 70.3 | 11.7 | 0.0 | 13.6 | 0.0 | 121.7 | 642147 |
