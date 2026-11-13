#!/usr/bin/env python3
"""Golden vectors for src/drone.rs (drones and side missiles) by running the original code in ppcemu.py.

    python3 -I gen_drone_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

Routines: drones: clear 0x2c70c, add 0x2c760(number, side), remove 0x2c87c(number), fire 0x2c960(ship) (+ the shot spawner 0x21288), update 0x2cb40(ship1, ship2),
draw 0x2d1d8(ship1, ship2); missiles: clear 0x27780, launch 0x277e4(ship), update 0x27dfc, draw 0x28260.
State: drone pool 0x149f0 (4 x 44) + counter D.17a0, missile pool 0x1079c (16 x 172) + D.16ec, player shots 0xd09c (60 x 48) + D.15dc, the two ship structures
(408 bytes at 0xc458 = player 1 and 0xc2c0 = player 2), tick D.13e4, options D.12bc (shadows) / D.12be / D.13c8.
Mocked (logged) callees: enemy box test 0x2ead4 (result from a queue), effect 0x22670, sound 0x45bac, the blitters 0x2f88 / 0x3170 / 0x3264 / 0x344c / 0x3540, MathLib sqrt 0x6260c.

A vector line: `seed=.. T=<tick> S1=<ship words> S2=<ship words> Q=<hit results> ops=.. -> tokens`; ops:
  `A<n>,<side>` add drone   `R<n>` remove   `F<n>` fire   `U<tick>` update drones   `D<tick>,<shadows>,<trans>,<fancy>` draw drones   `Z` clear drones
  `L<n>` launch missiles   `M<tick>` update missiles   `N<tick>,<shadows>,<trans>,<fancy>` draw missiles   `C` clear missiles
  `P<slot>,<off>,<hex>` / `p<slot>,<off>,<hex>` poke a drone record word / byte   `Q<slot>,<off>,<hex>` / `q<slot>,<off>,<hex>` poke a missile record word / byte
  `G<slot>,<byte>` poke a player-shot active byte   `H<count>` shot counter   `K<count>` drone counter   `J<count>` missile counter
  `X<n>,<off>,<hex>,<len>` poke ship n (len 1 / 4)   `T<tick>` set the tick
token: `h<drones>.<count>.<missiles>.<count>.<shots>.<count>.<ship1>.<ship2>` then the logged calls.
"""
import os, sys, random, struct, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_enemy_vectors import fnv64, sparse, unsparse, s32, f32bits, M32
from gen_ship_vectors import noise_ship, clean_flags, rand_float, put8, put32, putf

DPOOL, DCOUNT = 0x149f0, 0x17a0
MPOOL, MCOUNT = 0x1079c, 0x16ec
SPOOL, SCOUNT = 0xd09c, 0x15dc
TICK = 0x13e4
SHAD, TRAN, FANCY = 0x12bc, 0x12be, 0x13c8
P1, P2, SHIP = 0xc458, 0xc2c0, 408


class Rig:
    def __init__(self, exe):
        self.cpu = c = ppcemu.load(exe)
        c.keep_toc = True
        self.log, self.hq = [], []

        def hit(cpu):
            self.log.append('he_' + '_'.join(str(s32(cpu.r[i])) for i in range(3, 9)))
            cpu.r[3] = (self.hq.pop(0) if self.hq else 0) & M32

        def fx(cpu):
            self.log.append('fx_' + '_'.join(str(s32(cpu.r[i])) for i in (3, 4, 5, 6, 7, 8, 9)) + '_%08x_%08x' % (f32bits(cpu.getf(1)), f32bits(cpu.getf(2))))

        def snd(cpu):
            self.log.append('snd_%d_%d_%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[5])))

        def sqrt(cpu):
            cpu.ww(cpu.r[1] + 20, cpu.r[2])
            cpu.setf(1, math.sqrt(cpu.getf(1)))

        def prim(addr):
            def f(cpu):
                self.log.append('d%x_%s' % (addr, '_'.join(str(s32(cpu.r[i])) for i in range(3, 10))))
            return f

        c.mocks.update({0x2ead4: hit, 0x22670: fx, 0x45bac: snd, 0x6260c: sqrt})
        for a in (0x2f88, 0x3170, 0x3264, 0x344c, 0x3540):
            c.mocks[a] = prim(a)
        c.call(0x2c70c, [])
        c.call(0x27780, [])
        c.call(0x21224, [])
        c.r[2] = 0x8000
        self.base = bytes(c.mem)

    def setup(self, tick, s1, s2, hq):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        self.hq = list(hq)
        c.ww(TICK, tick)
        c.wb(SHAD, 1); c.wb(TRAN, 1); c.wb(FANCY, 1)
        c.mem[P1:P1 + SHIP] = bytes(s1)
        c.mem[P2:P2 + SHIP] = bytes(s2)

    def digest(self):
        c = self.cpu
        return 'h%016x.%d.%016x.%d.%016x.%d.%016x.%016x' % (
            fnv64(c.mem[DPOOL:DPOOL + 4 * 44]), s32(c.rw(DCOUNT)), fnv64(c.mem[MPOOL:MPOOL + 16 * 172]), s32(c.rw(MCOUNT)),
            fnv64(c.mem[SPOOL:SPOOL + 60 * 48]), s32(c.rw(SCOUNT)), fnv64(c.mem[P1:P1 + SHIP]), fnv64(c.mem[P2:P2 + SHIP]))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            a = arg.split(',') if arg else []
            ship = lambda n: P1 if int(n) == 1 else P2
            ret = None
            if kind == 'A':
                ret = c.call(0x2c760, [int(a[0]), int(a[1])]) & 0xff
            elif kind == 'R':
                c.call(0x2c87c, [int(arg)])
            elif kind == 'F':
                c.call(0x2c960, [ship(arg)])
            elif kind == 'U':
                c.ww(TICK, int(arg))
                c.call(0x2cb40, [P1, P2])
            elif kind == 'D':
                c.ww(TICK, int(a[0])); c.wb(SHAD, int(a[1])); c.wb(TRAN, int(a[2])); c.wb(FANCY, int(a[3]))
                c.call(0x2d1d8, [P1, P2])
            elif kind == 'Z':
                c.call(0x2c70c, [])
            elif kind == 'L':
                ret = c.call(0x277e4, [ship(arg)]) & 0xff
            elif kind == 'M':
                c.ww(TICK, int(arg))
                c.call(0x27dfc, [])
            elif kind == 'N':
                c.ww(TICK, int(a[0])); c.wb(SHAD, int(a[1])); c.wb(TRAN, int(a[2])); c.wb(FANCY, int(a[3]))
                c.call(0x28260, [])
            elif kind == 'C':
                c.call(0x27780, [])
            elif kind == 'P':
                c.ww(DPOOL + int(a[0]) * 44 + int(a[1]), int(a[2], 16))
            elif kind == 'p':
                c.mem[DPOOL + int(a[0]) * 44 + int(a[1])] = int(a[2], 16) & 0xff
            elif kind == 'Q':
                c.ww(MPOOL + int(a[0]) * 172 + int(a[1]), int(a[2], 16))
            elif kind == 'q':
                c.mem[MPOOL + int(a[0]) * 172 + int(a[1])] = int(a[2], 16) & 0xff
            elif kind == 'G':
                c.mem[SPOOL + int(a[0]) * 48] = int(a[1]) & 0xff
            elif kind == 'H':
                c.ww(SCOUNT, int(arg) & M32)
            elif kind == 'K':
                c.ww(DCOUNT, int(arg) & M32)
            elif kind == 'J':
                c.ww(MCOUNT, int(arg) & M32)
            elif kind == 'X':
                base = ship(a[0])
                if int(a[3]) == 1: c.mem[base + int(a[1])] = int(a[2], 16) & 0xff
                else: c.ww(base + int(a[1]), int(a[2], 16))
            elif kind == 'T':
                c.ww(TICK, int(arg))
            else:
                raise ValueError(op)
            tok = self.digest() + ''.join(',' + l for l in self.log)
            if ret is not None:
                tok += ',r%d' % ret
            toks.append(tok)
        return toks


EDGE = bool(os.environ.get('DRONE_EDGE'))
EDGE_X = [327.0, 327.5, 328.0, 328.5, 328.9, 329.5, 40.0, 40.5, 41.0, 41.5, 42.0, 100.0, 5.0]
EDGE_VX = [0.0, 0.3, -0.3, 0.9, 1.0, -1.0, 1.5, -0.0, 7.2]


def shipx(rng):
    return rng.choice(EDGE_X) if EDGE else rand_float(rng, -20, 440)


def rand_player(rng, n):
    b = noise_ship(rng)
    clean_flags(b, rng)
    b[1] = 3
    b[8] = n
    b[0] = 1
    putf(b, 20, shipx(rng)); putf(b, 24, rand_float(rng, -30, 470))
    putf(b, 28, rng.choice(EDGE_VX if EDGE else [0.0, 0.0, 2.5, -2.5, 5.0, -5.0, 7.2, -7.2, -0.0]))
    put32(b, 36, 48 if EDGE else rng.choice([48, 48, 44, 40, 33, 0, 1000]))
    put32(b, 332, rng.choice([0, 1, 2, 3, 3, 4, 5, 6, -1]))
    return b


def gen(exe, out, seed=1, n=30):
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    for it in range(n):
        tick = rng.randint(0, 5000)
        s = [rand_player(rng, 1), rand_player(rng, 2)]
        init = [bytearray(x) for x in s]
        hq = [rng.choice([0, 0, 0, 0, 0, 1, 3, 7, -1, 12]) for _ in range(400)]
        rig.setup(tick, s[0], s[1], hq)
        ops = []
        t = tick
        for fr in range(rng.randint(80, 260)):
            t += rng.choice([1, 1, 1, 1, 2])
            r = rng.random()
            if r < 0.03:
                ops.append('A%d,%d' % (rng.randint(1, 2), rng.choice([1, 1, 2, 2, 0, 3])))
            if rng.random() < 0.015:
                ops.append('R%d' % rng.choice([1, 2, 1, 2, 3, 0]))
            if rng.random() < 0.12:
                nn = rng.randint(1, 2)
                ops.append('X%d,20,%x,4' % (nn, f32bits(shipx(rng))))
                ops.append('X%d,24,%x,4' % (nn, f32bits(rand_float(rng, -30, 470))))
            if rng.random() < 0.10:
                ops.append('X%d,28,%x,4' % (rng.randint(1, 2), f32bits(rng.choice(EDGE_VX if EDGE else [0.0, 2.5, -2.5, 5.0, -5.0, 7.2, -7.2, -0.0]))))
            if rng.random() < 0.02:
                ops.append('X%d,332,%x,4' % (rng.randint(1, 2), rng.choice([0, 1, 2, 3, 4, 5, 6, M32]) & M32))
            if rng.random() < 0.02:
                ops.append('X%d,36,%x,4' % (rng.randint(1, 2), rng.choice([48, 44, 33, 0, 1000])))
            ops.append('U%d' % t)
            if rng.random() < 0.6: ops.append('F%d' % rng.randint(1, 2))
            if rng.random() < 0.12: ops.append('L%d' % rng.randint(1, 2))
            ops.append('M%d' % t)
            if rng.random() < 0.6:
                o = '%d,%d,%d,%d' % (t, rng.randint(0, 1), rng.randint(0, 1), rng.randint(0, 1))
                ops.append('D' + o)
                ops.append('N' + o)
            if rng.random() < 0.02:                      # poke records into rarely reached states
                sl = rng.randrange(4)
                off = rng.choice([(1, 0), (1, 1), (1, 2), (1, 3), (8, 1), (8, 2), (9, 1), (9, 0), (28, 1), (29, 1), (0, 1)])
                ops.append('p%d,%d,%x' % (sl, off[0], off[1]))
            if rng.random() < 0.02:
                sl = rng.randrange(16)
                ops.append(rng.choice(['Q%d,48,%x' % (sl, rng.choice([0, 11, 12, 13, 15, 19, 20, 21, 25])),
                                       'q%d,2,%x' % (sl, rng.randint(0, 1)), 'Q%d,4,%x' % (sl, rng.choice([1, 2, 3, 5, 0])),
                                       'Q%d,16,%x' % (sl, f32bits(rng.choice([0.0, 0.05, -0.05, 0.1, -0.1, 0.15, 1.2, -1.2]))),
                                       'Q%d,8,%x' % (sl, f32bits(rng.choice([-9.0, -8.0, -7.0, 0.0, 400.0, 416.0, 416.5, 424.0, 480.0]))),
                                       'Q%d,12,%x' % (sl, f32bits(rng.choice([-17.0, -16.0, -15.0, 0.0, 300.0, 480.0, 480.5, 481.0])))]))
            if rng.random() < 0.01:
                ops.append(rng.choice(['Z', 'C', 'K%d' % rng.choice([0, 1, 4, 5, -1 & M32]), 'J%d' % rng.choice([0, 1, 10, 11, 16, 17, -1 & M32]), 'H%d' % rng.choice([0, 55, 59, 60, 61])]))
            if rng.random() < 0.03:
                ops.append('G%d,%d' % (rng.randrange(60), rng.randint(0, 1)))
        head = 'seed=%d T=%d S1=%s S2=%s Q=%s' % (seed, tick, sparse(init[0]), sparse(init[1]), ','.join(str(v) for v in hq))
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'a' if os.environ.get('DRONE_APPEND') else 'w') as f:
        if not os.environ.get('DRONE_APPEND'):
            f.write('# generated by tools/re/gen_drone_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


def gen_bound(exe, out, seed=1):
    """Hand-placed missiles on the exact edges of the culling box (the kick-curve step is poked to 21 so that y does not move), at the
    step limits and on the vx-decay boundaries, plus trail points in unusual states. The hit queue stays empty."""
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    base = ['C', 'q0,0,1', 'q0,1,0', 'q0,2,0', 'q0,3,1', 'Q0,24,1', 'Q0,28,8', 'Q0,32,10', 'Q0,36,2d', 'Q0,40,1', 'J1']
    cases = []
    for x in [-9.0, -8.5, -8.0, -7.5, -7.0, 0.0, 415.5, 416.0, 416.5, 417.0]:
        cases.append(dict(x=x, y=100.0))
    for y in [-17.0, -16.5, -16.0, -15.5, -15.0, 479.5, 480.0, 480.5, 481.0]:
        cases.append(dict(y=y, x=100.0))
    for st in [19, 20, 21, 22, 12, 14, 16, 25]:   # (a negative step reads before the kick table in the original: unreachable, not ported)
        cases.append(dict(x=100.0, y=100.0, st=st, vx=0.3))
    for vx in [0.1, -0.1, 0.05, -0.05, 0.2, -0.2, 0.0, -0.0]:
        cases.append(dict(x=100.0, y=100.0, st=21, vx=vx))
    cases.append(dict(x=100.0, y=100.0, st=13, flame=1, trail=2))
    cases.append(dict(x=100.0, y=100.0, st=13, flame=1, trail=1))
    cases.append(dict(x=100.0, y=100.0, st=13, flame=1, trail=0))
    for per in range(0, len(cases), 8):
        tick = rng.randint(0, 5000)
        s = [rand_player(rng, 1), rand_player(rng, 2)]
        init = [bytearray(x) for x in s]
        hq = [0] * 400
        rig.setup(tick, s[0], s[1], hq)
        ops = []
        t = tick
        for c in cases[per:per + 8]:
            ops += base
            ops.append('Q0,8,%x' % f32bits(c.get('x', 100.0)))
            ops.append('Q0,12,%x' % f32bits(c.get('y', 100.0)))
            ops.append('Q0,16,%x' % f32bits(c.get('vx', 0.0)))
            ops.append('Q0,48,%x' % (c.get('st', 21) & M32))
            if c.get('flame'):
                ops.append('q0,1,1')
                for k in range(6):
                    ops.append('Q0,%d,%x' % (64 + 20 * k, (t + 1000) if c['trail'] == 1 else (t - 3 if k % 2 else t + 1000)))
                    ops.append('q0,%d,%x' % (68 + 20 * k, c['trail'] if k % 3 else 0))
            for _ in range(3):
                t += 1
                ops.append('M%d' % t)
                ops.append('N%d,1,1,1' % t)
        head = 'seed=%d T=%d S1=%s S2=%s Q=%s' % (seed, tick, sparse(init[0]), sparse(init[1]), ','.join(str(v) for v in hq))
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'a') as f:
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 30
    if os.environ.get('DRONE_BOUND'):
        print(gen_bound(exe, out, seed), 'boundary vectors appended ->', out)
    else:
        print(gen(exe, out, seed, n), 'vectors ->', out)
