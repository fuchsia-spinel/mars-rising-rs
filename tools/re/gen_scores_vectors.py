#!/usr/bin/env python3
"""Golden vectors for src/scores.rs (the high score table, its screen and the name entry) from the original code in ppcemu.py.

    python3 -I gen_scores_vectors.py MarsRising.pef OUT_VECTORS.txt [seed] [n]

Natively run original code: 0x199dc (does a score qualify), 0x19b28 (the name entry dialog: insertion into the table, the default / last
name, the name checks, the sounds), 0x194ac (the High Scores screen: the draw order of the 10 rows, the blinking of the new entries, the 600
tick time-out, the mouse / N key exit), 0x19894 (one row of the table) with the library routines NumToString, p2cstr, c2pstr, strcmp ...
Mocked: the Toolbox, the Dialog Manager calls (items are named by their number), the text drawing 0x29054 (verified by credits_golden), the
copies between the off-screen buffers (logged), TickCount / GetNextEvent / FlushEvents / the delay 0x967c (a virtual clock).

Lines (hex = bytes of the table / of a Pascal string):
    qual <player> <score1> <score2> <table> -> 0|1
    entry <player> <single> <score1> <score2> <level> <typed|-> <hits> <table> -> <ret> <idx1> <idx2> <table'> | <ops>
    screen <idx1> <idx2> <events> <table> -> <ops> end:<ticks> ret:0|1
Operations (ops):  snd:<id>:<prio>:<delay>  settext:<item>:<hex>  portrait:<id>  clear  pic:<id>  show  flush  delay:<n>
                   text:<hex>:<x>:<y>:<hilite>:<mono>  save:<rect>:<rect>  restore:<rect>:<rect>   (rects t.l.b.r) with @<tick> since the entry.
"""
import sys, os, struct, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
from gen_credits_vectors import Rig, s16

M32 = 0xffffffff
TABLE = 0xc674
TABLE_LEN = 332
D_IDX1, D_IDX2 = 0x1518, 0x151a          # table rows of the new scores (-1 none), player 1 / 2
D_SC1, D_SC2 = 0x1524, 0x1520            # final scores of player 1 / 2
D_SINGLE = 0x13e2                         # byte: single player game (0xece0)
D_LEVEL = 0x13ec                          # the value 0xd7a0 returns (stored beside the score)


class SRig(Rig):
    def __init__(self, exe):
        super().__init__(exe)
        c = self.c
        self.base = bytes(c.mem)
        n = {v: k for k, v in c.img.stub_name.items()}
        self.ops = []
        self.ticks = 1000
        self.t0 = 1000
        self.first_tick = True
        self.queue = []
        self.hits = []
        self.typed = b''
        self.item = 0

        def at():
            return '@%d' % (self.ticks - self.t0)

        def pstr(a):
            return bytes(c.mem[a + 1:a + 1 + c.mem[a]])

        def cstr(a):
            b = bytearray()
            while c.mem[a]: b.append(c.mem[a]); a += 1
            return bytes(b)

        def rect(a):
            return '.'.join(map(str, struct.unpack('>hhhh', bytes(c.mem[a:a + 8]))))

        def num2str(cpu):
            v = cpu.r[3]
            v = v - (1 << 32) if v & 0x80000000 else v
            s = str(v).encode()
            a = cpu.r[4]
            cpu.mem[a] = len(s); cpu.mem[a + 1:a + 1 + len(s)] = s

        def p2c(cpu):
            a = cpu.r[3]; ln = cpu.mem[a]
            cpu.mem[a:a + ln] = bytes(cpu.mem[a + 1:a + 1 + ln]); cpu.mem[a + ln] = 0

        def c2p(cpu):
            a = cpu.r[3]; s = cstr(a)[:255]
            cpu.mem[a + 1:a + 1 + len(s)] = s; cpu.mem[a] = len(s)

        def blockmove(cpu):
            s, d, k = cpu.r[3], cpu.r[4], cpu.r[5]
            cpu.mem[d:d + k] = bytes(cpu.mem[s:s + k])

        c.mocks[n['NumToString']] = num2str
        c.mocks[n['p2cstr']] = p2c
        c.mocks[n['c2pstr']] = c2p
        c.mocks[n['BlockMove']] = blockmove

        def tick(cpu):
            cpu.r[3] = self.ticks
            if self.first_tick: self.first_tick = False
            else: self.ticks += 1

        def delay(cpu):
            k = s16(cpu.r[3])
            self.ops.append('delay:%d%s' % (k, at()))
            self.ticks += k

        def getevent(cpu):
            now = self.ticks - 1
            for i, (t, what, msg) in enumerate(self.queue):
                if t + self.t0 <= now:
                    del self.queue[i]
                    a = cpu.r[4]
                    cpu.mem[a:a + 16] = struct.pack('>HIIHH', what, msg, 0, 0, 0) + bytes(2)
                    cpu.r[3] = 1
                    return
            cpu.r[3] = 0

        def flush(cpu):
            now = self.ticks - 1
            self.queue = [e for e in self.queue if e[0] + self.t0 > now]
            self.ops.append('flush' + at())

        c.mocks[n['TickCount']] = tick
        c.mocks[n['GetNextEvent']] = getevent
        c.mocks[n['FlushEvents']] = flush
        c.mocks[0x967c] = delay
        for a in (0x28750, 0x1620, 0xae6c, 0x456cc, 0x456d0):
            c.mocks[a] = lambda cpu: cpu.r.__setitem__(3, 0)
        c.mocks[0x1650] = lambda cpu: self.ops.append('clear' + at())
        c.mocks[0x1970] = lambda cpu: self.ops.append('pic:%d%s' % (s16(cpu.r[3]), at()))
        c.mocks[0x1b958] = lambda cpu: self.ops.append('show' + at())
        c.mocks[0x45bac] = lambda cpu: self.ops.append('snd:%d:%d:%d%s' % (cpu.r[3] & 0xff if cpu.r[3] & 0x80 == 0 else (cpu.r[3] & 0xff) - 256,
                                                                          cpu.r[4] & 0xff, cpu.r[5] & 0xff, at()))
        c.mocks[0x29054] = lambda cpu: self.ops.append('text:%s:%d:%d:%d:%d%s' % (cstr(cpu.r[3]).hex(), s16(cpu.r[4]), s16(cpu.r[5]), cpu.r[6] & 0xff,
                                                                                  cpu.r[7] & 0xff, at()))

        def pk(r):
            return '.'.join(str(s16(v)) for v in (r[0] >> 16, r[0], r[1] >> 16, r[1]))

        # 0x218(r3,r4 = the rect copied from the back buffer, r5,r6 = where it goes in the strip buffer): packed rects
        c.mocks[0x218] = lambda cpu: self.ops.append('save:%s:%s%s' % (pk((cpu.r[3], cpu.r[4])), pk((cpu.r[5], cpu.r[6])), at()))
        c.mocks[0x124] = lambda cpu: self.ops.append('restore:%s:%s%s' % (rect(cpu.r[3]), rect(cpu.r[4]), at()))

        # --- the Dialog Manager of the name entry
        c.mocks[0x480c8] = lambda cpu: cpu.r.__setitem__(3, 0x1000)       # GetNewDialog -> a dialog
        c.mocks[0x5b9f0] = lambda cpu: cpu.r.__setitem__(3, 0)

        def getitem(cpu):                                                    # GetDialogItem(dlg, item, &type, &handle, &box)
            self.item = cpu.r[4] & 0xffff
            cpu.ww(cpu.r[6], self.item)
        c.mocks[0x48628] = getitem

        def settext(cpu):                                                    # SetDialogItemText(handle, pstring)
            self.ops.append('settext:%d:%s' % (cpu.r[4], pstr(cpu.r[5]).hex()))
        c.mocks[0x487fc] = settext

        def gettext(cpu):                                                    # GetDialogItemText(handle, &pstring)
            a = cpu.r[5]
            s = self.typed if cpu.r[4] == 2 else b''
            cpu.mem[a] = len(s); cpu.mem[a + 1:a + 1 + len(s)] = s
        c.mocks[0x487d0] = gettext

        def modal(cpu):                                                      # ModalDialog(filter, &itemHit)
            a = cpu.r[5]
            cpu.mem[a:a + 2] = struct.pack('>h', self.hits.pop(0) if self.hits else 6)
        c.mocks[0x488d0] = modal
        c.mocks[0x1a0c] = lambda cpu: self.ops.append('portrait:%d' % s16(cpu.r[3]))
        for a in (0x47fcc, 0x1dc8c, 0x4804c, 0x486cc, 0x48778, 0x4a19c, 0x4a870, 0x488a8, 0x485e4, 0xf8d8):
            c.mocks[a] = lambda cpu: None
        self.pstr = pstr

    def reset(self, table, idx1=-1, idx2=-1, sc1=0, sc2=0, single=0, level=0):
        c = self.c
        c.mem[:] = self.base
        c.mem[TABLE:TABLE + TABLE_LEN] = table
        c.wh(D_IDX1, idx1); c.wh(D_IDX2, idx2)
        c.ww(D_SC1, sc1); c.ww(D_SC2, sc2)
        c.wb(D_SINGLE, single)
        c.ww(D_LEVEL, level)
        self.ops = []
        self.ticks = self.t0 = 1000
        self.first_tick = True

    def table_now(self):
        return bytes(self.c.mem[TABLE:TABLE + TABLE_LEN])

    def qual(self, player, s1, s2, table):
        self.reset(table, sc1=s1, sc2=s2)
        return self.c.call(0x199dc, [player], max_steps=10_000) & 0xff

    def entry(self, player, single, s1, s2, level, typed, hits, table):
        self.reset(table, sc1=s1, sc2=s2, single=single, level=level)
        self.typed = typed
        self.hits = list(hits)
        r = self.c.call(0x19b28, [player], max_steps=2_000_000) & 0xff
        c = self.c
        return r, s16(c.rh(D_IDX1)), s16(c.rh(D_IDX2)), self.table_now(), list(self.ops)

    def screen(self, table, idx1, idx2, events):
        self.reset(table, idx1, idx2)
        self.queue = sorted(events)
        r = self.c.call(0x194ac, [], max_steps=60_000_000) & 0xff
        return list(self.ops), self.ticks - self.t0, r


ALPHA = b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .,-\'!?'


def rand_table(rng):
    t = bytearray(TABLE_LEN)
    for i in range(12):
        n = rng.choice([0, 1, rng.randint(1, 20), 20, rng.randint(1, 12)])
        s = bytes(rng.choice(ALPHA) for _ in range(n))
        t[i * 21] = n
        t[i * 21 + 1:i * 21 + 1 + n] = s
    scores = sorted((rng.choice([rng.randint(0, 100000), rng.randint(0, 3000), 1000 * rng.randint(0, 10)]) for _ in range(10)), reverse=True)
    for i, v in enumerate(scores):
        t[252 + 4 * i:256 + 4 * i] = struct.pack('>i', v)
        t[292 + 4 * i:296 + 4 * i] = struct.pack('>i', rng.randint(0, 30))
    return bytes(t)


def mask_table(t):
    """The 21 byte name fields keep stack garbage behind the Pascal string in the original: only the string counts."""
    t = bytearray(t)
    for i in range(12):
        n = min(t[i * 21], 20)
        for k in range(1 + n, 21): t[i * 21 + k] = 0
    return bytes(t)


def main():
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 60
    rig = SRig(exe)
    rng = random.Random(seed)
    L = []
    for _ in range(n * 2):
        t = rand_table(rng)
        s1 = rng.choice([rng.randint(-5, 100000), struct.unpack('>i', t[288:292])[0], struct.unpack('>i', t[288:292])[0] + 1, struct.unpack('>i', t[288:292])[0] - 1])
        s2 = rng.choice([rng.randint(0, 100000), struct.unpack('>i', t[288:292])[0]])
        p = rng.choice([1, 2])
        L.append('qual %d %d %d %s -> %d' % (p, s1, s2, t.hex(), rig.qual(p, s1, s2, t)))
    names = [b'', b'Ann', b'david wareing', b'David Wareing', b'x' * 20, b'y' * 19, b'Swoop', b'a b']
    for _ in range(n * 3):
        t = rand_table(rng)
        scores = [struct.unpack('>i', t[252 + 4 * i:256 + 4 * i])[0] for i in range(10)]
        p = rng.choice([1, 2])
        single = rng.choice([0, 1])
        sc = rng.choice([rng.randint(0, 120000), rng.choice(scores), rng.choice(scores) + 1, rng.choice(scores) - 1, scores[9], scores[0] + 1])
        other = rng.randint(0, 5000)
        s1, s2 = (sc, other) if p == 1 else (other, sc)
        typed = rng.choice(names + [bytes(rng.choice(ALPHA) for _ in range(rng.randint(1, 20)))])
        hits = rng.choice([[1], [1], [6], [3, 1], [2, 6], [1, 6]])
        lvl = rng.randint(0, 40)
        r, i1, i2, t2, ops = rig.entry(p, single, s1, s2, lvl, typed, hits, t)
        L.append('entry %d %d %d %d %d %s %s %s -> %d %d %d %s | %s' % (p, single, s1, s2, lvl, typed.hex() or '-', ','.join(map(str, hits)), t.hex(), r, i1, i2,
                                                                      mask_table(t2).hex(), ' '.join(ops) or '-'))
    for k in range(n):
        t = rand_table(rng)
        i1 = rng.choice([-1, rng.randint(0, 9)])
        i2 = rng.choice([-1, rng.randint(0, 9)])
        if k % 4 == 0: i1 = i2 = -1
        if k % 4 == 1: i1, i2 = rng.randint(0, 9), -1
        evs = []
        used = set()
        for _ in range(rng.choice([0, 0, 1, 2, 3])):
            tt = rng.choice([rng.randint(0, 150), rng.randint(0, 700), rng.randint(95, 110)])
            if tt in used: continue
            used.add(tt)
            kind = rng.choice([(1, 0), (2, 0), (3, ord('N')), (3, ord('n')), (3, ord('x')), (3, 0x78 | 0x100), (4, ord('n')), (5, ord('n')), (0, 0), (3, 0xff6e), (3, 13)])
            evs.append((tt, kind[0], kind[1]))
        ops, end, ret = rig.screen(t, i1, i2, evs)
        L.append('screen %d %d %s %s -> %s end:%d ret:%d' % (i1, i2, ','.join('%d:%d:%d' % e for e in sorted(evs)) or '-', t.hex(), ' '.join(ops), end, ret))
    open(out, 'w').write('\n'.join(L) + '\n')
    print(len(L), 'vectors')


if __name__ == '__main__':
    main()
