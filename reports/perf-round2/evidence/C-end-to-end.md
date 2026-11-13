| mode | workload | base median / min ms | candidate median / min ms | wall ratio | base / candidate CPU ms | CPU ratio | base / candidate peak RSS KiB | output identical |
|---|---|---|---|---|---|---|---|---|
| collect | deep-nesting | 595 / 525 | 380 / 320 | 0.638 | 452 / 250 | 0.553 | 40694 / 39222 | True |
| collect | dense-file | 1682 / 1477 | 910 / 848 | 0.541 | 1447 / 738 | 0.510 | 102736 / 98072 | True |
| collect | many-callables | 979 / 919 | 563 / 522 | 0.575 | 857 / 464 | 0.541 | 74718 / 71078 | True |
| collect | small-files | 5111 / 4402 | 3140 / 2105 | 0.614 | 1306 / 675 | 0.517 | 17200 / 16430 | True |
| collect | solution | 7273 / 5249 | 4265 / 3739 | 0.586 | 2489 / 1515 | 0.609 | 12262 / 11760 | True |
| scan | dense-file | 696 / 644 | 742 / 610 | 1.065 | 578 / 601 | 1.041 | 66404 / 66428 | True |
| scan | small-files | 1063 / 886 | 1237 / 777 | 1.164 | 433 / 452 | 1.045 | 10192 / 10244 | True |
