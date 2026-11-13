#!/usr/bin/env python3
"""Golden vectors for src/enemy.rs by running the original enemy code in ppcemu.py.

    python3 -I gen_enemy_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors] [types]

State that matters (all addresses are data-section addresses, TOC r2 = 0x8000):
    enemy pool  0x17b70   40 slots x 176 bytes (D.0fd8 points there)       group table 0x14b00   100 x 124 bytes (D.0ca4)
    player 1    0xc458    player 2 0xc2c0 (only +1 state, +20/+24 x/y float, +36/+40 size, +44/+48 hit half size, +64 shield flag are read)
    tick        0x13e4    players D.13e2    D.17e4 / D.17e8 / D.1a00 (enemy-shot gate) counters

A vector is one line:  `ty=<type> seed=<n> G=<group words> S1=<ship words> S2=<..> R=<random raw queue> ops=<op;op;...> -> <tokens>`
ops:   `S<g>` spawn group g (0x2da30, which calls the type's init routine through the group's TVector)
       `F<tick>` set the tick and run one enemy frame (0x2dc20)
       `U<n>:<sparse words>` replace player n's relevant words
tokens (one per op): `h<enemy-pool hash>.<group-hash>.<counters>` then the mocked-call log entries `,<call>` in order.
Mocked callees (logged with their arguments, their own behaviour is outside the enemy code): Random 0xa0d4 (values come from the R queue),
score 0xe68c / 0xe6c0, ship damage 0xe6f4 / 0xe720, effects 0x22670 / 0x29714 / 0x23440 / 0x29edc, sound 0x45bac / 0x45d54, enemy shots 0x383b8 / 0x38754.
"""
import sys, os, random, struct, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

M32 = 0xffffffff
POOL, NPOOL, SLOT = 0x17b70, 40, 176
GROUPS, NGROUPS, GREC = 0x14b00, 100, 124
P1, P2 = 0xc458, 0xc2c0
TVBASE = 0x2f0000
TICK, NPLAYERS = 0x13e4, 0x13e2
C_17E4 = 0x17e4
SHIP_WORDS = (0, 1, 5, 6, 9, 10, 11, 12, 16)   # word indices of the player struct we set (offsets 0,4,20,24,36,40,44,48,64)
SHIP_BYTES = 0x44


def s32(v): return v - (1 << 32) if v & 0x80000000 else v
def f32bits(x): return struct.unpack('>I', struct.pack('>f', x))[0]
def bitsf32(b): return struct.unpack('>f', struct.pack('>I', b & M32))[0]


def fnv64(b):
    h = 0xcbf29ce484222325
    for x in b:
        h = ((h ^ x) * 0x100000001b3) & 0xffffffffffffffff
    return h


def sparse(buf):
    out = []
    for i in range(len(buf) // 4):
        w = struct.unpack('>I', bytes(buf[i * 4:i * 4 + 4]))[0]
        if w: out.append('%d=%08x' % (i, w))
    return ','.join(out) if out else '-'


def unsparse(s, size):
    b = bytearray(size)
    if s != '-':
        for tok in s.split(','):
            i, w = tok.split('=')
            b[int(i) * 4:int(i) * 4 + 4] = struct.pack('>I', int(w, 16))
    return b


class Rig:
    def __init__(self, exe):
        self.cpu = ppcemu.load(exe)
        self.base = bytes(self.cpu.mem)
        self.log = []
        self.rq = []
        self.shot_q = []
        c = self.cpu
        c.keep_toc = True

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
            # 0x383b8(r5..r7, x, y) and 0x38754(r5..r10, x, y): log the arguments the routines read, return a queued value
            n = 3 if cpu.cur_target == 0x383b8 else 6
            self.log.append('shot%x_%s_%08x_%08x' % (cpu.cur_target, '_'.join(str(s32(cpu.r[i])) for i in range(5, 5 + n)),
                                                    f32bits(cpu.getf(1)), f32bits(cpu.getf(2))))
            cpu.r[3] = self.shot_q.pop(0) if self.shot_q else 0

        def sqrt(cpu):
            cpu.ww(cpu.r[1] + 20, cpu.r[2])
            cpu.setf(1, math.sqrt(cpu.getf(1)))

        def tv(cpu):
            # CFM pointer glue 0x62cb4: r12 = TVector (code address, TOC); call the group's behaviour routine with the group index in r3
            code = cpu.rw(cpu.r[12])
            saved = (cpu.lr, cpu.pc, cpu.steps)
            g = cpu.r[3]
            cpu.call(code, [g])
            cpu.lr, cpu.pc, cpu.steps = saved

        def fatal(cpu):
            raise RuntimeError('fatal error routine reached')

        c.mocks.update({
            0xa0d4: rnd, 0x6260c: sqrt, 0x62cb4: tv,
            0xe68c: mk('score1', (3, 4)), 0xe6c0: mk('score2', (3, 4)),
            0xe6f4: mk('dmg1', (3,)), 0xe720: mk('dmg2', (3,)),
            0x22670: mk('fx', (3, 4, 5, 6, 7, 8, 9), 2), 0x29714: mk('aux', (3, 4, 5, 6, 7, 8)),
            0x23440: mk('part', (3, 4, 5, 6, 7, 8)), 0x29edc: mk('bonus', (5,), 2),
            0x45bac: mk('snd', (3, 4, 5)), 0x45d54: mk('sndr', (3, 4, 5)),
            0x383b8: shot, 0x38754: shot,
            0x88e4: fatal, 0xbf6c: fatal,
            0x6236c: lambda cpu: cpu.setf(1, math.atan(cpu.getf(1))),
        })
        def prim(addr):
            def f(cpu):    # the blit primitives of the draw routine 0x2e118: logged with r3..r9 (group, x, y, w, h, anim, frame)
                self.log.append('d%x_%s' % (addr, '_'.join(str(s32(cpu.r[i])) for i in range(3, 10))))
            return f
        for a in (0x2f88, 0x3170, 0x3264, 0x3540, 0x344c):
            c.mocks[a] = prim(a)
        # the arctangent table (s16 x 1024 at 0x8b2c) is built once at start-up by 0x92b4; run it so that the angle routine 0x9350/0x94bc works
        c.call(0x92b4, [])
        # the turret offset table D.1044 -> 0x14aa0 (24 x (s16, s16)), written by straight-line stores at the start of the level set-up 0x2d528
        c.r[2] = 0x8000
        c.run_block(0x2d580, 0x2d6dc)
        self.base = bytes(c.mem)

    def setup(self, tick, nplayers, groups, ships, rq, shotq=(), dif=0):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        self.rq = list(rq)
        self.shot_q = list(shotq)
        c.ww(TICK, tick)
        c.wb(NPLAYERS, nplayers)
        c.ww(0x13ec, dif)
        c.mem[GROUPS:GROUPS + NGROUPS * GREC] = groups
        for g in range(NGROUPS):           # TVectors {code, toc} of the groups' behaviour routines (normally built at level load by 0x2d7b8)
            p = struct.unpack('>I', bytes(groups[g * GREC + 84:g * GREC + 88]))[0]
            ty = struct.unpack('>I', bytes(groups[g * GREC + 44:g * GREC + 48]))[0]
            if TVBASE <= p < TVBASE + 8 * NGROUPS and ty in TYPE_INIT:
                c.ww(p, TYPE_INIT[ty]); c.ww(p + 4, 0x8000)
        self.set_ship(1, ships[0])
        self.set_ship(2, ships[1])

    def set_ship(self, n, words):
        base = P1 if n == 1 else P2
        c = self.cpu
        c.mem[base:base + SHIP_BYTES] = bytes(SHIP_BYTES)
        c.mem[base:base + len(words)] = words[:SHIP_BYTES]

    def digest(self):
        c = self.cpu
        return 'h%016x.%016x.%d' % (fnv64(c.mem[POOL:POOL + NPOOL * SLOT]), fnv64(c.mem[GROUPS:GROUPS + NGROUPS * GREC]), c.rw(C_17E4))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            if kind == 'S':
                c.call(0x2da30, [int(arg)])
            elif kind == 'F':
                c.ww(TICK, int(arg))
                c.call(0x2dc20, [])
            elif kind == 'D':            # 0x2e118 enemy draw / cull:  D<tick>,<translucent>,<shadows>
                tk, tr, sh = [int(v) for v in arg.split(',')]
                c.ww(TICK, tk)
                c.mem[0x12be] = tr
                c.mem[0x12bc] = sh
                c.call(0x2e118, [])
            elif kind == 'K':            # K<slot>,<mode>: hit box (+56..+68) in range except for one edge, which sits on / just past its limit
                sl, mode = [int(v) for v in arg.split(',')]
                base = POOL + sl * SLOT
                box = [100.0, 100.0, 200.0, 200.0]          # BX0, BY0, BX1, BY1
                ei, ev = [(2, -64.0), (2, -64.5), (0, 480.0), (0, 479.5), (3, -64.0), (3, -64.5), (1, 544.0), (1, 543.5)][mode]
                box[ei] = ev
                for k, v in enumerate(box): c.ww(base + 56 + 4 * k, f32bits(v))
            elif kind == 'X':            # 0x2e6a8 smart bomb by player <arg>
                c.call(0x2e6a8, [int(arg)])
            elif kind == 'B':            # mark slot as a boss part (+168 = 1) / 'b' clear it again; only the draw routine is run in between
                c.mem[POOL + int(arg) * SLOT + 168] = 1
            elif kind == 'P':            # P<slot>,<mode>: put the slot's position exactly on / just past a cull edge of the draw routine
                sl, mode = [int(v) for v in arg.split(',')]
                base = POOL + sl * SLOT
                wd, ht = s32(c.rw(base + 48)), s32(c.rw(base + 52))
                px, py = [(544.0 - wd, None), (544.0 - wd + 0.5, None), (None, -128.0), (None, -128.5), (None, 608.0 - ht), (None, 608.0 - ht + 0.5),
                          (-128.0, None), (-128.5, None)][mode]
                if px is not None: c.ww(base + 24, f32bits(px))
                if py is not None: c.ww(base + 28, f32bits(py))
            elif kind == 'b':
                c.mem[POOL + int(arg) * SLOT + 168] = 0
            elif kind == 'W':            # W<slot>,<offset>,<hex word>: poke a word into a pool slot (drives a slot into rarely reached states)
                sl, off, v = arg.split(',')
                c.ww(POOL + int(sl) * SLOT + int(off), int(v, 16))
            elif kind == 'V':            # V<slot>,<offset>,<byte>
                sl, off, v = arg.split(',')
                c.mem[POOL + int(sl) * SLOT + int(off)] = int(v) & 0xff
            elif kind == 'H':
                sl, amt, own = [int(v) for v in arg.split(',')]
                r = c.call(0x2ec14, [sl, amt, own])
                self.log.append('ret%d' % s32(r))
            elif kind == 'U':
                n, w = arg.split(':', 1)
                self.set_ship(int(n), unsparse(w, SHIP_BYTES))
            toks.append(self.digest() + ''.join(',' + l for l in self.log))
        return toks


def float_bits(rng, lo, hi):
    r = rng.random()
    if r < 0.4: v = float(rng.randint(int(lo), int(hi)))
    elif r < 0.6: v = rng.randint(int(lo * 2), int(hi * 2)) / 2.0
    else: v = rng.uniform(lo, hi)
    return f32bits(v)


def rand_ship(rng, alive=True):
    b = bytearray(SHIP_BYTES)
    b[1] = 3 if (alive and rng.random() < 0.92) else rng.randint(0, 5)
    w = rng.choice([48, 48, rng.randint(20, 64)]); h = rng.choice([49, 49, rng.randint(20, 64)])
    struct.pack_into('>I', b, 20, float_bits(rng, -20, 440))
    struct.pack_into('>I', b, 24, float_bits(rng, -20, 520))
    struct.pack_into('>I', b, 36, w); struct.pack_into('>I', b, 40, h)
    struct.pack_into('>I', b, 44, w // 2); struct.pack_into('>I', b, 48, h // 2)
    b[64] = 1 if rng.random() < 0.15 else 0
    return b


def rand_group(rng, ty, g_index):
    """Group record (124 bytes): the fields the spawn code and the init routines read."""
    b = bytearray(GREC)
    def w(i, v): struct.pack_into('>I', b, i * 4, v & M32)
    w(11, ty)                          # +44 enemy type
    w(12, rng.choice([1, 2, 3, 4, 5, 6, 8, 10, 10, 12, -1]))   # +48 member count
    for i in (15, 16, 17, 19, 20):     # +60..+80 formation / path parameters
        w(i, rng.randint(0, 40))
    w(16, rng.randint(0, 14)); w(17, rng.randint(0, 33))      # +64 x code (0x191ac), +68 y code (0x19248)
    w(22, 0)                           # +88 special-spawn flag
    return b


def rand_ship_near(rng, alive=True):
    """A ship inside the playfield (so that chasing / shooting enemies come into range)."""
    b = rand_ship(rng, alive)
    struct.pack_into('>I', b, 20, float_bits(rng, 60, 360))
    struct.pack_into('>I', b, 24, float_bits(rng, 80, 430))
    return b


LONG_TYPES = (15, 16, 17, 18, 19, 20, 21, 23)
DRAW_LONG_TYPES = (7, 8, 9, 10, 12, 13, 14, 20, 3, 6)


def gen(exe, out, seed=1, n=60, types=None, nlong=0, draw=False, poke=False, poke2=False):
    poke = poke or poke2
    rng = random.Random(seed)
    rig = Rig(exe)
    types = types or [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 23]
    lines = []
    from itertools import cycle
    tcycle = cycle(types)
    long_types = [t for t in (types or []) if t in (DRAW_LONG_TYPES if draw else LONG_TYPES)] or list(DRAW_LONG_TYPES if draw else LONG_TYPES)
    for it in range(n + nlong):
        long = it >= n
        ty = long_types[(it - n) % len(long_types)] if long else next(tcycle)
        tick = rng.randint(1, 100000)
        dif = rng.randint(0, 6)
        groups = bytearray(NGROUPS * GREC)
        ng = 1 if (long or poke) else rng.randint(1, 3)
        gl = []
        for gi in range(ng):
            g = rand_group(rng, ty, gi)
            struct.pack_into('>I', g, 84, TVBASE + 8 * gi)
            groups[gi * GREC:(gi + 1) * GREC] = g
            gl.append(gi)
        ships = [rand_ship(rng), rand_ship(rng, rng.random() < 0.85)]
        if long or poke:
            ships = [rand_ship_near(rng), rand_ship_near(rng, rng.random() < 0.7)]
        need_alive = False    # these use 0x2e8e0, which leaves stale registers behind when no ship is alive
        if need_alive:
            ships[0][1] = 3 if ships[1][1] != 3 else ships[0][1]
        nplayers = rng.choice([1, 2, 2])
        rq = [rng.getrandbits(16) for _ in range(160 if not long else 1200)]
        shotq = [rng.randint(0, 1) for _ in range(200 if not long else 2000)]
        ops = []
        t = tick
        alive = [ships[0][1] == 3, ships[1][1] == 3]
        cur_ships = [bytearray(ships[0]), bytearray(ships[1])]
        for gi in gl:
            ops.append('S%d' % gi)
            for fr in range(rng.randint(400, 600) if long else (rng.randint(60, 200) if poke else rng.randint(8, 60))):
                t += 1
                ops.append('F%d' % t)
                if poke and rng.random() < (0.3 if poke2 else 0.15):     # drive the slots into rarely reached states: position / velocity / acceleration / counters
                    for _ in range(rng.randint(1, 3)):
                        sl = rng.randrange(0, 12)
                        k = rng.randrange(14 if poke2 else 9)
                        if poke2 and ty == 4 and rng.random() < 0.8: k = 12          # type 4 slots are short-lived: put them next to a ship right away
                        if k == 0: ops.append('W%d,28,%08x' % (sl, f32bits(rng.uniform(60, 420))))                     # y
                        elif k == 1: ops.append('W%d,24,%08x' % (sl, f32bits(rng.uniform(20, 400))))                   # x
                        elif k in (2, 3):                                                                                # vx / vy: big, tiny, signed
                            v = rng.choice([0.02, -0.02, 0.0, 1.0, -1.0, 3.0, -3.0, 6.5, -6.5, 7.5, -7.5, 9.5, -9.5, 10.5, -10.5, 13.0, -13.0, 20.0, -20.0])
                            ops.append('W%d,%d,%08x' % (sl, 32 if k == 2 else 36, f32bits(v)))
                        elif k in (4, 5):                                                                                # ax / ay
                            v = rng.choice([0.0, 0.1, -0.1, 0.25, -0.25, 0.5, -0.5, 1.0, -1.0])
                            ops.append('W%d,%d,%08x' % (sl, 40 if k == 4 else 44, f32bits(v)))
                        elif k == 6: ops.append('V%d,%d,%d' % (sl, rng.choice([104, 148, 92]), rng.randint(0, 2)))
                        elif k == 7: ops.append('W%d,%d,%08x' % (sl, rng.choice([84, 112, 156]), rng.randint(0, 30)))   # state / timer / counter
                        elif k == 8: ops.append('W%d,%d,%08x' % (sl, rng.choice([140, 144, 160, 164]), f32bits(rng.uniform(-40, 480))))
                        elif k == 9:                                                                                           # vx just above zero (the `vx > 0` tests), also in the settled phase (+104 = 1, +12 = 1, y < 180)
                            ops.append('W%d,32,%08x' % (sl, f32bits(rng.choice([0.02, 0.01, 0.04]))))
                            if rng.random() < 0.5:
                                ops.append('W%d,40,00000000' % sl); ops.append('V%d,104,%d' % (sl, rng.randint(0, 1))); ops.append('W%d,12,00000001' % sl)
                                ops.append('W%d,28,%08x' % (sl, f32bits(rng.uniform(60, 175))))
                        elif k == 10: ops.append('W%d,28,%08x' % (sl, f32bits(rng.choice([0.02, 75.0, 80.0, 85.0, 128.0, 140.0, 310.0, 330.0, 360.0]))))   # y on / near window edges
                        elif k == 11: ops.append('W%d,84,%08x' % (sl, rng.choice([6, 7, 8, 24, 25, 28, 29])))               # state / frame
                        else:                                                                                                  # k 12/13: next to a ship, as if about to be hit / flash in (+104 = 0, delay = 0)
                            shp = rng.choice(cur_ships)
                            sx = struct.unpack('>f', bytes(shp[20:24]))[0]; sy = struct.unpack('>f', bytes(shp[24:28]))[0]
                            sx += struct.unpack('>I', bytes(shp[44:48]))[0]; sy += struct.unpack('>I', bytes(shp[48:52]))[0]      # ship centre (what the enemy routines compare against)
                            ops.append('W%d,24,%08x' % (sl, f32bits(sx + rng.choice([-62.0, -55.0, -49.0, -45.0, 0.0, 45.0, 49.0, 55.0, 62.0]))))
                            ops.append('W%d,28,%08x' % (sl, f32bits(sy + rng.choice([-62.0, -55.0, -49.0, -45.0, 0.0, 45.0, 85.0, 90.0, 95.0, 100.0, 105.0]))))
                            ops.append('V%d,104,0' % sl); ops.append('W%d,20,00000001' % sl)
                if draw and rng.random() < (0.12 if long else 0.3):
                    if rng.random() < 0.2:
                        ops.append('P%d,%d' % (rng.randrange(6), rng.randrange(8)))
                    if rng.random() < 0.15:
                        bs = rng.randrange(10)
                        ops.append('B%d' % bs)
                        ops.append('D%d,%d,%d' % (t, rng.randint(0, 1), rng.randint(0, 1)))
                        ops.append('b%d' % bs)
                    ops.append('D%d,%d,%d' % (t, rng.randint(0, 1), rng.randint(0, 1)))
                if rng.random() < (0.002 if long else 0.12):
                    ops.append('H%d,%d,%d' % (rng.randrange(40) if rng.random() < 0.3 else rng.randrange(1, 10), rng.choice([1, 1, 2, 3, 5, 8, 20]), rng.randint(1, 2)))
                if rng.random() < (0.04 if long else 0.15):
                    nn = rng.randint(1, 2)
                    sh = (rand_ship_near if long else rand_ship)(rng, rng.random() < 0.9)
                    al = list(alive); al[nn - 1] = sh[1] == 3
                    if need_alive and not any(al):
                        sh[1] = 3; al[nn - 1] = True
                    alive = al
                    cur_ships[nn - 1] = bytearray(sh)
                    ops.append('U%d:%s' % (nn, sparse(sh)))
            if not long and not draw and rng.random() < 0.12:       # smart bomb
                for _ in range(rng.randint(0, 3)):
                    ops.append('K%d,%d' % (rng.randrange(10), rng.randrange(8)))
                ops.append('X%d' % rng.randint(1, 2))
            if not long and rng.random() < 0.35:       # wipe the group out (bonus item path)
                own = rng.randint(1, 2)
                for sl in range(0, 14):
                    ops.append('H%d,20000,%d' % (sl, own))
        head = 'ty=%d seed=%d np=%d tick=%d dif=%d G=%s S1=%s S2=%s R=%s Q=%s' % (
            ty, seed, nplayers, tick, dif, sparse(groups), sparse(ships[0]), sparse(ships[1]),
            ','.join('%x' % v for v in rq), ','.join(str(v) for v in shotq))
        rig.setup(tick, nplayers, groups, ships, rq, shotq, dif)
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_enemy_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


# init (group behaviour) routine per enemy type = ENEMY_TYPES[].behaviour_code (src/enemies.rs)
TYPE_INIT = {1: 0x33cf4, 2: 0x344e8, 3: 0x33a64, 4: 0x34868, 5: 0x340d0, 6: 0x34b28, 7: 0x34de0, 8: 0x35124, 9: 0x35410, 10: 0x35714, 11: 0x359ec,
             12: 0x35d38, 13: 0x36080, 14: 0x363b4, 15: 0x36724, 16: 0x369ec, 17: 0x36c88, 18: 0x36fd4, 19: 0x3727c, 20: 0x37568, 21: 0x3785c, 23: 0x37c74}

if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 60
    types = [int(x) for x in sys.argv[5].split(',')] if len(sys.argv) > 5 else None
    nlong = int(sys.argv[6]) if len(sys.argv) > 6 else 0
    draw = len(sys.argv) > 7 and sys.argv[7] == 'draw'
    poke = len(sys.argv) > 7 and sys.argv[7] == 'poke'
    poke2 = len(sys.argv) > 7 and sys.argv[7] == 'poke2'
    print(gen(exe, out, seed, n, types, nlong, draw, poke, poke2), 'vectors ->', out)
