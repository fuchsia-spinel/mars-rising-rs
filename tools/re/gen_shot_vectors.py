#!/usr/bin/env python3
"""Golden vectors for src/enemy_shot.rs (the enemy-shot pool, 30 records x 80 bytes at D.1050 -> 0x196f0) from the original code in ppcemu.py.

    python3 -I gen_shot_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

Routines run natively: 0x37f60 (init), 0x383b8 (aimed shot), 0x38754 (shot with parameters, dispatching to the 10 kind routines
0x39650 0x3996c 0x39adc 0x39dfc 0x39f68 0x3a0bc 0x3a3c4 0x3a51c 0x3a670), 0x37fd4 (beam), 0x38b0c (per-frame update: animation, flash, movement,
cull, ground-object hit, ship hits), 0x38a1c (clear all), 0x39020 / 0x39338 (the two draw passes).
Mocked callees (logged with their arguments): ground-object hit test 0x2ca38 (answers are a fixed function of the box: (7 l + 13 t + 3 r + b) % 19 == 0), effects 0x22670, sound 0x45bac,
ship damage 0xe6f4 / 0xe720, the blit primitives 0x2f88 0x3170 0x344c, and the Toolbox InsetRect (0x60d64).

A line:  `seed=<n> np=<players> tick=<t> S1=<ship words> S2=<..> ops=<op;..> -> <token per op>`
ops:  I                    init (0x37f60)
      A<kind>,<mode>,<flag>,<xbits>,<ybits>                       0x383b8   (r5 = kind, r6 = target ship, r7 = flag byte, f1/f2 = position)
      P<kind>,<count>,<snd>,<mode>,<p>,<flag>,<xbits>,<ybits>     0x38754   (r5..r10, f1/f2)
      B<kind>,<xbits>,<ybits>,<x2bits>,<y2bits>,<snd>,<mode>,<flag>  0x37fd4
      F<tick>              set the tick, update (0x38b0c)
      C<tick>              set the tick, clear all (0x38a1c)
      Da<tr>,<sh> / Db<tr>,<sh>     draw passes (0x39020 / 0x39338) with the translucent / shadow options
      V<n>                 scroll speed D.13c4
      U<n>:<sparse>        replace player n's words
tokens: `h<pool hash>.<live>.<gate>` then the mocked calls `,<call>`.
"""
import sys, os, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_enemy_vectors as G
from gen_enemy_vectors import s32, f32bits, fnv64, sparse, unsparse, rand_ship, rand_ship_near, M32, P1, P2, TICK, NPLAYERS, SHIP_BYTES

POOLP = 0x196f0
NREC, RECSZ = 30, 80
LIVE, GATE, SCROLL = 0x1a04, 0x1a00, 0x13c4


class ShotRig(G.Rig):
    def __init__(self, exe):
        super().__init__(exe)
        c = self.cpu
        self.objq = []
        for a in (0x383b8, 0x38754):
            c.mocks.pop(a, None)

        def objhit(cpu):
            self.log.append('obj_%d_%d_%d_%d' % tuple(s32(cpu.r[i]) for i in (3, 4, 5, 6)))
            a = [s32(cpu.r[i]) for i in (3, 4, 5, 6)]
            cpu.r[3] = 1 if (a[0] * 7 + a[1] * 13 + a[2] * 3 + a[3]) % 19 == 0 else 0     # deterministic in the arguments, like the real test (a second call with the same box gives the same answer)

        def inset(cpu):
            p, dh, dv = cpu.r[3], s32(cpu.r[4]), s32(cpu.r[5])
            # Rect {top, left, bottom, right} of s16
            top, left, bottom, right = struct.unpack('>hhhh', bytes(cpu.mem[p:p + 8]))
            left += dh; right -= dh; top += dv; bottom -= dv
            cpu.mem[p:p + 8] = struct.pack('>hhhh', *[((v + 0x8000) & 0xffff) - 0x8000 for v in (top, left, bottom, right)])
            cpu.r[3] = 0

        c.mocks[0x2ca38] = objhit
        c.mocks[0x60d64] = inset
        self.base = bytes(c.mem)

    def setup(self, tick, nplayers, ships, objq):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        self.objq = list(objq)
        c.ww(TICK, tick)
        c.wb(NPLAYERS, nplayers)
        self.set_ship(1, ships[0])
        self.set_ship(2, ships[1])

    def digest(self):
        c = self.cpu
        return 'h%016x.%d.%d' % (fnv64(c.mem[POOLP:POOLP + NREC * RECSZ]), s32(c.rw(LIVE)), c.rw(GATE))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            if kind == 'I':
                c.call(0x37f60, [])
            elif kind == 'A':
                k, m, fl, xb, yb = arg.split(',')
                self.call_f(0x383b8, [int(k), int(m), int(fl)], [int(xb, 16), int(yb, 16)])
            elif kind == 'P':
                k, cnt, snd, m, p, fl, xb, yb = arg.split(',')
                self.call_f(0x38754, [int(k), int(cnt), int(snd), int(m), int(p), int(fl)], [int(xb, 16), int(yb, 16)])
            elif kind == 'B':
                k, xb, yb, x2, y2, snd, m, fl = arg.split(',')
                self.call_f(0x37fd4, [int(k), 0, 0, int(snd), int(m), int(fl)], [int(xb, 16), int(yb, 16), int(x2, 16), int(y2, 16)])
            elif kind == 'F':
                c.ww(TICK, int(arg)); c.call(0x38b0c, [])
            elif kind == 'C':
                c.ww(TICK, int(arg)); c.call(0x38a1c, [])
            elif kind == 'D':
                sub, rest = arg[0], arg[1:]
                tr, sh = [int(v) for v in rest.split(',')]
                c.mem[0x12be] = tr; c.mem[0x12bc] = sh
                c.call(0x39020 if sub == 'a' else 0x39338, [])
            elif kind == 'V':
                c.ww(SCROLL, int(arg))
            elif kind == 'U':
                n, w = arg.split(':', 1)
                self.set_ship(int(n), unsparse(w, SHIP_BYTES))
            toks.append(self.digest() + ''.join(',' + l for l in self.log))
        return toks

    def call_f(self, addr, iargs, fbits):
        """integer args go to r5.. (r3, r4 are not used by these routines), float args to f1.."""
        c = self.cpu
        # ppcemu.call(addr, args, fargs): args fill r3.., so pad r3/r4
        c.call(addr, [0, 0] + iargs, fargs=[G.bitsf32(b) for b in fbits])


def gen(exe, out, seed, n):
    rng = random.Random(seed)
    rig = ShotRig(exe)
    lines = []

    def rpos():
        r = rng.random()
        if r < 0.1:
            return rng.choice([0.0, 416.0, 415.5, 416.5, -0.5, 0.5]), rng.choice([15.0, 400.0, 14.5, 400.5, 399.5, 15.5])
        if r < 0.15:
            return rng.uniform(-40, 460), rng.uniform(-10, 430)
        return rng.uniform(0, 416), rng.uniform(15, 400)

    for it in range(n):
        tick = rng.randint(1, 100000)
        ships = [rand_ship_near(rng, rng.random() < 0.85), rand_ship_near(rng, rng.random() < 0.7)]
        nplayers = rng.choice([1, 2, 2])
        ops = ['I']
        t = tick
        for _ in range(rng.randint(30, 260)):
            t += rng.choice([1, 1, 1, 2, 3])
            r = rng.random()
            if r < 0.30:
                x, y = rpos()
                kind = rng.randrange(12)       # 10 and 11 are rejected by the routine
                ops.append('P%d,%d,%d,%d,%d,%d,%08x,%08x' % (kind, rng.choice([0, 1, 1, 2, 3, 5, 8]), rng.choice([-1, 1, 3, 40, 56]),
                                                            rng.choice([0, 1, 2, 3, 3, 4]), rng.randint(0, 6), rng.choice([0, 0, 1, 2]), f32bits(x), f32bits(y)))
            elif r < 0.40:
                x, y = rpos()
                ops.append('A%d,%d,%d,%08x,%08x' % (rng.randrange(0, 4), rng.choice([1, 2]), rng.choice([0, 1]), f32bits(x), f32bits(y)))
            elif r < 0.45:
                x, y = rpos()
                x = rng.choice([x, rng.uniform(20, 520), 32.0, 480.0, 31.5, 480.5])
                x2, y2 = rpos()
                ops.append('B%d,%08x,%08x,%08x,%08x,%d,%d,%d' % (rng.randrange(0, 4), f32bits(x), f32bits(y), f32bits(x2), f32bits(y2), rng.choice([-1, 2, 9]), rng.choice([1, 2]), rng.choice([0, 1])))
                if rng.random() < 0.7:           # the muzzle flash lives two ticks: update right away (with and without scrolling)
                    ops.append('V%d' % rng.randint(0, 1))
                    ops.append('F%d' % t)
                    t += rng.choice([0, 1])
                    ops.append('F%d' % t)
            elif r < 0.50:                       # a burst of shots without any update in between: fills the pool
                x, y = rpos()
                for _ in range(rng.randint(3, 6)):
                    ops.append('P%d,%d,%d,%d,%d,%d,%08x,%08x' % (rng.randrange(10), rng.randint(5, 12), -1, rng.choice([1, 2, 3]), rng.randint(0, 6), rng.choice([0, 1]), f32bits(x), f32bits(y)))
            elif r < 0.52:
                ops.append('C%d' % t)
            elif r < 0.55:
                nn = rng.randint(1, 2)
                sh = rand_ship_near(rng, rng.random() < 0.85)
                ops.append('U%d:%s' % (nn, sparse(sh)))
            elif r < 0.58:
                ops.append('V%d' % rng.randint(0, 1))
            elif r < 0.64:
                ops.append('Da%d,%d' % (rng.randint(0, 1), rng.randint(0, 1)))
                ops.append('Db%d,%d' % (rng.randint(0, 1), rng.randint(0, 1)))
            else:
                ops.append('F%d' % t)
        head = 'seed=%d np=%d tick=%d S1=%s S2=%s' % (seed, nplayers, tick, sparse(ships[0]), sparse(ships[1]))
        rig.setup(tick, nplayers, ships, [])
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_shot_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 40
    print(gen(exe, out, seed, n), 'vectors ->', out)
