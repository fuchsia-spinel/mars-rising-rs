#!/usr/bin/env python3
"""Fuller PowerPC (603, 32-bit) disassembler + PEF helper for the Mars Rising executable.

Adds to ppc.py: floating point (op 59/63, lfs/stfs/lfd/stfd/lfsx...), condition-register ops, pretty branches (beq/bne/bdnz...),
simplified mnemonics (mr/li/nop/slwi/srwi/clrlwi/extlwi...), PEF loader-section parsing (imports + relocations) so that
the CFM glue stubs ("lwz r12,N(r2) ... bctr") are named after the InterfaceLib/MathLib symbol they call, and TOC/global annotation
(`r2 = 0x8000`, every `N(r2)` access is shown as data offset 0x8000+N and the 32-bit word stored there).

Usage:
  python3 -I ppcdis.py EXE dis  START END            # linear disassembly of code offsets
  python3 -I ppcdis.py EXE func ADDR                 # disassemble the function that starts at ADDR (until the next prologue)
  python3 -I ppcdis.py EXE callers ADDR              # every `bl ADDR` / `b ADDR` and every TOC word holding ADDR
  python3 -I ppcdis.py EXE refs DATAOFF              # every instruction that touches r2+DATAOFF (signed offset, e.g. -29532)
  python3 -I ppcdis.py EXE imports                   # glue stub -> imported symbol
  python3 -I ppcdis.py EXE data ADDR LEN             # dump data section words
An optional names file (default tools/re/names.txt: lines `c 0x22170 name` / `d -29532 name`) labels code and data.
"""
import struct, sys, os, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pef as _pef

TOC = 0x8000


def sx(v, b):
    return v - (1 << b) if v & (1 << (b - 1)) else v


# ---------------------------------------------------------------- PEF loader section
class Image:
    def __init__(self, path):
        self.b, self.secs = _pef.load_pef(path)
        self.code = self.secs[0]['raw']
        d = self.secs[1]
        self.data = _pef.unpack_pattern(d['raw'], d['total'])
        self.loader = self.secs[2]['raw'] if len(self.secs) > 2 else b''
        self.import_names = []
        self.import_at = {}     # data address of the TOC slot -> import symbol
        self.stub_name = {}     # code offset of glue stub -> symbol
        self._parse_loader()
        self._find_stubs()

    def _parse_loader(self):
        L = self.loader
        (ms, mo, isec, ioff, ts, to, libc, symc, relsec, relinstr, strs, exph, expp, expc) = struct.unpack('>iIiIiIIIIIIIII', L[:56])
        p = 56 + libc * 24
        lib_ranges = []
        for i in range(libc):
            name_off, ov, cv, cnt, first, opt, ra, rb = struct.unpack('>IIIIIBBH', L[56 + i * 24:56 + i * 24 + 24])
            lib_ranges.append((self._cstr(strs + name_off), first, cnt))
        syms = []
        for i in range(symc):
            w = struct.unpack('>I', L[p + i * 4:p + i * 4 + 4])[0]
            syms.append(self._cstr(strs + (w & 0xffffff)))
        for lib, first, cnt in lib_ranges:
            for k in range(first, first + cnt):
                syms[k] = syms[k]
        self.import_names = syms
        self.import_lib = {}
        for lib, first, cnt in lib_ranges:
            for k in range(first, first + cnt):
                self.import_lib[k] = lib
        p += symc * 4
        rels = []
        for i in range(relsec):
            si, ra, cnt, first = struct.unpack('>HHII', L[p + i * 12:p + i * 12 + 12])
            rels.append((si, cnt, first))
        for si, cnt, first in rels:
            if si != 1:
                continue
            self._run_relocs(L, relinstr + first, cnt)

    def _cstr(self, off):
        e = self.loader.index(b'\0', off)
        return self.loader[off:e].decode('latin1')

    def _run_relocs(self, L, off, count):
        ins = [struct.unpack('>H', L[off + 2 * i:off + 2 * i + 2])[0] for i in range(count)]
        addr = 0
        imp = 0
        i = 0
        last_repeat_start = 0

        def do(op_i):
            nonlocal addr, imp, i
            w = ins[op_i]
            top2 = w >> 14
            top7 = w >> 9
            if top2 == 0:                      # RelocBySectDWithSkip
                addr += ((w >> 6) & 0xff) * 4
                addr += (w & 0x3f) * 4
            elif top7 == 0x20 or top7 == 0x21:  # BySectC / BySectD
                addr += ((w & 0x1ff) + 1) * 4
            elif top7 == 0x22:
                addr += ((w & 0x1ff) + 1) * 12
            elif top7 == 0x23:
                addr += ((w & 0x1ff) + 1) * 8
            elif top7 == 0x24:
                addr += ((w & 0x1ff) + 1) * 8
            elif top7 == 0x25:                  # ImportRun
                for _ in range((w & 0x1ff) + 1):
                    self.import_at[addr] = imp
                    imp += 1
                    addr += 4
            elif top7 == 0x30:                  # SmByImport
                imp = w & 0x1ff
                self.import_at[addr] = imp
                imp += 1
                addr += 4
            elif top7 in (0x31, 0x32):          # SmSetSectC / D
                pass
            elif top7 == 0x33:                  # SmBySection
                addr += 4
            elif (w >> 12) == 0x8:              # IncrPosition
                addr += (w & 0xfff) + 1
            else:
                raise ValueError('reloc op %04x' % w)

        while i < len(ins):
            w = ins[i]
            if (w >> 12) == 0x9:                # SmRepeat
                chunk = ((w >> 8) & 0xf) + 1
                rep = (w & 0xff) + 1
                for _ in range(rep):
                    for k in range(i - chunk, i):
                        do(k)
                i += 1
            elif (w >> 10) == 0x28:             # SetPosition (32-bit)
                addr = ((w & 0x3ff) << 16) | ins[i + 1]
                i += 2
            elif (w >> 10) == 0x29:             # LgByImport
                idx = ((w & 0x3ff) << 16) | ins[i + 1]
                self.import_at[addr] = idx
                imp = idx + 1
                addr += 4
                i += 2
            elif (w >> 10) == 0x2c:             # LgRepeat
                chunk = ((w >> 6) & 0xf) + 1
                rep = ((w & 0x3f) << 16) | ins[i + 1]
                for _ in range(rep + 1):
                    for k in range(i - chunk, i):
                        do(k)
                i += 2
            elif (w >> 10) == 0x2d:             # LgSetOrBySection
                sub = (w >> 6) & 0xf
                i += 2
                if sub in (0, 1, 4, 5):         # BySectionC/D with run... (not used by import slots)
                    addr += 4 if sub in (0, 1) else 0
            else:
                do(i)
                i += 1

    def _find_stubs(self):
        """CFM glue stub:  lwz r12,N(r2); stw r2,20(r1); lwz r0,0(r12); lwz r2,4(r12); mtctr r0; bctr"""
        code = self.code
        for a in range(0, len(code) - 24, 4):
            w0, w1, w2, w3, w4, w5 = struct.unpack('>6I', code[a:a + 24])
            if (w0 >> 16) == 0x819e or (w0 >> 16) == 0x8182:       # lwz r12,N(r2)   (0x8182 = r12,r2)
                pass
            if w0 >> 16 == 0x8182 and w1 == 0x90410014 and w2 == 0x800c0000 and w3 == 0x804c0004 and w4 == 0x7c0903a6 and w5 == 0x4e800420:
                n = sx(w0 & 0xffff, 16)
                idx = self.import_at.get(TOC + n)
                if idx is not None:
                    self.stub_name[a] = self.import_names[idx]

    def word_data(self, addr):
        if 0 <= addr and addr + 4 <= len(self.data):
            return struct.unpack('>I', self.data[addr:addr + 4])[0]
        return None


# ---------------------------------------------------------------- disassembler
CRB = ['lt', 'gt', 'eq', 'so']
BR_TRUE = {0: 'blt', 1: 'bgt', 2: 'beq', 3: 'bso'}
BR_FALSE = {0: 'bge', 1: 'ble', 2: 'bne', 3: 'bns'}

X31 = {
    0: 'cmpw', 4: 'tw', 8: 'subfc', 10: 'addc', 11: 'mulhwu', 19: 'mfcr', 20: 'lwarx', 23: 'lwzx', 24: 'slw', 26: 'cntlzw', 28: 'and', 32: 'cmplw',
    40: 'subf', 54: 'dcbst', 55: 'lwzux', 60: 'andc', 75: 'mulhw', 83: 'mfmsr', 86: 'dcbf', 87: 'lbzx', 104: 'neg', 119: 'lbzux', 124: 'nor',
    136: 'subfe', 138: 'adde', 144: 'mtcrf', 146: 'mtmsr', 150: 'stwcx.', 151: 'stwx', 183: 'stwux', 200: 'subfze', 202: 'addze', 215: 'stbx',
    232: 'subfme', 234: 'addme', 235: 'mullw', 246: 'dcbtst', 247: 'stbux', 266: 'add', 278: 'dcbt', 279: 'lhzx', 284: 'eqv', 311: 'lhzux',
    316: 'xor', 339: 'mfspr', 343: 'lhax', 371: 'mftb', 407: 'sthx', 412: 'orc', 444: 'or', 459: 'divwu', 467: 'mtspr', 476: 'nand', 491: 'divw',
    512: 'mcrxr', 535: 'lfsx', 536: 'srw', 567: 'lfsux', 597: 'lswi', 598: 'sync', 599: 'lfdx', 631: 'lfdux', 663: 'stfsx', 695: 'stfsux',
    725: 'stswi', 727: 'stfdx', 759: 'stfdux', 790: 'lhbrx', 792: 'sraw', 824: 'srawi', 854: 'eieio', 918: 'sthbrx', 922: 'extsh', 954: 'extsb',
    982: 'icbi', 983: 'stfiwx', 1014: 'dcbz', 341: 'lwax',
}
OE_OPS = {8, 10, 40, 104, 136, 138, 200, 202, 232, 234, 235, 266, 459, 491}
SPR = {1: 'xer', 8: 'lr', 9: 'ctr', 18: 'dsisr', 19: 'dar', 22: 'dec', 26: 'srr0', 27: 'srr1', 268: 'tbl', 269: 'tbu'}
LDST = {32: 'lwz', 33: 'lwzu', 34: 'lbz', 35: 'lbzu', 36: 'stw', 37: 'stwu', 38: 'stb', 39: 'stbu', 40: 'lhz', 41: 'lhzu', 42: 'lha', 43: 'lhau',
        44: 'sth', 45: 'sthu', 46: 'lmw', 47: 'stmw', 48: 'lfs', 49: 'lfsu', 50: 'lfd', 51: 'lfdu', 52: 'stfs', 53: 'stfsu', 54: 'stfd', 55: 'stfdu'}
FP59 = {18: 'fdivs', 20: 'fsubs', 21: 'fadds', 22: 'fsqrts', 24: 'fres', 25: 'fmuls', 28: 'fmsubs', 29: 'fmadds', 30: 'fnmsubs', 31: 'fnmadds'}
FP63 = {18: 'fdiv', 20: 'fsub', 21: 'fadd', 22: 'fsqrt', 23: 'fsel', 25: 'fmul', 26: 'frsqrte', 28: 'fmsub', 29: 'fmadd', 30: 'fnmsub', 31: 'fnmadd'}
FP63X = {0: 'fcmpu', 12: 'frsp', 14: 'fctiw', 15: 'fctiwz', 32: 'fcmpo', 38: 'mtfsb1', 40: 'fneg', 64: 'mcrfs', 70: 'mtfsb0', 72: 'fmr', 134: 'mtfsfi',
         136: 'fnabs', 264: 'fabs', 583: 'mffs', 711: 'mtfsf'}


def branch_name(bo, bi, link, kind, tgt=None):
    """kind: 'bc', 'bclr', 'bcctr'. Returns (mnemonic, operand_string)."""
    suf = 'l' if link else ''
    crf, bit = bi >> 2, bi & 3
    cr = '' if crf == 0 else f'cr{crf},'
    base = {'bc': '', 'bclr': 'lr', 'bcctr': 'ctr'}[kind]
    ign = bo & 16
    cv = bo & 8
    noctr = bo & 4
    ctrz = bo & 2
    if kind == 'bc':
        t = f'0x{tgt:x}'
    else:
        t = ''
    if ign and noctr:
        if kind == 'bc':
            return 'b' + suf, t
        return {'bclr': 'blr', 'bcctr': 'bctr'}[kind] + suf, ''
    if noctr:
        m = (BR_TRUE if cv else BR_FALSE)[bit] + (base if kind != 'bc' else '') + suf
        return m, (cr + t).rstrip(',') if kind == 'bc' else cr.rstrip(',')
    name = 'bdz' if ctrz else 'bdnz'
    if not ign:
        name += 't' if cv else 'f'
        name += suf
        return name + (base if kind != 'bc' else ''), f'{4 * crf + bit}' + (f',{t}' if t else '')
    return name + (base if kind != 'bc' else '') + suf, t


def dis(w, pc, img=None):
    op = w >> 26
    rd = (w >> 21) & 31
    ra = (w >> 16) & 31
    rb = (w >> 11) & 31
    imm = w & 0xffff
    si = sx(imm, 16)
    rc = '.' if w & 1 else ''
    note = ''

    def d_ra():
        nonlocal note
        if ra == 2 and img is not None:
            a = TOC + si
            v = img.word_data(a)
            note = f'; D[{si:+d}] = 0x{a:x}' + (f' (word 0x{v:08x})' if v is not None else '')
        return f'{si}(r{ra})'

    if op == 14:
        if ra == 0:
            return f'li r{rd},{si}', ''
        if ra == 2 and img is not None:
            note = f'; &D[{si:+d}] = 0x{TOC + si:x}'
        return f'addi r{rd},r{ra},{si}', note
    if op == 15:
        return (f'lis r{rd},0x{imm:04x}' if ra == 0 else f'addis r{rd},r{ra},0x{imm:04x}'), ''
    if op == 12:
        return f'addic r{rd},r{ra},{si}', ''
    if op == 13:
        return f'addic. r{rd},r{ra},{si}', ''
    if op == 8:
        return f'subfic r{rd},r{ra},{si}', ''
    if op == 7:
        return f'mulli r{rd},r{ra},{si}', ''
    if op == 10:
        return f'cmplwi {"" if (w>>23)&7==0 else "cr%d," % ((w>>23)&7)}r{ra},{imm}', ''
    if op == 11:
        return f'cmpwi {"" if (w>>23)&7==0 else "cr%d," % ((w>>23)&7)}r{ra},{si}', ''
    if op == 24:
        return ('nop' if w == 0x60000000 else f'ori r{ra},r{rd},0x{imm:x}'), ''
    if op == 25:
        return f'oris r{ra},r{rd},0x{imm:x}', ''
    if op == 26:
        return f'xori r{ra},r{rd},0x{imm:x}', ''
    if op == 27:
        return f'xoris r{ra},r{rd},0x{imm:x}', ''
    if op == 28:
        return f'andi. r{ra},r{rd},0x{imm:x}', ''
    if op == 29:
        return f'andis. r{ra},r{rd},0x{imm:x}', ''
    if op == 21:
        sh, mb, me = rb, (w >> 6) & 31, (w >> 1) & 31
        if mb == 0 and me == 31 - sh:
            return f'slwi{rc} r{ra},r{rd},{sh}', ''
        if me == 31 and sh == 32 - mb and mb != 0:
            return f'srwi{rc} r{ra},r{rd},{mb}', ''
        if sh == 0 and me == 31:
            return f'clrlwi{rc} r{ra},r{rd},{mb}', ''
        if sh == 0 and mb == 0:
            return f'clrrwi{rc} r{ra},r{rd},{31 - me}', ''
        return f'rlwinm{rc} r{ra},r{rd},{sh},{mb},{me}', ''
    if op == 20:
        return f'rlwimi{rc} r{ra},r{rd},{rb},{(w>>6)&31},{(w>>1)&31}', ''
    if op == 23:
        return f'rlwnm{rc} r{ra},r{rd},r{rb},{(w>>6)&31},{(w>>1)&31}', ''
    if op in LDST:
        r = ('f%d' % rd) if 48 <= op <= 55 else 'r%d' % rd
        return f'{LDST[op]} {r},{d_ra()}', note
    if op == 18:
        li = sx(w & 0x3fffffc, 26)
        t = (pc + li) & 0xffffffff if not w & 2 else li & 0xffffffff
        nm = ('bl' if w & 1 else 'b') + ('a' if w & 2 else '')
        s = f'{nm} 0x{t:x}'
        if img is not None and t in img.stub_name:
            note = f'; -> {img.stub_name[t]}'
        return s, note
    if op == 16:
        bd = sx(w & 0xfffc, 16)
        t = ((pc + bd) if not w & 2 else bd) & 0xffffffff
        m, o = branch_name(rd, ra, w & 1, 'bc', t)
        return f'{m} {o}'.strip(), ''
    if op == 19:
        x = (w >> 1) & 0x3ff
        if x == 16:
            m, o = branch_name(rd, ra, w & 1, 'bclr')
            return f'{m} {o}'.strip(), ''
        if x == 528:
            m, o = branch_name(rd, ra, w & 1, 'bcctr')
            return f'{m} {o}'.strip(), ''
        crn = {0: 'mcrf', 33: 'crnor', 129: 'crandc', 150: 'isync', 193: 'crxor', 225: 'crnand', 257: 'crand', 289: 'creqv', 417: 'crorc', 449: 'cror'}
        if x in crn:
            if x == 150:
                return 'isync', ''
            if x == 0:
                return f'mcrf cr{rd>>2},cr{ra>>2}', ''
            return f'{crn[x]} {rd},{ra},{rb}', ''
        return f'op19.{x}', ''
    if op == 31:
        x = (w >> 1) & 0x3ff
        if x == 339:
            spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5)
            return f'mfspr r{rd},{SPR.get(spr, spr)}', ''
        if x == 467:
            spr = ((w >> 16) & 31) | (((w >> 11) & 31) << 5)
            return f'mtspr {SPR.get(spr, spr)},r{rd}', ''
        if x == 444 and rd == rb:
            return f'mr{rc} r{ra},r{rd}', ''
        xo = x
        oe = ''
        if x & 0x200 and (x & 0x1ff) in OE_OPS:
            xo = x & 0x1ff
            oe = 'o'
        if xo == 0 or xo == 32:
            return f'{X31[xo]} {"" if (w>>23)&7==0 else "cr%d," % ((w>>23)&7)}r{ra},r{rb}', ''
        if xo == 824:
            return f'srawi{rc} r{ra},r{rd},{rb}', ''
        if xo in (922, 954, 26):
            return f'{X31[xo]}{rc} r{ra},r{rd}', ''
        if xo == 19:
            return f'mfcr r{rd}', ''
        if xo == 144:
            return f'mtcrf 0x{(w>>12)&0xff:02x},r{rd}', ''
        if xo in (598, 854):
            return X31[xo], ''
        if xo in (535, 567, 599, 631, 663, 695, 727, 759, 983):
            return f'{X31[xo]} f{rd},r{ra},r{rb}', ''
        if xo in X31:
            n = X31[xo]
            if xo in (23, 87, 279, 343, 55, 119, 311, 20, 790, 341, 151, 215, 407, 183, 247, 918, 150):
                return f'{n} r{rd},r{ra},r{rb}', ''
            if xo in (104, 200, 202, 232, 234):
                return f'{n}{oe}{rc} r{rd},r{ra}', ''
            if xo in (28, 444, 316, 124, 24, 536, 792, 60, 412, 476, 284):
                return f'{n}{rc} r{ra},r{rd},r{rb}', ''
            if xo in (54, 86, 246, 278, 982, 1014):
                return f'{n} r{ra},r{rb}', ''
            return f'{n}{oe}{rc} r{rd},r{ra},r{rb}', ''
        return f'op31.{x}', ''
    if op == 59:
        x5 = (w >> 1) & 31
        fc = (w >> 6) & 31
        if x5 in (25,):
            return f'fmuls{rc} f{rd},f{ra},f{fc}', ''
        if x5 in FP59:
            n = FP59[x5]
            if x5 in (28, 29, 30, 31):
                return f'{n}{rc} f{rd},f{ra},f{fc},f{rb}', ''
            return f'{n}{rc} f{rd},f{ra},f{rb}', ''
        return f'op59.{x5}', ''
    if op == 63:
        x5 = (w >> 1) & 31
        fc = (w >> 6) & 31
        x10 = (w >> 1) & 0x3ff
        if x5 in FP63:
            n = FP63[x5]
            if x5 == 25:
                return f'fmul{rc} f{rd},f{ra},f{fc}', ''
            if x5 in (23, 28, 29, 30, 31):
                return f'{n}{rc} f{rd},f{ra},f{fc},f{rb}', ''
            if x5 in (22, 26):
                return f'{n}{rc} f{rd},f{rb}', ''
            return f'{n}{rc} f{rd},f{ra},f{rb}', ''
        if x10 in FP63X:
            n = FP63X[x10]
            if x10 in (0, 32):
                return f'{n} cr{rd>>2},f{ra},f{rb}', ''
            if x10 in (12, 14, 15, 40, 72, 136, 264):
                return f'{n}{rc} f{rd},f{rb}', ''
            if x10 == 583:
                return f'mffs{rc} f{rd}', ''
            return n, ''
        return f'op63.{x10}', ''
    if op == 17:
        return 'sc', ''
    if op == 2:
        return f'tdi {rd},r{ra},{si}', ''
    if op == 3:
        return f'twi {rd},r{ra},{si}', ''
    return f'.long 0x{w:08x}', ''


class Names:
    def __init__(self, path):
        self.code = {}
        self.data = {}
        if path and os.path.exists(path):
            for line in open(path):
                line = line.split('#')[0].split()
                if len(line) >= 3:
                    v = int(line[1], 0)
                    (self.code if line[0] == 'c' else self.data)[v] = line[2]


def default_names_path():
    return os.path.join(os.path.dirname(os.path.abspath(__file__)), 'names.txt')


def listing(img, names, start, end):
    code = img.code
    # label branch targets inside the window
    targets = set()
    for a in range(start, end, 4):
        w = struct.unpack('>I', code[a:a + 4])[0]
        op = w >> 26
        if op == 18 and not (w & 1):
            targets.add(((a + sx(w & 0x3fffffc, 26)) & 0xffffffff) if not w & 2 else 0)
        elif op == 16:
            targets.add((a + sx(w & 0xfffc, 16)) & 0xffffffff)
    out = []
    for a in range(start, end, 4):
        w = struct.unpack('>I', code[a:a + 4])[0]
        t, note = dis(w, a, img)
        # name annotation for direct bl targets
        m = re.search(r'0x([0-9a-f]+)$', t)
        if m and t.startswith(('bl', 'b ')):
            tv = int(m.group(1), 16)
            if tv in names.code:
                note = (note + ' ' if note else '; ') + '-> ' + names.code[tv]
        if a in names.code:
            out.append(f'\n; ===== {names.code[a]} =====')
        if a in targets and a > start:
            out.append(f'L{a:x}:')
        out.append(f'  {a:06x}: {w:08x}  {t:<34}{note}')
    return '\n'.join(out)


def func_end(img, start, limit=0x3000):
    """End of the function starting at `start`: next `mflr r0` that follows a blr (prologue heuristic)."""
    code = img.code
    a = start + 4
    seen_blr = False
    while a < min(len(code) - 4, start + limit):
        w = struct.unpack('>I', code[a:a + 4])[0]
        if w == 0x4e800020:
            seen_blr = True
        elif seen_blr:
            if w == 0x7c0802a6:         # mflr r0
                return a
            # a conditional-return tail or padding keeps scanning
        a += 4
    return min(len(code), start + limit)


def main(argv):
    if len(argv) < 3:
        print(__doc__)
        return 1
    img = Image(argv[1])
    names = Names(default_names_path())
    cmd = argv[2]
    num = lambda s: int(s, 0)
    if cmd == 'dis':
        print(listing(img, names, num(argv[3]), num(argv[4])))
    elif cmd == 'func':
        s = num(argv[3])
        e = func_end(img, s)
        print(listing(img, names, s, e))
        print(f'\n; function 0x{s:x}..0x{e:x} ({(e-s)//4} instructions)')
    elif cmd == 'callers':
        t = num(argv[3])
        code = img.code
        for a in range(0, len(code) - 4, 4):
            w = struct.unpack('>I', code[a:a + 4])[0]
            if w >> 26 == 18 and not (w & 2):
                if ((a + sx(w & 0x3fffffc, 26)) & 0xffffffff) == t:
                    print(f'{a:06x}: {"bl" if w & 1 else "b "}')
        for off in range(0, len(img.data) - 3, 4):
            if struct.unpack('>I', img.data[off:off + 4])[0] == t:
                print(f'data word at D 0x{off:x} (r2{off - TOC:+d}) == 0x{t:x}')
    elif cmd == 'refs':
        d = num(argv[3])
        code = img.code
        for a in range(0, len(code) - 4, 4):
            w = struct.unpack('>I', code[a:a + 4])[0]
            op = w >> 26
            if ((w >> 16) & 31) == 2 and (op in LDST or op == 14) and sx(w & 0xffff, 16) == d:
                print(f'{a:06x}: {dis(w, a, img)[0]}')
    elif cmd == 'imports':
        for a, n in sorted(img.stub_name.items()):
            print(f'{a:06x}  {n}')
    elif cmd == 'data':
        s, n = num(argv[3]), num(argv[4])
        for off in range(s, s + n, 16):
            row = img.data[off:off + 16]
            print(f'{off:06x} (r2{off - TOC:+d}): ' + ' '.join(f'{x:02x}' for x in row))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv))
