#!/usr/bin/env python3
"""Golden vectors for src/levelsel.rs (the "Start at which level?" dialog) from the original code in ppcemu.py.

    python3 -I gen_levelsel_vectors.py MarsRising.pef OUT_VECTORS.txt [seed] [n]

Natively run original code: 0x1b254 (the dialog: initial text, OK / Cancel, the clamping, the 30 tick delay), 0x7e04 (its key filter: Return / Enter / Esc /
Cmd-. / digits / Backspace / the beep for everything else), 0x95e8 (upper case), the library routines.
Mocked: the Toolbox, the Dialog Manager wrappers (items by number), ModalDialog (a script), the delay 0x967c, the sound 0x45bac, the button flash 0x4a240.

Lines:
    lvl <max> <typed|-> <hits> -> <ret> | <ops>          typed = the text of the edit field when OK is hit (hex); hits = item numbers ModalDialog returns
    filt <ctx 0|1> <what> <message-hex> <modifiers-hex> -> <ret> <item|-> | <ops>
Operations: snd:<id>:<prio>:<delay>  settext:<hex>  select  delay:<n>  flash:<item>  beep
"""
import sys, os, struct, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

M32 = 0xffffffff


def s16(v):
    v &= 0xffff
    return v - 0x10000 if v & 0x8000 else v


class LRig:
    def __init__(self, exe):
        self.c = c = ppcemu.load(exe)
        c.keep_toc = True
        for a in c.img.stub_name:
            c.mocks.setdefault(a, lambda cpu: cpu.r.__setitem__(3, 0))
        n = {v: k for k, v in c.img.stub_name.items()}
        self.ops = []
        self.hits = []
        self.typed = b''
        self.ctx = 1
        self.item = 0

        def pstr(a):
            return bytes(c.mem[a + 1:a + 1 + c.mem[a]])

        def num2str(cpu):
            v = cpu.r[3]
            v = v - (1 << 32) if v & 0x80000000 else v
            s = str(v).encode()
            a = cpu.r[4]
            cpu.mem[a] = len(s); cpu.mem[a + 1:a + 1 + len(s)] = s

        def str2num(cpu):                         # StringToNum(pstring, &long): digits only here
            s = pstr(cpu.r[3])
            v = int(s) if s else 0
            cpu.ww(cpu.r[4], v & M32)

        def bitand(cpu):
            cpu.r[3] = cpu.r[3] & cpu.r[4]

        c.mocks[n['NumToString']] = num2str
        c.mocks[n['StringToNum']] = str2num
        c.mocks[n['BitAnd']] = bitand
        c.mocks[n['SysBeep']] = lambda cpu: self.ops.append('beep')
        c.mocks[0x967c] = lambda cpu: self.ops.append('delay:%d' % s16(cpu.r[3]))
        c.mocks[0x45bac] = lambda cpu: self.ops.append('snd:%d:%d:%d' % (s16(cpu.r[3] & 0xff if cpu.r[3] & 0x80 == 0 else (cpu.r[3] & 0xff) - 256), cpu.r[4] & 0xff, cpu.r[5] & 0xff))
        c.mocks[0x480c8] = lambda cpu: cpu.r.__setitem__(3, 0x1000)
        c.mocks[0x4a3c0] = lambda cpu: cpu.r.__setitem__(3, 0x2000 if self.ctx else 0)
        c.mocks[0x4a240] = lambda cpu: self.ops.append('flash:%d' % s16(cpu.r[4]))
        c.mocks[0x488a8] = lambda cpu: self.ops.append('select')

        def getitem(cpu):
            self.item = cpu.r[4] & 0xffff
            cpu.ww(cpu.r[6], self.item)
        c.mocks[0x48628] = getitem

        def gettext(cpu):
            a = cpu.r[5]
            s = self.typed
            cpu.mem[a] = len(s); cpu.mem[a + 1:a + 1 + len(s)] = s
        c.mocks[0x487d0] = gettext

        def settext(cpu):
            self.ops.append('settext:%s' % pstr(cpu.r[5]).hex())
        c.mocks[0x487fc] = settext

        def modal(cpu):
            a = cpu.r[5]
            cpu.mem[a:a + 2] = struct.pack('>h', self.hits.pop(0) if self.hits else 2)
        c.mocks[0x488d0] = modal
        for a in (0x47fcc, 0x4804c, 0x486cc, 0x48778, 0x485e4, 0x48650):
            c.mocks[a] = lambda cpu: None
        self.base = bytes(c.mem)

    def dialog(self, mx, typed, hits):
        c = self.c
        c.mem[:] = self.base
        self.ops = []
        self.typed = typed
        self.hits = list(hits)
        r = c.call(0x1b254, [mx], max_steps=2_000_000)
        r = r - (1 << 32) if r & 0x80000000 else r
        return r, list(self.ops)

    def filt(self, ctx, what, message, mods):
        c = self.c
        c.mem[:] = self.base
        self.ops = []
        self.ctx = ctx
        ev, item = 0x60000, 0x60100
        c.mem[ev:ev + 16] = struct.pack('>HIIIH', what, message, 0, 0, mods)      # EventRecord: what, message, when, where, modifiers
        c.mem[item:item + 2] = struct.pack('>h', -1)
        r = c.call(0x7e04, [0x1000, ev, item], max_steps=100_000) & 0xff
        it = s16(c.rh(item))
        return r, (it if it != -1 else None), list(self.ops)


def main():
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 60
    rig = LRig(exe)
    rng = random.Random(seed)
    L = []
    for _ in range(n * 2):
        mx = rng.choice([28, 28, 1, 5, 40, rng.randint(1, 60)])
        digits = rng.choice([0, 1, 1, 2, 2, 3, 9])
        typed = bytes(rng.choice(b'0123456789') for _ in range(digits))
        if rng.random() < 0.3 and typed: typed = str(rng.choice([0, 1, mx, mx + 1, mx - 1, rng.randint(0, 99)])).encode()
        hits = rng.choice([[1], [1], [2], [3, 1], [1, 2], [5, 2], [4, 1], [0, 2]])
        r, ops = rig.dialog(mx, typed, hits)
        L.append('lvl %d %s %s -> %d | %s' % (mx, typed.hex() or '-', ','.join(map(str, hits)), r, ' '.join(ops) or '-'))
    chars = [13, 3, 27, 8, ord('.'), ord('a'), ord('z'), ord('A'), ord('0'), ord('5'), ord('9'), ord(' '), ord('x'), 127, 0, 200, ord('/'), ord(':')]
    for _ in range(n * 6):
        ctx = rng.choice([1, 1, 1, 0])
        what = rng.choice([0, 1, 2, 3, 3, 3, 4, 5, 5, 6, 7])
        ch = rng.choice(chars)
        msg = (rng.randint(0, 255) << 8) | ch if rng.random() < 0.5 else ch
        mods = rng.choice([0, 0, 0x100, 0x100, 0x200, 0x800, 0x100 | 0x800, rng.randint(0, 0xffff)])
        r, it, ops = rig.filt(ctx, what, msg, mods)
        L.append('filt %d %d %08x %04x -> %d %s | %s' % (ctx, what, msg, mods, r, '-' if it is None else it, ' '.join(ops) or '-'))
    open(out, 'w').write('\n'.join(L) + '\n')
    print('%d vectors' % len(L))


if __name__ == '__main__':
    main()
