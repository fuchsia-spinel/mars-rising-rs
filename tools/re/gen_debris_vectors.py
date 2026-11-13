#!/usr/bin/env python3
"""Golden vectors for src/debris.rs (the debris effects: 40 coloured 3 x 3 pixel particles per burst) by running the original code in ppcemu.py.

    python3 -I gen_debris_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

Routines: initialisation 0x2942c (direction table 0x29470, record reset 0x2957c; the colour-grid reader 0x29668 needs the Toolbox and is mocked, the table it
fills is poked instead), spawners 0x29714 (outwards) / 0x298ec (inwards), update / draw 0x29b10. State: pool at *D.0fac (10 x 976 bytes), direction table at
*D.0fb0 (62 x 8 bytes), index D.1730, colour table at *D.0fa8 (130 bytes), scroll step D.13c4, the off-screen pixmap D.81bc (a 704 x 608 buffer whose
every nonzero byte is hashed after each operation). Mocked (logged) callees: random 0xa0d4, sound 0x45bac; mocked: sqrt 0x6260c, GetPixBaseAddr 0x621bc, SetGWorld 0x6191c.

A vector line: `seed=.. R=<random queue> C=<130 colour bytes, hex> ops=.. -> tokens`; ops:
  `I` initialise   `A<x>,<y>,<id>,<delay>,<snd>,<scrolls>` outwards   `B<...>` inwards   `U<scroll>` update   `W<off>,<hex>` poke a pool word   `V<off>,<hex>` poke a pool byte
token: `h<pool>.<stars>.<index>` then (after an update that drew something) `px<hash>.<count>`, then the logged calls.
"""
import sys, os, random, struct, math, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_enemy_vectors import fnv64, s32, f32bits, M32

POOL_PTR, STAR_PTR, COL_PTR, IDX, SCROLL, PIXH = 0xfac, 0xfb0, 0xfa8, 0x1730, 0x13c4, 0x81bc
BUF, ROW, ROWS = 0x310000, 704, 608
HANDLE, PIXMAP = 0x300000, 0x300010
RECS, SZ = 10, 976


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

        def pixbase(cpu):
            cpu.r[3] = BUF

        c.mocks.update({0xa0d4: rnd, 0x45bac: snd, 0x6260c: lambda cpu: cpu.setf(1, math.sqrt(cpu.getf(1))), 0x621bc: pixbase, 0x29668: lambda cpu: None, 0x6191c: lambda cpu: None})
        c.r[2] = 0x8000
        self.base = bytes(c.mem)
        self.pool = c.rw(POOL_PTR)
        self.stars = c.rw(STAR_PTR)
        self.cols = c.rw(COL_PTR)

    def setup(self, rq, colours):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        self.rq = list(rq)
        c.ww(SCROLL, 1)
        c.ww(PIXH, HANDLE)
        c.ww(HANDLE, PIXMAP)
        c.mem[PIXMAP + 4:PIXMAP + 6] = struct.pack('>H', ROW | 0x8000)
        c.mem[self.cols:self.cols + 130] = bytes(colours)

    def digest(self):
        c = self.cpu
        return 'h%016x.%016x.%d' % (fnv64(c.mem[self.pool:self.pool + RECS * SZ]), fnv64(c.mem[self.stars:self.stars + 62 * 8]), s32(c.rw(IDX)))

    def pixels(self):
        c = self.cpu
        buf = bytes(c.mem[BUF:BUF + ROW * ROWS])
        h, n = 0xcbf29ce484222325, 0
        for m in re.finditer(rb'[^\x00]+', buf):
            for k, v in enumerate(m.group(0)):
                pos = m.start() + k
                for byte in struct.pack('>IB', pos, v):
                    h = ((h ^ byte) * 0x100000001b3) & 0xffffffffffffffff
                n += 1
        if n:
            c.mem[BUF:BUF + ROW * ROWS] = bytes(ROW * ROWS)
        return ',px%016x.%d' % (h, n) if n else ''

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            a = arg.split(',') if arg else []
            px = ''
            if kind == 'I':
                c.call(0x2942c, [])
            elif kind in 'AB':
                x, y, i, dl, sn, sc = (int(v) for v in a)
                c.call(0x29714 if kind == 'A' else 0x298ec, [x & M32, y & M32, i & M32, dl & M32, sn & M32, sc & M32])
            elif kind == 'U':
                c.ww(SCROLL, int(a[0]) & M32)
                c.call(0x29b10, [])
                px = self.pixels()
            elif kind == 'W':
                c.ww(self.pool + int(a[0]), int(a[1], 16))
            elif kind == 'V':
                c.wb(self.pool + int(a[0]), int(a[1], 16))
            else:
                raise ValueError(op)
            toks.append(self.digest() + px + ''.join(',' + l for l in self.log))
        return toks


def gen(exe, out, seed=1, n=20):
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    for it in range(n):
        rq = [rng.getrandbits(16) for _ in range(900)]
        colours = [rng.randint(1, 255) for _ in range(130)]
        rig.setup(rq, colours)
        ops = ['I']
        scroll = rng.choice([1, 1, 1, 2, 0])
        heavy = rng.random() < 0.25
        for fr in range(rng.randint(40, 140)):
            for _ in range(rng.choice([0, 0, 0, 1, 1, 2]) + (3 if heavy and rng.random() < 0.5 else 0)):
                k = rng.choice('AABB')
                ops.append('%s%d,%d,%d,%d,%d,%d' % (
                    k, rng.choice([rng.randint(-60, 480), rng.randint(0, 416)]), rng.choice([rng.randint(-60, 540), rng.randint(0, 480)]),
                    rng.choice(list(range(13)) * 2 + [-1, -2, 13, 20, 255]), rng.choice([0, 0, 0, 1, 2, 5, 8, 14]),
                    rng.choice([-1, -1, 0, 11, 12, 25, 40, 255, 300]), rng.choice([0, 1, 1, 255, 256])))
            if rng.random() < 0.03: scroll = rng.choice([0, 1, 1, 2, 3])
            ops.append('U%d' % scroll)
            if rng.random() < 0.04:
                r = rng.randrange(RECS)
                ch = rng.random()
                if ch < 0.4: ops.append('W%d,%08x' % (r * SZ + 964, rng.choice([0, 1, 2, 0xffffffff, rng.randint(0, 5)])))
                elif ch < 0.7: ops.append('W%d,%08x' % (r * SZ + 968, rng.choice([0, 39, 40, 41, 0xffffffff])))
                else: ops.append('V%d,%02x' % (r * SZ + rng.choice([960, 961, 962]), rng.randint(0, 1)))
            if rng.random() < 0.01: ops.append('I')
        head = 'seed=%d R=%s C=%s' % (seed, ','.join('%x' % v for v in rq), ''.join('%02x' % v for v in colours))
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_debris_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 20
    print(gen(exe, out, seed, n), 'vectors ->', out)
