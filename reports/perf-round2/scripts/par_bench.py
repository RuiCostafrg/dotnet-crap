#!/usr/bin/env python3
"""par_bench.py RUNS BASE CAND CORPUS [--coverage F] : alternating runs of base and cand at several thread caps.
Prints median/min wall ms, median cpu ms (user+sys), median peak RSS KiB, and stdout identity vs base."""
import hashlib, os, statistics as st, sys, tempfile, time
runs = int(sys.argv[1]); base, cand, corpus = sys.argv[2:5]
extra = sys.argv[5:]
args = ["--root", corpus, "--format", "json", "--threshold", "1000000000000"] + extra
configs = [("base", base, None)] + [(f"t={t}", cand, str(t)) for t in (1, 2, 4, 8)]
res = {c[0]: {"wall": [], "cpu": [], "rss": [], "hash": set()} for c in configs}
def one(name, binary, threads):
    env = dict(os.environ)
    env.pop("DOTNET_CRAP_THREADS", None)
    if threads: env["DOTNET_CRAP_THREADS"] = threads
    f = tempfile.TemporaryFile()
    t0 = time.perf_counter()
    pid = os.fork()
    if pid == 0:
        os.dup2(f.fileno(), 1)
        os.execve(binary, [binary] + args, env)
    _, status, ru = os.wait4(pid, 0)
    wall = (time.perf_counter() - t0) * 1000
    f.seek(0)
    r = res[name]
    r["wall"].append(wall); r["cpu"].append((ru.ru_utime + ru.ru_stime) * 1000); r["rss"].append(ru.ru_maxrss)
    r["hash"].add((status, hashlib.sha256(f.read()).hexdigest()))
    f.close()
for c in configs: one(*c)  # warm-up, discarded
for r in res.values():
    for k in ("wall", "cpu", "rss"): r[k].clear()
for i in range(runs):
    order = configs if i % 2 == 0 else configs[::-1]
    for c in order: one(*c)
bh = res["base"]["hash"]
bw = st.median(res["base"]["wall"])
print(f"{os.path.basename(corpus.rstrip('/'))} {' '.join(extra)} runs={runs}")
print("| config | wall med ms | wall min ms | speedup (med) | cpu med ms | peak RSS med KiB | identical |")
print("|---|---|---|---|---|---|---|")
for name, _, _ in configs:
    r = res[name]
    w = st.median(r["wall"])
    print(f"| {name} | {w:.0f} | {min(r['wall']):.0f} | x{bw / w:.2f} | {st.median(r['cpu']):.0f} | {st.median(r['rss']):.0f} | {r['hash'] == bh and len(bh) == 1} |")
