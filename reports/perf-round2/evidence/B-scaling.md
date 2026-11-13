heavy-64  runs=7
| config | wall med ms | wall min ms | speedup (med) | cpu med ms | peak RSS med KiB | identical |
|---|---|---|---|---|---|---|
| base | 1804 | 1635 | x1.00 | 1634 | 25936 | True |
| t=1 | 1847 | 1645 | x0.98 | 1717 | 27984 | True |
| t=2 | 1448 | 1362 | x1.25 | 2366 | 31324 | True |
| t=4 | 1419 | 1343 | x1.27 | 3735 | 37668 | True |
| t=8 | 1594 | 1550 | x1.13 | 6282 | 50440 | True |
skew  runs=7
| config | wall med ms | wall min ms | speedup (med) | cpu med ms | peak RSS med KiB | identical |
|---|---|---|---|---|---|---|
| base | 1805 | 1700 | x1.00 | 1634 | 50656 | True |
| t=1 | 1788 | 1684 | x1.01 | 1666 | 52980 | True |
| t=2 | 1646 | 1498 | x1.10 | 2532 | 53204 | True |
| t=4 | 1666 | 1493 | x1.08 | 3917 | 40520 | True |
| t=8 | 1960 | 1864 | x0.92 | 6157 | 52136 | True |
corpus  runs=7
| config | wall med ms | wall min ms | speedup (med) | cpu med ms | peak RSS med KiB | identical |
|---|---|---|---|---|---|---|
| base | 681 | 582 | x1.00 | 565 | 64140 | True |
| t=1 | 609 | 545 | x1.12 | 509 | 64224 | True |
| t=2 | 616 | 564 | x1.11 | 512 | 64156 | True |
| t=4 | 691 | 573 | x0.99 | 564 | 64180 | True |
| t=8 | 729 | 582 | x0.93 | 579 | 64212 | True |
corpus  runs=5
| config | wall med ms | wall min ms | speedup (med) | cpu med ms | peak RSS med KiB | identical |
|---|---|---|---|---|---|---|
| base | 1407 | 1120 | x1.00 | 511 | 11196 | True |
| t=1 | 1412 | 1139 | x1.00 | 530 | 11196 | True |
| t=2 | 1003 | 895 | x1.40 | 489 | 11196 | True |
| t=4 | 729 | 516 | x1.93 | 524 | 11196 | True |
| t=8 | 571 | 459 | x2.46 | 540 | 11196 | True |
