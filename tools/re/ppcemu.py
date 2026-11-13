#!/usr/bin/env python3
"""Minimal PowerPC (32-bit, big endian) interpreter for running *single routines* of the Mars Rising executable.

Used to produce golden test vectors for the Rust port: the original machine code is executed on synthetic game state
(player struct, shot pool, enemy slots ...) and the resulting memory is compared with what the Rust code computes.

Memory model: one flat 4 MiB byte array. The PEF *data* section is loaded at address 0 (so TOC `r2 = 0x8000` and every
`N(r2)` access reads the initial image); player/enemy/pool structures are placed by the caller at free addresses above the data
section; the stack starts at 0x3f0000. Code is fetched from the code section. Calls (`bl`) to addresses listed in `mocks` run a
Python function instead of the machine code (used for sound/effect/score side effects).

Only the instructions the game's logic routines use are implemented (integer ALU, load/store, rotates, FP single/double, branches,
CR logic). Anything else raises NotImplementedError with the opcode.
"""
import struct, sys, os, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcdis
from ppcdis import sx

MEM_SIZE = 0x400000
STACK_TOP = 0x3f0000
SENTINEL = 0xdead0000
M32 = 0xffffffff


def f32(x):
    try:
        return struct.unpack('>f', struct.pack('>f', x))[0]
    except OverflowError:
        return math.copysign(math.inf, x)


def d2bits(x):
    return struct.unpack('>Q', struct.pack('>d', x))[0]


def bits2d(b):
    return struct.unpack('>d', struct.pack('>Q', b & 0xffffffffffffffff))[0]


class CPU:
    def __init__(self, img):
        self.img = img
        self.code = img.code
        self.mem = bytearray(MEM_SIZE)
        self.mem[0:len(img.data)] = img.data
        self.r = [0] * 32
        self.f = [0.0] * 32          # python floats; fctiwz results stored as raw-bit floats via fraw
        self.fraw = {}               # fpr index -> raw 64-bit pattern for fctiwz results
        self.cr = 0
        self.lr = 0
        self.ctr = 0
        self.ca = 0
        self.pc = 0
        self.mocks = {}
        self.keep_toc = False        # opt-in: ignore loads into r2 (TOC reload after mocked import glue)
        self.r[2] = 0x8000
        self.r[1] = STACK_TOP
        self.steps = 0
        self.trace = None

    # ---- memory
    def rb(self, a): return self.mem[a]
    def rh(self, a): return (self.mem[a] << 8) | self.mem[a + 1]
    def rw(self, a): return struct.unpack('>I', self.mem[a:a + 4])[0]
    def wb(self, a, v): self.mem[a] = v & 0xff
    def wh(self, a, v): self.mem[a:a + 2] = struct.pack('>H', v & 0xffff)
    def ww(self, a, v): self.mem[a:a + 4] = struct.pack('>I', v & M32)
    def rf32(self, a): return struct.unpack('>f', self.mem[a:a + 4])[0]
    def wf32(self, a, v): self.mem[a:a + 4] = struct.pack('>f', f32(v))
    def rf64(self, a): return struct.unpack('>d', self.mem[a:a + 8])[0]
    def wf64(self, a, v): self.mem[a:a + 8] = struct.pack('>d', v)

    # ---- FPR helpers (fctiwz leaves an integer in the low word)
    def getf(self, n):
        if n in self.fraw:
            return bits2d(self.fraw[n])
        return self.f[n]

    def setf(self, n, v):
        self.fraw.pop(n, None)
        self.f[n] = v

    def getfbits(self, n):
        return self.fraw[n] if n in self.fraw else d2bits(self.f[n])

    def setfbits(self, n, b):
        self.fraw[n] = b & 0xffffffffffffffff

    # ---- CR
    def setcr(self, field, lt, gt, eq, so=0):
        sh = 28 - 4 * field
        v = (lt << 3) | (gt << 2) | (eq << 1) | so
        self.cr = (self.cr & ~(0xf << sh) & M32) | (v << sh)

    def crbit(self, bi):
        return (self.cr >> (31 - bi)) & 1

    def setcrbit(self, bi, v):
        m = 1 << (31 - bi)
        self.cr = (self.cr | m) if v else (self.cr & ~m & M32)

    def cmp_signed(self, field, a, b):
        a, b = sx(a & M32, 32), sx(b & M32, 32)
        self.setcr(field, a < b, a > b, a == b)

    def cmp_unsigned(self, field, a, b):
        a, b = a & M32, b & M32
        self.setcr(field, a < b, a > b, a == b)

    def rc(self, v):
        v = sx(v & M32, 32)
        self.setcr(0, v < 0, v > 0, v == 0)

    # ---- running
    def call(self, addr, args=(), fargs=(), max_steps=2_000_000):
        for i, a in enumerate(args):
            self.r[3 + i] = a & M32
        for i, a in enumerate(fargs):
            self.setf(1 + i, a)
        self.lr = SENTINEL
        self.pc = addr
        self.steps = 0
        while self.pc != SENTINEL:
            self.step()
            self.steps += 1
            if self.steps > max_steps:
                raise RuntimeError('step limit')
        return self.r[3]

    cur_target = 0

    def run_block(self, start, stop, max_steps=200000):
        """Run the code at `start` (registers prepared by the caller) until the pc reaches `stop` (used for a block inside a big function)."""
        self.lr = SENTINEL
        self.pc = start
        self.steps = 0
        while self.pc != stop:
            self.step()
            self.steps += 1
            if self.steps > max_steps:
                raise RuntimeError('step limit')

    def do_call(self, target, ret):
        self.cur_target = target
        if target in self.mocks:
            self.mocks[target](self)
            self.pc = ret
        else:
            self.lr = ret
            self.pc = target

    def step(self):
        pc = self.pc
        w = struct.unpack('>I', self.code[pc:pc + 4])[0]
        if self.trace:
            self.trace(self, pc, w)
        self.pc = pc + 4
        op = w >> 26
        rd = (w >> 21) & 31
        ra = (w >> 16) & 31
        rb = (w >> 11) & 31
        imm = w & 0xffff
        si = sx(imm, 16)
        r = self.r
        if op == 14:
            r[rd] = ((r[ra] if ra else 0) + si) & M32
        elif op == 15:
            r[rd] = ((r[ra] if ra else 0) + (imm << 16)) & M32
        elif op == 12 or op == 13:
            a = r[ra]
            res = a + (si & M32)
            self.ca = 1 if res > M32 else 0
            r[rd] = res & M32
            if op == 13:
                self.rc(r[rd])
        elif op == 8:
            res = ((si & M32) + ((~r[ra]) & M32) + 1)
            self.ca = 1 if res > M32 else 0
            r[rd] = res & M32
        elif op == 7:
            r[rd] = (sx(r[ra], 32) * si) & M32
        elif op == 10:
            self.cmp_unsigned((w >> 23) & 7, r[ra], imm)
        elif op == 11:
            self.cmp_signed((w >> 23) & 7, r[ra], si)
        elif op == 24:
            r[ra] = r[rd] | imm
        elif op == 25:
            r[ra] = r[rd] | (imm << 16)
        elif op == 26:
            r[ra] = r[rd] ^ imm
        elif op == 27:
            r[ra] = r[rd] ^ (imm << 16)
        elif op == 28:
            r[ra] = r[rd] & imm
            self.rc(r[ra])
        elif op == 29:
            r[ra] = r[rd] & (imm << 16)
            self.rc(r[ra])
        elif op in (20, 21, 23):
            mb, me = (w >> 6) & 31, (w >> 1) & 31
            m = _mask(mb, me)
            if op == 23:
                sh = r[rb] & 31
            else:
                sh = rb
            x = r[rd]
            rot = ((x << sh) | (x >> (32 - sh))) & M32 if sh else x
            if op == 20:
                r[ra] = (rot & m) | (r[ra] & ~m & M32)
            else:
                r[ra] = rot & m
            if w & 1:
                self.rc(r[ra])
        elif op in (32, 33, 34, 35, 40, 41, 42, 43):
            a = ((r[ra] if (ra or op in (33, 35, 41, 43)) else 0) + si) & M32
            if op in (32, 33):
                if rd == 2 and self.keep_toc:
                    pass            # `lwz r2,20(r1)` after a mocked import glue: the glue would have saved r2 there
                else:
                    r[rd] = self.rw(a)
            elif op in (34, 35):
                r[rd] = self.rb(a)
            elif op in (40, 41):
                r[rd] = self.rh(a)
            else:
                r[rd] = sx(self.rh(a), 16) & M32
            if op in (33, 35, 41, 43):
                r[ra] = a
        elif op in (36, 37, 38, 39, 44, 45):
            a = ((r[ra] if (ra or op in (37, 39, 45)) else 0) + si) & M32
            if op in (36, 37):
                self.ww(a, r[rd])
            elif op in (38, 39):
                self.wb(a, r[rd])
            else:
                self.wh(a, r[rd])
            if op in (37, 39, 45):
                r[ra] = a
        elif op == 46:
            a = ((r[ra] if ra else 0) + si) & M32
            for i in range(rd, 32):
                r[i] = self.rw(a)
                a += 4
        elif op == 47:
            a = ((r[ra] if ra else 0) + si) & M32
            for i in range(rd, 32):
                self.ww(a, r[i])
                a += 4
        elif op in (48, 49, 50, 51, 52, 53, 54, 55):
            a = ((r[ra] if (ra or op in (49, 51, 53, 55)) else 0) + si) & M32
            if op in (48, 49):
                self.setf(rd, self.rf32(a))
            elif op in (50, 51):
                self.setf(rd, self.rf64(a))
            elif op in (52, 53):
                self.wf32(a, self.getf(rd))
            else:
                if rd in self.fraw:
                    self.mem[a:a + 8] = struct.pack('>Q', self.fraw[rd])
                else:
                    self.wf64(a, self.f[rd])
            if op in (49, 51, 53, 55):
                r[ra] = a
        elif op == 18:
            li = sx(w & 0x3fffffc, 26)
            t = ((pc + li) if not w & 2 else li) & M32
            if w & 1:
                self.do_call(t, pc + 4)
            else:
                self.pc = t
        elif op == 16:
            bo, bi = rd, ra
            ok = self._bc_taken(bo, bi)
            if ok:
                t = ((pc + sx(w & 0xfffc, 16)) if not w & 2 else sx(w & 0xfffc, 16)) & M32
                if w & 1:
                    self.lr = pc + 4
                self.pc = t
        elif op == 19:
            x = (w >> 1) & 0x3ff
            if x == 16 or x == 528:
                bo, bi = rd, ra
                ok = self._bc_taken(bo, bi) if x == 16 else self._bc_taken(bo | 4, bi)
                if ok:
                    t = (self.lr if x == 16 else self.ctr) & ~3 & M32
                    if w & 1:
                        self.do_call(t, pc + 4)
                    else:
                        self.pc = t
            elif x in (449, 193, 257, 129, 33, 225, 289, 417):
                a, b = self.crbit(ra), self.crbit(rb)
                v = {449: a | b, 193: a ^ b, 257: a & b, 129: a & (1 - b), 33: 1 - (a | b), 225: 1 - (a & b), 289: 1 - (a ^ b), 417: a | (1 - b)}[x]
                self.setcrbit(rd, v)
            elif x == 150:
                pass
            elif x == 0:
                src = (self.cr >> (28 - 4 * (ra >> 2))) & 0xf
                self.setcr(rd >> 2, src >> 3 & 1, src >> 2 & 1, src >> 1 & 1, src & 1)
            else:
                raise NotImplementedError('op19.%d at %x' % (x, pc))
        elif op == 31:
            self._op31(w, pc, rd, ra, rb)
        elif op == 59 or op == 63:
            self._fp(w, pc, op, rd, ra, rb)
        else:
            raise NotImplementedError('op %d at %x (%08x)' % (op, pc, w))

    def _bc_taken(self, bo, bi):
        if not (bo & 4):
            self.ctr = (self.ctr - 1) & M32
        ctr_ok = (bo & 4) or ((self.ctr != 0) != bool(bo & 2))
        cond_ok = (bo & 16) or (self.crbit(bi) == (1 if bo & 8 else 0))
        return bool(ctr_ok and cond_ok)

    def _op31(self, w, pc, rd, ra, rb):
        r = self.r
        x = (w >> 1) & 0x3ff
        rc = w & 1
        x9 = x & 0x1ff
        if x == 339:
            spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5)
            r[rd] = {8: self.lr, 9: self.ctr, 1: 0}.get(spr, 0)
        elif x == 467:
            spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5)
            if spr == 8:
                self.lr = r[rd]
            elif spr == 9:
                self.ctr = r[rd]
        elif x == 19:
            r[rd] = self.cr
        elif x == 144:
            fxm = (w >> 12) & 0xff
            for i in range(8):
                if fxm & (0x80 >> i):
                    sh = 28 - 4 * i
                    self.cr = (self.cr & ~(0xf << sh) & M32) | (r[rd] & (0xf << sh))
        elif x == 0:
            self.cmp_signed((w >> 23) & 7, r[ra], r[rb])
        elif x == 32:
            self.cmp_unsigned((w >> 23) & 7, r[ra], r[rb])
        elif x9 == 266:
            r[rd] = (r[ra] + r[rb]) & M32
            if rc: self.rc(r[rd])
        elif x9 == 40:
            r[rd] = (r[rb] - r[ra]) & M32
            if rc: self.rc(r[rd])
        elif x9 == 235:
            r[rd] = (sx(r[ra], 32) * sx(r[rb], 32)) & M32
            if rc: self.rc(r[rd])
        elif x9 == 491:
            b = sx(r[rb], 32)
            a = sx(r[ra], 32)
            r[rd] = (int(a / b) if b else 0) & M32
        elif x9 == 459:
            r[rd] = (r[ra] // r[rb]) if r[rb] else 0
        elif x9 == 75:
            r[rd] = ((sx(r[ra], 32) * sx(r[rb], 32)) >> 32) & M32
        elif x9 == 11:
            r[rd] = ((r[ra] * r[rb]) >> 32) & M32
        elif x9 == 104:
            r[rd] = (-sx(r[ra], 32)) & M32
            if rc: self.rc(r[rd])
        elif x9 == 202:   # addze
            res = r[ra] + self.ca
            self.ca = 1 if res > M32 else 0
            r[rd] = res & M32
            if rc: self.rc(r[rd])
        elif x9 == 10:   # addc
            res = r[ra] + r[rb]
            self.ca = 1 if res > M32 else 0
            r[rd] = res & M32
        elif x9 == 138:  # adde
            res = r[ra] + r[rb] + self.ca
            self.ca = 1 if res > M32 else 0
            r[rd] = res & M32
        elif x9 == 8:    # subfc
            res = ((~r[ra]) & M32) + r[rb] + 1
            self.ca = 1 if res > M32 else 0
            r[rd] = res & M32
        elif x == 28:
            r[ra] = r[rd] & r[rb]
            if rc: self.rc(r[ra])
        elif x == 444:
            r[ra] = r[rd] | r[rb]
            if rc: self.rc(r[ra])
        elif x == 316:
            r[ra] = r[rd] ^ r[rb]
            if rc: self.rc(r[ra])
        elif x == 60:
            r[ra] = r[rd] & (~r[rb] & M32)
            if rc: self.rc(r[ra])
        elif x == 124:
            r[ra] = ~(r[rd] | r[rb]) & M32
            if rc: self.rc(r[ra])
        elif x == 412:
            r[ra] = r[rd] | (~r[rb] & M32)
        elif x == 24:
            n = r[rb] & 63
            r[ra] = (r[rd] << n) & M32 if n < 32 else 0
            if rc: self.rc(r[ra])
        elif x == 536:
            n = r[rb] & 63
            r[ra] = (r[rd] >> n) if n < 32 else 0
            if rc: self.rc(r[ra])
        elif x == 792:
            n = r[rb] & 63
            v = sx(r[rd], 32)
            n = min(n, 31) if n < 32 else 31
            self.ca = 1 if (v < 0 and (v & ((1 << n) - 1))) else 0
            r[ra] = (v >> n) & M32
            if rc: self.rc(r[ra])
        elif x == 824:
            v = sx(r[rd], 32)
            n = rb
            self.ca = 1 if (v < 0 and (v & ((1 << n) - 1))) else 0
            r[ra] = (v >> n) & M32
            if rc: self.rc(r[ra])
        elif x == 922:
            r[ra] = sx(r[rd] & 0xffff, 16) & M32
            if rc: self.rc(r[ra])
        elif x == 954:
            r[ra] = sx(r[rd] & 0xff, 8) & M32
            if rc: self.rc(r[ra])
        elif x == 26:
            v = r[rd]
            r[ra] = 32 - v.bit_length()
        elif x in (23, 87, 279, 343, 151, 215, 407, 55, 119, 311, 183, 247):
            a = ((r[ra] if ra else 0) + r[rb]) & M32
            if x in (55, 119, 311, 183, 247):
                a = (r[ra] + r[rb]) & M32
            if x in (23, 55):
                r[rd] = self.rw(a)
            elif x in (87, 119):
                r[rd] = self.rb(a)
            elif x in (279, 311):
                r[rd] = self.rh(a)
            elif x == 343:
                r[rd] = sx(self.rh(a), 16) & M32
            elif x in (151, 183):
                self.ww(a, r[rd])
            elif x in (215, 247):
                self.wb(a, r[rd])
            elif x == 407:
                self.wh(a, r[rd])
            if x in (55, 119, 311, 183, 247):
                r[ra] = a
        elif x in (535, 599, 663, 727):
            a = ((r[ra] if ra else 0) + r[rb]) & M32
            if x == 535:
                self.setf(rd, self.rf32(a))
            elif x == 599:
                self.setf(rd, self.rf64(a))
            elif x == 663:
                self.wf32(a, self.getf(rd))
            else:
                if rd in self.fraw:
                    self.mem[a:a + 8] = struct.pack('>Q', self.fraw[rd])
                else:
                    self.wf64(a, self.f[rd])
        elif x in (598, 854, 86, 54, 246, 278, 982, 1014):
            pass
        else:
            raise NotImplementedError('op31.%d at %x (%08x)' % (x, pc, w))

    def _fp(self, w, pc, op, rd, ra, rb):
        x5 = (w >> 1) & 31
        x10 = (w >> 1) & 0x3ff
        rc = (w >> 6) & 31
        single = (op == 59)
        a, b, c = self.getf(ra), self.getf(rb), self.getf(rc)
        rnd = f32 if single else (lambda v: v)
        if x5 == 21:
            self.setf(rd, rnd(a + b))
        elif x5 == 20:
            self.setf(rd, rnd(a - b))
        elif x5 == 25:
            self.setf(rd, rnd(a * c))
        elif x5 == 18:
            self.setf(rd, rnd(a / b) if b != 0 else math.copysign(math.inf, a) if a else math.nan)
        elif x5 == 29:   # fmadd(s)
            self.setf(rd, rnd(math.fma(a, c, b)))
        elif x5 == 28:
            self.setf(rd, rnd(math.fma(a, c, -b)))
        elif x5 == 30:   # fnmsub(s)
            self.setf(rd, rnd(-math.fma(a, c, -b)))
        elif x5 == 31:   # fnmadd(s)
            self.setf(rd, rnd(-math.fma(a, c, b)))
        elif x5 == 22:
            self.setf(rd, rnd(math.sqrt(b)))
        elif op == 63 and x5 == 23:   # fsel
            self.setf(rd, c if a >= 0 else b)
        elif op == 63 and x10 in (0, 32):
            f = (w >> 23) & 7
            if math.isnan(a) or math.isnan(b):
                self.setcr(f, 0, 0, 0, 1)
            else:
                self.setcr(f, a < b, a > b, a == b)
        elif op == 63 and x10 == 12:
            self.setf(rd, f32(b))
        elif op == 63 and x10 in (14, 15):
            v = int(b) if not (math.isnan(b) or math.isinf(b)) else 0
            v = max(-0x80000000, min(0x7fffffff, v))
            self.setfbits(rd, 0xfff8000000000000 | (v & M32))
        elif op == 63 and x10 == 40:
            self.setf(rd, -b)
        elif op == 63 and x10 == 264:
            self.setf(rd, abs(b))
        elif op == 63 and x10 == 72:
            if rb in self.fraw:
                self.setfbits(rd, self.fraw[rb])
            else:
                self.setf(rd, b)
        else:
            raise NotImplementedError('fp op%d.%d at %x (%08x)' % (op, x10, pc, w))


def _mask(mb, me):
    if mb <= me:
        return ((1 << (me - mb + 1)) - 1) << (31 - me)
    return (M32 ^ (((1 << (mb - me - 1)) - 1) << (32 - mb))) & M32


def load(path):
    img = ppcdis.Image(path)
    return CPU(img)
