#!/usr/bin/env python3
"""Golden vectors + data for src/credits.rs (the title screen's Credits slideshow) from the original code in ppcemu.py.

    python3 -I gen_credits_vectors.py MarsRising.pef OUT_VECTORS.txt [seed] [n_text]
    python3 -I gen_credits_vectors.py MarsRising.pef --data OUT.txt      (the font table and the credit pages: assets/layout/title_text.txt)

Natively run: 0x28750 (builds the 128-entry glyph rectangle table of the text font sheet, pictures 9001 / 9002), 0x29054 (draw a string with the bitmap
font: x = -1 centres it on 640, -2 on the 416 wide playfield (+64), glyph advance = width + 1, or a fixed 12 with the `mono` flag; `hilite` takes the glyphs
from the second sheet row set 52 lines lower), 0x1bafc (the band wipe that shows the off-screen picture: two bands of h lines move from both ends towards each
other, one step per tick), 0x10dcc (one credit page).  Mocked: the Toolbox, CopyBits wrappers 0x1cc / 0x30 (logged), TickCount.

Lines:  text <x> <y> <hilite> <mono> <string hex> -> <t.l.b.r>:<t.l.b.r> ...        (source rect : destination rect of every glyph copy)
        wipe <h> <ox> <oy> -> <src>:<dst> ...                                          (every band copy, in order)
        page <n> -> <src>:<dst> ... icon<id>.<t>.<l>.<b>.<r>                          (a whole credit page: glyph copies + the 'IMAG' picture)
"""
import sys, os, struct, random, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

TAB = 0x40000          # the glyph rectangle table (D.0f8c points here)
STR = 0x50000
ORG_H, ORG_V = 0x9368, 0x936c
M32 = 0xffffffff


def s16(v):
    v &= 0xffff
    return v - 0x10000 if v & 0x8000 else v


class Rig:
    def __init__(self, exe):
        self.c = c = ppcemu.load(exe)
        c.keep_toc = True
        for a in c.img.stub_name:
            c.mocks.setdefault(a, lambda cpu: cpu.r.__setitem__(3, 0))
        names = {v: k for k, v in c.img.stub_name.items()}
        self.log = []
        self.ticks = 0
        self.tickstep = [1]

        def rect_at(a):
            return struct.unpack('>hhhh', bytes(c.mem[a:a + 8]))

        def setrect(cpu):
            p = cpu.r[3]
            l, t, r, b = [s16(cpu.r[i]) for i in (4, 5, 6, 7)]
            cpu.mem[p:p + 8] = struct.pack('>hhhh', t, l, b, r)

        def offset(cpu):
            p, dh, dv = cpu.r[3], s16(cpu.r[4]), s16(cpu.r[5])
            t, l, b, r = rect_at(p)
            cpu.mem[p:p + 8] = struct.pack('>hhhh', *[s16(x) for x in (t + dv, l + dh, b + dv, r + dh)])

        def tick(cpu):
            self.ticks += self.tickstep[0]
            cpu.r[3] = self.ticks

        def blit(cpu):          # 0x1cc (glyph sheet -> back buffer) and 0x30 (back buffer -> window): (src rect, dst rect)
            self.log.append('%s:%s' % ('.'.join(map(str, rect_at(cpu.r[3]))), '.'.join(map(str, rect_at(cpu.r[4])))))

        c.mocks[names['SetRect']] = setrect
        c.mocks[names['OffsetRect']] = offset
        c.mocks[names['TickCount']] = tick
        c.mocks[0x1a84] = lambda cpu: cpu.r.__setitem__(3, 1)
        c.mocks[0x15c0] = lambda cpu: None
        self.blit = blit
        c.mocks[0x1cc] = blit
        c.mocks[0x30] = blit
        c.mocks[0x1590] = lambda cpu: None
        c.mocks[0x15f0] = lambda cpu: None
        c.ww(0xf8c, TAB)
        c.call(0x28750, [], max_steps=5_000_000)       # the font table
        self.base = bytes(c.mem)

    def table(self):
        c = self.c
        return {i: struct.unpack('>hhhh', bytes(c.mem[TAB + 8 * i:TAB + 8 * i + 8])) for i in range(128)
                if bytes(c.mem[TAB + 8 * i:TAB + 8 * i + 8]) != bytes(8)}

    def text(self, s, x, y, hi, mono):
        c = self.c
        c.mem[:] = self.base
        self.log = []
        c.mem[STR:STR + len(s) + 1] = s + b'\0'
        c.call(0x29054, [STR, x & M32, y & M32, hi, mono], max_steps=2_000_000)
        return list(self.log)

    def wipe(self, h, ox, oy, step):
        c = self.c
        c.mem[:] = self.base
        self.log = []
        self.ticks = 1000
        self.tickstep = [step]
        c.ww(ORG_H, ox & M32)
        c.ww(ORG_V, oy & M32)
        c.call(0x1bafc, [h], max_steps=5_000_000)
        return list(self.log)

    def fullpage(self, n):
        """A whole credit page run natively (0x10dcc -> 0x29054 -> glyph copies 0x1cc): every glyph copy, and the picture 0x17e8 draws."""
        c = self.c
        c.mem[:] = self.base
        self.log = []

        def icon(cpu):
            self.log.append('icon%d.%d.%d.%d.%d' % (cpu.r[3], s16(cpu.r[4] >> 16), s16(cpu.r[4]), s16(cpu.r[5] >> 16), s16(cpu.r[5])))
        saved = {a: c.mocks.get(a) for a in (0x17e8, 0x1bafc)}
        c.mocks[0x17e8] = icon
        c.mocks[0x1bafc] = lambda cpu: None
        c.call(0x10dcc, [n], max_steps=5_000_000)
        for a, f in saved.items():
            if f is None: c.mocks.pop(a, None)
            else: c.mocks[a] = f
        return list(self.log)

    def page(self, n):
        c = self.c
        c.mem[:] = self.base
        out = []

        def text(cpu):
            a = cpu.r[3]
            b = bytearray()
            while c.mem[a]:
                b.append(c.mem[a]); a += 1
            out.append(dict(text=b.decode('mac_roman'), x=s16(cpu.r[4]), y=s16(cpu.r[5]), hilite=cpu.r[6] & 0xff, mono=cpu.r[7] & 0xff))

        def icon(cpu):
            # 0x17e8(id, topleft, bottomright): DrawPicture of the 'IMAG' resource into the rectangle given as two packed words
            out.append(dict(icon=cpu.r[3], top=s16(cpu.r[4] >> 16), left=s16(cpu.r[4]), bottom=s16(cpu.r[5] >> 16), right=s16(cpu.r[5])))
        c.mocks[0x29054] = text
        c.mocks[0x17e8] = icon
        c.mocks[0x1bafc] = lambda cpu: None
        c.call(0x10dcc, [n], max_steps=2_000_000)
        del c.mocks[0x29054]
        return out


def data_text(rig):
    """The text of layout/title_text.txt: the font table and the credit pages, as the original's code produces them."""
    L = ['# the Credits slideshow of the original (tools/re/gen_credits_vectors.py --data); read by src/credits.rs',
         'param page_ticks 240', 'param wipe_credits 8', 'param wipe_title 12', 'param hilite_offset 52']
    for k, (a, b) in enumerate(((0, 16), (100, 109), (200, 203), (300, 302))):
        L.append('mode %d %d %d' % (k, a, b))
    for k, v in sorted(rig.table().items()):
        L.append('font %d %d %d %d %d' % ((k,) + tuple(v)))
    m = rig.c.mem
    for name, a, pascal in (('prompt_single', 0x5aa5, 1), ('prompt_p1', 0x5ad4, 1), ('prompt_p2', 0x5b07, 1), ('default_name', 0x5b3a, 0),
                            ('taboo_name', 0x5b40, 0), ('taboo_replacement', 0x5b4e, 0),
                            ('copyright', 0x5fda, 0)):     # the high score dialog (0x19b28)
        if pascal: b = bytes(m[a + 1:a + 1 + m[a]])
        else:
            e = a
            while m[e]: e += 1
            b = bytes(m[a:e])
        L.append('string %s %s' % (name, b.hex()))
    for n in list(range(0, 17)) + list(range(100, 110)) + list(range(200, 204)) + list(range(300, 303)) + [400]:
        L.append('page %d' % n)
        for it in rig.page(n):
            if 'text' in it:
                L.append('text %d %d %d %d %s' % (it['x'], it['y'], it['hilite'], it['mono'], it['text'].encode('mac_roman').hex()))
            else:
                L.append('image %d %d %d %d %d' % (it['icon'], it['top'], it['left'], it['bottom'], it['right']))
    return '\n'.join(L) + '\n'


def main():
    exe = sys.argv[1]
    rig = Rig(exe)
    if sys.argv[2] == '--data':
        open(sys.argv[3], 'w').write(data_text(rig))
        return
    out, seed, n_text = sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else 1, int(sys.argv[4]) if len(sys.argv) > 4 else 300
    rng = random.Random(seed)
    lines = []
    alphabet = b'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 !"\'()*+,-./:;=?&@#%\xc4\xe9\x7f\x01'
    for _ in range(n_text):
        s = bytes(rng.choice(alphabet) for _ in range(rng.randint(0, 24)))
        x = rng.choice([-1, -2, rng.randint(0, 620), rng.randint(-20, 640)])
        y = rng.randint(-10, 470)
        hi, mono = rng.randint(0, 1), rng.choice([0, 0, 1])
        lines.append('text %d %d %d %d %s -> %s' % (x, y, hi, mono, s.hex(), ' '.join(rig.text(s, x, y, hi, mono)) or '-'))
    for n in list(range(0, 17)) + list(range(100, 110)) + list(range(200, 204)) + list(range(300, 303)) + [400]:
        lines.append('page %d -> %s' % (n, ' '.join(rig.fullpage(n)) or '-'))
    for h in (8, 12, 8, 12, 1, 3, 7, 16, 25, 40, 100, 239, 240, 241):
        ox, oy = rng.randint(-5, 50), rng.randint(-5, 50)
        lines.append('wipe %d %d %d -> %s' % (h, ox, oy, ' '.join(rig.wipe(h, ox, oy, rng.randint(1, 3))) or '-'))
    open(out, 'w').write('\n'.join(lines) + '\n')
    print(len(lines), 'vectors')


if __name__ == '__main__':
    main()
