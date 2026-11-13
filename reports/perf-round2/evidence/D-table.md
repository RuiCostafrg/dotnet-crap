| variant | setting | size (bytes) | sha256 (16) | output identical | build time |
|---|---|---|---|---|---|
| cgu1 | codegen-units=1 | 7170376 | 981e21a68e0ae349 | yes | 65.7 s |
| thinlto | lto=thin | 7365864 | 5a1715f1f626fdb6 | yes | 104.8 s |
| fatlto | lto=fat, codegen-units=1 | 6816096 | 0febbea620d5699e | yes | 134.8 s |
| fatlto-abort | fat LTO + panic=abort | 6672696 | 4fe57b27a06d3513 | yes | 123.2 s |
| fatlto-strip | fat LTO + strip=symbols | 6498248 | 262d0f7c884186c6 | yes | 148.5 s |
| opt2 | opt-level=2 | 7473256 | 8145be4458b6dc48 | yes | 87.2 s |
| opts | opt-level=s | 7631016 | f3200cf0eaaa9c41 | yes | 101.7 s |
| mimalloc | mimalloc global allocator (fat LTO not applied; default profile) | n/a | n/a | n/a | n/a |

| variant | workload | base median / min ms | candidate median / min ms | ratio (med) | separated | peak RSS KiB base / cand | identical |
|---|---|---|---|---|---|---|---|
| cgu1 | dense-file scan | 1058 / 669 | 1247 / 624 | 1.179 | no | 66370 / 63980 | True |
| cgu1 | dense-file scan-coverage | 989 / 737 | 967 / 757 | 0.977 | no | 68554 / 68528 | True |
| cgu1 | many-callables scan | 682 / 505 | 630 / 501 | 0.924 | no | 47176 / 47262 | True |
| cgu1 | small-files scan | 1580 / 915 | 1341 / 924 | 0.849 | no | 10148 / 10172 | True |
| thinlto | dense-file scan | 1469 / 750 | 1452 / 741 | 0.988 | no | 63928 / 63780 | True |
| thinlto | dense-file scan-coverage | 1169 / 767 | 1138 / 836 | 0.974 | no | 68492 / 67154 | True |
| thinlto | many-callables scan | 564 / 471 | 540 / 443 | 0.957 | no | 47126 / 47094 | True |
| thinlto | small-files scan | 1079 / 877 | 960 / 841 | 0.890 | no | 10110 / 10080 | True |
| fatlto | dense-file scan | 794 / 721 | 841 / 699 | 1.060 | no | 66284 / 65984 | True |
| fatlto | dense-file scan-coverage | 816 / 707 | 816 / 758 | 1.000 | no | 68534 / 68346 | True |
| fatlto | many-callables scan | 548 / 447 | 511 / 435 | 0.932 | no | 47090 / 46702 | True |
| fatlto | small-files scan | 1046 / 822 | 960 / 822 | 0.917 | no | 10052 / 9862 | True |
| fatlto-abort | dense-file scan | not run | | | | | |
| fatlto-abort | dense-file scan-coverage | not run | | | | | |
| fatlto-abort | many-callables scan | not run | | | | | |
| fatlto-abort | small-files scan | not run | | | | | |
| fatlto-strip | dense-file scan | not run | | | | | |
| fatlto-strip | dense-file scan-coverage | not run | | | | | |
| fatlto-strip | many-callables scan | not run | | | | | |
| fatlto-strip | small-files scan | not run | | | | | |
| opt2 | dense-file scan | not run | | | | | |
| opt2 | dense-file scan-coverage | not run | | | | | |
| opt2 | many-callables scan | not run | | | | | |
| opt2 | small-files scan | not run | | | | | |
| opts | dense-file scan | not run | | | | | |
| opts | dense-file scan-coverage | not run | | | | | |
| opts | many-callables scan | not run | | | | | |
| opts | small-files scan | not run | | | | | |
| mimalloc | dense-file scan | not run | | | | | |
| mimalloc | dense-file scan-coverage | not run | | | | | |
| mimalloc | many-callables scan | not run | | | | | |
| mimalloc | small-files scan | not run | | | | | |
