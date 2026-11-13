#!/usr/bin/env python3
"""multi_proc.py BIN CORPUS : run K independent copies at once; report per-process cpu ms and total wall."""
import os, sys, time, statistics as st
binary, corpus = sys.argv[1:3]
args = [binary, "--root", corpus, "--format", "json", "--threshold", "1000000000000"]
dn = os.open("/dev/null", os.O_WRONLY)
for k in (1, 2, 4, 8):
    rows = []
    for rep in range(4):
        t0 = time.perf_counter(); pids = []
        for _ in range(k):
            pid = os.fork()
            if pid == 0:
                os.dup2(dn, 1); os.execv(binary, args)
            pids.append(pid)
        cpus = []
        for pid in pids:
            _, _, ru = os.wait4(pid, 0); cpus.append((ru.ru_utime + ru.ru_stime) * 1000)
        rows.append(((time.perf_counter() - t0) * 1000, st.mean(cpus), max(cpus)))
    print(f"k={k} wall med {st.median(r[0] for r in rows):.0f} ms | per-process cpu mean {st.median(r[1] for r in rows):.0f} ms, max {st.median(r[2] for r in rows):.0f} ms")
