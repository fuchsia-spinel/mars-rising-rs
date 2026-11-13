#!/usr/bin/env python3
"""Tiny PowerPC -> pseudo-C lifter used to *read* the Mars Rising executable (not a decompiler you can recompile).

Per basic block it does symbolic execution: constants and TOC addresses are folded, int<->float conversion idioms
(`xoris/0x43300000/lfd/fsub`, `fctiwz/stfd/lwz`) become `(f32)(int)` / `(int)` casts, loads/calls become temporaries `tN`, registers that are
modified in a block and read at the entry of any block are written back (`r24 = ...;`) before the block's final branch, and the
compare + branch idiom becomes `if (a < b) goto Lxxxx;` (condition-register state is carried across fall-through edges).

  python3 -I lift.py EXE START END        # lift code range [START, END)
  python3 -I lift.py EXE func ADDR        # lift the function starting at ADDR

Memory syntax:  u32[addr] u16[..] s16[..] u8[..] f32[..] f64[..];   addresses of the data section are `D.xxxx` (offset in the data section,
r2 = 0x8000);  constants from the float constant pool print as <1.5>.
"""
import struct, sys, os, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcdis
from ppcdis import sx, TOC

KPOOL = (0x1500, 0x1800)          # float constant pool (TOC-relative constants live here)


class Names2(ppcdis.Names):
    pass


def mask32(mb, me):
    if mb <= me:
        m = 0
        for i in range(mb, me + 1):
            m |= 1 << (31 - i)
        return m
    m = 0xffffffff
    for i in range(me + 1, mb):
        m &= ~(1 << (31 - i))
    return m


class Val:
    __slots__ = ('t', 'c', 'atom', 'fc', 'tag')

    def __init__(self, t, c=None, atom=False, fc=None, tag=None):
        self.t = t        # text
        self.c = c        # python int constant (for gprs) if known
        self.atom = atom
        self.fc = fc      # python float constant
        self.tag = tag

    def s(self):
        return self.t

    def p(self):
        return self.t if self.atom else '(' + self.t + ')'


def C(n):
    n &= 0xffffffff
    t = str(n) if n < 0x10000 else '0x%x' % n
    return Val(t, c=n, atom=True)


def E(t, atom=False):
    return Val(t, atom=atom)


class Block:
    def __init__(self, start):
        self.start = start
        self.insns = []
        self.upexp = set()
        self.mod = set()
        self.label = False


class Lifter:
    def __init__(self, img, names):
        self.img = img
        self.names = names
        self.tmp = 0
        self.extra_targets = set()   # jump-table targets (--jt): become block leaders and labels

    def dname(self, addr):
        n = self.names.data.get(addr)
        return n if n else 'D.%04x' % addr

    # ------------------------------------------------------------ decode / blocks
    def blocks(self, start, end):
        code = self.img.code
        words = {a: struct.unpack('>I', code[a:a + 4])[0] for a in range(start, end, 4)}
        leaders = {start}
        for a, w in words.items():
            op = w >> 26
            nxt = a + 4
            if op == 18 and not (w & 1):
                t = (a + sx(w & 0x3fffffc, 26)) & 0xffffffff
                if start <= t < end:
                    leaders.add(t)
                leaders.add(nxt)
            elif op == 16:
                t = (a + sx(w & 0xfffc, 16)) & 0xffffffff
                if start <= t < end:
                    leaders.add(t)
                leaders.add(nxt)
            elif op == 19 and ((w >> 1) & 0x3ff) in (16, 528) and not (w & 1):
                leaders.add(nxt)
        targets = set()
        for a, w in words.items():
            op = w >> 26
            if op == 18 and not (w & 1):
                targets.add((a + sx(w & 0x3fffffc, 26)) & 0xffffffff)
            elif op == 16 and not (w & 1):
                targets.add((a + sx(w & 0xfffc, 16)) & 0xffffffff)
        for t in self.extra_targets:
            if start <= t < end:
                leaders.add(t)
                targets.add(t)
        order = sorted(l for l in leaders if start <= l < end)
        blocks = []
        for i, l in enumerate(order):
            e = order[i + 1] if i + 1 < len(order) else end
            b = Block(l)
            b.insns = [(a, words[a]) for a in range(l, e, 4)]
            b.label = l in targets
            blocks.append(b)
        return blocks

    # ------------------------------------------------------------ symbolic execution of one block
    def run_block(self, b, glob_up, carry_cr, emit):
        st = self
        gpr = {}
        fpr = {}
        out = []
        b.upexp = set()
        b.mod = set()
        cr = carry_cr if carry_cr is not None else {}
        stack = {}
        ca = [None]
        lr = [None]
        argmod = set()

        def rd(n):
            if n == 0 and False:
                pass
            if n in gpr:
                return gpr[n]
            if n == 2:
                return C(TOC)
            if n == 1:
                return E('sp', True)
            b.upexp.add(('r', n))
            return E('r%d' % n, True)

        def fd(n):
            if n in fpr:
                return fpr[n]
            b.upexp.add(('f', n))
            return E('f%d' % n, True)

        def wr(n, v):
            gpr[n] = v
            b.mod.add(('r', n))
            argmod.add(('r', n))

        def wf(n, v):
            fpr[n] = v
            b.mod.add(('f', n))
            argmod.add(('f', n))

        def newtmp():
            st.tmp += 1
            return 't%d' % st.tmp

        def spill(expr_text, f=False):
            t = newtmp()
            out.append('%s = %s;' % (t, expr_text))
            return E(t, True)

        def addr_of(base, off):
            if base.c is not None:
                return C(base.c + off)
            if off == 0:
                return base
            return E('%s %s %d' % (base.p(), '+' if off >= 0 else '-', abs(off)))

        def addr_idx(a, bb):
            if a.c is not None and bb.c is not None:
                return C(a.c + bb.c)
            return E('%s + %s' % (a.p(), bb.p()))

        def fmtaddr(a):
            if a.c is not None and 0 <= a.c < 0x20000:
                return 'D.%04x' % a.c if a.c >= 0x100 and a.c < len(st.img.data) else '0x%x' % a.c
            return a.s()

        def spoff(a):
            if a.t == 'sp':
                return 0
            m = re.fullmatch(r'sp ([+-]) (\d+)', a.t)
            if not m:
                return None
            return int(m.group(2)) * (1 if m.group(1) == '+' else -1)

        def load(kind, a, f=False):
            if spoff(a) is not None:
                off = spoff(a)
                if off in stack:
                    return stack[off]
            if a.c is not None and f and KPOOL[0] <= a.c < KPOOL[1] and kind in ('f32', 'f64'):
                d = st.img.data
                if kind == 'f32':
                    v = struct.unpack('>f', d[a.c:a.c + 4])[0]
                    return Val('<%g>' % v, fc=v, atom=True, tag='k')
                raw = d[a.c:a.c + 8]
                v = struct.unpack('>d', raw)[0]
                if raw == bytes.fromhex('4330000080000000'):
                    return Val('MAGIC', atom=True, tag='magic')
                if raw == bytes.fromhex('4330000000000000'):
                    return Val('MAGICU', atom=True, tag='magic')
                return Val('<%g>' % v, fc=v, atom=True, tag='k')
            return spill('%s[%s]' % (kind, fmtaddr(a)))

        def store(kind, a, v):
            if spoff(a) is not None:
                off = spoff(a)
                stack[off] = v
                if v.tag in ('xor', 'hi', 'f2i'):
                    return
                out.append('local[%d] = %s;' % (off, v.s()))
                return
            out.append('%s[%s] = %s;' % (kind, fmtaddr(a), v.s()))

        def setcr(n, a, bb, kind):
            if kind == 'u':
                cr[n] = {0: '%s <u %s' % (a.p(), bb.p()), 1: '%s >u %s' % (a.p(), bb.p()), 2: '%s == %s' % (a.p(), bb.p()), 3: '0'}
            else:
                cr[n] = {0: '%s < %s' % (a.p(), bb.p()), 1: '%s > %s' % (a.p(), bb.p()), 2: '%s == %s' % (a.p(), bb.p()), 3: '0'}

        def crbit(bi):
            f, bit = bi >> 2, bi & 3
            if f not in cr:
                return 'cr%d.%s' % (f, ['lt', 'gt', 'eq', 'so'][bit])
            return cr[f][bit]

        def argstr():
            # arguments: r3.. up to highest modified (since block start/last call)
            hi = -1
            for i in range(3, 11):
                if ('r', i) in argmod:
                    hi = i
            args = []
            for i in range(3, hi + 1):
                args.append(rd(i).s())
            fh = -1
            for i in range(1, 14):
                if ('f', i) in argmod:
                    fh = i
            for i in range(1, fh + 1):
                args.append(fd(i).s())
            return ', '.join(args)

        def callname(t):
            if t in st.img.stub_name:
                return st.img.stub_name[t]
            return st.names.code.get(t, 'sub_%x' % t)

        last_branch = None
        n = len(b.insns)
        for idx, (a, w) in enumerate(b.insns):
            op = w >> 26
            rd_ = (w >> 21) & 31
            ra = (w >> 16) & 31
            rb = (w >> 11) & 31
            imm = w & 0xffff
            si = sx(imm, 16)
            rc = bool(w & 1)
            if w == 0x60000000:
                continue

            def setrc(v, n_=None):
                if rc:
                    cr[0] = {0: '%s < 0' % v.p(), 1: '%s > 0' % v.p(), 2: '%s == 0' % v.p(), 3: '0'}

            if op == 14 and rd_ == 1 and ra == 1:
                continue
            if op == 14:
                if ra == 0:
                    wr(rd_, C(si))
                else:
                    x = rd(ra)
                    wr(rd_, addr_of(x, si) if x.c is not None or True else x)
            elif op == 15:
                if ra == 0:
                    wr(rd_, C(imm << 16))
                else:
                    x = rd(ra)
                    wr(rd_, C(x.c + (imm << 16)) if x.c is not None else E('%s + 0x%x' % (x.p(), imm << 16)))
            elif op == 12 or op == 13:
                x = rd(ra)
                v = C(x.c + si) if x.c is not None else E('%s + %d' % (x.p(), si))
                wr(rd_, v)
                if op == 13:
                    setrc(v) if False else None
                    cr[0] = {0: '%s < 0' % v.p(), 1: '%s > 0' % v.p(), 2: '%s == 0' % v.p(), 3: '0'}
            elif op == 8:   # subfic
                x = rd(ra)
                wr(rd_, C(si - x.c) if x.c is not None else E('%d - %s' % (si, x.p())))
            elif op == 7:
                x = rd(ra)
                wr(rd_, C(x.c * si) if x.c is not None else E('%s * %d' % (x.p(), si)))
            elif op in (10, 11):
                x = rd(ra)
                bb = C(imm) if op == 10 else Val(str(si), c=si & 0xffffffff, atom=True)
                setcr((w >> 23) & 7, x, bb, 'u' if op == 10 else 's')
            elif op in (24, 25, 26, 27):
                x = rd(rd_)
                sh = 16 if op in (25, 27) else 0
                nm = {24: '|', 25: '|', 26: '^', 27: '^'}[op]
                if op == 26 and imm == 0x8000 and False:
                    pass
                v = E('%s %s 0x%x' % (x.p(), nm, imm << sh))
                if x.c is not None:
                    v = C(x.c | (imm << sh) if nm == '|' else x.c ^ (imm << sh))
                if op == 27 and imm == 0x8000:
                    v = E('%s ^ 0x80000000' % x.p())
                    v.tag = 'xor'
                    v.fc = x   # remember original
                wr(ra, v)
            elif op in (28, 29):
                x = rd(rd_)
                sh = 16 if op == 29 else 0
                v = E('%s & 0x%x' % (x.p(), imm << sh))
                wr(ra, v)
                cr[0] = {0: '%s < 0' % v.p(), 1: '%s > 0' % v.p(), 2: '%s == 0' % v.p(), 3: '0'}
            elif op == 21:
                x = rd(rd_)
                sh, mb, me = rb, (w >> 6) & 31, (w >> 1) & 31
                m = mask32(mb, me)
                # CR bit extract idiom after mfcr
                if x.tag == 'CR' and mb == 31 and me == 31:
                    bi = (sh - 1) & 31
                    v = E('(%s)' % crbit(bi), True)
                elif x.c is not None:
                    r = ((x.c << sh) | (x.c >> (32 - sh))) & 0xffffffff if sh else x.c
                    v = C(r & m)
                elif sh == 0 and m == 0xff:
                    v = E('(u8)%s' % x.p(), True)
                elif sh == 0 and m == 0xffff:
                    v = E('(u16)%s' % x.p(), True)
                elif sh == 0:
                    v = E('%s & 0x%x' % (x.p(), m))
                elif mb == 0 and me == 31 - sh:
                    v = E('%s << %d' % (x.p(), sh))
                elif me == 31 and sh == 32 - mb:
                    v = E('(u32)%s >> %d' % (x.p(), mb))
                else:
                    v = E('rotl(%s,%d) & 0x%x' % (x.s(), sh, m))
                wr(ra, v)
                if rc:
                    cr[0] = {0: '%s < 0' % v.p(), 1: '%s > 0' % v.p(), 2: '%s == 0' % v.p(), 3: '0'}
            elif op == 20:
                x = rd(rd_)
                old = rd(ra)
                sh, mb, me = rb, (w >> 6) & 31, (w >> 1) & 31
                m = mask32(mb, me)
                v = E('(%s & 0x%x) | (rotl(%s,%d) & 0x%x)' % (old.p(), ~m & 0xffffffff, x.s(), sh, m))
                wr(ra, v)
            elif op == 23:
                x = rd(rd_)
                sh = rd(rb)
                m = mask32((w >> 6) & 31, (w >> 1) & 31)
                wr(ra, E('rotl(%s,%s) & 0x%x' % (x.s(), sh.s(), m)))
            elif op == 32 and ra == 1 and (rd_ >= 14 and si < 0):
                continue
            elif op == 32 and ra == 1 and rd_ == 0 and idx + 2 < n + 2 and any((b.insns[j][1] & 0xfc1fffff) == 0x7c0803a6 for j in range(idx + 1, min(n, idx + 3))):
                continue
            elif op in (32, 34, 40, 42, 33, 35, 41, 43):
                kind = {32: 'u32', 34: 'u8', 40: 'u16', 42: 's16', 33: 'u32', 35: 'u8', 41: 'u16', 43: 's16'}[op]
                if op in (33, 35, 41, 43):
                    base = rd(ra)
                    addr = addr_of(base, si)
                    wr(ra, addr)
                else:
                    base = rd(ra)
                    addr = addr_of(base, si)
                if not (op == 32 and ra == 1 and rd_ == 0 and False):
                    wr(rd_, load(kind, addr))
            elif op in (36, 38, 44, 37, 39, 45):
                kind = {36: 'u32', 38: 'u8', 44: 'u16', 37: 'u32', 39: 'u8', 45: 'u16'}[op]
                base = rd(ra)
                v = rd(rd_)
                if op == 37 and ra == 1:       # stwu r1,-N(r1) prologue
                    continue
                if op == 36 and ra == 1 and rd_ >= 14 and si < 0:
                    continue
                if ra == 1 and rd_ == 0 and si == 8:
                    continue
                if op == 37 or op == 39 or op == 45:
                    addr = addr_of(base, si)
                    wr(ra, addr)
                else:
                    addr = addr_of(base, si)
                if v.tag is None and v.c == 0x43300000 and kind == 'u32' and ra == 1:
                    v = Val('0x43300000', c=0x43300000, atom=True, tag='hi')
                store(kind, addr, v)
            elif op in (46, 47):
                pass     # lmw/stmw (prologue/epilogue)
            elif op in (48, 50, 49, 51):
                kind = 'f32' if op in (48, 49) else 'f64'
                if ra == 1 and rd_ >= 14:
                    continue
                base = rd(ra)
                addr = addr_of(base, si)
                # int->double idiom
                if kind == 'f64' and ra == 1:
                    off = spoff(addr)
                    if off is None:
                        off = -9999
                    hi = stack.get(off)
                    lo = stack.get(off + 4)
                    if hi is not None and lo is not None and (hi.tag == 'hi' or re.fullmatch(r'r\d+', hi.s())):
                        if lo.tag == 'xor':
                            wf(rd_, Val('I2D(%s)' % lo.fc.s(), atom=True, tag='i2d'))
                            continue
                        if hi.tag != 'hi' and lo.tag not in ('hi', 'f2i'):
                            wf(rd_, Val('U2D(%s)' % lo.s(), atom=True, tag='i2d'))
                            continue
                    if hi is not None and hi.tag == 'hi' and lo is not None and lo.tag == 'xor':
                        wf(rd_, Val('I2D(%s)' % lo.fc.s(), atom=True, tag='i2d'))
                        continue
                    if off in stack and stack[off].tag == 'f2i':
                        wf(rd_, stack[off])
                        continue
                wf(rd_, load(kind, addr, f=True))
            elif op in (52, 54, 53, 55):
                kind = 'f32' if op in (52, 53) else 'f64'
                base = rd(ra)
                addr = addr_of(base, si)
                v = fd(rd_)
                if ra == 1 and kind == 'f64':
                    off = spoff(addr)
                    if v.tag == 'f2i':
                        stack[off] = v
                        stack[off + 4] = Val('(int)%s' % v.fc.p(), atom=True, tag='f2i_lo')
                        continue
                    if rd_ >= 14:
                        continue
                store(kind, addr, v)
            elif op == 59 or op == 63:
                x5 = (w >> 1) & 31
                x10 = (w >> 1) & 0x3ff
                fa, fb, fcr = fd(ra), fd(rb), fd((w >> 6) & 31)
                sfx = 's' if op == 59 else 'd'
                sym = {21: '+', 20: '-', 18: '/'}
                if x5 in (21, 20, 18):
                    if x5 == 20 and fb.tag == 'magic' and fa.tag == 'i2d':
                        wf(rd_, Val('(float)%s' % fa.t[4:-1] if False else '(%s)%s' % ('f32' if op == 59 else 'f64', fa.t[4:-1]), atom=True))
                        # inner text is the integer expression
                        gp = fa.t[4:-1]
                        fpr[rd_] = Val('(%s)(%s)' % ('f32' if op == 59 else 'f64', gp), atom=True, tag='cvt')
                    else:
                        wf(rd_, E('%s %s%s %s' % (fa.p(), sym[x5], 's' if op == 59 else 'd', fb.p())))
                        fpr[rd_].t = fpr[rd_].t.replace('+s', '+').replace('-s', '-').replace('/s', '/') if op == 59 else fpr[rd_].t
                elif x5 == 25:
                    wf(rd_, E('%s * %s' % (fa.p(), fcr.p())))
                elif x5 in (28, 29, 30, 31):
                    f_ = {28: '%s * %s - %s', 29: '%s * %s + %s', 30: '-(%s * %s - %s)', 31: '-(%s * %s + %s)'}[x5]
                    wf(rd_, E(f_ % (fa.p(), fcr.p(), fb.p())))
                elif x5 == 23:
                    wf(rd_, E('(%s >= 0 ? %s : %s)' % (fa.p(), fcr.p(), fb.p()), True))
                elif x10 in (0, 32):
                    n_ = (w >> 23) & 7
                    cr[n_] = {0: '%s < %s' % (fa.p(), fb.p()), 1: '%s > %s' % (fa.p(), fb.p()), 2: '%s == %s' % (fa.p(), fb.p()), 3: 'unordered(%s,%s)' % (fa.s(), fb.s())}
                elif x10 == 12:
                    wf(rd_, Val('(f32)%s' % fb.p(), atom=True))
                elif x10 in (14, 15):
                    v = Val('F2I', atom=True, tag='f2i')
                    v.fc = fb
                    fpr[rd_] = v
                    b.mod.add(('f', rd_))
                elif x10 == 40:
                    wf(rd_, E('-%s' % fb.p(), True) if fb.fc is None else Val('<%g>' % (-fb.fc), fc=-fb.fc, atom=True, tag='k'))
                elif x10 == 264:
                    wf(rd_, E('fabs(%s)' % fb.s(), True))
                elif x10 == 72:
                    wf(rd_, fb)
                elif x5 == 22:
                    wf(rd_, E('sqrt(%s)' % fb.s(), True))
                else:
                    out.append('// ?? %06x: %08x' % (a, w))
            elif op == 31:
                x = (w >> 1) & 0x3ff
                x9 = x & 0x1ff if (x & 0x200) else x
                if x == 339:
                    spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5)
                    if spr == 8:
                        lr[0] = True
                        wr(rd_, E('LR', True))
                    elif spr == 9:
                        wr(rd_, E('CTR', True))
                elif x == 467:
                    spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5)
                    if spr == 9:
                        ctr = rd(rd_)
                        gpr['CTR'] = ctr
                elif x == 19:
                    wr(rd_, Val('CR', atom=True, tag='CR'))
                elif x == 444:
                    xv, yv = rd(rd_), rd(rb)
                    if rd_ == rb:
                        wr(ra, xv)
                    else:
                        wr(ra, C(xv.c | yv.c) if xv.c is not None and yv.c is not None else E('%s | %s' % (xv.p(), yv.p())))
                    if rc:
                        v = gpr[ra]
                        cr[0] = {0: '%s < 0' % v.p(), 1: '%s > 0' % v.p(), 2: '%s == 0' % v.p(), 3: '0'}
                elif x in (266, 40, 235, 28, 316, 60, 491, 459, 24, 536, 792, 124, 75, 11, 412):
                    xv = rd(ra if x in (266, 40, 235, 491, 459, 75, 11) else rd_)
                    yv = rd(rb)
                    if x == 40:       # subf rD = rB - rA
                        xv, yv = rd(ra), rd(rb)
                        v = C(yv.c - xv.c) if xv.c is not None and yv.c is not None else E('%s - %s' % (yv.p(), xv.p()))
                        wr(rd_, v)
                    elif x == 266:
                        v = C(xv.c + yv.c) if xv.c is not None and yv.c is not None else E('%s + %s' % (xv.p(), yv.p()))
                        wr(rd_, v)
                    elif x == 235:
                        v = E('%s * %s' % (xv.p(), yv.p()))
                        wr(rd_, v)
                    elif x in (491, 459):
                        v = E('%s / %s' % (xv.p(), yv.p()))
                        wr(rd_, v)
                    elif x in (75, 11):
                        v = E('mulhi(%s,%s)' % (xv.s(), yv.s()), True)
                        wr(rd_, v)
                    else:
                        sym = {28: '&', 316: '^', 60: '& ~', 24: '<<', 536: '>>u', 792: '>>s', 124: '|~', 412: '|~'}[x]
                        if x == 124:
                            v = E('~(%s | %s)' % (xv.p(), yv.p()), True)
                        else:
                            v = E('%s %s %s' % (xv.p(), sym, yv.p()))
                        wr(ra, v)
                    if rc:
                        vv = gpr[ra if x in (28, 316, 60, 24, 536, 792, 124, 412) else rd_]
                        cr[0] = {0: '%s < 0' % vv.p(), 1: '%s > 0' % vv.p(), 2: '%s == 0' % vv.p(), 3: '0'}
                elif x == 104:
                    xv = rd(ra)
                    wr(rd_, E('-%s' % xv.p(), True))
                elif x == 0 or x == 32:
                    setcr((w >> 23) & 7, rd(ra), rd(rb), 's' if x == 0 else 'u')
                elif x == 824:
                    xv = rd(rd_)
                    v = E('%s >>s %d' % (xv.p(), rb))
                    ca[0] = (xv, rb)
                    wr(ra, v)
                elif x == 202:   # addze
                    xv = rd(ra)
                    if ca[0] is not None and ca[0][0] is not None and xv.t.endswith('>>s %d' % ca[0][1]):
                        wr(rd_, E('%s / %d' % (ca[0][0].p(), 1 << ca[0][1])))
                    else:
                        wr(rd_, E('%s + CA' % xv.p()))
                elif x == 922:
                    wr(ra, E('(s16)%s' % rd(rd_).p(), True))
                elif x == 954:
                    wr(ra, E('(s8)%s' % rd(rd_).p(), True))
                elif x == 26:
                    wr(ra, E('clz(%s)' % rd(rd_).s(), True))
                elif x in (23, 87, 279, 343, 151, 215, 407):
                    base, idxr = rd(ra), rd(rb)
                    addr = addr_idx(base, idxr)
                    if x in (23, 87, 279, 343):
                        wr(rd_, load({23: 'u32', 87: 'u8', 279: 'u16', 343: 's16'}[x], addr))
                    else:
                        store({151: 'u32', 215: 'u8', 407: 'u16'}[x], addr, rd(rd_))
                elif x in (535, 599, 663, 727):
                    base, idxr = rd(ra), rd(rb)
                    addr = addr_idx(base, idxr)
                    if x in (535, 599):
                        wf(rd_, load('f32' if x == 535 else 'f64', addr, f=True))
                    else:
                        store('f32' if x == 663 else 'f64', addr, fd(rd_))
                elif x in (598, 854):
                    pass
                else:
                    out.append('// ?? %06x: %08x (op31.%d)' % (a, w, x))
            elif op == 18:
                t = (a + sx(w & 0x3fffffc, 26)) & 0xffffffff if not w & 2 else sx(w & 0x3fffffc, 26) & 0xffffffff
                if w & 1:
                    # call
                    nm = callname(t)
                    arg = argstr()
                    # volatile registers become unknown
                    nr3 = rd(3)
                    tt = newtmp()
                    out.append('%s = %s(%s);' % (tt, nm, arg))
                    for i in range(3, 13):
                        gpr[i] = E('?r%d' % i, True)
                        b.mod.add(('r', i))
                    for i in range(0, 14):
                        fpr[i] = E('?f%d' % i, True)
                        b.mod.add(('f', i))
                    gpr[3] = E(tt, True)
                    fpr[1] = E(tt, True)
                    argmod.clear()
                    cr.clear()
                else:
                    last_branch = ('goto', t)
            elif op == 16:
                bo, bi = rd_, ra
                t = (a + sx(w & 0xfffc, 16)) & 0xffffffff
                if w & 1:
                    out.append('// call-bc ??')
                elif bo & 16 and bo & 4:
                    last_branch = ('goto', t)
                elif not (bo & 4):
                    cond = '--ctr %s 0' % ('==' if bo & 2 else '!=')
                    if not (bo & 16):
                        cond += ' && ' + (crbit(bi) if bo & 8 else '!(%s)' % crbit(bi))
                    last_branch = ('if', cond, t)
                else:
                    c = crbit(bi)
                    cond = c if bo & 8 else self.negate(c)
                    last_branch = ('if', cond, t)
            elif op == 19:
                x = (w >> 1) & 0x3ff
                if x == 16:
                    bo, bi = rd_, ra
                    if bo & 16 and bo & 4:
                        last_branch = ('ret',)
                    else:
                        c = crbit(bi)
                        cond = c if bo & 8 else self.negate(c)
                        last_branch = ('retif', cond)
                elif x == 528:
                    if w & 1:
                        tgt = gpr.get('CTR', E('CTR', True))
                        arg = argstr()
                        tt = newtmp()
                        out.append('%s = (*%s)(%s);' % (tt, tgt.s(), arg))
                        for i in range(3, 13):
                            gpr[i] = E('?r%d' % i, True)
                            b.mod.add(('r', i))
                        for i in range(0, 14):
                            fpr[i] = E('?f%d' % i, True)
                            b.mod.add(('f', i))
                        gpr[3] = E(tt, True)
                        fpr[1] = E(tt, True)
                        argmod.clear()
                    else:
                        last_branch = ('bctr', gpr.get('CTR', E('CTR', True)).s())
                elif x == 449 or x == 33 or x == 193 or x == 257 or x == 225 or x == 129 or x == 289 or x == 417:
                    d, a_, b_ = rd_, ra, rb
                    ca_, cb_ = crbit(a_), crbit(b_)
                    sym = {449: '||', 257: '&&', 193: '^', 33: 'nor', 225: 'nand', 129: 'andc', 289: 'eqv', 417: 'orc'}[x]
                    if sym in ('||', '&&'):
                        v = '(%s) %s (%s)' % (ca_, sym, cb_)
                    elif sym == '^':
                        v = '(%s) != (%s)' % (ca_, cb_)
                    else:
                        v = '%s(%s, %s)' % (sym, ca_, cb_)
                    f_, bit = d >> 2, d & 3
                    cr.setdefault(f_, {0: 'cr%d.lt' % f_, 1: 'cr%d.gt' % f_, 2: 'cr%d.eq' % f_, 3: 'cr%d.so' % f_})
                    cr[f_] = dict(cr[f_])
                    cr[f_][bit] = v
            else:
                out.append('// ?? %06x: %08x' % (a, w))
        b.final = dict(gpr=gpr, fpr=fpr, cr=cr, last=last_branch, out=out, argmod=argmod)
        return cr

    def negate(self, c):
        m = re.fullmatch(r'(.*) (<|>|==|<=|>=|<u|>u) (.*)', c)
        if m and '&&' not in c and '||' not in c:
            inv = {'<': '>=', '>': '<=', '==': '!=', '<u': '>=u', '>u': '<=u'}
            return '%s %s %s' % (m.group(1), inv[m.group(2)], m.group(3))
        return '!(%s)' % c

    # ------------------------------------------------------------ driver
    def lift(self, start, end):
        blocks = self.blocks(start, end)
        # pass 1: gather up-exposed registers (with the same CR carry logic)
        self.tmp = 0
        carry = None
        for b in blocks:
            cr = self.run_block(b, None, None if b.label else carry, False)
            carry = cr if not self._ends_unconditionally(b) else None
        # liveness (backward dataflow over the block CFG)
        by_start = {b.start: b for b in blocks}
        succ = {}
        for i, b in enumerate(blocks):
            lb = b.final['last']
            nxt = blocks[i + 1].start if i + 1 < len(blocks) else None
            ss = []
            if lb is None or lb[0] == 'retif':
                ss = [nxt] if nxt is not None else []
            elif lb[0] == 'goto':
                ss = [lb[1]]
            elif lb[0] == 'if':
                ss = [lb[2]] + ([nxt] if nxt is not None else [])
            succ[b.start] = [x for x in ss if x in by_start]
        live_in = {b.start: set() for b in blocks}
        live_out = {b.start: set() for b in blocks}
        retlive = {('r', 3), ('f', 1)}
        changed = True
        while changed:
            changed = False
            for b in reversed(blocks):
                lo = set()
                for x in succ[b.start]:
                    lo |= live_in[x]
                lb = b.final['last']
                if lb is not None and lb[0] in ('ret', 'retif'):
                    lo |= retlive
                li = b.upexp | (lo - b.mod)
                if lo != live_out[b.start] or li != live_in[b.start]:
                    live_out[b.start], live_in[b.start] = lo, li
                    changed = True
        glob = None
        # pass 2: emit
        self.tmp = 0
        carry = None
        lines = []
        prev_ret = False
        for b in blocks:
            cr = self.run_block(b, glob, None if b.label else carry, True)
            f = b.final
            if prev_ret and not b.label:
                lines.append('')
                lines.append('// ======== sub_%x%s ========' % (b.start, ('  ' + self.names.code[b.start]) if b.start in self.names.code else ''))
            if b.label:
                lines.append('L%x:' % b.start)
            elif b.start == start:
                pass
            for l in f['out']:
                lines.append('  ' + l)
            # write back live modified regs
            wb = []
            for kind, n in sorted(b.mod, key=lambda k: (k[0], k[1])):
                if (kind, n) in live_out[b.start]:
                    v = (f['gpr'] if kind == 'r' else f['fpr']).get(n)
                    if v is None:
                        continue
                    nm = '%s%d' % (kind, n)
                    if v.t == nm or v.t.startswith('?'):
                        continue
                    wb.append('%s = %s;' % (nm, v.s()))
            lb = f['last']
            wbregs = {}
            for kind, n in b.mod:
                if (kind, n) in live_out[b.start]:
                    v = (f['gpr'] if kind == 'r' else f['fpr']).get(n)
                    if v is not None and v.t != '%s%d' % (kind, n) and not v.t.startswith('?'):
                        wbregs['%s%d' % (kind, n)] = v
            condtxt = None
            if lb is not None and lb[0] in ('if', 'retif') and wbregs:
                condtxt = lb[1]
                hold = {}
                for nm, v in wbregs.items():
                    if not v.atom and v.p() in condtxt:
                        condtxt = condtxt.replace(v.p(), '\x00' + nm + '\x00')
                bad = [nm for nm in wbregs if re.search(r'(?<![\w\x00.])%s(?![\w\x00])' % nm, condtxt)]
                if bad:
                    lines.append('  c = %s;' % lb[1])
                    condtxt = 'c'
                else:
                    condtxt = condtxt.replace('\x00', '')
                lb = (lb[0], condtxt) + tuple(lb[2:])
            for l in wb:
                lines.append('  ' + l)
            if lb is None:
                pass
            elif lb[0] == 'goto':
                lines.append('  goto L%x;' % lb[1])
            elif lb[0] == 'if':
                lines.append('  if (%s) goto L%x;' % (lb[1], lb[2]))
            elif lb[0] == 'ret':
                r3 = f['gpr'].get(3)
                f1 = f['fpr'].get(1)
                rv = []
                if r3 is not None and not r3.t.startswith('?') and r3.t != 'r3':
                    rv.append('r3=' + r3.s())
                if f1 is not None and not f1.t.startswith('?') and f1.t != 'f1' and (r3 is None or f1.t != r3.t):
                    rv.append('f1=' + f1.s())
                lines.append('  return%s;' % ((' ' + ', '.join(rv)) if rv else ''))
            elif lb[0] == 'retif':
                lines.append('  if (%s) return;' % lb[1])
            elif lb[0] == 'bctr':
                lines.append('  goto *(%s);   // switch' % lb[1])
            carry = cr if not self._ends_unconditionally(b) else None
            prev_ret = bool(f['last'] and f['last'][0] == 'ret')
        return '\n'.join(lines)

    @staticmethod
    def _ends_unconditionally(b):
        l = b.final['last']
        return l is not None and l[0] in ('goto', 'ret', 'bctr')


def main(argv):
    if len(argv) < 4:
        print(__doc__)
        return 1
    img = ppcdis.Image(argv[1])
    names = ppcdis.Names(ppcdis.default_names_path())
    L = Lifter(img, names)
    num = lambda s: int(s, 0)
    argv = list(argv)
    while '--jt' in argv:          # --jt DATAOFFSET:COUNT  (table of big-endian code addresses in the data section)
        i = argv.index('--jt')
        off, n = argv[i + 1].split(':')
        off, n = num(off), num(n)
        for k in range(n):
            L.extra_targets.add(struct.unpack('>I', img.data[off + 4 * k:off + 4 * k + 4])[0])
        del argv[i:i + 2]
    if argv[2] == 'func':
        s = num(argv[3])
        e = ppcdis.func_end(img, s)
    else:
        s, e = num(argv[2]), num(argv[3])
    print(L.lift(s, e))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
