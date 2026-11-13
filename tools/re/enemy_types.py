#!/usr/bin/env python3
"""
enemy_types.py -- derive the enemy-type table from the Mars Rising 1.0.1 PowerPC executable.

    python3 enemy_types.py "<assets>/executable/Mars Rising.pef" enemy_types.json
(produce the .pef with:  python3 ../mr_extract.py <installer> <assets> --with-executable)

Method (see RE_NOTES.md): the level-init routine at code 0x2D528 has a 24-entry jump table (TOC-5148) that stores a per-type
behaviour function pointer in each spawn group. We resolve each type's TVector, then linearly scan that behaviour routine for
the stores that initialise an enemy slot (176-byte struct) with constants. Only constants are recorded; values computed at
runtime (random spawn position, launch delay) are left out. Valid for the 1.0.1 executable only (checked by section sizes).
"""
import sys, json, struct, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from pef import load_pef, unpack_pattern

TYPE_TOC_SLOTS = {1:-28612,2:-28616,3:-28620,4:-28624,5:-28628,6:-28632,7:-28636,8:-28640,9:-28644,10:-28648,11:-28652,
                  12:-28656,13:-28660,14:-28664,15:-28668,16:-28672,17:-28676,18:-28680,19:-28684,20:-28688,21:-28692,23:-28696}
FIELDS = {8:'type_code', 80:'anim_index', 84:'field_84', 132:'score', 48:'sprite_w', 52:'sprite_h', 72:'hit_w', 76:'hit_h', 116:'field_116'}

def sx(v, b): return v - (1 << b) if v & (1 << (b - 1)) else v

def analyse(code, start, end):
    W = lambda a: struct.unpack('>I', code[a:a+4])[0]
    const, offreg, base, fields = {}, {}, None, {}
    for a in range(start, end, 4):
        x = W(a); op = x >> 26; rd = (x >> 21) & 31; ra = (x >> 16) & 31; rb = (x >> 11) & 31; si = sx(x & 0xffff, 16)
        if op == 7 and (x & 0xffff) == 176: base = rd; continue          # mulli rB,rIdx,176 -> enemy struct base
        if op == 14:
            if ra == 0: const[rd] = si; offreg.pop(rd, None)
            elif base is not None and ra == base: offreg[rd] = si; const.pop(rd, None)
            elif ra in const: const[rd] = const[ra] + si; offreg.pop(rd, None)
            else: const.pop(rd, None); offreg.pop(rd, None)
        elif op == 15 and ra == 0: const[rd] = (x & 0xffff) << 16; offreg.pop(rd, None)
        elif op == 31:
            xo = (x >> 1) & 0x3ff
            if xo in (151, 215, 407):
                size = {151: 'w', 215: 'b', 407: 'h'}[xo]; K = offreg.get(rb) if ra not in offreg else offreg.get(ra) if rb not in offreg else None
                if K is not None and (K, size) not in fields: fields[(K, size)] = const.get(rd)
            elif xo == 444 and rd == rb:
                if rd in const: const[ra] = const[rd]
                else: const.pop(ra, None)
                offreg.pop(ra, None)
        elif op == 18 and (x & 1):
            for r in range(3, 13): const.pop(r, None); offreg.pop(r, None)
    return fields

def main():
    if len(sys.argv) != 3: sys.exit(__doc__)
    b, secs = load_pef(sys.argv[1])
    if len(secs) < 3 or secs[0]['total'] != 404860: sys.exit('this script is only valid for the Mars Rising 1.0.1 executable')
    code = secs[0]['raw']; data = unpack_pattern(secs[1]['raw'], secs[1]['total'])
    ld = secs[2]['raw']; main_off, = struct.unpack('>I', ld[4:8]); _, toc = struct.unpack('>II', data[main_off:main_off + 8])
    R = lambda o: struct.unpack('>I', data[o:o+4])[0]
    entry = {t: R(R(toc + s)) for t, s in TYPE_TOC_SLOTS.items()}           # type -> behaviour code offset
    order = sorted(entry.items(), key=lambda kv: kv[1]); out = {}
    for i, (t, a) in enumerate(order):
        end = order[i+1][1] if i + 1 < len(order) else a + 0x400
        f = analyse(code, a, end)
        rec = dict(type=t, behaviour_code=a)
        for (K, sz), v in sorted(f.items()):
            if K in FIELDS and sz == 'w' and isinstance(v, int): rec[FIELDS[K]] = v
        out[t] = rec
    json.dump(dict(note='constants extracted from per-type init code; sprite_w/h verified against the animation sprite size for 20/22 types; field_84 is probably hit points',
                   types=[out[t] for t in sorted(out)]), open(sys.argv[2], 'w'), indent=1)
    print('wrote', len(out), 'enemy types to', sys.argv[2])

if __name__ == '__main__': main()
