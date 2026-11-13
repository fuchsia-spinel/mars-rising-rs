#!/usr/bin/env python3
"""Golden vectors for src/hud.rs (the side-panel HUD) by running the original code in ppcemu.py.

    python3 -I gen_hud_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

Routines: init 0x1e950 (panel rectangles), score 0x2633c(ship, force), power bar 0x1ef5c(player, value, flag), shield: 0x1f51c(player, target) /
0x1f5c4(player) tick / 0x1f69c(player, n) draw, lives 0x1f848(player, lives, flash), weapon icon 0x1fe28(player, flag) and 0x1ff94(player, level),
hit flash 0x1fc50(player).
State compared after every operation: both ship structures (408 bytes at 0xc458 / 0xc2c0), the HUD variables D.154c..D.157c, the rectangle table D.cab2..D.cb72.
Mocked (logged) callees: the three surface copies 0xd8 (restore), 0x30 (present), 0x7c (store), the clipped blitters 0x307c (Anim) and 0x3634 (Flash), the port switches
0x15f0 / 0x1590 / 0x15c0, and the QuickDraw calls of 0x1fc50 (OpColor, PenMode, ForeColor, PaintRect). Rectangles are logged as top_left_bottom_right.

A vector line: `seed=.. T=<tick> O=<D.9340 +64,+66,+72,+74> S1=<ship words> S2=<ship words> ops=.. -> tokens`; ops:
  `I` init   `S<n>,<force>` score of ship n   `B<p>,<v>,<flag>` power bar   `E<p>,<target>` shield target   `K<p>` shield tick   `H<p>,<n>` shield draw
  `L<p>,<lives>,<flash>` lives   `W<p>,<flag>` weapon icon   `V<p>,<level>` weapon level   `F<p>` hit flash   `T<tick>` set tick
  `X<n>,<off>,<hex>,<len>` poke ship n   `Y<addr>,<hex>` poke a data word (HUD variables)
token: `h<ships>.<vars>.<rects>` then the logged calls.
"""
import os, sys, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_enemy_vectors import fnv64, sparse, unsparse, s32, f32bits, M32
from gen_ship_vectors import noise_ship, clean_flags, put8, put32

TICK = 0x13e4
P1, P2, SHIP = 0xc458, 0xc2c0, 408
VARS, VARS_END = 0x154c, 0x1580
RECTS, RECTS_END = 0xcab2, 0xcb74
ORG = 0x9340


def rect_at(cpu, a):
    return '%d_%d_%d_%d' % tuple(struct.unpack('>hhhh', bytes(cpu.mem[a:a + 8])))


def rect_words(r3, r4):
    return '%d_%d_%d_%d' % struct.unpack('>hhhh', struct.pack('>II', r3 & M32, r4 & M32))


class Rig:
    def __init__(self, exe):
        self.cpu = c = ppcemu.load(exe)
        c.keep_toc = True
        self.log = []

        def toc(cpu):
            cpu.ww(cpu.r[1] + 20, cpu.r[2])

        def restore(cpu):
            self.log.append('rs_%s:%s' % (rect_at(cpu, cpu.r[3]), rect_at(cpu, cpu.r[4])))

        def present(cpu):
            self.log.append('pr_%s:%s' % (rect_at(cpu, cpu.r[3]), rect_at(cpu, cpu.r[4])))

        def store(cpu):
            self.log.append('st_%s:%s' % (rect_words(cpu.r[3], cpu.r[4]), rect_words(cpu.r[5], cpu.r[6])))

        def prim(addr):
            def f(cpu):
                self.log.append('d%x_%s' % (addr, '_'.join(str(s32(cpu.r[i])) for i in range(3, 10))))
            return f

        def port(name):
            def f(cpu):
                self.log.append(name)
            return f

        def opcolor(cpu):
            self.log.append('oc_%d_%d_%d' % struct.unpack('>hhh', bytes(cpu.mem[cpu.r[3]:cpu.r[3] + 6])))
            toc(cpu)

        def pen(cpu):
            self.log.append('pm_%d' % s32(cpu.r[3]))
            toc(cpu)

        def fore(cpu):
            self.log.append('fc_%d' % s32(cpu.r[3]))
            toc(cpu)

        def paint(cpu):
            self.log.append('pt_%s' % rect_at(cpu, cpu.r[3]))
            toc(cpu)

        c.mocks.update({0xd8: restore, 0x30: present, 0x7c: store, 0x307c: prim(0x307c), 0x3634: prim(0x3634),
                        0x15f0: port('port_hud'), 0x1590: port('port_main'), 0x15c0: port('port_store'),
                        0x62474: opcolor, 0x6107c: pen, 0x6227c: fore, 0x60f44: paint})
        c.r[2] = 0x8000
        self.base = bytes(c.mem)

    def setup(self, tick, org, s1, s2):
        c = self.cpu
        c.mem[:] = self.base
        self.log = []
        c.ww(TICK, tick)
        for k, off in enumerate((64, 66, 72, 74)):
            c.mem[ORG + off:ORG + off + 2] = struct.pack('>h', org[k])
        c.mem[P1:P1 + SHIP] = bytes(s1)
        c.mem[P2:P2 + SHIP] = bytes(s2)

    def digest(self):
        c = self.cpu
        return 'h%016x.%016x.%016x.%016x' % (fnv64(c.mem[P1:P1 + SHIP]), fnv64(c.mem[P2:P2 + SHIP]), fnv64(c.mem[VARS:VARS_END]), fnv64(c.mem[RECTS:RECTS_END]))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            a = arg.split(',') if arg else []
            ship = lambda n: P1 if int(n) == 1 else P2
            if kind == 'I':
                c.call(0x1e950, [])
            elif kind == 'S':
                c.call(0x2633c, [ship(a[0]), int(a[1]) & M32])
            elif kind == 'B':
                c.call(0x1ef5c, [int(a[0]) & M32, int(a[1]) & M32, int(a[2]) & M32])
            elif kind == 'E':
                c.call(0x1f51c, [int(a[0]) & M32, int(a[1]) & M32])
            elif kind == 'K':
                c.call(0x1f5c4, [int(a[0]) & M32])
            elif kind == 'H':
                c.call(0x1f69c, [int(a[0]) & M32, int(a[1]) & M32])
            elif kind == 'L':
                c.call(0x1f848, [int(a[0]) & M32, int(a[1]) & M32, int(a[2]) & M32])
            elif kind == 'W':
                c.call(0x1fe28, [int(a[0]) & M32, int(a[1]) & M32])
            elif kind == 'V':
                c.call(0x1ff94, [int(a[0]) & M32, int(a[1]) & M32])
            elif kind == 'F':
                c.call(0x1fc50, [int(a[0]) & M32])
            elif kind == 'T':
                c.ww(TICK, int(arg))
            elif kind == 'X':
                base = ship(a[0])
                if int(a[3]) == 1: c.mem[base + int(a[1])] = int(a[2], 16) & 0xff
                else: c.ww(base + int(a[1]), int(a[2], 16))
            elif kind == 'Y':
                c.ww(int(a[0], 16), int(a[1], 16))
            else:
                raise ValueError(op)
            toks.append(self.digest() + ''.join(',' + l for l in self.log))
        return toks


def rand_ship(rng, n):
    b = noise_ship(rng)
    clean_flags(b, rng)
    b[9] = n
    put32(b, 152, rng.choice([0, 1, 9, 10, 4050, 99999, 1234567, 9999999, 10000000, 12345678, 2147483647, -1, -57, rng.randint(0, 9999999), rng.randint(0, 99999)]))
    b[160] = rng.choice([0, 0, 1])
    b[161] = rng.choice([0, 0, 1])
    return b


def gen(exe, out, seed=1, n=30):
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    players = [1, 1, 2, 2, 0, 3, 257]
    for it in range(n):
        tick = rng.randint(0, 5000)
        org = rng.choice([(64, 480, 0, 416), (64, 480, 0, 416), (0, 0, 0, 0), (rng.randint(0, 100), rng.randint(0, 600), rng.randint(0, 100), rng.randint(0, 600))])
        s = [rand_ship(rng, 1), rand_ship(rng, 2)]
        init = [bytearray(x) for x in s]
        rig.setup(tick, org, s[0], s[1])
        ops = ['I']
        t = tick
        for fr in range(rng.randint(60, 200)):
            t += rng.choice([1, 1, 1, 2, 3])
            ops.append('T%d' % t)
            for _ in range(rng.randint(0, 4)):
                r = rng.random()
                p = rng.choice(players)
                if r < 0.20:
                    ops.append('S%d,%d' % (rng.randint(1, 2), rng.choice([0, 0, 1, 256])))
                elif r < 0.36:
                    ops.append('B%d,%d,%d' % (p, rng.choice([0, 5, 19, 20, 21, 22, 23, 30, 40, 55, 56, 57, 58, 60, 80, -4, rng.randint(0, 70)]), rng.choice([0, 0, 1, 256, 2])))
                elif r < 0.46:
                    ops.append('E%d,%d' % (p, rng.choice([0, 3, 9, 10, 11, 12, 13, rng.randint(0, 14)])))
                elif r < 0.60:
                    ops.append('K%d' % p)
                elif r < 0.68:
                    ops.append('H%d,%d' % (p, rng.choice([-2, 0, 1, 2, 6, 11, 12, 13, 15, rng.randint(0, 14)])))
                elif r < 0.78:
                    ops.append('L%d,%d,%d' % (p, rng.choice([-1, 0, 1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 20]), rng.choice([0, 0, 1, 256])))
                elif r < 0.84:
                    ops.append('W%d,%d' % (p, rng.choice([0, 1, 2, 256])))
                elif r < 0.90:
                    ops.append('V%d,%d' % (p, rng.choice([0, 1, 2, 3, 4, 5, 6, 7, 255, 257])))
                elif r < 0.93:
                    ops.append('F%d' % p)
                elif r < 0.97:
                    nn = rng.randint(1, 2)
                    off, val, ln = rng.choice([(152, rng.choice([0, 7, 100, 1234567, 8888888, 12345678, -9, 4]) & M32, 4), (160, rng.randint(0, 1), 1), (161, rng.randint(0, 1), 1),
                                               (164, (t + rng.choice([-5, -1, 0, 1, 2, 3, 4, 5, 10])) & M32, 4), (9, rng.choice([1, 2, 3, 0]), 1)])
                    ops.append('X%d,%d,%x,%d' % (nn, off, val & M32, ln))
                else:
                    ops.append('Y%x,%x' % (rng.choice([0x154c, 0x1554, 0x1560, 0x1564, 0x156c, 0x1570, 0x1574, 0x1578, 0x1568, 0x155c]), rng.choice([0, 1, 2, 3, 10, 11, 12, t - 31, t - 30, t - 29, t + 5, 20, 21, 25, 38, 40, 60]) & M32))
        head = 'seed=%d T=%d O=%s S1=%s S2=%s' % (seed, tick, ','.join(str(v) for v in org), sparse(init[0]), sparse(init[1]))
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'a' if os.environ.get('HUD_APPEND') else 'w') as f:
        if not os.environ.get('HUD_APPEND'):
            f.write('# generated by tools/re/gen_hud_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


def gen_bound(exe, out, seed=1):
    """Hand-made edge sessions: the score flash timers around their thresholds (tick = flash tick + 1 / + 3 are the exact boundaries), the shield refill delay
    boundary and the power-bar thresholds."""
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    for it in range(6):
        tick = rng.randint(100, 5000)
        org = (64, 480, 0, 416)
        s = [rand_ship(rng, 1), rand_ship(rng, 2)]
        init = [bytearray(x) for x in s]
        rig.setup(tick, org, s[0], s[1])
        ops = ['I']
        t = tick
        for rep in range(3):
            for delta in range(-6, 3):
                for nn in (1, 2):
                    t += 5
                    ops += ['T%d' % t, 'X%d,164,%x,4' % (nn, (t + delta) & M32), 'X%d,161,1,1' % nn, 'X%d,160,%d,1' % (nn, rep % 2), 'S%d,%d' % (nn, rep // 2),
                            'S%d,0' % nn, 'X%d,152,%x,4' % (nn, rng.choice([0, 7, 123456, 9999999, 10000000]) & M32), 'X%d,161,1,1' % nn, 'S%d,1' % nn]
            # shield refill boundary
            for target in (3, 10, 11, 12):
                for p in (1, 2):
                    t += 40
                    ops += ['T%d' % t, 'E%d,%d' % (p, target)]
                    for k in range(40):
                        t += 1
                        ops += ['T%d' % t, 'K%d' % p]
            # power bar around the thresholds
            for v in (0, 19, 20, 21, 22, 23, 24, 25, 26, 56, 57, 58, 59, 60, 70, 20, 0):
                for fl in (0, 1):
                    t += 1
                    ops += ['T%d' % t, 'B1,%d,%d' % (v, fl), 'B2,%d,%d' % (v, fl)]
            for v in (60, 50, 40, 30, 25, 21, 20, 19, 0):
                ops += ['B1,%d,0' % v, 'B2,%d,0' % v, 'B1,%d,0' % v]
            for lv in (-1, 0, 1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 99):
                for fl in (0, 1):
                    ops += ['L1,%d,%d' % (lv, fl), 'L2,%d,%d' % (lv, fl)]
        head = 'seed=%d T=%d O=%s S1=%s S2=%s' % (seed, tick, ','.join(str(v) for v in org), sparse(init[0]), sparse(init[1]))
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'a') as f:
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 30
    if os.environ.get('HUD_BOUND'):
        print(gen_bound(exe, out, seed), 'boundary vectors appended ->', out)
    else:
        print(gen(exe, out, seed, n), 'vectors ->', out)
