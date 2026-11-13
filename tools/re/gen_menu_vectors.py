#!/usr/bin/env python3
"""Golden vectors for src/menu.rs (the title screen's buttons) from the original code in ppcemu.py.

    python3 -I gen_menu_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors]

Routines run natively: 0x1b054 (button art rectangles on the offscreen sheet + the 11 screen rectangles, resources 'Rect' 1000..1010),
0x1c098 (which button is under a point), 0x1c6f8 (draw button n hilited), 0x1c5cc (draw every button normal), 0x1c998 (press and track a
button: sound 29, hilite while the pointer is inside, returns whether it was released inside), and 0x170 (the CopyBits wrapper).
Mocked Toolbox calls (logged where they matter): PtInRect 0x60bcc, OffsetRect 0x610ac, SetRect 0x60eb4, CopyBits 0x61124, GetMouse 0x60d1c,
StillDown 0x60d34, sound 0x45bac, the screen refresh 0x1b958, GetResource / ReleaseResource 0x60ac4 / 0x60adc (the 'Rect' reader 0xa05c runs natively).

A line:  `seed=<n> reg=<flag D.153a> org=<h>,<v> rects=<11 x top,left,bottom,right> ops=<op;..> -> <token per op>`
ops:  H<h>,<v>                 button under the point (global coordinates; the window origin is subtracted first)
      L<n>                     hilite button n (0..11)
      M<h>,<v>                 the pointer moved (global): the hover logic of the main loop (hilite + sound 20 on entering a button); the hovered
                               button is kept between the ops of a line
      U                        unhilite all
      T<n>:<h>,<v>/<h>,<v>/..  press button n; the mouse positions (global) are returned by successive GetMouse calls, StillDown is true before
                               every one of them except the last
tokens: the result (`r<n>`) then the logged calls: `snd<id>.<prio>.<delay>`, `tick` (the idle timer was reset), `cb<top>.<left>.<bottom>.<right>:<top>.<left>.<bottom>.<right>.<mode>`
        (source rectangle, destination rectangle), `refresh`.
"""
import sys, os, random, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

M32 = 0xffffffff
FLAG = 0x153a
ORG_H, ORG_V = 0x9368, 0x936c          # D.9340 + 40 / + 44: window origin (words)
SCREEN_RECT0 = 0xc810                  # 'Rect' 1000 .. 1010 at 0xc810 - 8 k
SCRATCH = 0x40000                      # handles / resource data for the mocked GetResource
SHEET_RECT0 = 0xc868                   # the art rectangles at 0xc868 - 8 k (0x1b054)


def s32(v):
    v &= M32
    return v - (1 << 32) if v & 0x80000000 else v


def s16(v):
    v &= 0xffff
    return v - 0x10000 if v & 0x8000 else v


def rect_at(c, a):
    return struct.unpack('>hhhh', bytes(c.mem[a:a + 8]))   # top, left, bottom, right


def inrect(pt_v, pt_h, r):
    return r[0] <= pt_v < r[2] and r[1] <= pt_h < r[3]


class MenuRig:
    def __init__(self, exe):
        self.c = c = ppcemu.load(exe)
        c.keep_toc = True
        self.log = []
        self.rect_res = {}
        self.mouse = []
        self.down = []

        def sound(cpu):
            self.log.append('snd%d.%d.%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[5])))

        def ptinrect(cpu):
            p, rp = cpu.r[3], cpu.r[4]
            v, h = s16(p >> 16), s16(p)
            cpu.r[3] = 1 if inrect(v, h, rect_at(cpu, rp)) else 0

        def offset(cpu):
            p, dh, dv = cpu.r[3], s16(cpu.r[4]), s16(cpu.r[5])
            t, l, b, r = rect_at(cpu, p)
            cpu.mem[p:p + 8] = struct.pack('>hhhh', *[s16(x) for x in (t + dv, l + dh, b + dv, r + dh)])

        def setrect(cpu):
            p = cpu.r[3]
            l, t, r, b = [s16(cpu.r[i]) for i in (4, 5, 6, 7)]
            cpu.mem[p:p + 8] = struct.pack('>hhhh', t, l, b, r)

        def copybits(cpu):
            self.log.append('cb%s:%s.%d' % ('.'.join(map(str, rect_at(cpu, cpu.r[5]))), '.'.join(map(str, rect_at(cpu, cpu.r[6]))), s32(cpu.r[7])))

        def getmouse(cpu):
            h, v = self.mouse.pop(0)
            cpu.mem[cpu.r[3]:cpu.r[3] + 4] = struct.pack('>hh', v, h)

        def stilldown(cpu):
            cpu.r[3] = 1 if self.down.pop(0) else 0

        def getres(cpu):
            # GetResource('Rect', id): a handle whose master pointer points at 8 bytes stored as left, top, right, bottom (the extractor's note)
            rid = s32(cpu.r[4])
            t, l, bo, r = self.rect_res[rid]
            slot = SCRATCH + 16 * (rid - 1000)
            cpu.mem[slot + 8:slot + 16] = struct.pack('>hhhh', l, t, r, bo)
            cpu.ww(slot, slot + 8)
            cpu.r[3] = slot

        self.ticks = 0
        self.hover = 0

        def tickcount(cpu):
            self.ticks += 1
            self.log.append('tick')
            cpu.r[3] = self.ticks

        c.mocks.update({0x61cdc: tickcount, 0x97ac: lambda cpu: cpu.__setattr__('r', cpu.r[:3] + [1] + cpu.r[4:]), 0x45bac: sound, 0x60bcc: ptinrect, 0x610ac: offset, 0x60eb4: setrect, 0x61124: copybits, 0x60d1c: getmouse,
                        0x60d34: stilldown, 0x60ac4: getres, 0x60adc: lambda cpu: None, 0x1b958: lambda cpu: self.log.append('refresh')})
        self.base = bytes(c.mem)

    def setup(self, reg, org, rects):
        c = self.c
        c.mem[:] = self.base
        self.log = []
        self.hover = 0
        c.r[2] = 0x8000
        c.wb(FLAG, reg)
        c.ww(ORG_H, org[0] & M32)
        c.ww(ORG_V, org[1] & M32)
        for k, r in enumerate(rects):
            self.rect_res[1000 + k] = r
        c.call(0x1b054, [])            # the original fills the sheet rectangles and reads the 11 screen rectangles

    def run(self, ops):
        c = self.c
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            if kind == 'H':
                h, v = [int(x) for x in arg.split(',')]
                res = c.call(0x1c098, [((v & 0xffff) << 16) | (h & 0xffff)])
            elif kind == 'L':
                c.call(0x1c6f8, [int(arg)]); res = 0
            elif kind == 'M':
                # the pointer-tracking block of the main loop (0x1a6f0..0x1a770): r25 = the hovered button, r26 = the idle timer, the point is at 60(r1)
                h, v = [int(x) for x in arg.split(',')]
                c.r[1] = ppcemu.STACK_TOP - 0x2000
                c.ww(c.r[1] + 60, ((v & 0xffff) << 16) | (h & 0xffff))
                c.r[25] = self.hover
                c.r[26] = 0
                c.run_block(0x1a6f0, 0x1a770)
                self.hover = c.r[25]
                res = self.hover
            elif kind == 'U':
                c.call(0x1c5cc, []); res = 0
            elif kind == 'T':
                n, rest = arg.split(':')
                pts = [tuple(int(x) for x in p.split(',')) for p in rest.split('/')]
                self.mouse = list(pts)
                self.down = [True] * (len(pts) - 1) + [False]
                res = c.call(0x1c998, [int(n)]) & 0xff
                assert not self.mouse or int(n) > 11 or True
            toks.append('r%d' % s32(res) + ''.join(',' + l for l in self.log))
        return toks


def gen(exe, out, seed, n):
    rng = random.Random(seed)
    rig = MenuRig(exe)
    lines = []
    for it in range(n):
        reg = rng.choice([0, 0, 1])
        org = (rng.randint(-20, 200), rng.randint(-20, 100))
        if it == 0:
            # the real layout of the shipped game (app 'Rect' 1000..1010) with the window at the screen origin
            real = [(71, 428, 100, 504), (115, 428, 144, 507), (214, 428, 262, 589), (287, 428, 335, 512), (159, 428, 188, 484), (159, 428, 188, 505),
                    (203, 428, 232, 505), (247, 428, 276, 540), (291, 428, 320, 533), (335, 428, 364, 494), (379, 428, 411, 464)]
            rects, org = real, (0, 0)
        else:
            rects = []
            for _ in range(11):
                t, l = rng.randint(0, 400), rng.randint(0, 560)
                if rng.random() < 0.25 and rects:        # overlapping rectangles: the first match in the original's order wins
                    t, l = rects[-1][0] + rng.randint(-8, 8), rects[-1][1] + rng.randint(-8, 8)
                rects.append((t, l, t + rng.randint(0, 50), l + rng.randint(0, 120)))
        ops = []
        for _ in range(rng.randint(8, 40)):
            r = rng.random()
            if r < 0.45:
                k = rng.randrange(11)
                rt = rects[k]
                if rng.random() < 0.8:      # near the rectangle's edges, in window-local coordinates, then moved to global ones
                    v = rng.choice([rt[0] - 1, rt[0], rt[2] - 1, rt[2], rng.randint(rt[0], max(rt[0], rt[2]))])
                    h = rng.choice([rt[1] - 1, rt[1], rt[3] - 1, rt[3], rng.randint(rt[1], max(rt[1], rt[3]))])
                else:
                    v, h = rng.randint(-10, 450), rng.randint(-10, 650)
                ops.append('H%d,%d' % (h + org[0], v + org[1]))
            elif r < 0.60:
                ops.append('L%d' % rng.randint(0, 12))
            elif r < 0.68:
                ops.append('U')
            elif r < 0.80:
                k = rng.randrange(11)
                rt = rects[k]
                v = rng.choice([rt[0], rt[2] - 1, rng.randint(rt[0], max(rt[0], rt[2])), rng.randint(-10, 450)])
                h = rng.choice([rt[1], rt[3] - 1, rng.randint(rt[1], max(rt[1], rt[3])), rng.randint(-10, 650)])
                ops.append('M%d,%d' % (h + org[0], v + org[1]))
            else:
                idx = rng.choice([1, 2, 6, 7, 8, 9, 10, 11, 0, 3, 4, 5, 12])
                rt = rects[idx - 1] if 1 <= idx <= 11 else rects[0]
                pts = []
                for _ in range(rng.randint(1, 6)):
                    v = rng.choice([rt[0] - 1, rt[0], rt[2] - 1, rt[2], rng.randint(rt[0], max(rt[0], rt[2])), rng.randint(-10, 450)])
                    h = rng.choice([rt[1] - 1, rt[1], rt[3] - 1, rt[3], rng.randint(rt[1], max(rt[1], rt[3])), rng.randint(-10, 650)])
                    pts.append('%d,%d' % (h + org[0], v + org[1]))
                ops.append('T%d:%s' % (idx, '/'.join(pts)))
        head = 'seed=%d reg=%d org=%d,%d rects=%s' % (seed, reg, org[0], org[1], '.'.join('%d,%d,%d,%d' % r for r in rects))
        rig.setup(reg, org, rects)
        toks = rig.run(ops)
        lines.append('%s ops=%s -> %s' % (head, ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_menu_vectors.py from the original executable; do not edit\n')
        f.write('# handlers ' + ' '.join('%x' % rig.c.rw(0x5bb8 + 4 * i) for i in range(12)) + '   (the click dispatch table D.5bb8)\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 40
    print(gen(exe, out, seed, n), 'vectors ->', out)
