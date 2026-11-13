#!/usr/bin/env python3 -I
"""Look at an F6 capture (`capture_<tick>.bin`, see `save_capture` in src/bin/play.rs).

For every pair of consecutive pictures it reports the game tick step, the scroll step (expected: 1 tick, 1 px) and how many pixels differ
from "the previous picture moved down by the scroll step" (sprites move, so a few thousand is normal; a ground flicker shows as pixels that differ
in *rows of the ground with nothing drawn on them* or as a jump in the scroll). Usage: analyze_capture.py capture_123.bin [--png DIR]"""
import struct, sys

def load(path):
    d = open(path, 'rb').read()
    assert d[:6] == b'MRCAP1', 'not a capture file'
    n, w, h = struct.unpack_from('<III', d, 6)
    pal = [tuple(d[18 + i * 3: 21 + i * 3]) for i in range(256)]
    o = 18 + 768
    frames = []
    for _ in range(n):
        tick, top = struct.unpack_from('<Ii', d, o); o += 8
        frames.append((tick, top, d[o:o + w * h])); o += w * h
    return w, h, pal, frames

def main():
    w, h, pal, fr = load(sys.argv[1])
    print(f'{len(fr)} pictures {w}x{h}')
    prev = None
    for tick, top, px in fr:
        if prev is not None:
            dt, ds = tick - prev[0], prev[1] - top
            diff = 0
            rows = set()
            if ds == 1:
                for y in range(h - 1):
                    a = px[(y + 1) * w:(y + 2) * w]; b = prev[2][y * w:(y + 1) * w]
                    if a != b:
                        n = sum(1 for i in range(w) if a[i] != b[i]); diff += n; rows.add(y)
            print(f'tick {tick} (+{dt})  scroll_top {top} (-{ds})  pixels differing from a pure 1 px shift: {diff if ds == 1 else "n/a"}  rows touched: {len(rows)}')
        prev = (tick, top, px)
    if '--png' in sys.argv:
        from PIL import Image
        out = sys.argv[sys.argv.index('--png') + 1]
        import os; os.makedirs(out, exist_ok=True)
        for i, (tick, top, px) in enumerate(fr):
            im = Image.new('RGB', (w, h)); im.putdata([pal[v] for v in px]); im.save(f'{out}/{i:03d}_tick{tick}.png')

if __name__ == '__main__':
    main()
