#!/usr/bin/env python3
"""Debug helper: replay one line of tests/vectors/enemy_vectors.txt in the emulator up to op K and print the pool/group words that are non-zero.
    python3 -I enemy_dump.py MarsRising.pef VECTORS LINE(1-based) K
"""
import sys, os, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_enemy_vectors as G

exe, path, ln, k = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4])
line = open(path).read().splitlines()[ln - 1]
lhs = line.split(' -> ')[0]
head, ops = lhs.split(' ops=')
kv = dict(t.split('=', 1) for t in head.split())
rig = G.Rig(exe)
groups = G.unsparse(kv['G'], G.NGROUPS * G.GREC)
ships = [G.unsparse(kv['S1'], G.SHIP_BYTES), G.unsparse(kv['S2'], G.SHIP_BYTES)]
rq = [int(x, 16) for x in kv['R'].split(',')]
sq = [int(x) for x in kv['Q'].split(',')] if kv['Q'] else []
rig.setup(int(kv['tick']), int(kv['np']), groups, ships, rq, sq, int(kv['dif']))
ops = ops.split(';')
for i, op in enumerate(ops[:k + 1]):
    toks = rig.run([op])
print('op', k, ops[k], toks[0][:200] if k < len(ops) else '')
c = rig.cpu
for s in range(40):
    w = [(o, struct.unpack('>I', bytes(c.mem[G.POOL + s * 176 + o:G.POOL + s * 176 + o + 4]))[0]) for o in range(0, 176, 4)]
    nz = [(o, v) for o, v in w if v]
    if nz: print('slot', s, ' '.join('%d=%08x' % x for x in nz))
