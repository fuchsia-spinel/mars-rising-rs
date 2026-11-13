#!/usr/bin/env python3
"""Generate golden test vectors for src/pickup.rs by *running the original PowerPC routines* in tools/re/ppcemu.py.

    python3 -I gen_pickup_vectors.py MarsRising.pef OUT.txt [seed] [n_per_kind]

Vector = one text line of `key=value` tokens (reader: tests/pickup_golden.rs):

    kind   spawn (0x2a040) | pupd (0x2b884, one frame of the whole pool) | coll (0x2c134, one ship collects one pickup)
    tick   game tick (D.13e4)
    count  pickup counter D.1754 before
    A, B   player 1 (0xc2c0) and player 2 (0xc458) 408-byte structs, sparse big-endian words
    K      the pickup pool (20 x 128 bytes, behind the pointer D.0fc4), sparse by slot
    Q      mock queues:  r:<raw random values>/d:<drone_add results>
    X      extra arguments of the call (spawn: kind,variant,param,announce,x_bits,y_bits  coll: slot,player)
    D      world data: scroll speed (D.13c4) and coin style (D.13d8)
    -> after: count2, A2, B2, K2, E (log of the mocked callee routines)

Mocked callees: 0xa0d4 Random(lo,hi), 0x6260c sqrt, 0x22670 effect, 0x29714 effect, 0x45bac sound, 0x2c760 drone add, HUD routines.
Everything else (the spawner itself and its recursion, 0x29ebc free, 0xe090, 0xecf0, 0xc098, the ship helpers 0x24164, 0x26210, 0x26fd8 ...)
is the original code.
"""
import sys, os, random, struct, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import gen_ship_vectors as G
from gen_ship_vectors import f32bits, sparse_words, noise_ship, rand_float, put8, put32, putf

P1 = 0xC2C0
P2 = 0xC458
SHIP_SIZE = 408
PP = 0x120000       # where D.0fc4 points: 20 records of 128 bytes
NP = 20
REC = 128
PCOUNT = 0x1754
PPTR = 0x0fc4
TICK = 0x13e4
SPEED = 0x13c4
COINSTYLE = 0x13d8


def s32(v):
    return v - (1 << 32) if v & 0x80000000 else v


class PH:
    def __init__(self, exe):
        self.H = G.Harness(exe)
        self.cpu = self.H.cpu
        c = self.cpu
        self.log = []
        self.q = {'r': [], 'd': []}
        c.mocks.clear()
        HUD_ARGC = {0x1fe28: 2, 0x1ff94: 2, 0x1f69c: 2, 0x1f848: 2, 0x3fbfc: 1, 0x2e6a8: 1, 0x38a1c: 0}

        def mk_hud(addr):
            def hud(cpu):
                n = HUD_ARGC[addr]
                self.log.append('hud_%x' % addr + ''.join('_%d' % s32(cpu.r[3 + i]) for i in range(n)))
            return hud

        for a in HUD_ARGC:
            c.mocks[a] = mk_hud(a)

        def rnd(cpu):
            lo, hi = s32(cpu.r[3]), s32(cpu.r[4])
            raw = self.q['r'].pop(0) if self.q['r'] else 0
            v = lo + raw % (hi - lo + 1)
            self.log.append('rnd_%d_%d_%d' % (lo, hi, v))
            cpu.r[3] = v & 0xffffffff

        def sqrt(cpu):
            cpu.ww(cpu.r[1] + 20, cpu.r[2])      # the CFM glue stub saves the TOC pointer here; the caller reloads it afterwards
            cpu.setf(1, math.sqrt(cpu.getf(1)))

        def fx(cpu):
            self.log.append('fx_%s_%08x_%08x' % ('_'.join(str(s32(cpu.r[i])) for i in (3, 4, 5, 6, 7, 8, 9)),
                                                 f32bits(cpu.getf(1)), f32bits(cpu.getf(2))))

        def aux(cpu):
            self.log.append('aux_%x_%d_%d_%d_%d_%d' % ((cpu.cur_target,) + tuple(s32(cpu.r[i]) for i in (3, 4, 5, 6, 7))))

        def snd(cpu):
            self.log.append('snd_%d_%d_%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[5])))

        def dadd(cpu):
            self.log.append('dadd_%d_%d' % (cpu.r[3] & 0xff, s32(cpu.r[4])))
            cpu.r[3] = self.q['d'].pop(0) if self.q['d'] else 0

        def fatal(cpu):
            raise RuntimeError('fatal routine reached')

        c.mocks.update({0xa0d4: rnd, 0x6260c: sqrt, 0x22670: fx, 0x29714: aux, 0x45bac: snd, 0x2c760: dadd,
                        0x88e4: fatal, 0xbf6c: fatal})

    def run(self, kind, tick, count, a, b, pool, q, data, extra):
        c = self.cpu
        c.mem[:] = self.H.base
        self.log = []
        self.q = {'r': list(q['r']), 'd': list(q['d'])}
        c.mem[P1:P1 + SHIP_SIZE] = a
        c.mem[P2:P2 + SHIP_SIZE] = b
        c.mem[PP:PP + NP * REC] = pool
        c.ww(PPTR, PP)
        c.ww(PCOUNT, count)
        c.ww(TICK, tick)
        c.ww(SPEED, data[0])
        c.mem[COINSTYLE] = data[1]
        if kind == 'spawn':
            k, var, par, ann, xb, yb = extra
            c.call(0x2a040, [0, 0, k, var, par, ann], fargs=[G.bitsf32(xb), G.bitsf32(yb)])
            xs = 'X=%d,%d,%d,%d,%08x,%08x' % (k, var, par, ann, xb, yb)
        elif kind == 'pupd':
            c.call(0x2b884, [P1, P2])
            xs = 'X=-'
        elif kind == 'coll':
            slot, player = extra
            c.call(0x2c134, [slot, P1 if player == 1 else P2])
            xs = 'X=%d,%d' % (slot, player)
        else:
            raise ValueError(kind)
        a2 = bytes(c.mem[P1:P1 + SHIP_SIZE])
        b2 = bytes(c.mem[P2:P2 + SHIP_SIZE])
        k2 = bytes(c.mem[PP:PP + NP * REC])
        return ('kind=%s tick=%d count=%d A=%s B=%s K=%s Q=%s D=%d,%d %s -> count2=%d A2=%s B2=%s K2=%s E=%s' % (
            kind, tick, count, sparse_words(a), sparse_words(b), pool_sparse(pool), q_str(q), data[0], data[1], xs,
            s32(c.rw(PCOUNT)), sparse_words(a2), sparse_words(b2), pool_sparse(k2), ';'.join(self.log) if self.log else '-'))


def q_str(q):
    return 'r:%s/d:%s' % (','.join(map(str, q['r'])) or '-', ','.join(map(str, q['d'])) or '-')


def pool_sparse(buf):
    out = []
    for s in range(NP):
        rec = buf[s * REC:(s + 1) * REC]
        if any(rec):
            out.append('%d:%s' % (s, ','.join('%08x' % struct.unpack('>I', bytes(rec[i:i + 4]))[0] for i in range(0, REC, 4))))
    return ';'.join(out) if out else '-'


# ------------------------------------------------------------------------------- random state
def pick_ship(rng, maxed=None, state=None):
    """A player struct. `maxed` = probability that every upgrade is at its maximum."""
    b = noise_ship(rng)
    for o in (0, 96, 97, 98, 99, 100, 101):
        b[o] &= 1
    b[0] = 1 if rng.random() < 0.85 else 0
    b[1] = state if state is not None else (3 if rng.random() < 0.8 else rng.choice([0, 1, 2, 4]))
    b[8] = rng.randint(1, 2)
    b[9] = rng.randint(1, 2)
    m = maxed if maxed is not None else rng.choice([0.0, 0.3, 0.7, 1.0])
    mx = lambda: rng.random() < m
    put32(b, 328, 5 if mx() else rng.randint(0, 5))             # spread
    put32(b, 296, 16 if mx() else rng.randint(1, 16))           # level
    put32(b, 332, 3 if mx() else rng.randint(0, 3))             # missiles
    put32(b, 80, 10 if mx() else rng.randint(1, 10))            # special
    putf(b, 52, 9.0 if mx() else rng.choice([7.2, 7.2, 9.0, 8.9, 9.5, 12.0, rand_float(rng, 5, 10)]))
    b[340] = 1 if mx() else rng.randint(0, 1)                   # keep
    b[341] = 1 if mx() else rng.randint(0, 1)                   # rear
    put32(b, 304, rng.randint(0, 3))
    b[308] = rng.randint(0, 1)
    b[309] = rng.randint(0, 1)
    put32(b, 12, rng.choice([1, 2, 3, 5, 11, 12, 13, rng.randint(-2, 20)]))      # lives
    put32(b, 112, rng.choice([0, 3, 6, 9, 12, 13, rng.randint(-5, 20)]))        # shield
    put32(b, 320, rng.choice([0, 5, 89, 90, 95, 99, rng.randint(0, 120)]))       # coins
    b[172] = rng.choice([1, 1, 2, 3, 4, 5, rng.randint(0, 8)])                  # armor / score multiplier
    put32(b, 152, rng.choice([0, 100, 29999, 29500, 30000, 59000, rng.randint(0, 100000), rng.getrandbits(32)]))   # score
    put32(b, 156, rng.choice([0, 30000, 30000, 70000, 110000, rng.randint(0, 200000)]))                            # next life
    put32(b, 168, rng.choice([0, 1, 2, 5, rng.randint(0, 9)]))
    putf(b, 20, rand_float(rng, -20, 400))
    putf(b, 24, rand_float(rng, -20, 460))
    put32(b, 36, rng.choice([48, 48, rng.randint(6, 70)]))
    put32(b, 40, rng.choice([49, 49, rng.randint(6, 70)]))
    return b


KINDS = [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21]


def rand_rec(rng, tick, kind=None):
    r = bytearray(REC)
    for i in range(0, REC, 4):
        if rng.random() < 0.25:
            r[i:i + 4] = struct.pack('>I', rng.getrandbits(32))
    k = kind if kind is not None else rng.choice(KINDS + [2, 2, 3, 5, 6, 12, 0, 1, 22, 30])
    r[0] = 1
    put32(r, 4, (tick - rng.choice([0, 1, 5, 100, 139, 140, 141, 184, 185, 186, 289, 290, 291, 500, rng.randint(0, 300)])) & 0xffffffff)
    put32(r, 8, k)
    put32(r, 12, 1)
    w = rng.choice([26, 30, 24, 29, 48, 48, rng.randint(4, 60)])
    h = rng.choice([26, 28, 20, 29, 29, rng.randint(4, 60)])
    put32(r, 40, w); put32(r, 44, h)
    put32(r, 48, w // 2); put32(r, 52, h // 2)
    putf(r, 16, rand_float(rng, -10, 420)); putf(r, 20, rand_float(rng, -10, 485))
    if rng.random() < 0.4:
        r[60] = 0
        for o in (24, 28, 32, 36):
            putf(r, o, 0.0)
    else:
        r[60] = rng.choice([1, 1, 1, 2, 255])
        putf(r, 24, rand_float(rng, -8, 8)); putf(r, 28, rand_float(rng, -8, 8))
        putf(r, 32, rand_float(rng, -1, 1)); putf(r, 36, rand_float(rng, -1, 1))
    put32(r, 56, rng.choice([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0, 100, rng.getrandbits(32)]))
    r[88] = rng.randint(0, 1)
    r[89] = rng.randint(0, 1)
    put32(r, 96, rng.choice([0, 1, 2, 3, rng.randint(-3, 30)]))
    put32(r, 100, (tick - rng.choice([0, 1, 2, 3, 4, 5, rng.randint(0, 10)])) & 0xffffffff)
    put32(r, 104, rng.choice([0, 1, 2, 3, 5, 8, 23, rng.randint(-2, 30)]))
    return r


def rand_pool(rng, tick, density):
    pool = bytearray(NP * REC)
    n = 0
    for s in range(NP):
        if rng.random() < density:
            pool[s * REC:(s + 1) * REC] = rand_rec(rng, tick)
            n += 1
        elif rng.random() < 0.3:       # stale data in a free slot (active byte clear)
            rec = rand_rec(rng, tick)
            rec[0] = 0
            pool[s * REC:(s + 1) * REC] = rec
    return pool, n


def rand_q(rng):
    return {'r': [rng.getrandbits(16) for _ in range(40)], 'd': [rng.randint(0, 1) for _ in range(3)]}


def gen(exe, out, seed=1, n=300):
    rng = random.Random(seed)
    ph = PH(exe)
    lines = []

    def data():
        return (rng.choice([0, 0, 1, 1, 2, 3]), rng.randint(0, 1))

    # spawner: every kind, with every variant / flag combination
    for _ in range(n * 6):
        tick = rng.randint(0, 100000)
        pool, cnt = rand_pool(rng, tick, rng.choice([0.0, 0.1, 0.3, 0.6, 0.9, 1.0]))
        for s in range(NP):          # spawn only makes sense on consistent pools most of the time
            pass
        count = cnt if rng.random() < 0.93 else max(0, cnt + rng.randint(-2, 3))
        a = pick_ship(rng)
        b = pick_ship(rng)
        k = rng.choice(KINDS + [2, 3, 3, 5, 6, 7, 9, 10, 18, 19, 1, 0, 22, 30])
        var = rng.choice([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, -1]) if k == 3 else rng.choice([1, 1, 0, 2, 5])
        par = rng.choice([0, 1, 1, 2, 5, 8, 256, 255])
        ann = rng.choice([0, 1, 1, 256])
        x = rand_float(rng, -20, 440)
        y = rand_float(rng, -20, 500)
        lines.append(ph.run('spawn', tick, count, a, b, pool, rand_q(rng), data(), (k, var, par, ann, f32bits(x), f32bits(y))))
    # one frame of the whole pool
    for _ in range(n * 6):
        tick = rng.randint(0, 100000)
        pool, cnt = rand_pool(rng, tick, rng.choice([0.0, 0.05, 0.2, 0.5, 0.9, 1.0]))
        count = cnt if rng.random() < 0.93 else rng.choice([0, 1, -1, cnt + 2])
        a = pick_ship(rng)
        b = pick_ship(rng)
        # park the ships on top of random live pickups so that collections actually happen
        for ship in (a, b):
            if rng.random() < 0.6:
                s = rng.randrange(NP)
                rec = pool[s * REC:(s + 1) * REC]
                px = struct.unpack('>f', bytes(rec[16:20]))[0]
                py = struct.unpack('>f', bytes(rec[20:24]))[0]
                if px == px and py == py and abs(px) < 1000 and abs(py) < 1000:
                    putf(ship, 20, px + rng.randint(-40, 30)); putf(ship, 24, py + rng.randint(-40, 30))
                    ship[1] = 3
        lines.append(ph.run('pupd', tick, count, a, b, pool, rand_q(rng), data(), None))
    # exact edge contacts: still pickups (no scroll) with the player hit box touching / overlapping the pickup box by -1, 0, +1 pixel
    for _ in range(n * 3):
        tick = rng.randint(0, 100000)
        pool = bytearray(NP * REC)
        cnt = 0
        for s in rng.sample(range(NP), rng.randint(1, 3)):
            rec = rand_rec(rng, tick, rng.choice([2, 3, 5, 6, 8, 12, 13, 21]))
            put32(rec, 4, (tick - rng.randint(0, 100)) & 0xffffffff)
            rec[60] = 0
            for o in (24, 28, 32, 36):
                putf(rec, o, 0.0)
            putf(rec, 16, float(rng.randint(30, 350))); putf(rec, 20, float(rng.randint(30, 400)))
            pool[s * REC:(s + 1) * REC] = rec
            cnt += 1
        a = pick_ship(rng, state=3)
        b = pick_ship(rng, state=3)
        a[0] = b[0] = 1
        for ship in (a, b):
            s = rng.choice([s for s in range(NP) if pool[s * REC]])
            rec = pool[s * REC:(s + 1) * REC]
            px = int(struct.unpack('>f', bytes(rec[16:20]))[0]); py = int(struct.unpack('>f', bytes(rec[20:24]))[0])
            pw = struct.unpack('>I', bytes(rec[40:44]))[0]; ph_ = struct.unpack('>I', bytes(rec[44:48]))[0]
            sw, sh = rng.choice([48, 30, 20]), rng.choice([49, 30, 20])
            put32(ship, 36, sw); put32(ship, 40, sh)
            d = rng.choice([-1, 0, 1])
            edge = rng.randint(0, 3)
            if edge == 0:      # ship box left edge (x+2) against the pickup right edge
                x, y = px + pw + d - 2, py + rng.randint(-10, 10)
            elif edge == 1:    # ship box right edge (x+2+w-4) against the pickup left edge
                x, y = px + d - (sw - 2), py + rng.randint(-10, 10)
            elif edge == 2:
                x, y = px + rng.randint(-10, 10), py + ph_ + d - 2
            else:
                x, y = px + rng.randint(-10, 10), py + d - (sh - 2)
            putf(ship, 20, float(x)); putf(ship, 24, float(y))
        lines.append(ph.run('pupd', tick, cnt, a, b, pool, rand_q(rng), (0, rng.randint(0, 1)), None))
    # collection effects: every kind, random ship state
    for _ in range(n * 10):
        tick = rng.randint(0, 100000)
        pool = bytearray(NP * REC)
        slot = rng.randrange(NP)
        k = rng.choice(KINDS + [2, 3, 3, 3, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 21, 0, 1, 22])
        pool[slot * REC:(slot + 1) * REC] = rand_rec(rng, tick, k)
        player = rng.randint(1, 2)
        a = pick_ship(rng)
        b = pick_ship(rng)
        lines.append(ph.run('coll', tick, 1, a, b, pool, rand_q(rng), data(), (slot, player)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_pickup_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 300
    print(gen(exe, out, seed, n), 'vectors ->', out)
