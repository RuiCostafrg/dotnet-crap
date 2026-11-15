import subprocess, sys, os, tempfile
BIN, F = sys.argv[1], sys.argv[2]
lines = open(F, 'rb').read().split(b'\n')
d = tempfile.mkdtemp(dir=os.path.dirname(BIN))
def ok(ls):
    open(os.path.join(d, 'f.cs'), 'wb').write(b'\n'.join(ls))
    return subprocess.run([BIN, '--root', d, 'f.cs'], capture_output=True).returncode in (0, 1)
hits = [i for i in range(len(lines)) if lines[i].strip() and ok(lines[:i] + [b''] + lines[i+1:])]
print(F, 'single-line culprits:', len(hits))
for i in hits[:4]: print('  ', i + 1, lines[i][:160])
