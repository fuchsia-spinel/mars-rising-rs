#!/usr/bin/env python3
"""Golden vectors for src/levelend.rs (the end-of-level coin bonus: setup 0x20764, per-frame state machine 0x208fc, drawing 0x20be0 with the number
drawers 0x21090 / 0x21150) by running the original code in ppcemu.py.

    python3 -I gen_levelend_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

State: the fields of the block D.1584..D.15d8 (hashed in the order of FIELDS), ship records 0xc458 (player 1) and 0xc2c0 (player 2) of which the routines read +0 (in game), +12 (lives)
and +320 (coins), globals tick D.13e4, players D.13e2, level D.13ec, coin style D.13d8, translucency D.12be.
Mocked callees (logged): sound 0x45bac, blitters 0x2f88 / 0x3170 / 0x3264 / 0x307c, score adders 0xe68c / 0xe6c0, coin converter 0x2b73c, 0x15f0 (port setup, no effect).

A vector line: `seed=.. -> tokens`; ops (separated by `;`):
  `P<players>,<level>,<coin style>,<translucent>`   set the globals       `H<ship 1|2>,<active>,<lives>,<coins>`   poke a ship record
  `T<tick>`   set the tick     `S`   setup 0x20764     `A`   one step 0x208fc     `D<points per coin>`   draw 0x20be0
token: `h<block hash>` then the logged calls, then `r<return value>` for `A`.
"""
import sys, os, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_enemy_vectors import fnv64, s32, M32

# the fields of the state block D.1584..D.15d8 as (address, width); the hash covers exactly these bytes (not the padding between them)
FIELDS = [(0x1584, 4), (0x1588, 4), (0x158c, 4), (0x1590, 1), (0x1594, 4), (0x1598, 4), (0x159c, 4), (0x15a0, 1), (0x15a4, 4), (0x15a8, 4), (0x15ac, 1)] + \
         [(0x15b0 + 4 * i, 4) for i in range(7)] + [(0x15cc, 4), (0x15d0, 1), (0x15d4, 4), (0x15d8, 1)]
SHIP = {1: 0xc458, 2: 0xc2c0}
TICK, PLAYERS, LEVEL, STYLE, TRAN = 0x13e4, 0x13e2, 0x13ec, 0x13d8, 0x12be


class Rig:
    def __init__(self, exe):
        self.cpu = c = ppcemu.load(exe)
        c.keep_toc = True
        self.log = []

        def logger(name, n):
            def f(cpu):
                self.log.append('%s_%s' % (name, '_'.join(str(s32(cpu.r[i])) for i in range(3, 3 + n))))
            return f

        for a in (0x2f88, 0x3170, 0x3264, 0x307c):
            c.mocks[a] = logger('d%x' % a, 7)
        c.mocks[0x45bac] = logger('snd', 3)
        c.mocks[0xe68c] = logger('sc1', 2)
        c.mocks[0xe6c0] = logger('sc2', 2)
        c.mocks[0x2b73c] = logger('conv', 0)
        c.mocks[0x15f0] = lambda cpu: None
        c.r[2] = 0x8000
        self.base = bytes(c.mem)

    def setup(self):
        self.cpu.mem[:] = self.base
        self.log = []

    def digest(self):
        return 'h%016x' % fnv64(b''.join(bytes(self.cpu.mem[a:a + n]) for a, n in FIELDS))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            a = [int(v) for v in arg.split(',')] if arg else []
            ret = None
            if kind == 'P':
                c.wb(PLAYERS, a[0]); c.ww(LEVEL, a[1]); c.wb(STYLE, a[2]); c.wb(TRAN, a[3])
            elif kind == 'H':
                base = SHIP[a[0]]
                c.wb(base, a[1]); c.ww(base + 12, a[2]); c.ww(base + 320, a[3])
            elif kind == 'T':
                c.ww(TICK, a[0])
            elif kind == 'S':
                c.call(0x20764, [SHIP[1], SHIP[2]])
            elif kind == 'A':
                ret = c.call(0x208fc, [SHIP[1], SHIP[2]]) & 0xff
            elif kind == 'D':
                c.call(0x20be0, [a[0] & M32])
            else:
                raise ValueError(op)
            tok = self.digest() + ''.join(',' + l for l in self.log)
            if ret is not None:
                tok += ',r%d' % ret
            toks.append(tok)
        return toks


def gen(exe, out, seed=1, n=40):
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    for it in range(n):
        rig.setup()
        players = rng.choice([1, 1, 2])
        ops = ['P%d,%d,%d,%d' % (players, rng.choice([1, 2, 3, 5, 9, 10, 11, 28, 40, 255]), rng.randint(0, 1), rng.randint(0, 1))]
        for s in (1, 2):
            ops.append('H%d,%d,%d,%d' % (s, rng.choice([0, 1, 1, 1]), rng.choice([-1, 0, 1, 3, 5]),
                                          rng.choice([0, 0, 1, 2, 5, 17, 40, 99, 100, 255])))
        t = rng.randint(0, 50)
        ops.append('T%d' % t)
        ops.append('S')
        for _ in range(rng.randint(60, 220)):
            t += rng.choice([0, 1, 1, 1, 2, 3, 4, 30, 44, 45, 46, 50, 59, 60, 61])
            ops.append('T%d' % t)
            ops.append('A')
            if rng.random() < 0.5:
                ops.append('D%d' % rng.choice([0, 100, 100, 500, 1000, 300, 2000, 12345]))
            if rng.random() < 0.03:
                s = rng.choice([1, 2])
                ops.append('H%d,%d,%d,%d' % (s, rng.randint(0, 1), rng.choice([-1, 0, 1, 3]), rng.choice([0, 3, 20, 99])))
            if rng.random() < 0.02:
                ops.append('S')
        toks = rig.run(ops)
        lines.append('seed=%d ops=%s -> %s' % (seed, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_levelend_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 40
    print(gen(exe, out, seed, n), 'vectors ->', out)
