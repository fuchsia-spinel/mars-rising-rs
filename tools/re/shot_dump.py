#!/usr/bin/env python3
"""Debug helper: replay line N of tests/vectors/enemy_shot_vectors.txt in the emulator up to op K and print the non-zero words of every pool record.
    python3 -I shot_dump.py MarsRising.pef LINE OPINDEX"""
import sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_shot_vectors as S
from gen_enemy_vectors import unsparse, SHIP_BYTES
exe, ln, k = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
line = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), '../../tests/vectors/enemy_shot_vectors.txt')).read().split('\n')[ln - 1]
lhs = line.split(' -> ')[0]
head, ops = lhs.split(' ops=')
kv = dict(t.split('=', 1) for t in head.split())
rig = S.ShotRig(exe)
ships = [unsparse(kv['S1'], SHIP_BYTES), unsparse(kv['S2'], SHIP_BYTES)]
rig.setup(int(kv['tick']), int(kv['np']), ships, [])
rig.run(ops.split(';')[:k + 1])
c = rig.cpu
for i in range(30):
    r = c.mem[S.POOLP + i * 80:S.POOLP + (i + 1) * 80]
    nz = ['%d=%02x%02x%02x%02x' % (j * 4, *r[j * 4:j * 4 + 4]) for j in range(20) if any(r[j * 4:j * 4 + 4])]
    if nz: print('rec', i, ' '.join(nz))
