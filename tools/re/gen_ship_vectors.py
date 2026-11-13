#!/usr/bin/env python3
"""Generate golden test vectors for src/ship.rs by *running the original PowerPC routines* in tools/re/ppcemu.py.

    python3 -I gen_ship_vectors.py MarsRising.pef OUT.txt [seed] [n_per_kind]

Every vector is one text line of `key=value` tokens (see tests/ship_golden.rs for the reader):

    kind   mv | front | beam | rear | upd | fire | bupd
    tick   game tick (data 0x13e4, what sub_ecf0 returns)
    count  value of the shot counter D[0x15dc] before
    S      the 408-byte player struct, sparse: idx=hex32,idx=hex32 (big endian words, zero words omitted)
    P      the shot pool, sparse by slot: slot:w0,...,w11;slot:...   ('-' if empty)
    Q      mock return queues: h=<damage list>/m=<missile results>/p=<is-playing list>  (comma separated)
    -> after the routine:  ret, count, S2, P2, E (event log of the mocked callee routines)

Mocked callees (their own behaviour is not part of the ship logic): 0x2ead4 enemy hit test, 0x22670 effects, 0x45bac sound,
0x277e4 missiles, 0x2c960 drones, 0x26fd8 overheat tail, 0x45ca4 / 0x51d2c / 0x51b4c / 0x45e24 sound handles.
"""
import sys, os, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

SHIP = 0x100000
SHIP_SIZE = 408
POOL = 0xd09c
COUNT = 0x15dc
TICK = 0x13e4
SLOT = 48
NSLOT = 60


def f32bits(x):
    return struct.unpack('>I', struct.pack('>f', x))[0]


def bitsf32(b):
    return struct.unpack('>f', struct.pack('>I', b))[0]


def sparse_words(buf):
    out = []
    for i in range(len(buf) // 4):
        w = struct.unpack('>I', bytes(buf[i * 4:i * 4 + 4]))[0]
        if w:
            out.append('%d=%08x' % (i, w))
    return ','.join(out) if out else '-'


def pool_sparse(mem):
    out = []
    for s in range(NSLOT):
        rec = mem[POOL + s * SLOT:POOL + (s + 1) * SLOT]
        if any(rec):
            out.append('%d:%s' % (s, ','.join('%08x' % struct.unpack('>I', bytes(rec[i:i + 4]))[0] for i in range(0, SLOT, 4))))
    return ';'.join(out) if out else '-'


class Harness:
    def __init__(self, exe):
        self.cpu = ppcemu.load(exe)
        self.base = bytes(self.cpu.mem)
        self.log = []
        self.start_level = 1
        self.level_bonus = 0
        self.q = {'h': [], 'm': [], 'p': []}
        c = self.cpu
        s32 = lambda v: v - (1 << 32) if v & 0x80000000 else v

        def pop(k):
            return self.q[k].pop(0) if self.q[k] else 0

        def hit(cpu):
            a = [s32(cpu.r[i]) for i in range(3, 9)]
            self.log.append('hit_%d_%d_%d_%d_%d_%d' % tuple(a))
            cpu.r[3] = pop('h') & 0xffffffff

        def fx(cpu):
            self.log.append('fx_%d_%d_%d_%d_%d_%d_%d' % tuple(s32(cpu.r[i]) for i in (3, 4, 5, 6, 7, 8, 9)))

        def aux(cpu):
            self.log.append('aux_%x_%d_%d_%d_%d_%d' % ((cpu.cur_target,) + tuple(s32(cpu.r[i]) for i in (3, 4, 5, 6, 7))))

        def drop(cpu):
            self.log.append('drop_%d_%d_%d' % (s32(cpu.r[5]), int(cpu.getf(1)), int(cpu.getf(2))))

        def d7a0(cpu):
            cpu.r[3] = self.start_level & 0xffffffff

        def d7a8(cpu):
            cpu.r[3] = self.level_bonus & 0xffffffff

        HUD_ARGC = {0x1fe28: 2, 0x1ff94: 2, 0x1f69c: 2, 0x1f848: 2, 0x1f51c: 2, 0x1f5c4: 1, 0x1fc50: 0, 0x28510: 1, 0x4560c: 1}

        def mk_hud(addr):
            def hud(cpu):
                n = HUD_ARGC[addr]
                self.log.append('hud_%x' % addr + ''.join('_%d' % s32(cpu.r[3 + i]) for i in range(n)))
            return hud

        def dres(cpu):
            self.log.append('dres_%d' % (cpu.r[3] & 0xff))

        def f5b8(cpu):
            self.log.append('demo')
            cpu.r[3] = pop('p')

        def snd(cpu):
            self.log.append('snd_%d_%d_%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[5])))

        def msl(cpu):
            self.log.append('msl')
            cpu.r[3] = pop('m')

        def drn(cpu):
            self.log.append('drn')

        def ovh(cpu):
            self.log.append('ovh')

        def nop(cpu):
            pass

        def chg(cpu):
            self.log.append('chg')
            cpu.r[3] = 0x77

        def playing(cpu):
            self.log.append('playing_%d' % cpu.r[3])
            cpu.r[3] = pop('p')

        def stop(cpu):
            self.log.append('stop_%d' % cpu.r[3])

        def stream(cpu):
            self.log.append('stream_%d' % (cpu.r[7] & 0xff))
            cpu.r[3] = 0x55

        def fatal(cpu):
            raise RuntimeError('fatal error routine reached')

        for a in HUD_ARGC:
            c.mocks[a] = mk_hud(a)
        c.mocks.update({0x29714: aux, 0x298ec: aux, 0x2a040: drop, 0xd7a0: d7a0, 0xd7a8: d7a8, 0x2c87c: dres, 0xf5b8: f5b8})
        c.mocks.update({0x2ead4: hit, 0x22670: fx, 0x45bac: snd, 0x277e4: msl, 0x2c960: drn, 0x26fd8: ovh,
                        0x24544: nop, 0x45ca4: chg, 0x51d2c: playing, 0x51b4c: stop, 0x45e24: stream,
                        0x88e4: fatal, 0xbf6c: fatal})

    def reset(self):
        self.cpu.mem[:] = self.base
        self.log = []
        self.q = {'h': [], 'm': [], 'p': []}

    def load_state(self, ship, pool, count, tick):
        c = self.cpu
        self.reset()
        c.mem[SHIP:SHIP + SHIP_SIZE] = ship
        for s, rec in pool.items():
            c.mem[POOL + s * SLOT:POOL + (s + 1) * SLOT] = rec
        c.ww(COUNT, count)
        c.ww(TICK, tick)

    def snapshot(self):
        c = self.cpu
        return (bytes(c.mem[SHIP:SHIP + SHIP_SIZE]), bytearray(c.mem[POOL:POOL + NSLOT * SLOT]), c.rw(COUNT))


# ---------------------------------------------------------------- random state builders
def put8(b, off, v): b[off] = v & 0xff
def put32(b, off, v): b[off:off + 4] = struct.pack('>I', v & 0xffffffff)
def putf(b, off, v): b[off:off + 4] = struct.pack('>f', v)


def rand_float(rng, lo, hi):
    r = rng.random()
    if r < 0.3:
        return float(rng.randint(int(lo), int(hi)))
    if r < 0.5:
        return rng.randint(int(lo * 2), int(hi * 2)) / 2.0
    return struct.unpack('>f', struct.pack('>f', rng.uniform(lo, hi)))[0]


LIFE_FLAGS = (64, 65, 72, 84, 116, 117, 128, 136, 144, 308, 309, 340, 341, 360, 212, 256, 268, 288, 108)


def noise_ship(rng):
    """Ship struct with random garbage in every word the ported routines do not model (checks they stay untouched)."""
    b = bytearray(SHIP_SIZE)
    for i in range(0, SHIP_SIZE, 4):
        b[i:i + 4] = struct.pack('>I', rng.getrandbits(32) if rng.random() < 0.3 else 0)
    for o in LIFE_FLAGS + (0, 96, 97, 98, 99, 100, 101, 108, 212, 256, 268, 288, 341, 360):
        b[o] &= 1
    return b


FLAG_OFFS = [0, 8, 9, 10, 84, 96, 97, 98, 99, 100, 101, 108, 212, 256, 268, 288, 341, 360, 340, 206, 308, 309, 64, 65, 72, 144, 168, 172]


def clean_flags(b, rng):
    for o in (0, 96, 97, 98, 99, 100, 101, 108, 212, 256, 268, 288, 341, 360):
        b[o] = rng.randint(0, 1)
    b[8] = rng.randint(1, 2)
    b[9] = rng.randint(1, 2)
    b[10] = rng.randint(0, 1)


def mv_case(rng):
    b = noise_ship(rng)
    clean_flags(b, rng)
    b[1] = 3 if rng.random() < 0.9 else rng.randint(0, 5)
    for o in (96, 97, 98, 99):
        b[o] = rng.randint(0, 1)
    vmax = rng.choice([7.2, 9.0, 7.2, rng.uniform(1, 12)])
    vmax = struct.unpack('>f', struct.pack('>f', vmax))[0]
    w = rng.choice([48, 48, 48, rng.randint(10, 80)])
    h = rng.choice([49, 49, 49, rng.randint(10, 80)])
    put32(b, 36, w); put32(b, 40, h)
    putf(b, 52, vmax)
    edge = rng.random()
    if edge < 0.3:
        x = rng.choice([0.0, 416.0 - w, 416.0 - w - 1, 1.0, 3.0, 400.0])
        y = rng.choice([0.0, 480.0 - h, 480.0 - h - 1, 1.0, 3.0, 450.0])
    else:
        x = rand_float(rng, -20, 440)
        y = rand_float(rng, -20, 500)
    putf(b, 20, x); putf(b, 24, y)
    vs = [0.0, 2.5, -2.5, 5.0, -5.0, 7.5, vmax, -vmax, vmax - 2.5, -(vmax - 2.5), 1.25, -1.25, 0.5, -0.5]
    putf(b, 28, rng.choice(vs) if rng.random() < 0.7 else rand_float(rng, -12, 12))
    putf(b, 32, rng.choice(vs) if rng.random() < 0.7 else rand_float(rng, -12, 12))
    put32(b, 60, rng.randint(0, 6))
    put32(b, 312, rng.randint(-200, 600) & 0xffffffff)
    put32(b, 316, rng.randint(-300, 700) & 0xffffffff)
    return b


def rand_shot(rng, active=True):
    r = bytearray(SLOT)
    r[0] = 1 if active else 0
    r[1] = rng.randint(0, 1)
    r[2] = rng.randint(1, 2)
    kind = rng.choice([0, 0, 1, 2])
    put32(r, 20, kind)
    x = rand_float(rng, -40, 440); y = rand_float(rng, -80, 520)
    putf(r, 4, x); putf(r, 8, y)
    putf(r, 12, rng.choice([0.0, 0.0, 1.5, -1.5, 3.0, -3.0]))
    putf(r, 16, rng.choice([-20.0, -30.0, 20.0, -5.0, 8.0, rand_float(rng, -30, 30)]))
    put32(r, 24, rng.choice([3, 4, 5, rng.randint(1, 30)]))
    put32(r, 28, rng.choice([10, 32, 16, 60, rng.randint(1, 80)]))
    put32(r, 32, rng.randint(30, 45))
    put32(r, 36, rng.randint(1, 4))
    put32(r, 40, rng.randint(-1, 6))
    put32(r, 44, rng.randint(0, 2000))
    return r


def rand_pool(rng, density=None, garbage=True):
    d = rng.random() if density is None else density
    pool = {}
    for s in range(NSLOT):
        if rng.random() < d:
            pool[s] = rand_shot(rng, True)
        elif garbage and rng.random() < 0.15:
            pool[s] = rand_shot(rng, False)
    cnt = sum(1 for r in pool.values() if r[0])
    return pool, cnt


def q_str(q):
    return '/'.join('%s=%s' % (k, ','.join(str(v) for v in q[k]) if q[k] else '-') for k in ('h', 'm', 'p'))


def pool_to_mem_dict(pool):
    return pool


def emit(out, kind, tick, count, ship, pool, q, ret, snap):
    s2, p2, c2 = snap
    line = 'kind=%s tick=%d count=%d S=%s P=%s Q=%s -> ret=%d count2=%d S2=%s P2=%s E=%s' % (
        kind, tick, count, sparse_words(ship), pool_sparse_dict(pool), q_str(q), ret, c2, sparse_words(s2),
        pool_sparse_bytes(p2), '')
    return line


def pool_sparse_dict(pool):
    out = []
    for s in sorted(pool):
        rec = pool[s]
        if any(rec):
            out.append('%d:%s' % (s, ','.join('%08x' % struct.unpack('>I', bytes(rec[i:i + 4]))[0] for i in range(0, SLOT, 4))))
    return ';'.join(out) if out else '-'


def pool_sparse_bytes(buf):
    out = []
    for s in range(NSLOT):
        rec = buf[s * SLOT:(s + 1) * SLOT]
        if any(rec):
            out.append('%d:%s' % (s, ','.join('%08x' % struct.unpack('>I', bytes(rec[i:i + 4]))[0] for i in range(0, SLOT, 4))))
    return ';'.join(out) if out else '-'


def run_case(H, kind, tick, count, ship, pool, q, addr, args=None, extra=None, session=None):
    extra = extra or {}
    H.load_state(ship, pool, count, tick)
    H.start_level = extra.get('sl', 1)
    H.level_bonus = extra.get('lb', 0)
    H.q = {k: list(v) for k, v in q.items()}
    tail = ''
    if kind in ('dmg', 'dstr', 'init'):
        H.cpu.mocks.pop(0x24544, None)       # run the real destruction routine
    else:
        H.cpu.mocks[0x24544] = lambda cpu: None   # fire tests: the overheat hook only logs 'ovh' (mock of 0x26fd8)
    if session is not None:
        c = H.cpu
        solo, other, demo, mode, ingame = session
        c.mem[0x13f4] = 1
        c.mem[0x13e3] = mode
        c.r[26] = SHIP
        c.r[27] = 1 if solo else 2
        c.r[21] = 1 if other else 0
        c.r[22] = 1 if ingame else 0
        c.r[23] = 1 if demo else 0
        c.r[30] = 60
        c.run_block(0xc9d0, 0xcc9c)
        ret = 0
        tail = ' go=%d ig=%d' % (1 - c.mem[0x13f4], c.r[22] & 0xff)
    else:
        ret = H.cpu.call(addr, args if args is not None else [SHIP]) & 0xffffffff
    snap = H.snapshot()
    s2, p2, c2 = snap
    ex = ''.join(' %s=%d' % (k, v) for k, v in extra.items())
    if session is not None:
        ex += ' X=%d,%d,%d,%d,%d' % (1 if session[0] else 0, 1 if session[1] else 0, 1 if session[2] else 0, session[3], 1 if session[4] else 0)
    return ('kind=%s tick=%d count=%d S=%s P=%s Q=%s%s -> ret=%d count2=%d S2=%s P2=%s E=%s%s' % (
        kind, tick, count, sparse_words(ship), pool_sparse_dict(pool), q_str(q), ex, ret, c2, sparse_words(s2),
        pool_sparse_bytes(p2), ';'.join(H.log) if H.log else '-', tail))


def spawn_ship(rng, kind, spread):
    b = noise_ship(rng)
    clean_flags(b, rng)
    b[1] = 3
    put32(b, 324, kind)
    put32(b, 328, spread)
    putf(b, 20, rand_float(rng, 0, 380))
    putf(b, 24, rand_float(rng, 0, 440))
    put32(b, 36, rng.choice([48, 48, rng.randint(8, 70)]))
    put32(b, 40, rng.choice([49, 49, rng.randint(8, 70)]))
    return b


def gen(exe, out_path, seed=1, n=400):
    rng = random.Random(seed)
    H = Harness(exe)
    lines = []
    for _ in range(n * 8):
        b = mv_case(rng)
        lines.append(run_case(H, 'mv', 0, 0, b, {}, {'h': [], 'm': [], 'p': []}, 0x25264))
    for _ in range(n):
        for kind_name, addr in (('front', 0x217c8), ('beam', 0x21538), ('rear', 0x2169c)):
            wk = rng.choice([0, 2]) if kind_name == 'front' else 0
            spread = rng.randint(0, 6) if kind_name == 'front' else rng.randint(0, 5)
            b = spawn_ship(rng, wk, spread)
            pool, cnt = rand_pool(rng, density=rng.choice([0.0, 0.1, 0.5, 0.8, 0.95, 1.0, None]))
            count = cnt if rng.random() < 0.9 else max(0, cnt + rng.randint(-3, 3))
            tick = rng.randint(0, 100000)
            lines.append(run_case(H, kind_name, tick, count, b, pool, {'h': [], 'm': [], 'p': []}, addr))
    for _ in range(n * 2):
        pool, cnt = rand_pool(rng, density=rng.choice([0.05, 0.2, 0.5, 1.0, None]))
        b = noise_ship(rng)
        clean_flags(b, rng)
        tick = rng.randint(0, 3000)
        q = {'h': [rng.choice([0, 0, 1, 2, 3, 5]) for _ in range(70)], 'm': [], 'p': []}
        lines.append(run_case(H, 'upd', tick, cnt, b, pool, q, 0x22170, args=[]))
    for _ in range(n * 4):
        b = fire_ship(rng)
        pool, cnt = rand_pool(rng, density=rng.choice([0.0, 0.1, 0.5, 0.9, 1.0, None]))
        tick = struct.unpack('>I', b[336:340])[0] + rng.choice([0, 1, 2, 3, 4, 5, 6, 40, 41, 42, 100, rng.randint(0, 200)])
        tick &= 0xffffffff
        q = {'h': [], 'm': [rng.randint(0, 1) for _ in range(3)], 'p': [rng.randint(0, 1) for _ in range(4)]}
        try:
            lines.append(run_case(H, 'fire', tick, cnt, b, pool, q, 0x26cf8))
        except RuntimeError:
            continue
    for _ in range(n * 2):
        b = beam_ship(rng)
        q = {'h': [rng.choice([0, 0, 1, 2, 3, 5, 20]) for _ in range(3)], 'm': [], 'p': []}
        lines.append(run_case(H, 'bupd', rng.randint(0, 3000), 0, b, {}, q, 0x27434))
    gen_life(H, rng, lines, n)
    with open(out_path, 'w') as f:
        f.write('# generated by tools/re/gen_ship_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


def life_ship(rng):
    b = noise_ship(rng)
    clean_flags(b, rng)
    for o in LIFE_FLAGS:
        b[o] = rng.randint(0, 1)
    b[0] = 1 if rng.random() < 0.9 else 0
    b[1] = rng.choice([0, 1, 2, 3, 3, 4])
    b[8] = rng.randint(1, 2)
    b[9] = rng.randint(1, 2)
    b[10] = rng.randint(0, 1)
    putf(b, 20, rand_float(rng, 0, 380)); putf(b, 24, rand_float(rng, 0, 440))
    put32(b, 36, 48); put32(b, 40, 49)
    put32(b, 44, rng.choice([24, 24, rng.randint(1, 40)])); put32(b, 48, rng.choice([24, 24, rng.randint(1, 40)]))
    putf(b, 52, rng.choice([7.2, 9.0, 9.0, 5.0]))
    put32(b, 4, rng.randint(0, 3000))
    put32(b, 12, rng.randint(-1, 5))
    put32(b, 56, rng.choice([14, 15, 15, 14, rng.randint(0, 20)]))
    put32(b, 68, rng.randint(0, 3000)); put32(b, 76, rng.randint(0, 3000))
    put32(b, 80, rng.randint(0, 12))
    put32(b, 112, rng.choice([-2, 0, 1, 2, 5, 6, 7, 12, 13, rng.randint(-3, 16)]) & 0xffffffff)
    put32(b, 296, rng.randint(1, 16)); put32(b, 304, rng.choice([0, 0, 1, 2]))
    put32(b, 320, rng.choice([0, 3, 9, 10, 11, 25, rng.randint(0, 99)]))
    put32(b, 324, rng.choice([0, 1, 2])); put32(b, 328, rng.randint(0, 5)); put32(b, 332, rng.randint(0, 3))
    b[172] = rng.randint(0, 255)
    put32(b, 120, rng.randint(0, 3000)); put32(b, 124, rng.randint(0, 8)); put32(b, 132, rng.randint(0, 3000)); put32(b, 140, rng.randint(0, 20))
    put32(b, 148, rng.randint(0, 3000))
    put32(b, 208, rng.choice([0, 25, 60])); put32(b, 336, rng.randint(0, 3000)); put32(b, 348, rng.randint(0, 3000))
    return b


def gen_life(H, rng, lines, n):
    nopool = {'h': [], 'm': [], 'p': []}
    for _ in range(n * 2):
        b = life_ship(rng)
        tick = rng.randint(0, 5000)
        amt = rng.randint(0, 8)
        lines.append(run_case(H, 'dmg', tick, 0, b, {}, nopool, 0x243a0, args=[SHIP, amt], extra={'A': amt}))
    for _ in range(n * 3):
        b = life_ship(rng)
        tick = rng.randint(0, 5000)
        q = rng.randint(0, 1)
        lines.append(run_case(H, 'dstr', tick, 0, b, {}, nopool, 0x24544, args=[SHIP, q], extra={'A': q, 'sl': rng.randint(1, 6)}))
    for _ in range(n):
        b = life_ship(rng)
        k = rng.randint(0, 1)
        lines.append(run_case(H, 'rst', rng.randint(0, 5000), 0, b, {}, nopool, 0x26680, args=[SHIP, k], extra={'A': k, 'sl': rng.randint(1, 20)}))
    for _ in range(n):
        b = life_ship(rng)
        k = rng.randint(0, 1)
        lines.append(run_case(H, 'init', rng.randint(0, 5000), 0, b, {}, nopool, 0x23de4, args=[SHIP, k],
                              extra={'A': k, 'sl': rng.randint(1, 20), 'lb': rng.randint(0, 2)}))
    for _ in range(n * 4):
        b = life_ship(rng)
        st = struct.unpack('>I', b[4:8])[0]
        tick = (st + rng.choice([0, 1, 40, 41, 60, 61, 62, 90, 91, 119, 120, 121, 500])) & 0xffffffff
        sess = (rng.random() < 0.5, rng.random() < 0.6, rng.random() < 0.2, rng.choice([0, 1, 2]), rng.random() < 0.8)
        q = {'h': [], 'm': [], 'p': [rng.randint(0, 1)]}
        lines.append(run_case(H, 'sess', tick, 0, b, {}, q, 0, session=sess))


def fire_ship(rng):
    b = noise_ship(rng)
    clean_flags(b, rng)
    b[1] = 3 if rng.random() < 0.95 else rng.randint(0, 5)
    b[100] = 1 if rng.random() < 0.6 else 0
    b[108] = rng.randint(0, 1)
    put32(b, 296, rng.randint(1, 16))
    put32(b, 300, rng.choice([0, 0, 0, 1, 2, rng.randint(0, 16)]))
    put32(b, 336, rng.randint(0, 5000))
    put32(b, 324, rng.choice([0, 1, 2]))
    put32(b, 328, rng.randint(0, 6))
    put32(b, 332, rng.choice([0, 0, 1, 2, 3]))
    put32(b, 348, rng.randint(0, 5000))
    put32(b, 304, rng.choice([0, 0, 1, 2]))
    put32(b, 208, rng.choice([0, 5, 19, 20, 21, 34, 35, 49, 50, 54, 55, 63, 64, rng.randint(0, 70)]))
    put32(b, 280, rng.choice([0, 100, 118, 119, 120, 150, rng.randint(0, 300)]))
    put32(b, 284, rng.randint(0, 40))
    put32(b, 292, rng.randint(0, 9))
    put32(b, 344, rng.randint(0, 3))
    put32(b, 356, rng.getrandbits(16))
    put32(b, 364, rng.getrandbits(16))
    putf(b, 20, rand_float(rng, 0, 380)); putf(b, 24, rand_float(rng, 0, 440))
    put32(b, 36, 48); put32(b, 40, 49)
    return b


def beam_ship(rng):
    b = noise_ship(rng)
    clean_flags(b, rng)
    b[1] = 3 if rng.random() < 0.95 else rng.randint(0, 5)
    b[212] = 1 if rng.random() < 0.9 else 0
    put32(b, 252, rng.choice([5, 4, 3, 2, 1, 0, -1, -4, rng.randint(-10, 8)]))
    w = rng.choice([15, 17, 19, 21])
    h = rng.choice([32, 48, 72, 96])
    put32(b, 232, w); put32(b, 236, h)
    put32(b, 216, rng.randint(-40, 440) & 0xffffffff)
    put32(b, 220, rng.randint(-120, 520) & 0xffffffff)
    put32(b, 224, 0)
    put32(b, 228, rng.choice([-20, -20, -20, 20, -5, -50]) & 0xffffffff)
    put32(b, 240, rng.randint(39, 42)); put32(b, 244, rng.randint(1, 3))
    put32(b, 248, rng.choice([1, 3, 6, 16, rng.randint(-3, 60)]) & 0xffffffff)
    putf(b, 20, rand_float(rng, 0, 380)); putf(b, 24, rand_float(rng, 0, 440))
    put32(b, 36, 48); put32(b, 40, 49)
    return b


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 400
    print(gen(exe, out, seed, n), 'vectors ->', out)
