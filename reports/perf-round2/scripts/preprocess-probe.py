"""Prototype: blank inactive #if regions (keeping line structure) and report scan results.
usage: prep.py BIN REPO_DIR MODE   MODE = none | all  (symbols treated as all-undefined / all-defined)"""
import os, re, subprocess, sys, shutil, tempfile
BIN, REPO, MODE = sys.argv[1], sys.argv[2], sys.argv[3]
TOK = re.compile(r'\s*(\|\||&&|==|!=|!|\(|\)|[A-Za-z_][A-Za-z0-9_]*)')
def ev(expr):
    expr = expr.split('//')[0]
    toks = TOK.findall(expr); pos = [0]
    def peek(): return toks[pos[0]] if pos[0] < len(toks) else None
    def eat(): pos[0] += 1; return toks[pos[0]-1]
    def prim():
        t = eat()
        if t == '(':
            v = or_(); eat(); return v
        if t == '!': return not prim()
        if t == 'true': return True
        if t == 'false': return False
        return MODE == 'all'
    def eq():
        v = prim()
        while peek() in ('==', '!='):
            o = eat(); r = prim(); v = (v == r) if o == '==' else (v != r)
        return v
    def and_():
        v = eq()
        while peek() == '&&': eat(); r = eq(); v = v and r
        return v
    def or_():
        v = and_()
        while peek() == '||': eat(); r = and_(); v = v or r
        return v
    try: return or_()
    except Exception: return False
D = re.compile(rb'^[ \t]*#[ \t]*(if|elif|else|endif)\b(.*)$')
def blank(b): return bytes(c if c in (10, 13) else 32 for c in b)
def prep(data):
    out = []; stack = []  # (parent_active, taken, active)
    for line in data.split(b'\n'):
        m = D.match(line.rstrip(b'\r'))
        active = all(s[2] for s in stack)
        if m:
            k = m.group(1); e = m.group(2).decode('utf8', 'replace')
            if k == b'if':
                v = active and ev(e); stack.append([active, v, v])
            elif k == b'elif' and stack:
                s = stack[-1]; v = s[0] and not s[1] and ev(e); s[2] = v; s[1] = s[1] or v
            elif k == b'else' and stack:
                s = stack[-1]; v = s[0] and not s[1]; s[2] = v; s[1] = True
            elif k == b'endif' and stack: stack.pop()
            out.append(blank(line))
        else:
            out.append(line if active else blank(line))
    return b'\n'.join(out)
def scan(root, rel):
    r = subprocess.run([BIN, '--root', root, rel], capture_output=True, text=True)
    return r.returncode, r.stderr.strip()
tmp = tempfile.mkdtemp(dir=os.environ.get('PREP_TMP'))
n = bad = fixed = 0; still = []
for d, ds, fs in os.walk(REPO):
    ds[:] = [x for x in ds if x not in ('.git', 'bin', 'obj')]
    for f in fs:
        if not f.endswith('.cs'): continue
        p = os.path.join(d, f); rel = os.path.relpath(p, REPO); n += 1
        rc, err = scan(REPO, rel)
        if rc in (0, 1): continue
        bad += 1
        data = open(p, 'rb').read()
        q = os.path.join(tmp, 'f.cs'); open(q, 'wb').write(prep(data))
        rc2, err2 = scan(tmp, 'f.cs')
        if rc2 in (0, 1): fixed += 1
        else: still.append((rel, err[:80], b'#if' in data))
print(f'{REPO} mode={MODE}: files={n} failing={bad} fixed_by_preprocessing={fixed} still_failing={len(still)}')
for s in still[:25]: print('  STILL', s)
shutil.rmtree(tmp)
