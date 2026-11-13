#!/usr/bin/env python3
"""Golden vectors for src/ground.rs (turrets, missile towers and the other ground objects) by running the original code in ppcemu.py.

    python3 -I gen_ground_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors] [mode]

State (data-section addresses, TOC r2 = 0x8000):
    object pool    0x1a818  250 records x 168 bytes (D.0934)         active list  0x1a050  50 x i32, -1 = free (D.108c)
    decal list     0x1a118  50 x 32 bytes (D.1054)                   muzzle tables 0x1a758 / 0x1a7b8: 24 x (s16, s16) (D.1090 / D.1094)
    counters       D.1a60 (targets that count), D.1a64 (destroyed), D.1a68 (activated)
    tick D.13e4, players D.13e2, difficulty D.13ec, scroll step D.13c4, D.9394 (s16), shadows D.12bc
    level structure (fake, at 0x2e0000):  +0 current row, +4 height, +8 width, +24 -> marker layer (255 = none)
    the level's `__GS` resource (250 x 50 bytes) is handed to 0x3ade0 through a mocked GetResource.

A vector line is `seed=.. np=.. tick=.. dif=.. W=.. H=.. M=<marker hex> G=<resource words> S1=.. S2=.. R=<random queue> Q=<shot results> ops=.. -> tokens`.
ops:  `L` level load (0x3ade0)   `N` count targets (0x3acb0)   `A<row>` activate row (0x3b450)   `F<tick>,<scroll>` update (0x3d0fc)
      `G<tick>,<scroll>,<c08c>,<shadows>` move/cull/draw pass (0x3f554)   `Q<tick>,<scroll>,<c08c>,<shadows>` shadow pass (0x3fa24)
      `H<idx>,<amount>,<owner>` damage (0x40040)   `T<l>,<t>,<r>,<b>,<amount>,<owner>` box damage (0x3fec8)   `B<owner>` everything on screen (0x3fbfc)
      `K<l>,<t>,<r>,<b>` blocked? (0x3fd1c)   `Y<x>,<y>,<anim>,<frame>,<w>,<h>,<f9>,<f10>,<p9>,<p10>` add decal (0x40954)
      `W<idx>,<off>,<hex>` / `V<idx>,<off>,<byte>` poke the record   `U<n>:<words>` ship   `C<scroll>` scroll step
token per op: `h<pool>.<list>.<decals>.<c60>.<c64>.<c68>` then the logged calls of the mocked surroundings.
"""
import sys, os, random, struct, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_enemy_vectors import (fnv64, sparse, unsparse, s32, f32bits, bitsf32, rand_ship, rand_ship_near, float_bits, P1, P2, SHIP_BYTES,
                               TICK, NPLAYERS, M32)

POOL, NREC, REC = 0x1a818, 250, 168
ALIST, STAT, TAB1, TAB2 = 0x1a050, 0x1a118, 0x1a758, 0x1a7b8
C60, C64, C68 = 0x1a60, 0x1a64, 0x1a68
DIF, SCROLL, C08C, SHAD = 0x13ec, 0x13c4, 0x9394, 0x12bc
LEVEL, MARK, RES = 0x2e0000, 0x2e1000, 0x200000
RES_REC = 50


class Rig:
    def __init__(self, exe):
        self.cpu = c = ppcemu.load(exe)
        c.keep_toc = True
        self.log = []
        self.rq = []
        self.shot_q = []

        def rnd(cpu):
            lo, hi = s32(cpu.r[3]), s32(cpu.r[4])
            raw = self.rq.pop(0) if self.rq else 0
            v = lo + raw % (hi - lo + 1)
            self.log.append('rnd_%d_%d_%d' % (lo, hi, v))
            cpu.r[3] = v & M32

        def mk(name, ints, floats=0):
            def f(cpu):
                parts = [name] + [str(s32(cpu.r[i])) for i in ints] + ['%08x' % f32bits(cpu.getf(1 + k)) for k in range(floats)]
                self.log.append('_'.join(parts))
            return f

        def shot(cpu):
            self.log.append('shot383b8_%s_%08x_%08x' % ('_'.join(str(s32(cpu.r[i])) for i in (5, 6, 7)), f32bits(cpu.getf(1)), f32bits(cpu.getf(2))))
            cpu.r[3] = self.shot_q.pop(0) if self.shot_q else 0

        def sqrt(cpu):
            cpu.ww(cpu.r[1] + 20, cpu.r[2])
            cpu.setf(1, math.sqrt(cpu.getf(1)))

        def getres(cpu):
            cpu.r[3] = RES - 8             # a handle: the word at RES-8 points at the data
            cpu.ww(RES - 8, RES)

        def prim(addr):
            def f(cpu):
                self.log.append('d%x_%s' % (addr, '_'.join(str(s32(cpu.r[i])) for i in range(3, 10))))
            return f

        c.mocks.update({
            0xa0d4: rnd, 0x6260c: sqrt, 0x6236c: lambda cpu: cpu.setf(1, math.atan(cpu.getf(1))),
            0x60ac4: getres, 0x60adc: lambda cpu: None,
            0xe68c: mk('score1', (3, 4)), 0xe6c0: mk('score2', (3, 4)),
            0x22670: mk('fx', (3, 4, 5, 6, 7, 8, 9), 2), 0x29714: mk('aux', (3, 4, 5, 6, 7, 8)), 0x23440: mk('part', (3, 4, 5, 6, 7, 8)),
            0x45bac: mk('snd', (3, 4, 5)),
            0x2a040: mk('pick', (5, 6, 7, 8), 2),
            0x37fd4: mk('beam', (5, 8, 9, 10), 4),
            0x383b8: shot,
            0xea4c: mk('clear', (), 2),
        })
        for a in (0x2f88, 0x3170, 0x3264, 0x3540, 0x344c, 0x3358, 0x3910, 0x3728, 0x381c):
            c.mocks[a] = prim(a)
        c.call(0x92b4, [])                     # arctangent table for the aiming routine 0x9350
        c.call(0x3a998, [])                    # object system init: muzzle tables, free list
        c.r[2] = 0x8000
        self.base = bytes(c.mem)

    def setup(self, tick, nplayers, dif, w, h, marker, res, ships, rq, shotq):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        self.rq, self.shot_q = list(rq), list(shotq)
        c.ww(TICK, tick)
        c.wb(NPLAYERS, nplayers)
        c.ww(DIF, dif)
        c.ww(SCROLL, 2)
        c.wh(C08C, 0)
        c.wb(SHAD, 1)
        c.ww(LEVEL + 0, 0); c.ww(LEVEL + 4, h); c.ww(LEVEL + 8, w); c.ww(LEVEL + 24, MARK)
        c.mem[MARK:MARK + len(marker)] = marker
        c.mem[RES:RES + len(res)] = res
        for n, words in ((1, ships[0]), (2, ships[1])):
            self.set_ship(n, words)

    def set_ship(self, n, words):
        base = P1 if n == 1 else P2
        c = self.cpu
        c.mem[base:base + SHIP_BYTES] = bytes(SHIP_BYTES)
        c.mem[base:base + len(words)] = words[:SHIP_BYTES]

    def digest(self):
        c = self.cpu
        return 'h%016x.%016x.%016x.%d.%d.%d' % (fnv64(c.mem[POOL:POOL + NREC * REC]), fnv64(c.mem[ALIST:ALIST + 200]), fnv64(c.mem[STAT:STAT + 1600]),
                                                s32(c.rw(C60)), s32(c.rw(C64)), s32(c.rw(C68)))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            a = arg.split(',') if arg else []
            if kind == 'L':
                c.call(0x3ade0, [1])
            elif kind == 'N':
                c.call(0x3acb0, [LEVEL])
            elif kind == 'A':
                c.ww(LEVEL, int(arg))
                c.call(0x3b450, [LEVEL])
            elif kind == 'F':
                c.ww(TICK, int(a[0])); c.ww(SCROLL, int(a[1]))
                c.call(0x3d0fc, [])
            elif kind in 'GQ':
                c.ww(TICK, int(a[0])); c.ww(SCROLL, int(a[1])); c.wh(C08C, int(a[2])); c.wb(SHAD, int(a[3]))
                c.call(0x3f554 if kind == 'G' else 0x3fa24, [])
            elif kind == 'H':
                c.call(0x40040, [int(a[0]), int(a[1]), int(a[2])])
            elif kind == 'T':
                r = c.call(0x3fec8, [int(v) for v in a])
                self.log.append('ret%d' % s32(r))
            elif kind == 'B':
                c.call(0x3fbfc, [int(arg)])
            elif kind == 'K':
                r = c.call(0x3fd1c, [int(v) for v in a])
                self.log.append('ret%d' % (r & 0xff))
            elif kind == 'Y':
                v = [int(x) for x in a]
                c.ww(c.r[1] + 56, v[8]); c.ww(c.r[1] + 60, v[9])
                c.call(0x40954, v[:8])
            elif kind == 'D':
                c.ww(SCROLL, int(arg))
                c.call(0x40b50, [])
            elif kind == 'E':
                c.ww(TICK, int(a[0])); c.ww(SCROLL, int(a[1])); c.wh(C08C, int(a[2])); c.wb(SHAD, int(a[3]))
                c.call(0x40bf4, [])
            elif kind == 'X':
                c.call(0x40904, [])
            elif kind == 'W':
                c.ww(POOL + int(a[0]) * REC + int(a[1]), int(a[2], 16))
            elif kind == 'V':
                c.mem[POOL + int(a[0]) * REC + int(a[1])] = int(a[2]) & 0xff
            elif kind == 'U':
                n, w = arg.split(':', 1)
                self.set_ship(int(n), unsparse(w, SHIP_BYTES))
            elif kind == 'C':
                c.ww(SCROLL, int(arg))
            else:
                raise ValueError(op)
            toks.append(self.digest() + ''.join(',' + l for l in self.log))
        return toks


# ------------------------------------------------------------------------------------------------- vector generation
def rand_res(rng, n_obj, kinds, link_rate=0.25):
    """The `__GS` resource: 250 records of 50 bytes. Returns (bytes, list of used indices)."""
    res = bytearray(250 * RES_REC)
    idx = rng.sample(range(250), n_obj)
    used = []
    def put(i, kind, variant, state, x, y, prev, nxt, tail, flag1=None):
        r = bytearray(RES_REC)
        r[0] = 1
        r[1] = rng.randint(0, 1) if flag1 is None else flag1
        struct.pack_into('>iii', r, 2, kind, state, variant)
        struct.pack_into('>ff', r, 14, x, y)
        struct.pack_into('>ii', r, 22, prev, nxt)
        struct.pack_into('>5i', r, 30, *tail)
        res[i * RES_REC:(i + 1) * RES_REC] = r
    def tail():
        return [rng.choice([1, 1, 1, 2, 5, 0]), rng.choice([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14]),
                rng.choice([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33]),
                rng.choice([-1, 0, 1, 2, 3, 4, 5, 6, 7, 8]), rng.choice([-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 15, 18, 19, 21])]
    i = 0
    while i < len(idx):
        k = rng.choice(kinds)
        st = rng.choice([1, 1, 1, 2, 3, 12, 11, 15, 20, 13])
        x = float(rng.randrange(0, 14) * 32)
        y = float(rng.randrange(0, 40))
        if k in (1, 24) and i + 1 < len(idx) and rng.random() < link_rate + 0.4:
            a, b = idx[i], idx[i + 1]
            put(a, k, rng.randint(0, 9), st, x, y, -1, b, tail())
            put(b, k, rng.randint(0, 9), st, x, y, a, -1, tail())
            used += [a, b]
            i += 2
        else:
            put(idx[i], k, rng.randint(0, 9), st, x, y, -1, -1, tail())
            used.append(idx[i])
            i += 1
    return bytes(res), used


ALL_KINDS = list(range(1, 25)) + [0, 25, 26]


def gen(exe, out, seed=1, n=40, kinds=None, frames=(80, 260), mode='normal'):
    rng = random.Random(seed)
    rig = Rig(exe)
    kinds = kinds or ALL_KINDS
    for tok in mode.split('-'):
        if tok.startswith('k') and tok[1:].replace('_', '').isdigit():
            kinds = [int(v) for v in tok[1:].split('_')]
    lines = []
    for it in range(n):
        tick = rng.randint(0, 40) if rng.random() < 0.85 else rng.randint(1, 100000)   # the game resets the tick at level start
        dif = rng.randint(0, 25)
        w, h = 13, 40
        ks = [kinds[it % len(kinds)]] if 'one' in mode else kinds
        res, used = rand_res(rng, rng.randint(3, 40), ks if 'one' not in mode else [ks[0]] * 3 + ALL_KINDS[:4])
        nframes = rng.randint(*frames)
        row = rng.randint(0, 3)
        marker = bytearray([255] * (w * h))
        # the records sit in the rows that the scroll activates during the run (one row per 11 frames), so that they really get used
        last_row = min(h - 1, row + max(2, nframes // 11))
        for u in used:
            r = rng.randint(row + 1, last_row)
            marker[r * w + rng.randrange(0, w)] = u
        ships = [rand_ship_near(rng), rand_ship_near(rng, rng.random() < 0.7)]
        init_ships = [bytearray(sh) for sh in ships]
        nplayers = rng.choice([1, 2, 2])
        rq = [rng.choice([0, 1, 8, 9, 65535, rng.getrandbits(16)]) if 'edge' in mode and rng.random() < 0.4 else rng.getrandbits(16) for _ in range(600)]
        shotq = [rng.randint(0, 1) for _ in range(200)]
        head_only = None
        rig.setup(tick, nplayers, dif, w, h, marker, res, init_ships, rq, shotq)
        ops, toks = [], []
        hot = 'hot' in mode

        def emit(op):
            ops.append(op)
            toks.extend(rig.run([op]))

        def rec(i):
            a = POOL + i * REC
            c = rig.cpu
            return dict(x=bitsf32(c.rw(a + 24)), y=bitsf32(c.rw(a + 28)), w=s32(c.rw(a + 16)), h=s32(c.rw(a + 20)), act=c.rb(a), f51=c.rb(a + 51))

        def listed():
            c = rig.cpu
            return [v for v in (c.rw(ALIST + 4 * k) for k in range(50)) if v != 0xffffffff and v < 250]

        def near_edges(v, size):
            """integers around both edges of [v, v+size]"""
            out = []
            for e in (v, v + size):
                f = int(math.floor(e))
                out += [f - 1, f, f + 1, f + 2]
            return out

        emit('L')
        if rng.random() < 0.7: emit('N')
        t = tick
        scroll = rng.choice([1, 2, 2, 3])
        alive_cells = list(used)
        if rng.random() < (0.9 if 'shoot' in mode else 0.5):    # a few default shooters (the +50 flag is not set by any init)
            for r in used:
                if rng.random() < (0.8 if 'shoot' in mode else 0.3): emit('V%d,50,1' % r)
            if 'shoot' in mode:
                for r in range(0, 12): emit('W%d,112,%08x' % (r, rng.choice([0, 1, 2, 5, 10])))
        for fr in range(nframes):
            t += 1
            if fr % 11 == 0:
                emit('A%d' % (row % h)); row += 1
            if rng.random() < 0.02: scroll = rng.choice([0, 1, 2, 3, 4, 6]); emit('C%d' % scroll)
            emit('F%d,%d' % (t, scroll))
            if rng.random() < 0.5: emit('D%d' % scroll)
            emit('G%d,%d,%d,%d' % (t, scroll, rng.choice([0, 0, 0, 5, -3, 12]), rng.randint(0, 1)))
            if rng.random() < 0.15:
                emit('Q%d,%d,%d,%d' % (t, scroll, rng.choice([0, 7]), rng.randint(0, 1)))
            if rng.random() < 0.15:
                emit('E%d,%d,%d,%d' % (t, scroll, rng.choice([0, 7]), rng.randint(0, 1)))
            if rng.random() < 0.004: emit('X')
            if rng.random() < (0.03 if 'natural' in mode else 0.25):   # poke records into rarely reached states
                for _ in range(rng.randint(1, 3)):
                    r = rng.choice(alive_cells)
                    k = rng.randrange(17)
                    if k == 0: emit('W%d,28,%08x' % (r, f32bits(rng.choice([0.0, 14.5, 15.0, 15.5, 60.0, 200.0, 399.5, 400.0, 400.5, 401.0, 420.0, -20.0]))))
                    elif k == 1: emit('W%d,24,%08x' % (r, f32bits(rng.uniform(-70, 560))))
                    elif k == 2: emit('W%d,40,%08x' % (r, f32bits(rng.choice([0.0, 0.6, -0.6, 1.0, -1.0, -0.5]))))
                    elif k == 3: emit('W%d,44,%08x' % (r, f32bits(rng.choice([0.0, 0.6, -0.6, 1.0, -1.0, -0.5]))))
                    elif k == 4: emit('W%d,56,%08x' % (r, rng.randint(0, 26)))
                    elif k == 5: emit('W%d,72,%08x' % (r, (t - rng.randint(0, 12)) & M32))
                    elif k == 6: emit('W%d,132,%08x' % (r, (t - rng.randint(0, 6)) & M32))
                    elif k == 7: emit('V%d,106,%d' % (r, rng.randint(0, 1)))
                    elif k == 8: emit('W%d,12,%08x' % (r, rng.randint(0, 10)))
                    elif k == 9: emit('V%d,51,%d' % (r, rng.randint(0, 1)))
                    elif k == 10: emit('V%d,48,%d' % (r, rng.randint(0, 1)))
                    elif k == 11: emit('W%d,140,%08x' % (r, (t - rng.randint(0, 100)) & M32))
                    elif k == 12: emit('W%d,64,%08x' % (r, rng.randint(0, 3)))
                    elif k == 14: emit('V%d,50,%d' % (r, rng.randint(0, 1)))
                    elif k == 15:                           # exactly on the culling / smart-bomb limits
                        q = rec(r)
                        emit('W%d,24,%08x' % (r, f32bits(rng.choice([-64.0 - q['w'], -63.0 - q['w'], -64.5 - q['w'], 479.5, 480.0, 480.5, 416.0 - q['w'], 417.0 - q['w']]))))
                    elif k == 16:
                        q = rec(r)
                        emit('W%d,28,%08x' % (r, f32bits(rng.choice([-64.0 - q['h'], -63.0 - q['h'], -65.0 - q['h'], 543.0, 543.5, 544.0, 544.5, 480.0 - q['h'], 481.0 - q['h']]))))
                    else:                               # next to a ship
                        shp = ships[rng.randrange(2)]
                        sx = struct.unpack('>f', bytes(shp[20:24]))[0]; sy = struct.unpack('>f', bytes(shp[24:28]))[0]
                        emit('W%d,24,%08x' % (r, f32bits(sx + rng.choice([-70.0, -30.0, 0.0, 30.0, 70.0]))))
                        emit('W%d,28,%08x' % (r, f32bits(sy + rng.choice([-150.0, -60.0, 0.0, 60.0, 150.0]))))
            if rng.random() < (0.35 if hot else 0.10):
                r = rng.choice(alive_cells)
                emit('H%d,%d,%d' % (r, rng.choice([1, 1, 2, 4, 10, 50, 10000]), rng.randint(1, 2)))
            if rng.random() < (0.15 if hot else 0.05):
                lst = listed()
                if lst and rng.random() < 0.8:              # a box whose edges are within one pixel of a record
                    q = rec(rng.choice(lst))
                    xs, ys = near_edges(q['x'], q['w']), near_edges(q['y'], q['h'])
                    l, r_ = sorted([rng.choice(xs), rng.choice(xs)]); t_, b_ = sorted([rng.choice(ys), rng.choice(ys)])
                    emit('T%d,%d,%d,%d,%d,%d' % (l, t_, r_, b_, rng.choice([0, 1, 4, 5, 20]), rng.randint(1, 2)))
                else:
                    emit('T%d,%d,%d,%d,%d,%d' % (rng.randint(-20, 400), rng.randint(0, 400), rng.randint(0, 500), rng.randint(0, 500),
                                                     rng.choice([0, 1, 4, 5]), rng.randint(1, 2)))
            if rng.random() < (0.03 if hot else 0.01): emit('B%d' % rng.randint(1, 2))
            if rng.random() < 0.05:
                c = rig.cpu
                dec = [k for k in range(50) if c.rb(STAT + k * 32) and c.rb(STAT + k * 32 + 2)]
                lst = [i for i in listed() if rec(i)['f51']]
                if (dec or lst) and rng.random() < 0.8:
                    if dec and (not lst or rng.random() < 0.5):
                        k = rng.choice(dec)
                        x0, y0 = s32(c.rw(STAT + k * 32 + 8)), s32(c.rw(STAT + k * 32 + 12))
                        xs, ys = [x0 - 1, x0, x0 + 1, x0 + 59, x0 + 60, x0 + 61], [y0 - 1, y0, y0 + 1, y0 + 59, y0 + 60, y0 + 61]
                    else:
                        q = rec(rng.choice(lst))
                        xs, ys = near_edges(q['x'], q['w']), near_edges(q['y'], q['h'])
                    l, r_ = sorted([rng.choice(xs), rng.choice(xs)]); t_, b_ = sorted([rng.choice(ys), rng.choice(ys)])
                    emit('K%d,%d,%d,%d' % (l, t_, r_, b_))
                else:
                    emit('K%d,%d,%d,%d' % (rng.randint(-60, 500), rng.randint(-60, 500), rng.randint(0, 560), rng.randint(0, 560)))
            if rng.random() < 0.03:
                emit('Y%d,%d,%d,%d,%d,%d,%d,%d,%d,%d' % (rng.choice([rng.randint(-100, 600), -16, -15, 0, 1, 351, 352, 353, 416]), rng.choice([rng.randint(-100, 600), -16, -15, 0, 1, 415, 416, 417, 479, 480, 481]), rng.randint(1, 80), rng.randint(0, 5),
                                                             rng.choice([16, 32, 50, 52, 64]), rng.choice([16, 32, 50, 52, 64]), rng.randint(0, 1), rng.randint(0, 1),
                                                             rng.randint(0, 1), rng.randint(0, 1)))
            if rng.random() < 0.06:
                nn = rng.randint(1, 2)
                sh = rand_ship_near(rng, rng.random() < 0.9)
                ships[nn - 1] = sh
                emit('U%d:%s' % (nn, sparse(sh)))
        head = 'seed=%d np=%d tick=%d dif=%d W=%d H=%d M=%s G=%s S1=%s S2=%s R=%s Q=%s' % (
            seed, nplayers, tick, dif, w, h, marker.hex(), sparse(res), sparse(init_ships[0]), sparse(init_ships[1]),
            ','.join('%x' % v for v in rq), ','.join(str(v) for v in shotq))
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_ground_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 40
    mode = sys.argv[5] if len(sys.argv) > 5 else 'normal'
    print(gen(exe, out, seed, n, mode=mode), 'vectors ->', out)
