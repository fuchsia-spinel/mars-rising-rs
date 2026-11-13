#!/usr/bin/env python3
"""Golden vectors for the draw steps in src/ship_draw.rs and PickupPool::draw by running the original routines in ppcemu.py.

    python3 -I gen_draw_vectors.py MarsRising.pef OUT.txt [seed] [n_per_kind]

kind   sdraw (0x257b4 ship) | bdraw (0x275c8 beam) | shdraw (0x2242c shots) | pdraw (0x2c480 pickups)
tokens S (ship 408 B, sparse words) P (shot pool 60x48) K (pickup pool 20x128) tick count pcount D=<translucent>,<shadows> Q=g:<ground target results>
after: S2 P2 K2 count2 pcount2 E (log). Mocked: the blit primitives 0x2f88 0x3170 0x3264 0x3540 0x344c (logged with their 7 arguments),
0x3fec8, 0x45bac, 0x45d54, 0x1f848.
"""
import sys, os, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_ship_vectors as G
import gen_pickup_vectors as GP
from gen_ship_vectors import sparse_words, noise_ship, rand_float, put32, putf, f32bits

SHIP = G.SHIP
SHIP_SIZE = 408
POOL, COUNT, SLOT, NSLOT = G.POOL, G.COUNT, G.SLOT, G.NSLOT
PP, NP, REC, PPTR, PCOUNT = GP.PP, GP.NP, GP.REC, GP.PPTR, GP.PCOUNT
TICK = 0x13e4
D_TRANS, D_SHADOW = 0x12be, 0x12bc
s32 = GP.s32


class DH:
    def __init__(self, exe):
        self.H = G.Harness(exe)
        self.cpu = self.H.cpu
        c = self.cpu
        self.log = []
        self.g = []
        c.mocks.clear()

        def mk_prim(addr):
            def prim(cpu):
                self.log.append('d%x_%s' % (addr, '_'.join(str(s32(cpu.r[i])) for i in range(3, 10))))
            return prim

        for a in (0x2f88, 0x3170, 0x3264, 0x3540, 0x344c):
            c.mocks[a] = mk_prim(a)

        def gt(cpu):
            self.log.append('gt_%s' % '_'.join(str(s32(cpu.r[i])) for i in range(3, 9)))
            cpu.r[3] = (self.g.pop(0) if self.g else -1) & 0xffffffff

        def snd(cpu):
            self.log.append('snd_%d_%d_%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[5])))

        def snp(cpu):
            self.log.append('snp_%d_%d_%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[5])))

        def hud(cpu):
            self.log.append('hud_1f848_%d_%d_%d' % (cpu.r[3] & 0xff, s32(cpu.r[4]), s32(cpu.r[5])))

        def fatal(cpu):
            raise RuntimeError('fatal routine reached')

        c.mocks.update({0x3fec8: gt, 0x45bac: snd, 0x45d54: snp, 0x1f848: hud, 0x88e4: fatal, 0xbf6c: fatal})

    def run(self, kind, tick, count, pcount, ship, pool, kpool, g, opts):
        c = self.cpu
        c.mem[:] = self.H.base
        self.log = []
        self.g = list(g)
        c.mem[SHIP:SHIP + SHIP_SIZE] = ship
        for s, rec in pool.items():
            c.mem[POOL + s * SLOT:POOL + (s + 1) * SLOT] = rec
        c.mem[PP:PP + NP * REC] = kpool
        c.ww(PPTR, PP)
        c.ww(COUNT, count)
        c.ww(PCOUNT, pcount)
        c.ww(TICK, tick)
        c.mem[D_TRANS] = opts[0]
        c.mem[D_SHADOW] = opts[1]
        addr = {'sdraw': 0x257b4, 'bdraw': 0x275c8, 'shdraw': 0x2242c, 'pdraw': 0x2c480}[kind]
        c.call(addr, [SHIP] if kind in ('sdraw', 'bdraw') else [])
        s2 = bytes(c.mem[SHIP:SHIP + SHIP_SIZE])
        p2 = bytearray(c.mem[POOL:POOL + NSLOT * SLOT])
        k2 = bytes(c.mem[PP:PP + NP * REC])
        return ('kind=%s tick=%d count=%d pcount=%d S=%s P=%s K=%s D=%d,%d Q=g:%s -> S2=%s P2=%s K2=%s count2=%d pcount2=%d E=%s' % (
            kind, tick, count, pcount, sparse_words(ship), G.pool_sparse_dict(pool), GP.pool_sparse(kpool), opts[0], opts[1],
            ','.join(map(str, g)) or '-', sparse_words(s2), G.pool_sparse_bytes(p2), GP.pool_sparse(k2), s32(c.rw(COUNT)), s32(c.rw(PCOUNT)),
            ';'.join(self.log) if self.log else '-'))


def draw_ship(rng, tick):
    b = noise_ship(rng)
    for o in (0, 96, 97, 98, 99, 100, 101):
        b[o] &= 1
    b[0] = 1
    b[1] = 3 if rng.random() < 0.9 else rng.choice([0, 1, 2])
    b[8] = rng.randint(1, 2)
    b[9] = rng.randint(1, 2)
    rt = lambda spread: (tick - rng.choice([0, 1, 2, 3, 4, 5, 6, 7, 80, 81, rng.randint(0, 120)] if spread else [0, 1, 2, 3])) & 0xffffffff
    put32(b, 76, rt(True))                                  # hud refresh
    b[72] = rng.randint(0, 1)
    put32(b, 336, rng.choice([tick, tick, tick, (tick - 1) & 0xffffffff, rt(True)]))   # last shot
    put32(b, 344, rng.choice([0, 1, 3, 3, 2, -1 & 0xffffffff, 7]))                    # pattern
    b[341] = rng.randint(0, 1)
    b[256] = rng.randint(0, 1)
    put32(b, 260, rt(True)); put32(b, 264, rng.choice([0, 1, 2, 3, 4, 5, 6, rng.randint(-2, 9)]))
    b[268] = rng.randint(0, 1)
    put32(b, 272, rt(True)); put32(b, 276, rng.choice([0, 1, 2, 3, 4, 5, 6, rng.randint(-2, 9)]))
    b[64] = rng.randint(0, 1); b[65] = rng.randint(0, 1); put32(b, 68, rt(True))
    b[144] = rng.randint(0, 1); put32(b, 148, rt(True))
    b[128] = 1 if rng.random() < 0.6 else 0
    put32(b, 132, rng.choice([(tick - d) & 0xffffffff for d in (0, 1, 40, 79, 80, 81, 200)] + [(tick + 3) & 0xffffffff]))
    b[136] = rng.randint(0, 1)
    put32(b, 140, rng.choice([0, 1, 7, 8, 9, 20]))
    b[116] = rng.randint(0, 1); b[117] = rng.randint(0, 1)
    put32(b, 120, rt(True)); put32(b, 124, rng.choice([0, 1, 2, 3, 4, 5, 6]))
    put32(b, 12, rng.randint(0, 13))
    put32(b, 56, rng.choice([14, 15, 14, 15, 99]))
    put32(b, 60, rng.choice([1, 2, 3, 4, 5, 0, 7]))
    put32(b, 36, rng.choice([48, 48, 30])); put32(b, 40, rng.choice([49, 49, 32]))
    put32(b, 44, rng.choice([24, 24, 20])); put32(b, 48, 24)
    G.putf(b, 20, rand_float(rng, -20, 430)); G.putf(b, 24, rand_float(rng, -20, 500))
    put32(b, 312, rng.randint(-30, 450) & 0xffffffff); put32(b, 316, rng.randint(-200, 500) & 0xffffffff)
    return b


def gen(exe, out, seed=1, n=300):
    rng = random.Random(seed)
    dh = DH(exe)
    lines = []
    opt = lambda: (rng.randint(0, 1), rng.randint(0, 1))
    for _ in range(n * 6):
        tick = rng.randint(0, 100000)
        ship = draw_ship(rng, tick)
        g = [rng.choice([-1, -1, 0, 3])]
        lines.append(dh.run('sdraw', tick, 0, 0, ship, {}, bytearray(NP * REC), g, opt()))
    for _ in range(n * 2):
        tick = rng.randint(0, 100000)
        b = draw_ship(rng, tick)
        b[1] = 3 if rng.random() < 0.9 else 1
        b[212] = rng.randint(0, 1)
        put32(b, 252, rng.choice([-3, -1, 0, 0, 1, 5]) & 0xffffffff)
        put32(b, 244, rng.choice([0, 1, 2, 3, 5, 9, -2 & 0xffffffff]))
        put32(b, 216, rng.randint(-40, 440) & 0xffffffff); put32(b, 220, rng.randint(-300, 520) & 0xffffffff)
        put32(b, 232, rng.choice([15, 17, 19, 21])); put32(b, 240, rng.randint(39, 42))
        lines.append(dh.run('bdraw', tick, 0, 0, b, {}, bytearray(NP * REC), [], opt()))
    for _ in range(n * 2):
        pool, cnt = G.rand_pool(rng, density=rng.choice([0.0, 0.1, 0.5, 1.0, None]))
        count = rng.choice([cnt, cnt, 0, -1, 5, 60])
        lines.append(dh.run('shdraw', rng.randint(0, 1000), count, 0, bytearray(SHIP_SIZE), pool, bytearray(NP * REC), [], opt()))
    for _ in range(n * 4):
        tick = rng.randint(0, 100000)
        kp, cnt = GP.rand_pool(rng, tick, rng.choice([0.0, 0.1, 0.4, 0.8, 1.0]))
        # push some pickups across the 64 px drop-off lines
        for s in range(NP):
            if kp[s * REC] and rng.random() < 0.4:
                w = struct.unpack('>I', bytes(kp[s * REC + 40:s * REC + 44]))[0]
                h = struct.unpack('>I', bytes(kp[s * REC + 44:s * REC + 48]))[0]
                edge = rng.randint(0, 3)
                d = rng.choice([-1, 0, 1, 2])
                if edge == 0: GP.putf(kp, s * REC + 16, -64.0 + d)
                elif edge == 1: GP.putf(kp, s * REC + 16, 480.0 - w + d)
                elif edge == 2: GP.putf(kp, s * REC + 20, -64.0 + d)
                else: GP.putf(kp, s * REC + 20, 544.0 - h + d)
        count = cnt if rng.random() < 0.9 else rng.choice([0, -1, cnt + 2])
        lines.append(dh.run('pdraw', tick, 0, count, bytearray(SHIP_SIZE), {}, kp, [], opt()))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_draw_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 100
    print(gen(exe, out, seed, n), 'vectors ->', out)
