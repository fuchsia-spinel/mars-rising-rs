#!/usr/bin/env python3
"""Golden vectors for src/madh.rs + src/madplay.rs: the ORIGINAL PlayerPRO engine of MarsRising.pef (music_rig.py) renders PCM.

    python3 -I gen_music_vectors.py MarsRising.pef ASSETS_DIR OUT.txt [n_synth] [seed]

Lines (one stream = one driver run; every token covers up to 16 callback buffers of 732 stereo frames, FNV-1a 64 over the i16 LE bytes):
    real <track> ops=<op;...> -> <token> ...         the five game tracks, assets/music/<track>_music.madh
    synth <seed> ops=<op;...> -> <token> ...         a module generated from <seed> by synth_module() (same algorithm in the Rust test)
ops:  B<n> render n buffers | S stop | R resume | T restart (start)     tokens: h<hash> | s | r | t
Mocked: only the Memory Manager / Sound Manager glue (see music_rig.py). The synthetic modules exercise every effect, the volume column, volume
envelopes (sustain / loop), fade-out, sample loops, sample offsets, speed/tempo changes, position jumps and pattern breaks.
"""
import sys, os, struct, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import music_rig

M64 = 0xffffffffffffffff


class Lcg:
    def __init__(self, seed): self.s = seed & 0xffffffff

    def next(self):
        self.s = (self.s * 1664525 + 1013904223) & 0xffffffff
        return self.s >> 8

    def r(self, n): return self.next() % n


def synth_module(seed):
    """Deterministic MADH module (see the same function in tests/madplay_vectors.rs)."""
    g = Lcg(seed * 7919 + 17)
    r = g.r
    num_chn = [4, 8, 8, 16][r(4)]
    num_pat = 1 + r(3)
    rows = [8 + r(40) for _ in range(num_pat)]
    num_ptr = 1 + r(5)
    optr = [r(num_pat) for _ in range(num_ptr)] + [0] * (128 - num_ptr)
    speed = 2 + r(7)
    tempo = 90 + r(100)
    num_ins = 1 + r(4)
    hdr = bytearray(16044)
    hdr[0:4] = b'MADH'
    hdr[4:15] = b'synthetic  '
    hdr[292], hdr[293], hdr[294] = num_pat, num_chn, num_ptr
    hdr[295:295 + 128] = bytes(optr)
    struct.pack_into('>HH', hdr, 552, speed, tempo)
    samples = []
    for i in range(num_ins):
        o = 556 + 242 * i
        ns = 1 + r(3)
        struct.pack_into('>h', hdr, o + 34, ns)
        for k in range(96): hdr[o + 36 + k] = r(ns)
        vs = r(7)
        x = 0
        for k in range(vs):
            if k: x += 1 + r(30)
            struct.pack_into('>hh', hdr, o + 132 + 4 * k, x, r(70))
        hdr[o + 228] = vs
        hdr[o + 230] = r(max(vs, 1))
        hdr[o + 231] = r(max(vs, 1))
        hdr[o + 232] = r(max(vs, 1))
        hdr[o + 236] = r(8)
        fade = 0 if r(4) == 0 else r(3000)
        struct.pack_into('>H', hdr, o + 238, fade)
        for k in range(ns):
            size = 100 + r(3000)
            data = bytes(g.next() & 255 for _ in range(size))
            kind = r(4)
            if kind == 0: lb, ls = 0, 0
            elif kind == 1: lb, ls = r(size // 2), 1 + r(2)
            else:
                lb = r(size // 2)
                ls = 3 + r(size - lb - 3)
            vol = r(70)
            c2 = 3000 + r(14000)
            rel = r(13)
            sh = bytearray(56)
            struct.pack_into('>III', sh, 0, size, lb, ls)
            sh[12] = vol
            struct.pack_into('>H', sh, 14, c2)
            sh[17] = 8
            sh[19] = rel & 255
            samples.append(bytes(sh) + data)
    pats = bytearray()
    for p in range(num_pat):
        ph = bytearray(48)
        struct.pack_into('>I', ph, 0, rows[p])
        body = bytearray(rows[p] * num_chn * 6)
        for ch in range(num_chn):
            for row in range(rows[p]):
                ins = r(num_ins + 2) if r(3) == 0 else 0
                note = (254 if r(4) == 0 else r(96)) if r(3) == 0 else 255
                cmd = r(16) if r(3) == 0 else 0
                if cmd == 0: arg = r(256) if r(6) == 0 else 0
                else: arg = 0 if r(5) == 0 else r(256)
                if cmd == 11: arg = r(num_ptr + 1)
                elif cmd == 13: arg = (r(6) << 4) | r(10)
                elif cmd == 15: arg = 1 + r(8) if r(2) == 0 else 32 + r(130)
                if r(4) == 0: vol = 96 + r(64) if r(3) == 0 else 16 + r(65)
                else: vol = 255
                o = (row + rows[p] * ch) * 6
                body[o:o + 5] = bytes([ins, note, cmd, arg, vol])
        pats += ph + body
    return bytes(hdr) + bytes(pats) + b''.join(samples)


def fnv(b, h=0xcbf29ce484222325):
    for x in b:
        h = ((h ^ x) * 0x100000001b3) & M64
    return h


def run(rig, ops):
    toks = []
    group = []

    def flush():
        if group:
            data = b''.join(struct.pack('<%dh' % len(blk), *blk) for blk in group)
            toks.append('h%016x' % fnv(data))
            group.clear()
    for op in ops:
        k, n = op[0], int(op[1:] or 0)
        if k == 'B':
            for _ in range(n):
                group.append(rig.next_block())
                if len(group) == 16: flush()
            flush()
        elif k == 'S': rig.stop(); toks.append('s')
        elif k == 'R': rig.resume(); toks.append('r')
        elif k == 'T': rig.start(); toks.append('t')
    return toks


def fresh(exe, data):
    rig = music_rig.MusicRig(exe).create().load(data)
    rig.start()
    return rig


def main():
    exe, assets, out = sys.argv[1:4]
    n_synth = int(sys.argv[4]) if len(sys.argv) > 4 else 60
    seed = int(sys.argv[5]) if len(sys.argv) > 5 else 1
    rng = random.Random(seed)
    lines = []
    fout = open(out, 'w')
    def emit(l):
        lines.append(l); fout.write(l + '\n'); fout.flush()
    # the five tracks: through one loop of the song (and a bit more), then stop/resume/restart in the middle
    nb = int(os.environ.get('MV_BLOCKS', '1500'))
    tracks = [('interface', nb), ('level_1', nb), ('level_2', nb), ('level_3', nb), ('special', nb)]
    if os.environ.get('MV_REAL') == '0': tracks = []
    if os.environ.get('MV_REAL_ONLY'): tracks = [t for t in tracks if t[0] in os.environ['MV_REAL_ONLY'].split(',')]
    for name, blocks in tracks:
        data = open(os.path.join(assets, 'music', name + '_music.madh'), 'rb').read()
        rig = fresh(exe, data)
        ops = ['B%d' % blocks, 'S', 'B5', 'R', 'B100', 'T', 'B100', 'S', 'R', 'B60']
        toks = run(rig, ops)
        emit('real %s ops=%s -> %s' % (name, ';'.join(ops), ' '.join(toks)))
        print(name, len(toks), 'tokens', file=sys.stderr)
    if os.environ.get('MV_SYNTH') == '0': n_synth = 0
    for i in range(n_synth):
        sd = rng.randrange(1, 1 << 30)
        data = synth_module(sd)
        rig = fresh(exe, data)
        ops = []
        for _ in range(rng.randint(2, 5)):
            q = rng.random()
            ops.append('B%d' % rng.choice([30, 60, 120, 250, 400]) if q < 0.7 else rng.choice(['S', 'R', 'T']))
        toks = run(rig, ops)
        emit('synth %d ops=%s -> %s' % (sd, ';'.join(ops), ' '.join(toks)))
        print('synth', i, file=sys.stderr)
    fout.close()


if __name__ == '__main__':
    main()
