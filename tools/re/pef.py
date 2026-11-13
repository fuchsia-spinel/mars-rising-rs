import struct
def load_pef(path):
    b=open(path,'rb').read()
    t1,t2,arch,fv,ts,od,oi,cv,secc,instc,_=struct.unpack('>4s4s4sIIIIIHHI',b[:40])
    secs=[]
    for i in range(secc):
        nmo,da,tl,ul,cl,co,k,sh,al,rs=struct.unpack('>iIIIIIBBBB',b[40+i*28:68+i*28])
        secs.append(dict(i=i,kind=k,addr=da,total=tl,unpacked=ul,clen=cl,off=co,raw=b[co:co+cl]))
    return b,secs
def unpack_pattern(d,total):
    out=bytearray(); p=0
    while p<len(d):
        op=d[p]; cnt=op&0x1f; opc=op>>5; p+=1
        def varint():
            nonlocal p; v=0
            while True:
                x=d[p]; p+=1; v=(v<<7)|(x&0x7f)
                if not x&0x80: return v
        if cnt==0: cnt=varint()
        if opc==0: out+=bytes(cnt)
        elif opc==1: out+=d[p:p+cnt]; p+=cnt
        elif opc==2: r=varint(); out+=d[p:p+cnt]*(r+1); p+=cnt
        elif opc in (3,4):
            c=varint(); r=varint()
            if opc==3: common=d[p:p+cnt]; p+=cnt
            else: common=bytes(cnt)
            for i in range(r):
                cu=d[p:p+c]; p+=c; out+=common+cu
            out+=common
        else: raise ValueError('bad pattern op %d'%opc)
    out+=bytes(max(0,total-len(out)))
    return bytes(out)
