#!/usr/bin/env python3
"""
mr_extract.py -- extract Mars Rising (Ambrosia Software, 1998) game assets into standard open formats.

    python3 mr_extract.py <Mars_Rising_Installer | installed game folder> <output dir> [--with-executable]

Input:  the Mars Rising installer application's DATA FORK (what you get after unpacking Mars Rising-1.01.sit
        with e.g. unar/stuffit-rs), OR an installed game folder whose resource forks are available as
        "<name>.rsrc", AppleDouble "._<name>", or "<name>/..namedfork/rsrc".
Output: PNG (tiles, sprites, pictures, icons, cursors), WAV (sounds), JSON (levels, strings, menus, ...), raw .bin
        (lookup tables), MADH music, UTF-8 docs. No resource forks are needed afterwards. Pure standard library.
You must own a legitimate copy of the game; the extracted assets remain (c) Ambrosia Software / David Wareing.
"""
import sys, os, json, struct
# ---------------------------------------------------------------- container ------------------
def parse_table(d):
    """File table at the start of the data fork. Returns list of entry dicts."""
    pos, stack, ents = 10, [], []          # 10-byte header (purpose of its fields unknown)
    while True:
        b = d[pos]
        if b & 0x80:                        # folder: 0x80|len, name, u16 = number of entries inside
            n = b & 0x7f; name = d[pos+1:pos+1+n].decode('mac_roman'); pos += 1 + n
            cnt = struct.unpack('>H', d[pos:pos+2])[0]; pos += 2
            for f in stack: f[1] -= 1
            stack.append([name, cnt])
        else:                               # file record
            n = b; name = d[pos+1:pos+1+n].decode('mac_roman'); p = pos + 1 + n
            off, = struct.unpack('>I', d[p+1:p+5])           # absolute offset of payload (d[p] is always 1)
            typ, cre = d[p+5:p+9], d[p+9:p+13]
            cdate, mdate = struct.unpack('>II', d[p+13:p+21])
            flags, = struct.unpack('>H', d[p+21:p+23])
            crc = d[p+23:p+27].hex()                         # algorithm not identified
            meth, = struct.unpack('>H', d[p+27:p+29])        # bit1(2)=rsrc Huffman/LZ, bit2(4)=data Huffman/LZ
            rlen, dlen, rclen, dclen = struct.unpack('>IIII', d[p+29:p+45])
            ents.append(dict(name=name, off=off, type=typ.decode('mac_roman'), creator=cre.decode('mac_roman'),
                             cdate=cdate, mdate=mdate, flags=flags, crc=crc, meth=meth,
                             rlen=rlen, dlen=dlen, rclen=rclen, dclen=dclen, dirs=[f[0] for f in stack]))
            for f in stack: f[1] -= 1
            pos = p + 45
        while stack and stack[-1][1] <= 0: stack.pop()
        if ents and pos >= min(e['off'] for e in ents): return ents   # table ends where payload begins

# ---------------------------------------------------------------- decompression --------------
class DecodeError(Exception): pass

def _read_table(buf, pos, size):
    """[N][N bytes of nibbles, hi first] -> code lengths for 2N symbols; canonical code, MSB first,
    expanded into a 0x8000-entry direct lookup table (15-bit index)."""
    n = buf[pos]; pos += 1
    if 2*n > size: raise DecodeError('table too large')
    lens, ents, sym = [], [], 0
    for _ in range(n):
        b = buf[pos]; pos += 1
        for nib in (b >> 4, b & 15):
            lens.append(nib)
            if nib: ents.append((nib, sym))
            sym += 1
    ents.sort(key=lambda t: t[0])                   # stable => (length, symbol) ascending
    lens += [0] * (size - len(lens))
    tab, d3 = bytearray(0x8000), 0
    for l, s in ents:
        c = 0x8000 >> l
        if d3 + c > 0x8000: raise DecodeError('oversubscribed code')
        tab[d3:d3+c] = bytes([s]) * c; d3 += c
    return lens, tab, pos

class _RLE:
    """Escape 81 82 <n> = repeat previous byte so the run is n long; 81 82 00 = literal 81 82."""
    def __init__(s, total): s.out = bytearray(); s.rem = total; s.st = 0; s.last = 0
    def put(s, b):
        o = s.out
        if s.st == 0:
            if b == 0x81 and s.rem != 1: s.st = 1; return
            s.last = b; o.append(b); s.rem -= 1; return
        if s.st == 1:
            if b == 0x82: s.st = 2; return
            o.append(0x81); s.last = 0x81; s.rem -= 1
            if s.rem == 0: return
            if b == 0x81 and s.rem != 1: return
            s.st = 0; s.last = b; o.append(b); s.rem -= 1; return
        s.st = 0
        if b == 0:
            o.append(0x81); s.rem -= 1
            if s.rem == 0: return
            s.last = 0x82; o.append(0x82); s.rem -= 1; return
        s.rem -= b - 1
        if s.rem < 0: raise DecodeError('RLE overrun')
        o += bytes([s.last]) * (b - 1)

def decode_fork(buf, outlen, lz):
    r = _RLE(outlen)
    if not lz:
        for b in buf:
            if r.rem <= 0: break
            r.put(b)
        return bytes(r.out)
    ring = bytearray(8192); wp = 0; pos = 0
    def emit(b):
        nonlocal wp
        ring[wp] = b; wp = (wp + 1) & 8191; r.put(b)
    while r.rem > 0:                                 # independent blocks, each with its own 3 tables
        l1, t1, pos = _read_table(buf, pos, 256)     #   literals
        l2, t2, pos = _read_table(buf, pos, 0x40)    #   match lengths
        l3, t3, pos = _read_table(buf, pos, 0x80)    #   match distance (high 7 bits)
        d3 = ((buf[pos] << 8) | buf[pos+1]) << 16; pos += 2; d4 = 0
        budget = 0x1fff0                             # literal costs 2, match costs 3
        def take(need):
            nonlocal d3, d4, pos
            if need <= d4: d3 = (d3 << need) & 0xffffffff; d4 -= need; return
            d3 = (d3 << d4) & 0xffffffff; need -= d4
            d3 = (d3 & 0xffff0000) | (buf[pos] << 8) | buf[pos+1]; pos += 2; d4 = 16
            d3 = (d3 << need) & 0xffffffff; d4 -= need
        while budget > 0 and r.rem > 0:
            if d3 & 0x80000000:                      # 1 + Huffman(literal)
                s = t1[(d3 >> 16) & 0x7fff]; take(l1[s] + 1); emit(s); budget -= 2
            else:                                    # 0 + Huffman(len) + Huffman(dist_hi) + 6 raw bits
                ln = t2[(d3 >> 16) & 0x7fff]; take(l2[ln] + 1)
                hs = t3[d3 >> 17]; take(l3[hs])
                dist = (hs << 6) | (d3 >> 26); take(6)
                src = (wp - dist) & 8191
                for _ in range(ln):
                    b = ring[src]; src = (src + 1) & 8191; emit(b)
                if r.rem < 0: raise DecodeError('overrun')
                budget -= 3
    return bytes(r.out)

# ================================================================ generic helpers ================
import re, zlib, wave, io
from collections import OrderedDict

def slug(s):
    s = re.sub(r'[^A-Za-z0-9]+', '_', s.strip()).strip('_').lower()
    return s or 'unnamed'

def write_json(path, obj):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w', encoding='utf-8') as f: json.dump(obj, f, indent=1, ensure_ascii=False, allow_nan=False)

def write_bytes(path, data):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'wb') as f: f.write(data)

def _chunk(t, d):
    c = struct.pack('>I', len(d)) + t + d
    return c + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)

def png_bytes(w, h, rgba=None, indexed=None, palette=None):
    if indexed is not None:
        raw = b''.join(b'\x00' + bytes(indexed[y*w:(y+1)*w]) for y in range(h))
        return (b'\x89PNG\r\n\x1a\n' + _chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 3, 0, 0, 0)) +
                _chunk(b'PLTE', b''.join(bytes(c) for c in palette)) + _chunk(b'IDAT', zlib.compress(raw, 9)) + _chunk(b'IEND', b''))
    raw = b''.join(b'\x00' + bytes(rgba[y*w*4:(y+1)*w*4]) for y in range(h))
    return (b'\x89PNG\r\n\x1a\n' + _chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 6, 0, 0, 0)) +
            _chunk(b'IDAT', zlib.compress(raw, 9)) + _chunk(b'IEND', b''))

def pstr(b, p):
    n = b[p]; return b[p+1:p+1+n].decode('mac_roman'), p + 1 + n

# ================================================================ resource forks ================
def parse_rsrc(b):
    """-> OrderedDict type -> OrderedDict id -> (name, bytes)"""
    do, mo, dl, ml = struct.unpack('>IIII', b[:16]); m = b[mo:mo+ml]
    tl, = struct.unpack('>H', m[24:26]); nl, = struct.unpack('>H', m[26:28])
    nt = struct.unpack('>H', m[tl:tl+2])[0] + 1; out = OrderedDict()
    for i in range(nt):
        p = tl + 2 + i*8; t = m[p:p+4].decode('mac_roman')
        n = struct.unpack('>H', m[p+4:p+6])[0] + 1; ro, = struct.unpack('>H', m[p+6:p+8]); L = OrderedDict()
        for j in range(n):
            e = m[tl+ro+j*12: tl+ro+j*12+12]
            rid, = struct.unpack('>h', e[:2]); no, = struct.unpack('>H', e[2:4]); ao = int.from_bytes(e[5:8], 'big')
            ln, = struct.unpack('>I', b[do+ao:do+ao+4])
            nm = m[nl+no+1:nl+no+1+m[nl+no]].decode('mac_roman') if no != 0xFFFF else ''
            L[rid] = (nm, b[do+ao+4:do+ao+4+ln])
        out[t] = L
    return out

# ================================================================ PICT -> RGBA ==================
def _unpackbits(src, p, n, unit=1):
    out = bytearray()
    while len(out) < n*unit:
        c = src[p]; p += 1
        if c < 128: k = (c+1)*unit; out += src[p:p+k]; p += k
        elif c > 128: out += src[p:p+unit] * (257-c); p += unit
    return bytes(out)

_PICT_FIXED = {0x00:0,0x02:8,0x03:2,0x04:1,0x05:2,0x06:4,0x07:4,0x08:2,0x09:8,0x0A:8,0x0B:4,0x0C:4,0x0D:2,0x0E:4,0x0F:4,0x10:8,
    0x15:2,0x16:2,0x1A:6,0x1B:6,0x1C:0,0x1D:6,0x1E:0,0x1F:6,0x20:8,0x21:4,0x22:6,0x23:2,0xFF:0}
for _o in range(0x30,0x38): _PICT_FIXED[_o]=8
for _o in range(0x38,0x40): _PICT_FIXED[_o]=0
for _o in range(0x40,0x48): _PICT_FIXED[_o]=8
for _o in range(0x48,0x50): _PICT_FIXED[_o]=0
for _o in range(0x50,0x58): _PICT_FIXED[_o]=8
for _o in range(0x58,0x60): _PICT_FIXED[_o]=0
for _o in range(0x60,0x68): _PICT_FIXED[_o]=12
for _o in range(0x68,0x70): _PICT_FIXED[_o]=4
for _o in range(0x78,0x80): _PICT_FIXED[_o]=0

def decode_pict(d, skip512=False):
    """First bitmap in a QuickDraw PICT -> (w, h, rgba bytes)."""
    if skip512: d = d[512:]
    v2 = d[10:14] == b'\x00\x11\x02\xff'; p = 14 if v2 else 10
    while p < len(d):
        if v2:
            p += p & 1
            op, = struct.unpack('>H', d[p:p+2]); p += 2
        else: op = d[p]; p += 1
        if op == 0x11: p += 0 if v2 else 1; continue
        if op == 0x2FF: continue
        if op == 0xC00: p += 24; continue
        if op in (0x01, 0x70,0x71,0x72,0x73,0x74,0x75,0x76,0x77, 0x24,0x25,0x26,0x27,0x2C,0x2D,0x2E,0x2F):
            n, = struct.unpack('>H', d[p:p+2]); p += n if op in (0x01,) or 0x70<=op<=0x77 else 2+n; continue
        if op == 0x28: p += 5; p += d[p-1]; continue
        if op == 0xA0: p += 2; continue
        if op == 0xA1: p += 2; n, = struct.unpack('>H', d[p+0:p+2]); p += 2+n; continue
        if op in (0x90,0x91,0x98,0x99,0x9A,0x9B):
            direct = op in (0x9A,0x9B); region = op in (0x91,0x99,0x9B)
            if direct: p += 4
            rb, = struct.unpack('>H', d[p:p+2]); p += 2; ispix = bool(rb & 0x8000); rb &= 0x3FFF
            top, left, bot, right = struct.unpack('>hhhh', d[p:p+8]); p += 8
            psize, packtype, cmpcnt = 1, 0, 1
            if ispix:
                _, packtype, _, _, _, _, psize, cmpcnt, _, _, _, _ = struct.unpack('>HHIIIHHHHIII', d[p:p+36]); p += 36
            ctab = None
            if not direct:
                if ispix:
                    _, flags, csz = struct.unpack('>IHH', d[p:p+8]); p += 8; ctab = {}
                    for i in range(csz+1):
                        v, r, g, b = struct.unpack('>HHHH', d[p:p+8]); p += 8
                        ctab[i if flags & 0x8000 else v] = (r >> 8, g >> 8, b >> 8)
                else: ctab = {0: (255,255,255), 1: (0,0,0)}
            p += 16 + 2                                  # srcRect, dstRect, mode
            if region: n, = struct.unpack('>H', d[p:p+2]); p += n
            w = right-left; h = bot-top; out = bytearray()
            for y in range(h):
                if packtype == 1 or rb < 8: row = d[p:p+rb]; p += rb
                else:
                    if rb > 250: cnt, = struct.unpack('>H', d[p:p+2]); p += 2
                    else: cnt = d[p]; p += 1
                    row = _unpackbits(d, p, rb//2 if psize == 16 else rb, 2 if psize == 16 else 1); p += cnt
                if psize in (1,2,4,8):
                    per = 8//psize
                    for x in range(w):
                        idx = (row[x//per] >> (8 - psize*(x%per+1))) & ((1 << psize)-1)
                        out += bytes([*ctab.get(idx, (0,0,0)), 255])
                elif psize == 16:
                    for x in range(w):
                        v = (row[x*2] << 8) | row[x*2+1]; r = (v>>10)&31; g = (v>>5)&31; b = v&31
                        out += bytes([(r<<3)|(r>>2), (g<<3)|(g>>2), (b<<3)|(b>>2), 255])
                elif psize == 32:
                    o = 0 if cmpcnt == 3 else w
                    R, G, B = row[o:o+w], row[o+w:o+2*w], row[o+2*w:o+3*w]
                    for x in range(w): out += bytes([R[x], G[x], B[x], 255])
                else: raise ValueError('pixel size %d' % psize)
            return w, h, bytes(out)
        if op in _PICT_FIXED: p += _PICT_FIXED[op]; continue
        if op == 0xFF: break
        raise ValueError('unhandled PICT opcode %04x' % op)
    raise ValueError('PICT contains no bitmap')

# ================================================================ sound ==========================
def snd_to_wav(d):
    """Mac 'snd ' resource (format 1/2, standard 8-bit header) -> (wav bytes, info)."""
    fmt, = struct.unpack('>H', d[:2]); p = 2
    if fmt == 1: n, = struct.unpack('>H', d[p:p+2]); p += 2 + n*6
    elif fmt == 2: p += 2
    else: raise ValueError('snd format')
    ncmd, = struct.unpack('>H', d[p:p+2]); p += 2; off = None
    for _ in range(ncmd):
        cmd, _, p2 = struct.unpack('>HHi', d[p:p+8]); p += 8
        if cmd & 0x7fff in (80, 81): off = p2
    _, length, rate = struct.unpack('>III', d[off:off+12]); ls, le, enc, base = struct.unpack('>IIBB', d[off+12:off+22])
    if enc != 0: raise ValueError('only standard sound headers supported')
    pcm = d[off+22: off+22+length]; hz = int(round(rate / 65536.0))
    bio = io.BytesIO(); w = wave.open(bio, 'wb'); w.setnchannels(1); w.setsampwidth(1); w.setframerate(hz); w.writeframes(pcm); w.close()
    return bio.getvalue(), dict(sample_rate=hz, exact_rate=rate/65536.0, frames=len(pcm), loop_start=ls, loop_end=le, base_note=base)

# ================================================================ sprites ========================
def decode_btsp(d, pal):
    """Cydonia 'btSP' sprite: u16 width, u16 aux (meaning unknown), then 4-byte commands [cmd][u24 n]:
         01 n  n literal palette-index bytes follow (padded to 4)    02 n  n transparent pixels
         03    end of row                                            04    end of sprite
       Returns (width, height, aux, rgba)."""
    w, aux = struct.unpack('>HH', d[:4]); p = 4; rows = []; row = bytearray()
    while True:
        c = d[p]; n = int.from_bytes(d[p+1:p+4], 'big'); p += 4
        if c == 1:
            for b in d[p:p+n]: row += bytes([*pal[b], 255])
            p += (n + 3) & ~3
        elif c == 2: row += bytes(4*n)
        elif c == 3:
            if len(row) != 4*w: raise ValueError('row width mismatch')
            rows.append(bytes(row)); row = bytearray()
        elif c == 4: break
        else: raise ValueError('unknown btSP command %d' % c)
    return w, len(rows), aux, b''.join(rows)

def clut_to_palette(d):
    _, _, n = struct.unpack('>IHH', d[:8]); n += 1
    return [tuple(x >> 8 for x in struct.unpack('>HHH', d[10+i*8:16+i*8])) for i in range(n)]

# ================================================================ small Mac structs ==============
def parse_strn(d):
    n, = struct.unpack('>H', d[:2]); p = 2; out = []
    for _ in range(n): s, p = pstr(d, p); out.append(s)
    return out

def parse_menu(d):
    mid, width, height, proc, _, flags = struct.unpack('>hhhhhI', d[:14])
    title, p = pstr(d, 14); items = []
    while p < len(d) and d[p]:
        t, p = pstr(d, p); icon, key, mark, style = d[p:p+4]; p += 4
        items.append(dict(text=t, icon=icon, key=chr(key) if key else '', mark=mark, style=style))
    return dict(id=mid, title=title, proc_id=proc, enable_flags=flags, items=items)

def parse_ditl(d):
    n, = struct.unpack('>H', d[:2]); p = 2; out = []
    for _ in range(n+1):
        t, l, b, r = struct.unpack('>hhhh', d[p+4:p+12]); typ = d[p+12]; ln = d[p+13]; data = d[p+14:p+14+ln]; p += 14 + ln + (ln & 1)
        kind = typ & 0x7f
        item = dict(rect=[l, t, r, b], type=kind, disabled=bool(typ & 0x80))
        if kind in (4, 5, 6, 7, 8, 16): item['text'] = data.decode('mac_roman')
        elif kind in (32, 64): item['resource_id'] = struct.unpack('>h', data)[0] if len(data) == 2 else None
        out.append(item)
    return out

def parse_dlog(d):
    t, l, b, r, proc = struct.unpack('>hhhhh', d[:10]); vis, _, go, _ = d[10:14]; ref, items = struct.unpack('>Ih', d[14:20]); title, _ = pstr(d, 20)
    return dict(rect=[l, t, r, b], proc_id=proc, visible=bool(vis), go_away=bool(go), items_id=items, title=title)

def parse_alrt(d):
    t, l, b, r, items, stages = struct.unpack('>hhhhhH', d[:12]); return dict(rect=[l, t, r, b], items_id=items, stages=stages)

def bitmap_rows(d, off, rb, h): return [d[off+i*rb: off+(i+1)*rb] for i in range(h)]

def parse_curs(d):
    """CURS: 16x16 1-bit data[32], mask[32], hotspot v,h  ->  RGBA (black/white, transparent outside mask)."""
    out = bytearray()
    for y in range(16):
        for x in range(16):
            bit = (d[y*2 + x//8] >> (7 - x%8)) & 1; m = (d[32 + y*2 + x//8] >> (7 - x%8)) & 1
            out += bytes([0,0,0,255]) if (m and bit) else (bytes([255,255,255,255]) if m else bytes(4))
    v, h = struct.unpack('>hh', d[64:68]); return 16, 16, bytes(out), (h, v)

def parse_cicn(d):
    """Color icon -> (w, h, rgba)."""
    rb, = struct.unpack('>H', d[4:6]); rb &= 0x3FFF
    t, l, b, r = struct.unpack('>hhhh', d[6:14]); w, h = r-l, b-t; psize, = struct.unpack('>H', d[32:34])
    mrb, = struct.unpack('>H', d[54:56]); brb, = struct.unpack('>H', d[68:70]); p = 82
    mask = bitmap_rows(d, p, mrb, h); p += mrb*h; p += brb*h
    _, flags, n = struct.unpack('>IHH', d[p:p+8]); n += 1; p += 8; ct = {}
    for i in range(n):
        v, R, G, B = struct.unpack('>HHHH', d[p:p+8]); p += 8; ct[i if flags & 0x8000 else v] = (R>>8, G>>8, B>>8)
    out = bytearray(); per = 8//psize
    for y in range(h):
        row = d[p+y*rb: p+(y+1)*rb]
        for x in range(w):
            idx = (row[x//per] >> (8 - psize*(x%per+1))) & ((1 << psize)-1); m = (mask[y][x//8] >> (7 - x%8)) & 1
            out += bytes([*ct.get(idx, (0,0,0)), 255 if m else 0])
    return w, h, bytes(out)
# ================================================================ sources ========================
def load_from_installer(path):
    """Smaller Installer data fork -> {stripped name: dict(data, rsrc, type, creator)}"""
    d = open(path, 'rb').read(); out = {}
    for e in parse_table(d):
        o = e['off']; rs = d[o:o+e['rclen']]; ds = d[o+e['rclen']:o+e['rclen']+e['dclen']]
        rd = decode_fork(rs, e['rlen'], bool(e['meth'] & 2)) if e['rlen'] else b''
        dd = decode_fork(ds, e['dlen'], bool(e['meth'] & 4)) if e['dlen'] else b''
        out[e['name'].strip()] = dict(data=dd, rsrc=rd, type=e['type'], creator=e['creator'])
    return out

def _read_appledouble(path):
    b = open(path, 'rb').read()
    if b[:4] != b'\x00\x05\x16\x07': return b''
    n, = struct.unpack('>H', b[24:26])
    for i in range(n):
        eid, off, ln = struct.unpack('>III', b[26+i*12:38+i*12])
        if eid == 2: return b[off:off+ln]
    return b''

def load_from_folder(path):
    """Installed game folder. Resource forks are found as <name>.rsrc, AppleDouble ._<name>, or <name>/..namedfork/rsrc."""
    out = {}
    for root, _, files in os.walk(path):
        for f in files:
            if f.endswith('.rsrc') or f.startswith('._') or f == '.DS_Store': continue
            p = os.path.join(root, f); rs = b''
            for cand in (p + '.rsrc', os.path.join(root, '._' + f), os.path.join(p, '..namedfork', 'rsrc')):
                if os.path.isfile(cand):
                    rs = _read_appledouble(cand) if os.path.basename(cand).startswith('._') else open(cand, 'rb').read()
                    if rs: break
            out[f.strip()] = dict(data=open(p, 'rb').read(), rsrc=rs, type='????', creator='????')
    for root, _, files in os.walk(path):                  # resource-only files: "<name>.rsrc" with no data-fork file
        for f in files:
            if f.endswith('.rsrc') and f[:-5].strip() not in out:
                out[f[:-5].strip()] = dict(data=b'', rsrc=open(os.path.join(root, f), 'rb').read(), type='????', creator='????')
    return out

# ================================================================ exporters ======================
class Ctx:
    def __init__(s, out): s.out = out; s.files = []; s.notes = []; s.sprite_dims = {}; s.anim_table = []
    def path(s, *p): return os.path.join(s.out, *p)
    def json(s, rel, obj): write_json(s.path(rel), obj); s.files.append(rel)
    def bytes(s, rel, b): write_bytes(s.path(rel), b); s.files.append(rel)
    def png(s, rel, w, h, rgba=None, **kw): write_bytes(s.path(rel), png_bytes(w, h, rgba=rgba, **kw)); s.files.append(rel)

def export_palettes(cx, app):
    pals = {}
    for rid, (nm, d) in app.get('clut', {}).items():
        pal = clut_to_palette(d); pals[rid] = (nm, pal)
        cx.json('palettes/%d_%s.json' % (rid, slug(nm)), dict(id=rid, name=nm, colors=[list(c) for c in pal]))
    sysid = next((r for r, (n, _) in pals.items() if n == 'System colours'), None)
    if sysid is None: raise SystemExit('system palette (clut "System colours") not found')
    return pals[sysid][1]

def export_tiles(cx, spr, pal):
    t = spr['TILE'][1][1]; f = spr['TILF'][1][1]
    n = (len(t) - 12) // 1024; cols = 16; rows = (n + cols - 1) // cols
    def sheet(body):
        W, H = cols*32, rows*32; img = bytearray(W*H)
        for i in range(n):
            tx, ty = (i % cols)*32, (i // cols)*32
            for y in range(32): img[(ty+y)*W+tx: (ty+y)*W+tx+32] = body[i*1024+y*32: i*1024+y*32+32]
        return W, H, img
    W, H, img = sheet(t[12:]); cx.png('tiles/tiles.png', W, H, indexed=img, palette=pal)
    W, H, img = sheet(f[12:]); cx.png('tiles/tiles_mirrored.png', W, H, indexed=img, palette=pal)
    cx.json('tiles/tiles.json', dict(tile_size=32, count=n, columns=cols, sheet='tiles/tiles.png', mirrored_sheet='tiles/tiles_mirrored.png',
            note='8-bit palette-indexed (palettes/*System colours*). tiles_mirrored is tiles with each tile flipped horizontally.',
            resource_header_hex=t[:12].hex()))

def export_sprites(cx, spr, pal):
    index = {}; ids = sorted(spr['btSP'])
    for rid in ids:
        w, h, aux, rgba = decode_btsp(spr['btSP'][rid][1], pal)
        cx.png('sprites/%05d.png' % rid, w, h, rgba=rgba); index[str(rid)] = dict(width=w, height=h, aux=aux)
        cx.sprite_dims[rid] = (w, h)
    cx.json('sprites/index.json', dict(count=len(ids), note='aux = unidentified 16-bit header field of the original btSP resource', sprites=index))
    sets = []
    for rid, (nm, d) in spr.get('SpIc', {}).items():
        n, = struct.unpack('>H', d[:2]); ent = [dict(first_id=struct.unpack('>H', d[2+i*4:4+i*4])[0], count=struct.unpack('>H', d[4+i*4:6+i*4])[0])
                                                for i in range(len(d[2:])//4)]
        sets.append(dict(id=rid, entries=ent))
        if rid == 1000: cx.anim_table = ent
    sl = []
    for rid, (nm, d) in spr.get('SpIL', {}).items(): n, = struct.unpack('>H', d[:2]); sl = list(struct.unpack('>%dH' % (n+1), d[2:2+2*(n+1)]))
    cx.json('sprites/sets.json', dict(note='SpIc: ranges of consecutive sprite ids (first_id,count); 1002=ids 10000.., 1000=animation strips', sets=sets, set_list=sl))

# Ground-object kind -> animation index (1-based index into the SpIc 1000 table in sprites/sets.json).
# Read from the original's object-setup jump tables (PPC 0x14ca8-0x14ff8). Kind 4 and kind 8 were verified against the
# original screenshot (28160x24-frame turret strip, 28400 pad); the rest are decoded from the code but not yet seen on screen.
_KIND_ANIM = {0: 23, 2: 56, 3: 58, 4: 59, 5: 26, 6: 89, 7: 60, 8: 64, 15: 57, 16: 158, 17: 155, 18: 131, 19: 139, 20: 135, 21: 66, 22: 68, 23: 62}
_KIND_ANIM.update({k: 23 for k in range(9, 15)})
_VARIANT_ANIM = {1: {1: 51, 2: 54, 3: 52, 4: 53, 5: 51, 6: 54, 7: 52, 8: 53}, 24: {1: 70, 2: 73, 3: 71, 4: 72, 5: 70, 6: 73, 7: 71, 8: 72}}
_LINKED_ANIM = {1: 65, 24: 75}

def ground_anim_index(kind, variant, prev_link):
    if kind in _VARIANT_ANIM:
        return _LINKED_ANIM[kind] if prev_link != -1 else _VARIANT_ANIM[kind].get(variant, 23)
    return _KIND_ANIM.get(kind, 23)

def _object_draw_info(cx, anim_index, kind, variant):
    if not cx.anim_table or not (1 <= anim_index <= len(cx.anim_table)): return dict(sprite_id=0, initial_frame=0, draw_offset=[0, 0])
    e = cx.anim_table[anim_index - 1]; fr = min(initial_frame(kind, variant), max(e['count'] - 1, 0)); sid = e['first_id'] + fr
    w, h = cx.sprite_dims.get(sid, (32, 32))
    return dict(sprite_id=sid, initial_frame=fr, draw_offset=draw_offset(w, h))

def initial_frame(kind, variant):
    """0-based frame shown for a ground object (state field is 1-based; default 1). Kinds 7/15 start at 3; kinds 16-21 use
    their variant. Verified on screen for kinds 16, 18, 21 (frame == variant-1). Kind 4 turrets aim at runtime."""
    if kind in (7, 15): return 2
    if 16 <= kind <= 21: return max(variant, 1) - 1
    return 0

def draw_offset(w, h):
    """Objects are centred in a 64x64 box (if larger than 32) or a 32x32 cell, anchored at the cell origin (from PPC 0x3B248;
    division truncates toward zero). Verified: 50x50 turret -> (7,7), 32x32 pad -> (0,0)."""
    tz = lambda v: int(v / 2)
    return [tz(64 - w) if w > 32 else tz(32 - w), tz(64 - h) if h > 32 else tz(32 - h)]

def export_levels(cx, mp):
    names = parse_strn(mp['STR#'][4000][1]) if 4000 in mp.get('STR#', {}) else []
    briefs = {}
    for rid, (nm, d) in mp.get('STR#', {}).items():
        if rid != 4000: briefs[rid] = dict(name=nm, strings=parse_strn(d))
    lv = []
    for rid in sorted(mp['DIME']):
        h, w = struct.unpack('>II', mp['DIME'][rid][1]); n = w*h
        base = struct.unpack('>%dH' % n, mp['BASE'][rid][1])
        asgm = struct.unpack('>%dI' % h, mp['ASGM'][rid][1])
        g = mp['_ASG'][rid][1]; groups = []
        for i in range(len(g)//124):
            rec = g[i*124:(i+1)*124]
            if any(rec): groups.append(dict(index=i, u32=list(struct.unpack('>31I', rec))))
        gs = mp['__GS'][rid][1]; sprites = []
        for i in range(len(gs)//50):
            rec = gs[i*50:(i+1)*50]
            if any(rec):
                x, y = struct.unpack('>ff', rec[14:22])
                a = list(struct.unpack('>3i', rec[2:14])); bl = list(struct.unpack('>2i', rec[22:30]))
                sprites.append(dict(index=i, flags=list(rec[:2]), casts_shadow=bool(rec[1]), kind=a[0], param=a[1], variant=a[2], x=x, y=y,
                                    prev_link=bl[0], next_link=bl[1], anim_index=ground_anim_index(a[0], a[2], bl[0]),
                                    **_object_draw_info(cx, ground_anim_index(a[0], a[2], bl[0]), a[0], a[2]),
                                    i32_a=a, i32_b=bl, i32_c=list(struct.unpack('>5i', rec[30:50]))))
        # Game rules (from the original's map-draw code, verified against its own screenshot):
        #   value 0xFFFF = empty; tile id = max(value mod 10000, 1) (ids are 1-based) -> sheet index id-1;
        #   value div 10000 = flip flags: 1 = flip vertically, 2 = use the mirrored (horizontally flipped) sheet, 3 = both.
        doc = dict(id=rid, name=names[rid-1] if rid-1 < len(names) else 'Sector %d' % rid, width=w, height=h,
                   base_tile=[min(max(v % 10000, 1), 255) - 1 for v in base], base_flip=[v // 10000 for v in base],
                   fringe=list(mp['FRIN'][rid][1]), shadow=list(mp['SHAD'][rid][1]), ground_marker=list(mp['GS  '][rid][1]),
                   spawn_trigger_by_row=list(asgm), groups=groups, ground_sprites=sprites)
        cx.json('levels/level_%02d.json' % rid, doc); lv.append(dict(id=rid, name=doc['name'], width=w, height=h, file='levels/level_%02d.json' % rid))
    cx.json('levels/index.json', dict(levels=lv, briefings=briefs, rules=
        'ROW ORDER: row 0 is the BOTTOM of the level; the game draws rows bottom-up (screen y of row r = (height-1-r)*32 - scroll). '
        'base_tile = 0-based index into tiles/tiles.png (original ids are 1-based; 0 was treated as 1). '
        'base_flip: 1 = flip vertically, 2 = take the tile from tiles_mirrored.png, 3 = both. '
        'fringe[i] (255 = none) -> sprite 10000 + max(v,1) - 1, drawn over the base layer; '
        'shadow[i] (255 = none) -> sprite 15000 + max(v,1) - 1, drawn after fringe. '
        'ground_sprites[].casts_shadow (record byte 1): the object also draws a shadow = its silhouette at (-6,+8) with the pixels underneath remapped through tables/dark_128_system_shadow.bin. '
        'ground_marker value = index of a ground_sprites record (x = pixel column, y = map row; anim_index = 1-based entry of the animation table in sprites/sets.json set 1000). '
        'spawn_trigger_by_row[row] = index into groups (255 = none).'))

def export_sounds(cx, snd):
    idx = []
    for rid, (nm, d) in snd['snd '].items():
        wav, info = snd_to_wav(d); rel = 'sounds/%05d_%s.wav' % (rid, slug(nm)); cx.bytes(rel, wav); idx.append(dict(id=rid, name=nm, file=rel, **info))
    cx.json('sounds/index.json', dict(sounds=idx))

def export_pictures(cx, titles, app, shots):
    rects = {}
    def one(src, tag, R, typ, prefix=''):
        for rid, (nm, d) in R.get(typ, {}).items():
            try: w, h, px = decode_pict(d)
            except Exception as ex: cx.notes.append('PICT %s %d failed: %s' % (tag, rid, ex)); continue
            cx.png('pictures/%s/%s%d_%s.png' % (tag, prefix, rid, slug(nm)), w, h, rgba=px)
    one('t', 'titles', titles, 'PICT'); one('a', 'app', app, 'PICT'); one('a', 'app', app, 'IMAG', 'imag_')
    for name, data in shots.items():
        try: w, h, px = decode_pict(data, skip512=True); cx.png('pictures/screenshots/%s.png' % slug(os.path.splitext(name)[0]), w, h, rgba=px)
        except Exception as ex: cx.notes.append('screenshot %s failed: %s' % (name, ex))
    for tag, R in (('titles', titles), ('app', app)):
        rects[tag] = {}
        for rid, (nm, d) in R.get('Rect', {}).items():
            l, t, r, b = struct.unpack('>4h', d); rects[tag][str(rid)] = dict(name=nm, left=l, top=t, right=r, bottom=b)
    cx.json('layout/rects.json', dict(note='screen rectangles of UI elements (640x480 coordinates); stored on disk as left,top,right,bottom', rects=rects))

def export_tables(cx, app):
    tabs = []
    for t in ('TRAN', 'DARK'):
        for rid, (nm, d) in app.get(t, {}).items():
            if t == 'DARK': vals = [x[0] for x in struct.iter_unpack('>I', d)]; assert all(v < 256 or v >= 0xFFFFFF00 for v in vals), 'DARK entry not a sign-extended byte'; data = bytes(v & 255 for v in vals); kind = 'remap: 256 x u8 palette index (stored in the original as sign-extended 32-bit values)'
            else: data = d; kind = 'blend: 256x256 x u8, index = a*256+b (orientation unverified)'
            rel = 'tables/%s_%d_%s.bin' % (t.lower(), rid, slug(nm)); cx.bytes(rel, data); tabs.append(dict(type=t, id=rid, name=nm, file=rel, format=kind))
    cx.json('tables/index.json', dict(tables=tabs))

def export_ui(cx, app, maps, spr, titles, snd, pal):
    strings = {}
    for tag, R in (('app', app), ('maps', maps)):
        for rid, (nm, d) in R.get('STR#', {}).items(): strings.setdefault(tag, {})['STR#%d' % rid] = dict(name=nm, strings=parse_strn(d))
        for rid, (nm, d) in R.get('STR ', {}).items(): strings.setdefault(tag, {})['STR %d' % rid] = dict(name=nm, string=pstr(d, 0)[0])
    cx.json('ui/strings.json', strings)
    cx.json('ui/menus.json', dict(menus=[dict(res_id=rid, **parse_menu(d)) for rid, (nm, d) in app.get('MENU', {}).items()],
            menu_bars={str(rid): list(struct.unpack('>%dh' % (struct.unpack('>H', d[:2])[0]), d[2:])) for rid, (nm, d) in app.get('MBAR', {}).items()}))
    dlg = dict(dialogs={}, items={}, alerts={}, controls={})
    for rid, (nm, d) in app.get('DLOG', {}).items(): dlg['dialogs'][str(rid)] = dict(name=nm, **parse_dlog(d))
    for rid, (nm, d) in app.get('DITL', {}).items(): dlg['items'][str(rid)] = dict(name=nm, items=parse_ditl(d))
    for rid, (nm, d) in app.get('ALRT', {}).items(): dlg['alerts'][str(rid)] = dict(name=nm, **parse_alrt(d))
    for rid, (nm, d) in app.get('CNTL', {}).items():
        t, l, b, r, val, vis, _, mx, mn, proc, ref = struct.unpack('>hhhhhBBhhhI', d[:22]); dlg['controls'][str(rid)] = dict(name=nm, rect=[l, t, r, b], value=val, min=mn, max=mx, proc_id=proc, title=pstr(d, 22)[0])
    cx.json('ui/dialogs.json', dlg)
    # the dialogs' texts (Level Select 160, Preferences 190..192) in a plain format the port reads without a JSON parser (layout: DITL 190 / 191 / 192, menus 1001..1004 / 20000)
    pl = ['# ditl <id> <item> <type> <left> <top> <right> <bottom> <text hex or ->   /   menu <id> <item> <text hex>   /   help <STR# id> <index> <text hex>  (MacRoman)']
    for rid in (160, 190, 191, 192):
        for i, it in enumerate(dlg['items'].get(str(rid), dict(items=[]))['items'], 1):
            l, t, r, b = it['rect']
            pl.append('ditl %d %d %d %d %d %d %d %s' % (rid, i, it['type'], l, t, r, b, it.get('text', '').encode('mac_roman', 'replace').hex() or '-'))
    for m in app.get('MENU', {}).items():
        rid = m[0]
        if rid in (1001, 1002, 1003, 1004, 20000):
            for i, it in enumerate(parse_menu(m[1][1])['items'], 1): pl.append('menu %d %d %s' % (rid, i, it['text'].encode('mac_roman', 'replace').hex() or '-'))
    # the Preferences dialog's hover help (STR# 132 general page, 133 audio page, 135 buttons and the rest; read by the filter proc 0x7d24: STR# 131 + page, index item - 17, or 135 and the item)
    for rid in (132, 133, 135):
        for i, st in enumerate(strings.get('app', {}).get('STR#%d' % rid, dict(strings=[]))['strings'], 1):
            pl.append('help %d %d %s' % (rid, i, st.encode('mac_roman', 'replace').hex() or '-'))
    cx.bytes('ui/dialog_text.txt', ('\n'.join(pl) + '\n').encode('ascii'))
    for rid, (nm, d) in app.get('TEXT', {}).items(): cx.bytes('ui/text_%d_%s.txt' % (rid, slug(nm)), d.decode('mac_roman').replace('\r', '\n').encode('utf-8'))
    cur = {}
    for rid, (nm, d) in app.get('CURS', {}).items():
        w, h, px, hot = parse_curs(d); cx.png('cursors/curs_%d.png' % rid, w, h, rgba=px); cur[str(rid)] = dict(file='cursors/curs_%d.png' % rid, hotspot=list(hot))
    cx.json('cursors/index.json', dict(cursors=cur, note='1-bit CURS resources; the color cursor crsr 128 is not exported (1-bit version of it is included in CURS-style form only if present)'))
    for rid, (nm, d) in app.get('cicn', {}).items():
        try: w, h, px = parse_cicn(d); cx.png('icons/cicn_%d_%s.png' % (rid, slug(nm)), w, h, rgba=px)
        except Exception as ex: cx.notes.append('cicn %d failed: %s' % (rid, ex))
    for rid, (nm, d) in app.get('icl8', {}).items():
        mask = app.get('ICN#', {}).get(rid, (None, None))[1]; out = bytearray()
        for i in range(1024):
            m = 255 if mask is None else (255 if (mask[128 + (i//32)*4 + (i%32)//8] >> (7 - (i%32) % 8)) & 1 else 0)
            out += bytes([*pal[d[i]], m])
        cx.png('icons/icl8_%d.png' % rid, 32, 32, rgba=bytes(out))

def export_misc(cx, app, titles):
    for rid, (nm, d) in titles.get('FILM', {}).items(): cx.bytes('misc/film_%d.bin' % rid, d)
    cx.json('misc/film_note.json', dict(note='FILM resources (3 x 12068 bytes) are exported verbatim (the attract-mode demos: seed, level, players, one input byte per frame; see RE_NOTES.md)'))
    if 1 in app.get('SPIN', {}): d = app['SPIN'][1][1]; cx.json('misc/spin.json', dict(points=[list(p) for p in struct.iter_unpack('>hh', d)], note='60 signed (a,b) int16 pairs; purpose unverified'))
    out = {}
    for rid, (nm, d) in app.get('tset', {}).items():
        v = struct.unpack('>%dI' % (len(d)//4), d); out[str(rid)] = dict(name=nm, u32=list(v))
    cx.json('misc/tset.json', dict(note='tset resources: u32 arrays (version, count, values); meaning unverified', tsets=out))
    for rid, (nm, d) in app.get('setl', {}).items(): cx.bytes('misc/setl_%d.bin' % rid, d)
    for rid, (nm, d) in app.get('SCOR', {}).items():
        if rid == 128 and len(d) >= 332: cx.bytes('misc/default_scores.bin', bytes(d[:332]))
        names = [d[i*21+1:i*21+1+d[i*21]].decode('mac_roman') for i in range(12)]
        cx.json('misc/default_scores.json', dict(name=nm, names=names, tail_u32=list(struct.unpack('>20I', d[252:332])), note='12 x 21-byte Pascal names then 20 u32 (scores then ranks/levels); layout partly verified'))
    if 0 in app.get('Cydo', {}): cx.json('misc/engine_credit.json', dict(owner_resource=pstr(app['Cydo'][0][1], 0)[0]))
    vers = {}
    for tag, R in (('app', app),):
        for rid, (nm, d) in R.get('vers', {}).items(): vers[str(rid)] = dict(short=pstr(d, 6)[0], long=pstr(d, 7 + d[6])[0])
    cx.json('misc/versions.json', vers)

# ================================================================ driver =========================
def main():
    args = [a for a in sys.argv[1:] if not a.startswith('--')]; keep_exe = '--with-executable' in sys.argv
    if len(args) != 2: sys.exit(__doc__)
    src_path, out = args
    srcs = load_from_installer(src_path) if os.path.isfile(src_path) else load_from_folder(src_path)
    need = ['MR Maps', 'MR Sounds', 'MR Sprites', 'MR Titles', 'Mars Rising']
    miss = [n for n in need if n not in srcs or not srcs[n]['rsrc']]
    if miss: sys.exit('missing (or no resource fork for): ' + ', '.join(miss))
    rs = {n: parse_rsrc(srcs[n]['rsrc']) for n in need}
    cx = Ctx(out); os.makedirs(out, exist_ok=True)
    pal = export_palettes(cx, rs['Mars Rising']); print('palettes ok')
    export_tiles(cx, rs['MR Sprites'], pal); print('tiles ok')
    export_sprites(cx, rs['MR Sprites'], pal); print('sprites ok')
    export_levels(cx, rs['MR Maps']); print('levels ok')
    export_sounds(cx, rs['MR Sounds']); print('sounds ok')
    shots = {n: v['data'] for n, v in srcs.items() if n.lower().endswith('.pict')}
    export_pictures(cx, rs['MR Titles'], rs['Mars Rising'], shots); print('pictures ok')
    export_tables(cx, rs['Mars Rising']); export_ui(cx, rs['Mars Rising'], rs['MR Maps'], rs['MR Sprites'], rs['MR Titles'], rs['MR Sounds'], pal); export_misc(cx, rs['Mars Rising'], rs['MR Titles']); print('ui/misc ok')
    for n, v in srcs.items():
        if v['data'][:4] == b'MADH': cx.bytes('music/%s.madh' % slug(n), v['data'])
        elif v['data'] and (v['type'] == 'TEXT' or n.lower().endswith(('.text', '.txt')) or 'read me' in n.lower()): cx.bytes('docs/%s.txt' % slug(os.path.splitext(n)[0]), v['data'].decode('mac_roman').replace('\r', '\n').encode('utf-8'))
    if srcs['Mars Rising']['data']:       # the Credits slideshow's font table and pages: data of the original's code, read by running it (tools/re, pure Python)
        try:
            import tempfile
            sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), 're'))
            import gen_credits_vectors
            with tempfile.TemporaryDirectory() as td:
                pef = os.path.join(td, 'MarsRising.pef'); open(pef, 'wb').write(srcs['Mars Rising']['data'])
                cx.bytes('layout/title_text.txt', gen_credits_vectors.data_text(gen_credits_vectors.Rig(pef)).encode('utf-8'))
            print('credits text ok')
        except Exception as e:
            cx.notes.append('layout/title_text.txt (Credits) could not be made: %r' % (e,)); print('WARNING: credits text not extracted:', repr(e))
    if keep_exe and srcs['Mars Rising']['data']:      # for tools/re/enemy_types.py (reverse-engineering only; not needed to play)
        cx.bytes('executable/Mars Rising.pef', srcs['Mars Rising']['data'])
    cx.json('manifest.json', dict(extractor='mr_extract.py 1.0', source=os.path.basename(src_path), file_count=len(cx.files), warnings=cx.notes, files=sorted(cx.files)))
    print('wrote %d files to %s%s' % (len(cx.files), out, ('  (%d warnings, see manifest.json)' % len(cx.notes)) if cx.notes else ''))

if __name__ == '__main__': main()
