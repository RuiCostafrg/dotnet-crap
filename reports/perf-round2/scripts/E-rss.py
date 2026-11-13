#!/usr/bin/env python3
"""E-rss.py N binA binB -- args...   alternating runs, prints median/min/max ru_maxrss KiB and cmp of stdout."""
import os, sys, statistics, hashlib, tempfile
n=int(sys.argv[1]); bins=[sys.argv[2],sys.argv[3]]; args=sys.argv[sys.argv.index("--")+1:]
res={0:[],1:[]}; hashes={}
for i in range(n):
    for b in ((0,1) if i%2==0 else (1,0)):
        f=tempfile.TemporaryFile()
        pid=os.fork()
        if pid==0:
            os.dup2(f.fileno(),1); os.execv(bins[b],[bins[b]]+args)
        _,st,u=os.wait4(pid,0)
        res[b].append(u.ru_maxrss); f.seek(0); hashes[b]=hashlib.sha256(f.read()).hexdigest(); f.close()
print("identical" if hashes[0]==hashes[1] else "DIFFERENT")
for b in (0,1):
    r=res[b]; print(os.path.basename(bins[b]), "median",statistics.median(r),"min",min(r),"max",max(r))
print("delta median KiB", statistics.median(res[1])-statistics.median(res[0]))
