#!/usr/bin/env python3
"""Golden vectors for src/bomb.rs (the button-2 weapon that drops bombs on ground targets) by running the original code in ppcemu.py.

    python3 -I gen_bomb_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

Routines: button-2 control 0x26b58(ship), launch 0x201c0(owner), update 0x203a4, draw 0x205ec, clear 0x2015c. State: pool 0xcb74 (30 x 44 bytes), counter D.157c,
the two ship structures (408 bytes at 0xc458 = player 1 and 0xc2c0 = player 2), tick D.13e4, scroll step D.13c4, options D.12bc (shadows) / D.12be / D.13c8.
Mocked (logged) callees: random 0xa0d4, sound 0x45bac, effect 0x22670, ground damage 0x3fec8 (result from a queue), decal 0x40954, blitters 0x344c / 0x3170 / 0x2f88.

A vector line: `seed=.. T=<tick> S1=<ship words> S2=<ship words> R=<random queue> Q=<ground results> ops=.. -> tokens`; ops:
  `S<n>` button-2 control of ship n   `U<tick>,<scroll>` update   `D<tick>,<shadows>,<trans>,<fancy>` draw   `Z` clear   `W<slot>,<off>,<hex>` poke a record word
  `X<n>,<off>,<hex>,<len>` poke ship n (len 1 / 4)   `K<count>` poke the counter
token: `h<pool>.<count>.<ship1>.<ship2>` then the logged calls.
"""
import sys, os, random, struct, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_enemy_vectors import fnv64, sparse, unsparse, s32, f32bits, M32
from gen_ship_vectors import noise_ship, clean_flags, rand_float, put8, put32, putf

POOL, COUNT = 0xcb74, 0x157c
TICK, SCROLL = 0x13e4, 0x13c4
SHAD, TRAN, FANCY = 0x12bc, 0x12be, 0x13c8
P1, P2, SHIP = 0xc458, 0xc2c0, 408
SLOTS, SZ = 30, 44


class Rig:
    def __init__(self, exe):
        self.cpu = c = ppcemu.load(exe)
        c.keep_toc = True
        self.log, self.rq, self.gq = [], [], []

        def rnd(cpu):
            lo, hi = s32(cpu.r[3]), s32(cpu.r[4])
            raw = self.rq.pop(0) if self.rq else 0
            v = lo + raw % (hi - lo + 1)
            self.log.append('rnd_%d_%d_%d' % (lo, hi, v))
            cpu.r[3] = v & M32

        def mk(name, ints):
            def f(cpu):
                self.log.append('_'.join([name] + [str(s32(cpu.r[i])) for i in ints]))
            return f

        def fx(cpu):
            self.log.append('fx_' + '_'.join(str(s32(cpu.r[i])) for i in (3, 4, 5, 6, 7, 8, 9)) + '_%08x_%08x' % (
                struct.unpack('>I', struct.pack('>f', cpu.getf(1)))[0], struct.unpack('>I', struct.pack('>f', cpu.getf(2)))[0]))

        def gh(cpu):
            self.log.append('gh_' + '_'.join(str(s32(cpu.r[i])) for i in range(3, 9)))
            cpu.r[3] = (self.gq.pop(0) if self.gq else 0xffffffff) & M32

        def decal(cpu):
            self.log.append('decal_' + '_'.join(str(s32(cpu.r[i])) for i in range(3, 11)) + '_%d_%d' % (s32(cpu.rw(cpu.r[1] + 56)), s32(cpu.rw(cpu.r[1] + 60))))

        def prim(addr):
            def f(cpu):
                self.log.append('d%x_%s' % (addr, '_'.join(str(s32(cpu.r[i])) for i in range(3, 10))))
            return f

        c.mocks.update({0xa0d4: rnd, 0x45bac: mk('snd', (3, 4, 5)), 0x22670: fx, 0x3fec8: gh, 0x40954: decal})
        for a in (0x344c, 0x3170, 0x2f88):
            c.mocks[a] = prim(a)
        c.call(0x2015c, [])
        c.r[2] = 0x8000
        self.base = bytes(c.mem)

    def setup(self, tick, s1, s2, rq, gq):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        self.rq, self.gq = list(rq), list(gq)
        c.ww(TICK, tick)
        c.ww(SCROLL, 1)
        c.wb(SHAD, 1); c.wb(TRAN, 1); c.wb(FANCY, 1)
        c.mem[P1:P1 + SHIP] = bytes(s1)
        c.mem[P2:P2 + SHIP] = bytes(s2)

    def digest(self):
        c = self.cpu
        return 'h%016x.%d.%016x.%016x' % (fnv64(c.mem[POOL:POOL + SLOTS * SZ]), s32(c.rw(COUNT)), fnv64(c.mem[P1:P1 + SHIP]), fnv64(c.mem[P2:P2 + SHIP]))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            a = arg.split(',') if arg else []
            if kind == 'S':
                c.call(0x26b58, [P1 if int(arg) == 1 else P2])
            elif kind == 'U':
                c.ww(TICK, int(a[0])); c.ww(SCROLL, int(a[1]))
                c.call(0x203a4, [])
            elif kind == 'D':
                c.ww(TICK, int(a[0])); c.wb(SHAD, int(a[1])); c.wb(TRAN, int(a[2])); c.wb(FANCY, int(a[3]))
                c.call(0x205ec, [])
            elif kind == 'Z':
                c.call(0x2015c, [])
            elif kind == 'W':
                c.ww(POOL + int(a[0]) * SZ + int(a[1]), int(a[2], 16))
            elif kind == 'X':
                base = P1 if int(a[0]) == 1 else P2
                if int(a[3]) == 1: c.mem[base + int(a[1])] = int(a[2], 16) & 0xff
                else: c.ww(base + int(a[1]), int(a[2], 16))
            elif kind == 'K':
                c.ww(COUNT, int(arg) & M32)
            elif kind == 'T':
                c.ww(TICK, int(arg))
            else:
                raise ValueError(op)
            toks.append(self.digest() + ''.join(',' + l for l in self.log))
        return toks


def rand_player(rng, n):
    b = noise_ship(rng)
    clean_flags(b, rng)
    b[1] = 3 if rng.random() < 0.9 else rng.choice([0, 1, 2, 4])
    b[8] = n
    b[0] = 1
    b[101] = rng.randint(0, 1)
    b[84] = rng.randint(0, 1)
    put32(b, 80, rng.randint(1, 10))
    put32(b, 88, rng.randint(0, 40))
    put32(b, 92, rng.choice([0, 0, 1, 2, 3, 5, 10, -1]))
    putf(b, 20, rand_float(rng, 20, 380)); putf(b, 24, rand_float(rng, 100, 440))
    put32(b, 36, rng.choice([48, 48, 44, 40, 33]))
    put32(b, 312, rng.randint(0, 416)); put32(b, 316, rng.choice([rng.randint(-20, 460), rng.randint(0, 460)]))
    return b


def gen(exe, out, seed=1, n=30):
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    for it in range(n):
        tick = rng.randint(0, 60)
        s = [rand_player(rng, 1), rand_player(rng, 2)]
        init = [bytearray(x) for x in s]
        rq = [rng.getrandbits(16) for _ in range(300)]
        gq = [rng.choice([-1, -1, -1, 1, 1, 0]) for _ in range(100)]
        rig.setup(tick, s[0], s[1], rq, gq)
        ops = []
        t = tick
        scroll = rng.choice([1, 1, 1, 2, 0])
        for fr in range(rng.randint(60, 220)):
            t += 1
            if rng.random() < 0.25:
                nn = rng.randint(1, 2)
                ops.append('X%d,101,%x,1' % (nn, rng.randint(0, 1)))
            if rng.random() < 0.05:
                ops.append('X%d,1,%x,1' % (rng.randint(1, 2), rng.choice([3, 3, 3, 2, 1, 0])))
            if rng.random() < 0.08:
                nn = rng.randint(1, 2)
                ops.append('X%d,92,%x,4' % (nn, rng.choice([0, 1, 2, 3, 5, 10, M32])))
            if rng.random() < 0.08:
                nn = rng.randint(1, 2)
                ops.append('X%d,312,%x,4' % (nn, rng.randint(0, 416)))
                ops.append('X%d,316,%x,4' % (nn, rng.choice([rng.randint(-20, 460), rng.randint(0, 460), 0, M32 - 3]) & M32))
            if rng.random() < 0.05:
                nn = rng.randint(1, 2)
                ops.append('X%d,20,%x,4' % (nn, struct.unpack('>I', struct.pack('>f', rand_float(rng, 20, 380)))[0]))
                ops.append('X%d,24,%x,4' % (nn, struct.unpack('>I', struct.pack('>f', rand_float(rng, 60, 440)))[0]))
            if rng.random() < 0.02: scroll = rng.choice([0, 1, 1, 2, 3])
            ops.append('T%d' % t)
            ops.append('S1')
            if rng.random() < 0.7: ops.append('S2')
            ops.append('U%d,%d' % (t, scroll))
            if rng.random() < 0.5:
                ops.append('D%d,%d,%d,%d' % (t, rng.randint(0, 1), rng.randint(0, 1), rng.randint(0, 1)))
            if rng.random() < 0.03:                         # poke a record into rarely reached states
                ops.append('W%d,%d,%08x' % (rng.randrange(SLOTS), rng.choice([4, 8, 12, 16, 28, 32]), rng.getrandbits(8) if rng.random() < 0.5 else (t - rng.randint(0, 5)) & M32))
            if rng.random() < 0.01: ops.append('Z')
            if rng.random() < 0.01: ops.append('K%d' % rng.choice([0, 1, 5, 30, 40, -1 & M32]))
        head = 'seed=%d T=%d S1=%s S2=%s R=%s Q=%s' % (seed, tick, sparse(init[0]), sparse(init[1]), ','.join('%x' % v for v in rq), ','.join(str(v) for v in gq))
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_bomb_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 30
    print(gen(exe, out, seed, n), 'vectors ->', out)
