#!/usr/bin/env python3
"""Rig that runs the ORIGINAL PlayerPRO music engine of MarsRising.pef inside ppcemu.py.

    rig = MusicRig(exe); rig.load(madh_bytes); frames = rig.callback()   # one 732-frame stereo i16 buffer per call

What runs as original code: driver creation 0x53418 (settings as Music_Init leaves them with default preferences: 16 bit, 44.1 kHz,
mode 3 'deluxe', Sound Manager double buffer), the loader 0x540b4 (memory mode), the post-load steps 0x54ca4/0x54c0c/0x550dc and
the double-buffer callback 0x56d44 (-> Play 0x56410 -> ReadNote/effects/envelopes/mixer).
Mocked: the Memory Manager (bump allocator), BlockMove, Sound Manager/MIDI calls (no-ops).
"""
import sys, os, struct
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ppcemu

M32 = 0xffffffff
DRV_CELL = 0x25bf0       # u32[D.10a8] -> address of the cell that holds the driver pointer
SETTINGS = 0x25510       # u32[D.10b0]
HEAP0, HEAP1 = 0x40000, 0x2a0000
FILE_AT = 0x2b0000
DBUF = 0x3e0000          # fake SndDoubleBuffer for the callback (header 16 bytes + data)
IMPORT = dict(NewPtr=0x60944, NewPtrClear=0x6143c, NewPtrSys=0x61db4, DisposePtr=0x60974, BlockMove=0x619dc, BlockMoveData=0x60b24,
              SndNewChannel=0x61d84, SndPlayDoubleBuffer=0x61e44, NewRoutineDescriptor=0x61a84, DebugStr=0x614cc,
              SndDisposeChannel=0x6152c, SndDoImmediate=0x614b4)


class MusicRig:
    def __init__(self, exe):
        self.cpu = c = ppcemu.load(exe)
        self.base = bytes(c.mem)
        c.keep_toc = True
        self.heap = HEAP0
        c.mocks[IMPORT['NewPtr']] = self._newptr
        c.mocks[IMPORT['NewPtrClear']] = self._newptr
        c.mocks[IMPORT['NewPtrSys']] = self._newptr
        c.mocks[IMPORT['DisposePtr']] = lambda cpu: cpu.r.__setitem__(3, 0)
        c.mocks[IMPORT['DebugStr']] = lambda cpu: None
        def newchan(cpu):                         # *chan = a (fake) channel pointer
            cpu.ww(cpu.r[3], 0x2c0000); cpu.r[3] = 0
        c.mocks[IMPORT['SndNewChannel']] = newchan
        for n in ('SndPlayDoubleBuffer', 'SndDisposeChannel', 'SndDoImmediate'):
            c.mocks[IMPORT[n]] = lambda cpu: cpu.r.__setitem__(3, 0)
        c.mocks[IMPORT['NewRoutineDescriptor']] = lambda cpu: None   # returns its first argument
        # every other toolbox import: no-op returning 0 (MemError, HGetVol, ...)
        for addr in c.img.stub_name:
            c.mocks.setdefault(addr, lambda cpu: cpu.r.__setitem__(3, 0))
        def move(cpu):
            src, dst, n = cpu.r[3], cpu.r[4], cpu.r[5]
            cpu.mem[dst:dst + n] = bytes(cpu.mem[src:src + n])
            cpu.r[3] = 0
        c.mocks[IMPORT['BlockMove']] = move
        c.mocks[IMPORT['BlockMoveData']] = move

    def _newptr(self, cpu):
        n = (cpu.r[3] + 15) & ~15
        a = self.heap
        self.heap += n + 16
        assert self.heap < HEAP1, 'emulated heap exhausted'
        cpu.mem[a:a + n] = bytes(n)
        cpu.r[3] = a

    @property
    def drv(self):
        return self.cpu.rw(DRV_CELL)

    def create(self, bits=16, rate=0xac440000, mode=3, drvmode=4, surround=0, oversample=0):
        c = self.cpu
        c.mem[:] = self.base
        self.heap = HEAP0
        # the driver block as 0x53c48 makes it (the library-path scan 0x5769c is skipped)
        a = self.heap; self.heap += 13600
        c.ww(DRV_CELL, a)
        c.ww(a, 0x4d414444)
        c.mem[a + 13518:a + 13518 + 48] = bytes(c.mem[0x7db0:0x7db0 + 48])
        # settings as Music_Init (0x444f8) leaves them
        s = SETTINGS
        c.mem[s:s + 24] = bytes(24)
        c.wh(s, 4); c.wh(s + 2, bits); c.ww(s + 4, rate); c.wh(s + 8, mode); c.wh(s + 10, drvmode)
        c.wb(s + 12, surround); c.wb(s + 13, 1); c.wb(s + 14, 0); c.wb(s + 15, surround); c.wb(s + 16, oversample)
        c.ww(s + 18, 0); c.wb(s + 22, oversample)
        r = c.call(0x53418, [s], max_steps=50_000_000)
        assert r == 0, 'driver create failed: %x' % r
        return self

    def load(self, data):
        c = self.cpu
        c.mem[FILE_AT:FILE_AT + len(data)] = data
        c.call(0x54ca4, [])
        r = c.call(0x540b4, [0, 3, 0, FILE_AT], max_steps=200_000_000)
        assert r == 0, 'load failed: %x' % r
        c.call(0x54c0c, [])
        c.call(0x550dc, [])
        return self

    def _read_buf(self, a, n):
        return list(struct.unpack('>%dh' % (n * 2), bytes(self.cpu.mem[a + 16:a + 16 + n * 4])))

    pending = ()

    def _prime(self):
        """0x54a84 ends in 0x55958: both double buffers are filled by callbacks (playing is off at that time) -- those two
        buffers are the next audio the Sound Manager plays."""
        c = self.cpu
        d = self.drv
        c.call(0x54a84, [], max_steps=200_000_000)
        self.bufs = [c.rw(d + 13472), c.rw(d + 13476)]
        self.turn = 0
        n = c.rw(d + 13114)
        self.pending = [self._read_buf(b, n) for b in self.bufs]

    def start(self):
        """0x449dc: channel volumes 64, 54c0c (rewind), 54a84 (stop, reset, scan speed/tempo, prime), playing = 1."""
        c = self.cpu
        d = self.drv
        for i in range(32):
            c.ww(d + 12616 + 4 * i, 64)
        c.call(0x54c0c, [])
        self._prime()
        c.wb(d + 12880, 1)
        return self.pending

    def stop(self):
        """0x44ab8: 0x54b5c (flush, reset channels), playing = 0."""
        c = self.cpu
        c.call(0x54b5c, [])
        c.wb(self.drv + 12880, 0)
        self.pending = []

    def resume(self):
        """0x44b44: 0x54a84 (reset, rescan, prime), playing = 1."""
        self._prime()
        self.cpu.wb(self.drv + 12880, 1)

    def callback(self):
        """The Sound Manager's next buffer-empty callback (alternating buffers); returns the interleaved L/R i16 samples."""
        c = self.cpu
        d = self.drv
        b = self.bufs[self.turn & 1]
        self.turn += 1
        n = c.rw(d + 13114)
        c.call(0x56d44, [c.rw(d + 13484), b], max_steps=200_000_000)
        return self._read_buf(b, n)

    def next_block(self):
        if self.pending:
            blk = self.pending[0]
            self.pending = self.pending[1:]
            return blk
        return self.callback()


if __name__ == '__main__':
    exe, path = sys.argv[1], sys.argv[2]
    rig = MusicRig(exe).create().load(open(path, 'rb').read())
    d = rig.drv
    c = rig.cpu
    print('drv', hex(d), 'numChn', c.rh(d + 4), 'bits', c.rh(d + 6), 'rate', hex(c.rw(d + 8)), 'mode', c.rh(d + 12), 'dm', c.rh(d + 14), 'buf', c.rw(d + 13114))
    rig.start()
    for i in range(6):
        pcm = rig.next_block()
        print(i, len(pcm), max(abs(x) for x in pcm), pcm[:8], 'row', c.rh(d + 12610), 'pos', c.rh(d + 12614), 'tick', c.rh(d + 12960))
