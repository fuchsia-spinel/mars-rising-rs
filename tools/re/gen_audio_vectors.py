#!/usr/bin/env python3
"""Golden vectors for src/audio.rs by running the original sound code in ppcemu.py.

    python3 -I gen_audio_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

What runs in the emulator (all original code, nothing re-implemented except the Mac toolbox calls it makes):
    0x45a38 queue init | 0x45a6c per-frame queue pump | 0x45bac play(id, prio, delay) | 0x45ca4 start(id, prio) -> handle
    0x45d54 start with playback rate | 0x45e24 general request (callback + parameter) -> handle | 0x51b4c stop(handle)
    0x51d2c is-playing | 0x52030 the double-buffer callback: mixes one 1024-sample buffer (mono) and retires finished voices
Mocked toolbox glue: CallUniversalProc (interrupt mask, no-op), FixDiv, FixMul, BlockMoveData (memmove) and the callback glue 0x62cb4
(recorded). FixDiv truncates toward zero, FixMul rounds to nearest (ties up); both are assumptions about the Mac OS routines (see audio.rs).

Line:   rate=<out rate hex> max=<voices mixed> smp=<id>:<len>:<rate hex>:<seed>,... ops=<op;op;...> -> <tokens>
ops:    P<id>,<prio>,<delay>  T<now>  S<id>,<prio>  R<id>,<prio>,<rate>  Q<id>,<prio>,<cb>,<param>  X<handle>  I<handle>  B<n>  M<0|1>  E<0|1>
tokens: p (nothing) | t | h<handle> | x | i<count> | b<fnv64>/<voices>:<handle>.<remaining>,... | c<sel>.<cb>.<param> | m | e
The sample data of `smp` is lcg(seed): value = ((s >> 24) & 255) - 128, `len` samples, stored as the loader 0x51d98 does.
"""
import sys, os, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

M32 = 0xffffffff
SB, STRIDE = 0x300000, 0x4000
BUF = 0x2e0000
REQ = 0x2f0000           # request blocks for 0x45e24 (28 bytes each)
VOICES = 0x25868
TABLE = 0x25564
QUEUE = 0x25528
V_COUNT, V_MAX, V_RATE, V_STEREO, V_HANDLE, V_ENABLED = 0x1c84, 0x1c80, 0x1c88, 0x1c98, 0x1c9c, 0x1ca0
D_SOUND_ON, D_SUPPRESS, D_TICK = 0x1be4, 0x13e3, 0x13e4
RATES = [0x56EE8BA3, 0x2B7745D1, 0x56220000, 0xAC440000, 0x56EE8BA3]
PLAYRATES = [0x10000, 0x10000, 0x9000, 0x20000, 0x8000, 0x18000, 0x12345, 0xC000]


def s32(v): return v - (1 << 32) if v & 0x80000000 else v


def fixdiv(a, b):
    a, b = s32(a), s32(b)
    if b == 0: return 0x7fffffff if a >= 0 else 0x80000000
    n = a << 16
    q = abs(n) // abs(b)
    if (n < 0) != (b < 0): q = -q
    return max(-(1 << 31), min((1 << 31) - 1, q)) & M32


def fixmul(a, b):
    p = s32(a) * s32(b)
    return ((p + 0x8000) >> 16) & M32


def smp_data(n, seed):
    out = []; s = seed & M32
    for _ in range(n):
        s = (s * 1664525 + 1013904223) & M32
        out.append(((s >> 24) & 255) - 128)
    return out


def fnv64(b):
    h = 0xcbf29ce484222325
    for x in b:
        h = ((h ^ x) * 0x100000001b3) & 0xffffffffffffffff
    return h


class Rig:
    def __init__(self, exe):
        self.cpu = ppcemu.load(exe)
        self.base = bytes(self.cpu.mem)
        c = self.cpu
        c.keep_toc = True
        self.events = []
        c.mocks[0x6182c] = lambda cpu: None
        c.mocks[0x62414] = lambda cpu: cpu.r.__setitem__(3, fixdiv(cpu.r[3], cpu.r[4]))
        c.mocks[0x6242c] = lambda cpu: cpu.r.__setitem__(3, fixmul(cpu.r[3], cpu.r[4]))

        def move(cpu):
            src, dst, n = cpu.r[3], cpu.r[4], cpu.r[5]
            cpu.mem[dst:dst + n] = bytes(cpu.mem[src:src + n])
        c.mocks[0x60b24] = move

        def cb(cpu):
            sel = cpu.r[3]
            if sel in (1, 2):
                self.events.append('c%d.%d.%d' % (sel, cpu.r[12], cpu.rw(cpu.r[5] + 16)))
        c.mocks[0x62cb4] = cb

    def setup(self, out_rate, maxv, smps):
        c = self.cpu
        c.mem[:] = self.base
        self.events = []
        c.ww(V_COUNT, 0); c.ww(V_MAX, maxv); c.ww(V_RATE, out_rate); c.wb(V_STEREO, 0); c.ww(V_HANDLE, 1)
        c.wb(V_ENABLED, 1); c.wb(D_SOUND_ON, 1); c.wb(D_SUPPRESS, 0); c.ww(D_TICK, 0)
        for k, (sid, ln, rate, seed) in enumerate(smps):
            a = SB + k * STRIDE
            chunks = max(1, (ln + 1023) >> 10)
            c.mem[a:a + (chunks << 11) + 1036] = bytes((chunks << 11) + 1036)
            c.mem[a:a + 4] = b'snd '
            c.ww(a + 4, chunks); c.ww(a + 8, rate)
            for i, v in enumerate(smp_data(ln, seed)):
                c.wh(a + 12 + 2 * i, v & 0xffff)
            c.ww(TABLE + 4 * sid, a)
        c.call(0x45a38, [])

    def voices(self):
        c = self.cpu
        n = c.rw(V_COUNT)
        return ','.join('%d.%d' % (c.rw(VOICES + 52 * i), c.rw(VOICES + 52 * i + 12)) for i in range(n)) or '-'

    def take_events(self):
        e = self.events; self.events = []
        return e

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            kind, args = op[0], [int(x, 0) for x in op[1:].split(',')] if len(op) > 1 else []
            if kind == 'P':
                c.call(0x45bac, args); toks.append('p')
            elif kind == 'T':
                c.ww(D_TICK, args[0]); c.call(0x45a6c, []); toks.append('t')
            elif kind == 'S':
                toks.append('h%d' % c.call(0x45ca4, args))
            elif kind == 'R':
                c.call(0x45d54, args); toks.append('p')
            elif kind == 'Q':
                sid, prio, cbp, param = args
                req = REQ
                toks.append('h%d' % c.call(0x45e24, [sid, prio, req, cbp, param]))
            elif kind == 'X':
                c.call(0x51b4c, args); toks.append('x')
            elif kind == 'I':
                toks.append('i%d' % (c.call(0x51d2c, args) & 0xffff))
            elif kind == 'B':
                for _ in range(args[0]):
                    c.mem[BUF:BUF + 1040] = bytes(1040)
                    c.call(0x52030, [0, BUF])
                    toks.append('b%016x/%s' % (fnv64(c.mem[BUF + 16:BUF + 16 + 1024]), self.voices()))
            elif kind == 'M':
                c.wb(D_SUPPRESS, args[0]); toks.append('m')
            elif kind == 'E':
                c.wb(D_SOUND_ON, args[0]); toks.append('e')
            toks.extend(self.take_events())
        return toks


def gen(exe, out, seed=1, n=200):
    rng = random.Random(seed)
    rig = Rig(exe)
    lines = []
    for it in range(n):
        out_rate = rng.choice(RATES)
        maxv = rng.choice([6, 6, 6, 2, 16, 1])
        ns = rng.randint(1, 5)
        ids = rng.sample(range(72), ns)
        smps = []
        for sid in ids:
            ln = rng.choice([1, 5, 300, 1023, 1024, 1025, 2000, 3000, 5000, rng.randint(1, 6000)])
            smps.append((sid, ln, rng.choice(RATES), rng.getrandbits(32)))
        now = 0
        handles = []
        ops = []
        for _ in range(rng.randint(6, 40)):
            r = rng.random()
            sid = rng.choice(ids + [rng.randrange(72)] + ([-1, 72] if rng.random() < 0.05 else []))
            prio = rng.choice([0, 1, 2, 3, 4, 5, 6, 9])
            if r < 0.20:
                ops.append('P%d,%d,%d' % (sid & 0xff, prio, rng.choice([0, 0, 0, 1, 2, 3, 40])))
            elif r < 0.30:
                now += rng.choice([0, 1, 1, 2, 5, 50]); ops.append('T%d' % now)
            elif r < 0.40:
                ops.append('S%d,%d' % (sid & 0xff, prio))
            elif r < 0.48:
                ops.append('R%d,%d,%d' % (sid & 0xff, prio, rng.choice(PLAYRATES)))
            elif r < 0.56:
                ops.append('Q%d,%d,%d,%d' % (sid & 0xff, prio, rng.choice([0, 0x1d60, 0x2000]), rng.randrange(3)))
            elif r < 0.62:
                ops.append('X%d' % rng.choice([0, 1, 3, 5, 7, 9, 11, 13, 100]))
            elif r < 0.70:
                ops.append('I%d' % rng.choice([0, 1, 3, 5, 7, 9, 11, 13, 100]))
            elif r < 0.72:
                ops.append('M%d' % rng.choice([0, 1]))
            elif r < 0.74:
                ops.append('E%d' % rng.choice([0, 1, 1]))
            else:
                ops.append('B%d' % rng.choice([1, 1, 2, 3]))
        rig.setup(out_rate, maxv, smps)
        toks = rig.run(ops)
        lines.append('rate=%x max=%d smp=%s ops=%s -> %s' % (
            out_rate, maxv, ','.join('%d:%d:%x:%d' % s for s in smps), ';'.join(ops), ' '.join(toks)))
    # stress vectors: many requests at once to exercise the 16-slot table, priority insertion and the 6-voice mix
    for it in range(max(10, n // 5)):
        out_rate = 0x56EE8BA3
        ids = rng.sample(range(72), 3)
        smps = [(sid, rng.choice([2000, 3000, 5000, 6000]), rng.choice(RATES[:2]), rng.getrandbits(32)) for sid in ids]
        ops = []
        for _ in range(rng.randint(18, 34)):
            sid = rng.choice(ids)
            r = rng.random()
            if r < 0.6: ops.append('S%d,%d' % (sid, rng.randint(1, 5)))
            elif r < 0.8: ops.append('Q%d,%d,%d,%d' % (sid, rng.randint(1, 5), rng.choice([0x1d60, 0x1e18]), rng.randrange(2)))
            elif r < 0.9: ops.append('R%d,%d,%d' % (sid, rng.randint(1, 5), rng.choice(PLAYRATES)))
            else: ops.append('P%d,%d,0' % (sid, rng.randint(1, 5)))
        for _ in range(rng.randint(2, 5)):
            ops.append('B%d' % rng.choice([1, 2]))
            if rng.random() < 0.5: ops.append('X%d' % rng.choice([0, 3, 5, 9, 15, 31]))
            if rng.random() < 0.7: ops.append('S%d,%d' % (rng.choice(ids), rng.randint(1, 5)))
        rig.setup(out_rate, 6, smps)
        toks = rig.run(ops)
        lines.append('rate=%x max=%d smp=%s ops=%s -> %s' % (
            out_rate, 6, ','.join('%d:%d:%x:%d' % s for s in smps), ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_audio_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 200
    print(gen(exe, out, seed, n), 'vectors ->', out)
