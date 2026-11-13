#!/usr/bin/env python3
"""Golden vectors for src/prefs.rs (the Preferences dialog and the preference block) from the original code in ppcemu.py.

    python3 -I gen_prefs_vectors.py MarsRising.pef OUT_VECTORS.txt [seed] [n]

Natively run original code: 0x6810 (the whole Preferences dialog: both pages, every item, Save / Cancel / Factory Settings / Revert), the page
fillers 0x789c / 0x7a38 / 0x7984, the page switch 0x7660, the factory defaults 0xb064 (-> 0xb0d0, 0xb14c, 0xb1e4, the key set record 0xab10), the
block accessors 0xae6c / 0xa9ac / 0xaa20 / 0xaed4, the backup / restore copies 0x7868 / 0x7834.
Mocked: the Toolbox, ModalDialog (a script of item hits), the control access 0x83e4 / 0x8420 / 0x82a0 / 0x82f0 (logged), the resource
loads 0x752c / 0x761c (logged), the sound 0x45bac, the side effects 0x45f6c (volume), 0x45848 (music), 0x1d180 (key menu), 0xa4d4 (the file save),
0x29ac / 0x299c / 0xe35c (the three display switches: written and logged).

Lines (hex = bytes of the block at 0..303 and of the key record at 702..737):
    prefs <mod-available> <cd-available> <block> <keys> <hits> -> <block'> <keys'> <D.12be> <D.12bc> <D.13c8> | <ops>
    fact <flag> <block> <keys> -> <block'> <keys'>            (0xb064 on its own)
hits = item[:value] separated by commas (the value is what the control reads: 0x83e4); the list ends with Save (1) or Cancel (2).
"""
import sys, os, struct, random
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

BLK, BAK = 0x100000, 0x110000
LEN = 10240
KEYS = (702, 738)
D_BLK, D_BAK = 0x932c, 0x130c
D_TRANS, D_SHAD, D_DETAIL = 0x12be, 0x12bc, 0x13c8
D_MOD, D_CD = 0x1bd0, 0x1bd2


def s16(v):
    v &= 0xffff
    return v - 0x10000 if v & 0x8000 else v


class PRig:
    def __init__(self, exe):
        self.c = c = ppcemu.load(exe)
        c.keep_toc = True
        for a in c.img.stub_name:
            c.mocks.setdefault(a, lambda cpu: cpu.r.__setitem__(3, 0))
        n = {v: k for k, v in c.img.stub_name.items()}
        self.ops = []
        self.hits = []
        self.ctl = {}

        def blockmove(cpu):
            s, d, k = cpu.r[3], cpu.r[4], cpu.r[5]
            cpu.mem[d:d + k] = bytes(cpu.mem[s:s + k])

        c.mocks[n['BlockMove']] = blockmove
        c.mocks[n['GetNewDialog']] = lambda cpu: cpu.r.__setitem__(3, 0x1000)

        def modal(cpu):
            a = cpu.r[4]
            if self.hits:
                item, val = self.hits.pop(0)
            else:
                item, val = 1, None
            if val is not None: self.ctl[item] = val
            cpu.mem[a:a + 2] = struct.pack('>h', item)
        c.mocks[n['ModalDialog']] = modal

        def log(fmt):
            return lambda cpu: self.ops.append(fmt(cpu))

        c.mocks[0x752c] = log(lambda cpu: 'app:%d' % s16(cpu.r[4]))
        c.mocks[0x761c] = log(lambda cpu: 'clr')
        c.mocks[0x846c] = lambda cpu: None
        c.mocks[0x9c10] = lambda cpu: None

        def getctl(cpu):
            cpu.r[3] = self.ctl.get(s16(cpu.r[4]), 1) & M32

        def setctl(cpu):
            self.ctl[s16(cpu.r[4])] = s16(cpu.r[5])
            self.ops.append('set:%d:%d' % (s16(cpu.r[4]), s16(cpu.r[5])))
        c.mocks[0x83e4] = getctl
        c.mocks[0x8420] = setctl
        c.mocks[0x82a0] = log(lambda cpu: 'en:%d' % s16(cpu.r[4]))
        c.mocks[0x82f0] = log(lambda cpu: 'dis:%d' % s16(cpu.r[4]))
        c.mocks[0x8354] = lambda cpu: cpu.ww(cpu.r[5], 0)
        c.mocks[0x8340] = lambda cpu: cpu.r.__setitem__(3, 0x2000)
        c.mocks[n['DisableItem']] = log(lambda cpu: 'mdis:%d' % s16(cpu.r[4]))
        c.mocks[n['EnableItem']] = log(lambda cpu: 'men:%d' % s16(cpu.r[4]))

        def snd(cpu):
            i = cpu.r[3] & 0xff
            self.ops.append('snd:%d:%d:%d' % (i - 256 if i & 0x80 else i, cpu.r[4] & 0xff, cpu.r[5] & 0xff))
        c.mocks[0x45bac] = snd
        c.mocks[0x45f6c] = log(lambda cpu: 'vol')
        c.mocks[0x45848] = log(lambda cpu: 'mus')
        c.mocks[0x1d180] = log(lambda cpu: 'keys')
        c.mocks[0xa4d4] = log(lambda cpu: 'save')

        def setter(addr, name):
            def f(cpu):
                cpu.mem[addr] = cpu.r[3] & 0xff
                self.ops.append('%s:%d' % (name, cpu.r[3] & 0xff))
            return f
        c.mocks[0x29ac] = setter(D_TRANS, 'trans')
        c.mocks[0x299c] = setter(D_SHAD, 'shad')
        c.mocks[0xe35c] = setter(D_DETAIL, 'det')
        self.base = bytes(c.mem)

    def reset(self, block, keys, mod=1, cd=0):
        c = self.c
        c.mem[:] = self.base
        c.ww(D_BLK, BLK)
        c.ww(D_BAK, BAK)
        full = bytearray(LEN)
        full[0:len(block)] = block
        full[KEYS[0]:KEYS[1]] = keys
        c.mem[BLK:BLK + LEN] = full
        c.wb(D_MOD, mod)
        c.wb(D_CD, cd)
        c.wb(0x1307, 1)                  # the first-call flag of 0x6810 (set -> the byte below is not cleared twice; harmless)
        self.ops = []
        self.ctl = {}
        return bytes(full)

    def out(self, before):
        c = self.c
        full = bytes(c.mem[BLK:BLK + LEN])
        # nothing outside the preference region and the key record may change
        mask = bytearray(before)
        mask[0:304] = full[0:304]
        mask[KEYS[0]:KEYS[1]] = full[KEYS[0]:KEYS[1]]
        assert bytes(mask) == full, 'the dialog wrote outside the preference region'
        return full[0:304], full[KEYS[0]:KEYS[1]]

    def dialog(self, block, keys, hits, mod, cd):
        before = self.reset(block, keys, mod, cd)
        self.hits = list(hits)
        self.c.call(0x6810, [], max_steps=5_000_000)
        b, k = self.out(before)
        c = self.c
        return b, k, c.mem[D_TRANS], c.mem[D_SHAD], c.mem[D_DETAIL], list(self.ops)

    def factory(self, flag, block, keys):
        before = self.reset(block, keys)
        self.c.call(0xb064, [flag], max_steps=1_000_000)
        return self.out(before)


M32 = 0xffffffff


def rand_block(rng):
    b = bytearray(304)
    for i in range(2, 102):                                   # byte preferences 1..100
        b[i] = rng.choice([0, 1, 0, 1, rng.randint(0, 255)])
    for i in range(1, 101):                                   # short preferences 1..100
        v = rng.choice([0, 1, 2, 3, 4, 7, 8, 9, rng.randint(-32768, 32767)])
        struct.pack_into('>h', b, 102 + 2 * (i - 1), v)
    for i, lo, hi in ((51, 0, 8), (55, 1, 3), (56, 1, 7), (57, 1, 3)):
        struct.pack_into('>h', b, 102 + 2 * (i - 1), rng.choice([rng.randint(lo, hi)] * 3 + [rng.randint(-2, 12)]))
    return bytes(b)


def rand_keys(rng):
    return bytes(rng.randint(0, 255) for _ in range(KEYS[1] - KEYS[0]))


def rand_hits(rng):
    hits = []
    for _ in range(rng.choice([0, 1, 2, 3, 5, 8, 12])):
        item = rng.choice([3, 4, 11, 11] + list(range(18, 30)) * 2 + [5, 12, 40])
        if item == 11: val = rng.choice([1, 2])
        elif item in (19, 22, 24, 26): val = rng.choice([rng.randint(1, 9), rng.randint(1, 3), rng.randint(1, 7)])
        else: val = rng.randint(0, 2)
        hits.append((item, val))
    hits.append((rng.choice([1, 2]), None))
    return hits


def hits_str(h):
    return ','.join('%d' % i if v is None else '%d:%d' % (i, v) for i, v in h)


def main():
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 60
    rig = PRig(exe)
    rng = random.Random(seed)
    L = []
    for k in range(n // 4 + 2):
        blk, keys = rand_block(rng), rand_keys(rng)
        flag = k % 2
        b, kk = rig.factory(flag, blk, keys)
        L.append('fact %d %s %s -> %s %s' % (flag, blk.hex(), keys.hex(), b.hex(), kk.hex()))
    for k in range(n * 3):
        blk, keys = rand_block(rng), rand_keys(rng)
        mod, cd = rng.choice([(1, 0), (1, 1), (0, 0), (0, 1)])
        hits = rand_hits(rng)
        if k % 5 == 0:                          # the common route: switch to the audio page and change things
            hits = [(11, 2)] + [h for h in hits if h[0] != 11]
        b, kk, t, s, d, ops = rig.dialog(blk, keys, hits, mod, cd)
        L.append('prefs %d %d %s %s %s -> %s %s %d %d %d | %s' % (mod, cd, blk.hex(), keys.hex(), hits_str(hits), b.hex(), kk.hex(), t, s, d, ' '.join(ops) or '-'))
    open(out, 'w').write('\n'.join(L) + '\n')
    print('%d vectors' % len(L))


if __name__ == '__main__':
    main()
