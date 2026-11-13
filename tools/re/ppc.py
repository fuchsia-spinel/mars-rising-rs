import struct
def sx(v,b): return v-(1<<b) if v&(1<<(b-1)) else v
SPR={1:'xer',8:'lr',9:'ctr'}
def rn(n): return 'r%d'%n
def dis(w,pc):
    op=w>>26; rd=(w>>21)&31; ra=(w>>16)&31; rb=(w>>11)&31; imm=w&0xffff; si=sx(imm,16)
    D=lambda: f'{si}(r{ra})'
    if op==14: return f'li r{rd},{si}' if ra==0 else f'addi r{rd},r{ra},{si}'
    if op==15: return f'lis r{rd},0x{imm:04x}' if ra==0 else f'addis r{rd},r{ra},0x{imm:04x}'
    if op==12: return f'addic r{rd},r{ra},{si}'
    if op==13: return f'addic. r{rd},r{ra},{si}'
    if op==8: return f'subfic r{rd},r{ra},{si}'
    if op==7: return f'mulli r{rd},r{ra},{si}'
    if op==10: return f'cmplwi cr{(w>>23)&7},r{ra},{imm}'
    if op==11: return f'cmpwi cr{(w>>23)&7},r{ra},{si}'
    if op in (24,25,26,27,28,29): 
        n={24:'ori',25:'oris',26:'xori',27:'xoris',28:'andi.',29:'andis.'}[op]; return f'{n} r{ra},r{rd},0x{imm:x}'
    if op==21:
        sh=rb; mb=(w>>6)&31; me=(w>>1)&31; return f'rlwinm{"." if w&1 else ""} r{ra},r{rd},{sh},{mb},{me}'
    if op==20: return f'rlwimi r{ra},r{rd},{rb},{(w>>6)&31},{(w>>1)&31}'
    if op==23: return f'rlwnm r{ra},r{rd},r{rb},{(w>>6)&31},{(w>>1)&31}'
    ld={32:'lwz',33:'lwzu',34:'lbz',35:'lbzu',36:'stw',37:'stwu',38:'stb',39:'stbu',40:'lhz',41:'lhzu',42:'lha',43:'lhau',44:'sth',45:'sthu',46:'lmw',47:'stmw',48:'lfs',50:'lfd',52:'stfs',54:'stfd'}
    if op in ld: 
        r=('f%d'%rd) if op in (48,50,52,54) else 'r%d'%rd
        return f'{ld[op]} {r},{D()}'
    if op==18:
        li=sx(w&0x3fffffc,26); t=(pc+li)&0xffffffff if not w&2 else li&0xffffffff
        return f'{"bl" if w&1 else "b"}{"a" if w&2 else ""} 0x{t:x}'
    if op==16:
        bo=rd; bi=ra; bd=sx(w&0xfffc,16); t=(pc+bd)&0xffffffff
        return f'bc{"l" if w&1 else ""} {bo},{bi},0x{t:x}'
    if op==19:
        x=(w>>1)&0x3ff
        if x==16: return 'blr' if rd==20 else f'bclr {rd},{ra}'
        if x==528: return 'bctr' if rd==20 else f'bcctr {rd},{ra}'
        if x==0: return 'mcrf'
        return f'op19.{x}'
    if op==31:
        x=(w>>1)&0x3ff; rc='.' if w&1 else ''
        tb={266:'add',40:'subf',235:'mullw',491:'divw',459:'divwu',28:'and',444:'or',316:'xor',124:'nor',24:'slw',536:'srw',792:'sraw',
            23:'lwzx',87:'lbzx',279:'lhzx',343:'lhax',151:'stwx',215:'stbx',407:'sthx',104:'neg',75:'mulhw',11:'mulhwu',60:'andc',412:'orc',476:'nand',284:'eqv',
            55:'lwzux',119:'lbzux',311:'lhzux',183:'stwux',247:'stbux'}
        if x==339:
            spr=((w>>16)&31)|(((w>>11)&31)<<5); return f'mfspr r{rd},{SPR.get(spr,spr)}'
        if x==467:
            spr=((w>>16)&31)|(((w>>11)&31)<<5); return f'mtspr {SPR.get(spr,spr)},r{rd}'
        if x==0: return f'cmpw cr{(w>>23)&7},r{ra},r{rb}'
        if x==32: return f'cmplw cr{(w>>23)&7},r{ra},r{rb}'
        if x==824: return f'srawi{rc} r{ra},r{rd},{rb}'
        if x==922: return f'extsh r{ra},r{rd}'
        if x==954: return f'extsb r{ra},r{rd}'
        if x==26: return f'cntlzw r{ra},r{rd}'
        if x in tb:
            if x in (104,): return f'{tb[x]}{rc} r{rd},r{ra}'
            if x in (23,87,279,343,55,119,311): return f'{tb[x]} r{rd},r{ra},r{rb}'
            if x in (151,215,407,183,247): return f'{tb[x]} r{rd},r{ra},r{rb}'
            if x in (28,444,316,124,24,536,792,60,412,476,284): return f'{tb[x]}{rc} r{ra},r{rd},r{rb}'
            return f'{tb[x]}{rc} r{rd},r{ra},r{rb}'
        return f'op31.{x}'
    if op==17: return 'sc'
    return f'.long 0x{w:08x}'
def run(code,start,end,base=0,syms=None):
    for a in range(start,end,4):
        w=struct.unpack('>I',code[a:a+4])[0]; t=dis(w,base+a)
        print(f'{a:06x}: {w:08x}  {t}')
