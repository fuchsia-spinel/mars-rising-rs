#!/usr/bin/env python3
"""Golden vectors for src/effects.rs (the effect pool: explosions, sparks, smoke and their sounds) by running the original code in ppcemu.py.

    python3 -I gen_effects_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

Routines: pool initialisation 0x22618 (which calls the direction-table builder 0x224e8), effect spawner 0x22670, particle burst 0x23440, update / draw
0x236b4. State: pool 0xdd6c (60 x 180 bytes), direction table 0xdbdc (50 x 8 bytes), next-entry counter D.165c, tick D.13e4, scroll step D.13c4,
options D.12be (translucent) / D.13c8 (fancy). Mocked callees: sqrt 0x6260c; logged: random 0xa0d4, sound 0x45bac, blitters 0x344c / 0x3170 / 0x3264 / 0x2f88.

A vector line: `seed=.. T=<tick> R=<random queue> ops=.. -> tokens`; ops:
  `I` initialise   `E<kind>,<x>,<y>,<delay>,<snd>,<scrolls>,<loops>,<vx>,<vy>` spawn (vx, vy as f32 bit patterns in hex)   `P<x>,<y>,<anim>,<count>,<delay>,<scrolls>` burst
  `U<tick>,<scroll>,<trans>,<fancy>` update   `W<slot>,<off>,<hex>` poke a record word   `B<slot>,<off>,<hex>` poke a record byte   `T<tick>`
token: `h<pool>.<stars>.<index>` then the logged calls.
"""
import sys, os, random, struct, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_enemy_vectors import fnv64, s32, f32bits, M32

POOL, STARS, IDX = 0xdd6c, 0xdbdc, 0x165c
TICK, SCROLL = 0x13e4, 0x13c4
TRAN, FANCY = 0x12be, 0x13c8
SLOTS, SZ = 60, 180


class Rig:
    def __init__(self, exe):
        self.cpu = c = ppcemu.load(exe)
        c.keep_toc = True
        self.log, self.rq = [], []

        def rnd(cpu):
            lo, hi = s32(cpu.r[3]), s32(cpu.r[4])
            raw = self.rq.pop(0) if self.rq else 0
            v = lo + raw % (hi - lo + 1)
            self.log.append('rnd_%d_%d_%d' % (lo, hi, v))
            cpu.r[3] = v & M32

        def snd(cpu):
            self.log.append('snd_%d_%d_%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[5])))

        def prim(addr):
            def f(cpu):
                self.log.append('d%x_%s' % (addr, '_'.join(str(s32(cpu.r[i])) for i in range(3, 10))))
            return f

        c.mocks.update({0xa0d4: rnd, 0x45bac: snd, 0x6260c: lambda cpu: cpu.setf(1, math.sqrt(cpu.getf(1)))})
        for a in (0x344c, 0x3170, 0x3264, 0x2f88):
            c.mocks[a] = prim(a)
        c.r[2] = 0x8000
        self.base = bytes(c.mem)

    def setup(self, tick, rq):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        self.rq = list(rq)
        c.ww(TICK, tick)
        c.ww(SCROLL, 1)
        c.wb(TRAN, 1); c.wb(FANCY, 1)

    def digest(self):
        c = self.cpu
        return 'h%016x.%016x.%d' % (fnv64(c.mem[POOL:POOL + SLOTS * SZ]), fnv64(c.mem[STARS:STARS + 400]), s32(c.rw(IDX)))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            a = arg.split(',') if arg else []
            if kind == 'I':
                c.call(0x22618, [])
            elif kind == 'E':
                kd, x, y, dl, sn, sc, lp = (int(v) for v in a[:7])
                vx = struct.unpack('>f', struct.pack('>I', int(a[7], 16)))[0]
                vy = struct.unpack('>f', struct.pack('>I', int(a[8], 16)))[0]
                c.call(0x22670, [x & M32, y & M32, kd & M32, dl & M32, sn & M32, sc & M32, lp & M32], [vx, vy])
            elif kind == 'P':
                x, y, an, ct, dl, sc = (int(v) for v in a)
                c.call(0x23440, [x & M32, y & M32, an & M32, ct & M32, dl & M32, sc & M32])
            elif kind == 'U':
                c.ww(TICK, int(a[0])); c.ww(SCROLL, int(a[1]) & M32); c.wb(TRAN, int(a[2])); c.wb(FANCY, int(a[3]))
                c.call(0x236b4, [])
            elif kind == 'W':
                c.ww(POOL + int(a[0]) * SZ + int(a[1]), int(a[2], 16))
            elif kind == 'B':
                c.wb(POOL + int(a[0]) * SZ + int(a[1]), int(a[2], 16))
            elif kind == 'T':
                c.ww(TICK, int(arg))
            else:
                raise ValueError(op)
            toks.append(self.digest() + ''.join(',' + l for l in self.log))
        return toks


def rfloat(rng):
    return f32bits(rng.choice([0.0, 0.0, 0.0, rng.uniform(-3, 3), rng.uniform(-1, 1), 1.0, -1.0, 2.5]))


def gen(exe, out, seed=1, n=40):
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    for it in range(n):
        tick = rng.randint(0, 80)
        rq = [rng.getrandbits(16) for _ in range(1200)]
        rig.setup(tick, rq)
        ops = ['I']
        t = tick
        scroll = rng.choice([1, 1, 1, 2, 0])
        heavy = rng.random() < 0.25            # fill the pool
        for fr in range(rng.randint(40, 160)):
            t += 1
            for _ in range(rng.choice([0, 0, 1, 1, 2, 3]) + (4 if heavy and rng.random() < 0.5 else 0)):
                kd = rng.choice(list(range(15)) * 3 + [-1, -2, 15, 16, 99])
                ops.append('E%d,%d,%d,%d,%d,%d,%d,%08x,%08x' % (
                    kd, rng.choice([rng.randint(-40, 460), rng.randint(0, 416)]), rng.choice([rng.randint(-40, 520), rng.randint(0, 480)]),
                    rng.choice([0, 0, 0, 1, 2, 3, 6]), rng.choice([-1, -1, 0, 3, 7, 12, 25, 40, 255, 256, 300]),
                    rng.choice([0, 1, 1, 255, 256]), rng.choice([0, 0, 0, 1, 2]), rfloat(rng), rfloat(rng)))
            if rng.random() < 0.15:
                ops.append('P%d,%d,%d,%d,%d,%d' % (rng.randint(-20, 440), rng.randint(-20, 500), rng.randint(9, 12),
                                                    rng.choice([0, 1, 3, 5, 8, 12, -2]), rng.choice([0, 0, 1, 4]), rng.choice([0, 1, 1, 255])))
            if rng.random() < 0.02: scroll = rng.choice([0, 1, 1, 2, 3])
            ops.append('U%d,%d,%d,%d' % (t, scroll, rng.randint(0, 1) if rng.random() < 0.3 else 1, rng.randint(0, 1) if rng.random() < 0.3 else 1))
            if rng.random() < 0.04:                          # poke a record into rarely reached states
                s_ = rng.randrange(SLOTS)
                ch = rng.random()
                if ch < 0.4: ops.append('W%d,%d,%08x' % (s_, 4, rng.choice([0, 1, 2, 0xffffffff, rng.randint(0, 5)])))
                elif ch < 0.7: ops.append('W%d,%d,%08x' % (s_, 12, rng.choice([0, 3, 6, 7, 14, 15, 0xffffffff])))
                elif ch < 0.85: ops.append('W%d,%d,%08x' % (s_, 48, (t - rng.randint(0, 5)) & M32))
                else: ops.append('B%d,%d,%02x' % (s_, rng.choice([0, 1, 2, 3, 53, 176]), rng.randint(0, 1)))
            if rng.random() < 0.01: ops.append('I')
        head = 'seed=%d T=%d R=%s' % (seed, tick, ','.join('%x' % v for v in rq))
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_effects_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 40
    print(gen(exe, out, seed, n), 'vectors ->', out)
