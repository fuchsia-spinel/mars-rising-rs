#!/usr/bin/env python3
"""Debug aid: replay one vector of ground_vectors.txt in the original code and write the pool / decals after op K.
    python3 -I ground_dump.py EXE VECTORS LINE OPINDEX OUT_PREFIX     (LINE is 1-based as in the test's failure message; OPINDEX 0-based)"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_ground_vectors as g
from gen_enemy_vectors import unsparse, SHIP_BYTES

exe, vec, ln, k, pre = sys.argv[1], sys.argv[2], int(sys.argv[3]), int(sys.argv[4]), sys.argv[5]
line = open(vec).read().split('\n')[ln - 1]
lhs, _ = line.split(' -> ')
head, ops = lhs.split(' ops=')
kv = dict(t.split('=', 1) for t in head.split())
rig = g.Rig(exe)
w, h = int(kv['W']), int(kv['H'])
marker = bytes.fromhex(kv['M'])
res = bytes(unsparse(kv['G'], 250 * g.RES_REC))
ships = [unsparse(kv['S1'], SHIP_BYTES), unsparse(kv['S2'], SHIP_BYTES)]
rq = [int(v, 16) for v in kv['R'].split(',')]
shotq = [int(v) for v in kv['Q'].split(',')] if kv['Q'] else []
rig.setup(int(kv['tick']), int(kv['np']), int(kv['dif']), w, h, marker, res, ships, rq, shotq)
oplist = ops.split(';')
if k > 0:
    rig.run(oplist[:k])
rig.log = []
toks = rig.run(oplist[k:k + 1])
c = rig.cpu
open(pre + '.pool', 'wb').write(bytes(c.mem[g.POOL:g.POOL + g.NREC * g.REC]))
open(pre + '.decals', 'wb').write(bytes(c.mem[g.STAT:g.STAT + 1600]))
open(pre + '.list', 'wb').write(bytes(c.mem[g.ALIST:g.ALIST + 200]))
print(toks[0][:300])
