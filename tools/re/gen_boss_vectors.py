#!/usr/bin/env python3
"""Golden vectors for src/boss.rs (the boss structures: registration 0x40dc4, free 0x40db0 / 0x40d6c, spawn 0x40e7c with the type inits 0x433e0 / 0x43bcc /
0x44048, per-frame update 0x4127c with the type routines 0x42344 / 0x4274c / 0x42f48, draw 0x417a4, damage 0x41c88, sound-end callback 0x4123c) by running the
original code in ppcemu.py.

    python3 -I gen_boss_vectors.py MarsRising.pef OUT.txt [seed] [n_vectors] [types]

State: the boss pool (D.109c -> 0x24c28, 10 x 228 bytes), the boss counter D.1abc, and everything of the enemy rig (enemy pool, group table, ships, tick).
Mocked callees (logged): random 0xa0d4, scores, ship damage, effects 0x22670 / 0x29714 / 0x23440, sounds 0x45bac / 0x45d54, the looping sound start 0x45e24,
pickup spawner 0x2a040, enemy shots 0x383b8 / 0x38754, blit primitives, MathLib sin / cos.

A vector line:  `ty=<t> seed=<n> G=<group words> S1=<ship words> S2=<..> R=<random queue> ops=<op;..> -> <tokens>`   (same layout as the enemy vectors)
ops:   R<g>  register group g (0x40dc4)      Z  free all (0x40d6c)       S<g>  spawn (0x40e7c)       F<tick>  set tick, one update (0x4127c)
       D<tick>,<translucent>,<shadows>  draw (0x417a4)        H<enemy>,<boss>,<amount>,<owner>  damage (0x41c88)       C<sel>,<boss>  sound-end callback (0x4123c)
       U<n>:<sparse words>  replace player n's words        W<slot>,<off>,<hex> / V<slot>,<off>,<byte>  poke an enemy slot
       Q<boss>,<off>,<hex> / q<boss>,<off>,<byte>  poke a boss record
token: `h<enemy pool>.<groups>.<counter>` + `b<boss pool hash>.<boss count>` then the logged calls `,<call>` and `,r<return>` where the routine returns something.
"""
import sys, os, random, struct, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu
import gen_enemy_vectors as G
from gen_enemy_vectors import fnv64, s32, M32, f32bits, rand_ship, rand_ship_near, unsparse, sparse, SHIP_BYTES, NGROUPS, GREC, TVBASE

BPOOL_PTR, BCOUNT = 0x109c, 0x1abc
BREC, NBOSS = 228, 10
SCRATCH = 0x2f1000      # parameter block for the callback test


class BRig(G.Rig):
    def __init__(self, exe):
        super().__init__(exe)
        c = self.cpu

        def trig(fn):
            def f(cpu):
                cpu.ww(cpu.r[1] + 20, cpu.r[2])
                cpu.setf(1, fn(cpu.getf(1)))
            return f

        def scb(cpu):    # 0x45e24(id, prio, &rec+200, callback, boss index)
            assert s32(cpu.r[5]) - self.bbase() == s32(cpu.r[7]) * BREC + 200 and cpu.r[6] == cpu.rw(0x1098)     # the block inside the record, the callback
            self.log.append('scb_%d_%d_%d' % (s32(cpu.r[3]), s32(cpu.r[4]), s32(cpu.r[7])))
            cpu.r[3] = (0x7100 + s32(cpu.r[7])) & M32

        def pick(cpu):   # 0x2a040(.., kind r5, variant r6, param r7, announce r8, x f1, y f2)
            self.log.append('pk_%d_%d_%d_%d_%08x_%08x' % (s32(cpu.r[5]), s32(cpu.r[6]), s32(cpu.r[7]), s32(cpu.r[8]), f32bits(cpu.getf(1)), f32bits(cpu.getf(2))))

        c.mocks[0x6272c] = trig(math.cos)
        c.mocks[0x62744] = trig(math.sin)
        c.mocks[0x45e24] = scb
        c.mocks[0x2a040] = pick
        self.base = bytes(c.mem)

    def bbase(self):
        return self.cpu.rw(BPOOL_PTR)

    def setup(self, tick, nplayers, groups, ships, rq, shotq=(), dif=0):
        super().setup(tick, nplayers, groups, ships, rq, shotq, dif)

    def digest(self):
        c = self.cpu
        b = self.bbase()
        return super().digest() + ',b%016x.%d' % (fnv64(c.mem[b:b + NBOSS * BREC]), s32(c.rw(BCOUNT)))

    def run(self, ops):
        c = self.cpu
        toks = []
        for op in ops:
            self.log = []
            kind, arg = op[0], op[1:]
            b = self.bbase()
            ret = None
            if kind == 'R':
                ret = c.call(0x40dc4, [int(arg)])
            elif kind == 'Z':
                c.call(0x40d6c, [])
            elif kind == 'S':
                c.call(0x40e7c, [int(arg)])
            elif kind == 'F':
                c.ww(G.TICK, int(arg))
                c.call(0x4127c, [])
            elif kind == 'D':
                tk, tr, sh = [int(v) for v in arg.split(',')]
                c.ww(G.TICK, tk)
                c.mem[0x12be] = tr
                c.mem[0x12bc] = sh
                c.call(0x417a4, [])
            elif kind == 'H':
                e, bi, amt, own = [int(v) for v in arg.split(',')]
                ret = c.call(0x41c88, [e & M32, bi & M32, amt & M32, own])
            elif kind == 'C':
                sel, bi = [int(v) for v in arg.split(',')]
                c.ww(SCRATCH + 16, bi)
                c.call(0x4123c, [sel, 0, SCRATCH])
            elif kind == 'U':
                n, w = arg.split(':', 1)
                self.set_ship(int(n), unsparse(w, SHIP_BYTES))
            elif kind == 'W':
                sl, off, v = arg.split(',')
                c.ww(G.POOL + int(sl) * G.SLOT + int(off), int(v, 16))
            elif kind == 'V':
                sl, off, v = arg.split(',')
                c.mem[G.POOL + int(sl) * G.SLOT + int(off)] = int(v) & 0xff
            elif kind == 'Q':
                bi, off, v = arg.split(',')
                c.ww(b + int(bi) * BREC + int(off), int(v, 16))
            elif kind == 'q':
                bi, off, v = arg.split(',')
                c.mem[b + int(bi) * BREC + int(off)] = int(v) & 0xff
            else:
                raise ValueError(op)
            tok = self.digest() + ''.join(',' + l for l in self.log)
            if ret is not None:
                tok += ',r%d' % s32(ret)
            toks.append(tok)
        return toks


def boss_group(rng, ty):
    b = bytearray(GREC)
    def w(i, v): struct.pack_into('>I', b, i * 4, v & M32)
    w(11, ty)
    w(12, rng.choice([1, 2, 4, 6, 7]))
    w(16, rng.choice([0, 1, 2, 3, 4, 5, 6, 8, 10, 14, 20]))      # +64 -> boss +104 (converted by 0x191ac at spawn)
    w(17, rng.choice([1, 2, 3, 4, 6, 8, 11, 13, 15, 17, 20]))    # +68 -> boss +108 (hit points, converted by 0x19248)
    w(19, rng.choice([0, 1, 2, 3, 4, 5, 6, 9]))                  # +76 -> boss +152 (bonus count)
    w(20, rng.choice([0, 1, 2, 3, 5, 7, 12, 18, 21]))            # +80 -> boss +148 (bonus kind)
    w(22, 1)
    return b


KINDS = [int(v) for v in os.environ.get('BOSS_KINDS', '').split(',') if v]
EDGE = [int(v) for v in os.environ.get('BOSS_EDGE', '').split(',') if v]


def gen(exe, out, seed=1, n=60, types=None):
    rng = random.Random(seed)
    rig = BRig(exe)
    types = types or [2, 4, 5]
    lines = []
    for it in range(n):
        ty = types[it % len(types)]
        if rng.random() < 0.05:
            ty = rng.choice([1, 3, 0, 6, -1])
        tick = rng.randint(1, 100000)
        dif = rng.randint(0, 6)
        groups = bytearray(NGROUPS * GREC)
        gl = []
        for gi in range(rng.randint(1, 3)):
            g = boss_group(rng, ty)
            groups[(gi * 7) * GREC:(gi * 7 + 1) * GREC] = g
            gl.append(gi * 7)
        extra = []
        if rng.random() < 0.2:          # more registrations than the pool has records (the surplus ones return -1)
            for k in range(rng.randint(8, 12)):
                groups[(3 + 7 * k) * GREC:(4 + 7 * k) * GREC] = boss_group(rng, rng.choice([1, 2, 3, 4, 5]))
                extra.append(3 + 7 * k)
        ships = [rand_ship_near(rng), rand_ship_near(rng, rng.random() < 0.7)]
        nplayers = rng.choice([1, 2, 2])
        rq = [rng.getrandbits(16) for _ in range(2500)]
        if EDGE:                         # boundary vectors: the spawn x lands next to the 208 steering threshold (x + half width)
            rq[0] = EDGE[it % len(EDGE)] - 32
        shotq = [rng.randint(0, 1) for _ in range(2500)]
        ops = []
        for gi in gl + extra:
            ops.append('R%d' % gi)
        ops.append('Z')
        t = tick
        cur = [bytearray(ships[0]), bytearray(ships[1])]
        for gi in gl:
            ops.append('S%d' % gi)
            nsteps = rng.randint(20, 60) if EDGE else rng.randint(150, 500)
            for fr in range(nsteps):
                t += rng.choice([1, 1, 1, 1, 2])
                ops.append('F%d' % t)
                r = rng.random()
                if r < 0.10:
                    ops.append('D%d,%d,%d' % (t, rng.randint(0, 1), rng.randint(0, 1)))
                if r > 0.88:       # damage: a part or a random enemy slot
                    ops.append('H%d,%d,%d,%d' % (rng.choice([-1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 12]), rng.randint(0, 3),
                                                 rng.choice([1, 3, 5, 10, 20, 100]), rng.randint(1, 2)))
                if rng.random() < 0.03:
                    ops.append('C%d,%d' % (rng.choice([1, 2, 3]), rng.randint(0, 4)))
                if rng.random() < 0.10:      # ship next to the boss, or move it
                    n_ = rng.choice([1, 2])
                    sh = rand_ship_near(rng, rng.random() < 0.9)
                    cur[n_ - 1] = sh
                    ops.append('U%d:%s' % (n_, sparse(sh)))
                if rng.random() < 0.08:
                    bi = rng.randint(0, 3)
                    k = rng.randrange(6)
                    if k == 0: ops.append('Q%d,44,%08x' % (bi, f32bits(rng.uniform(-20, 440))))
                    elif k == 1: ops.append('Q%d,48,%08x' % (bi, f32bits(rng.choice([-130.0, -60.0, 0.0, 49.0, 50.0, 51.0, 99.0, 100.0, 101.0, 150.0, 199.0, 200.0, 201.0, 540.0, 560.0]))))
                    elif k == 2: ops.append('Q%d,52,%08x' % (bi, f32bits(rng.choice([-3.0, -2.5, -1.0, 0.0, 1.0, 2.5, 3.0, 3.5, 4.0, 4.5]))))
                    elif k == 3: ops.append('Q%d,56,%08x' % (bi, f32bits(rng.choice([-5.0, -1.0, 0.0, 0.2, 1.0, 2.0, 3.0, 4.5, 5.0]))))
                    elif k == 4: ops.append('Q%d,%d,%08x' % (bi, rng.choice([96, 124, 8, 84]), rng.choice([0, 1, 2, 14, 15, 29, 30, 31, t - 1, t - 60, t - 61, t - 700, t - 701, t - 100]) & M32))
                    else: ops.append('Q%d,%d,%08x' % (bi, rng.choice([156, 160, 164]), f32bits(rng.uniform(-40, 400))))
        if EDGE:   # splice boundary operations in after random frames: exact kills, bad boss indices, culling / timing edges, odd sound / aux / particle bytes
            nops = []
            for op in ops:
                nops.append(op)
                if not op.startswith('F') or rng.random() > 0.3:
                    continue
                e, bi_ = rng.choice([0, 0, 1, 1, 2, 3, 4]), rng.choice([0, 0, 0, 1, 2, 3])
                k = rng.choice(KINDS) if KINDS else rng.randrange(13)
                tt = int(op[1:])
                if k == 0:
                    amt = rng.choice([1, 3, 5, 10]); hp = rng.randint(amt, amt * 4)
                    e = rng.randint(1, 4)   # a satellite / gun pod (the body carries the boss's hit points; a dead body with a living boss would make the original write to slot -1)
                    nops += ['W%d,96,%x' % (e, hp), 'W%d,100,%x' % (e, hp - amt), 'V%d,126,%d' % (e, rng.randint(0, 60)),
                            'H%d,%d,%d,%d' % (e, bi_, amt, rng.randint(1, 2))]
                elif k == 1:
                    nops.append('H%d,%d,%d,%d' % (e, bi_, 3, 2))   # (clearing the boss flag of a part is an impossible state: its death would index the group table with -1)
                elif k == 2:
                    nops.append('H%d,%d,%d,%d' % (e, rng.choice([-1, 10, 11, 99]), 5, 1))
                elif k == 3:
                    nops += ['Q%d,132,ffffffff' % bi_, 'V%d,124,255' % e, 'V%d,126,%d' % (e, rng.randint(0, 60)), 'H%d,%d,%d,%d' % (e, bi_, rng.choice([10, 100]), 1)]
                elif k == 4:
                    nops.append('Q%d,160,%08x' % (bi_, f32bits(rng.choice([13.5, 14.5, 15.0, 15.5, 16.0, 16.5, 17.0, 17.5, 18.0, 18.5, 59.5, 60.0, 60.5, 61.5, 62.0]))))
                elif k == 5:
                    if rng.random() < 0.5:
                        nops.append('Q%d,44,%08x' % (bi_, f32bits(rng.choice([-129.0, -128.0, -127.0, 479.0, 480.0, 481.0, 482.0, 483.0, 484.0, 485.0, 486.0, 487.0, 488.0]))))
                    else:
                        nops.append('Q%d,48,%08x' % (bi_, f32bits(rng.choice([-129.0, -128.0, -127.0, 399.0, 400.0, 401.0, 543.0, 544.0, 545.0]))))
                    nops.append('D%d,%d,%d' % (tt, rng.randint(0, 1), rng.randint(0, 1)))
                elif k == 6:
                    nops += ['Q%d,124,%08x' % (bi_, (tt - rng.choice([30, 31, 32, 33, 34])) & M32), 'Q%d,8,%08x' % (bi_, (tt - rng.choice([697, 698, 699, 700, 701, 702])) & M32)]
                elif k == 7:
                    nops += ['q%d,100,1' % bi_, 'W1,84,%x' % rng.randint(1, 3)]   # cannon cooling with state 1..3
                elif k == 8:
                    nops += ['H%d,%d,%d,%d' % (e, bi_, rng.choice([1, 5, 20]), 1)]
                elif k == 10:   # the culling edges of the draw pass for each body width
                    wd_ = {2: 58, 4: 61, 5: 64}.get(ty, 58)
                    v_ = rng.choice([('Q%d,44,%08x' % (bi_, f32bits(-128.0))), ('Q%d,44,%08x' % (bi_, f32bits(float(544 - wd_)))),
                                     ('Q%d,48,%08x' % (bi_, f32bits(-128.0))), ('Q%d,48,%08x' % (bi_, f32bits(544.0)))])
                    nops += [v_, 'D%d,%d,%d' % (tt, rng.randint(0, 1), rng.randint(0, 1))]
                elif k == 11:
                    nops.append('Q%d,48,%08x' % (bi_, f32bits(400.0)))
                elif k == 12:   # kill the boss through its body (slot 0) with an odd sound byte in the slot
                    nops += ['Q%d,112,00000200' % bi_, 'V0,126,%d' % rng.randint(1, 60), 'H%d,%d,1,%d' % (rng.choice([0, 0, -1, 20, 45]), bi_, rng.randint(1, 2))]
                else:     # vertical speed that reaches exactly +-3 after this frame's acceleration
                    sg = rng.choice([1.0, -1.0])
                    nops += ['Q%d,56,%08x' % (bi_, f32bits(2.5 * sg)), 'Q%d,64,%08x' % (bi_, f32bits(0.5 * sg))]
            ops = nops
        toks = rig.run_vec(ops, tick, nplayers, groups, ships, rq, shotq, dif) if hasattr(rig, 'run_vec') else None
        if toks is None:
            rig.setup(tick, nplayers, groups, ships, rq, shotq, dif)
            toks = rig.run(ops)
        lines.append('ty=%d seed=%d tick=%d np=%d dif=%d G=%s S1=%s S2=%s R=%s Z=%s ops=%s -> %s' % (
            ty, seed, tick, nplayers, dif, sparse(groups), sparse(ships[0]), sparse(ships[1]),
            ','.join('%x' % v for v in rq), ','.join(str(v) for v in shotq), ';'.join(ops), ' '.join(toks)))
    with open(out, 'w') as f:
        f.write('# generated by tools/re/gen_boss_vectors.py from the original executable; do not edit\n')
        f.write('\n'.join(lines) + '\n')
    return len(lines)


if __name__ == '__main__':
    exe, out = sys.argv[1], sys.argv[2]
    seed = int(sys.argv[3]) if len(sys.argv) > 3 else 1
    n = int(sys.argv[4]) if len(sys.argv) > 4 else 30
    types = [int(v) for v in sys.argv[5].split(',')] if len(sys.argv) > 5 else None
    print(gen(exe, out, seed, n, types), 'vectors ->', out)
