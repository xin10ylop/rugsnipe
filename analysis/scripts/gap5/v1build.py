import struct,base64
from sol import *
NONCE_SYSVAR='SysvarRecentB1ockHashes11111111111111111111'
class Ix:
    def __init__(s,prog,metas,data): s.prog=prog; s.metas=metas; s.data=data  # metas: (key,signer,writable)
def compile_keys(payer,ixs):
    acc={}
    order=[]
    def add(k,sg,w):
        if k not in acc: acc[k]=[False,False]; order.append(k)
        acc[k][0]|=sg; acc[k][1]|=w
    add(payer,True,True)
    for ix in ixs:
        for k,sg,w in ix.metas: add(k,sg,w)
        add(ix.prog,False,False)
    cls=lambda k:(0 if k==payer else 1, {(True,True):0,(True,False):1,(False,True):2,(False,False):3}[tuple(acc[k])])
    keys=sorted(order,key=lambda k:(cls(k),order.index(k)))
    nsig=sum(1 for k in keys if acc[k][0]); nro_s=sum(1 for k in keys if acc[k][0] and not acc[k][1]); nro_u=sum(1 for k in keys if not acc[k][0] and not acc[k][1])
    return keys,(nsig,nro_s,nro_u)
def v1_tx(payer,ixs,lifetime,prio=None,cul=None,lads=None,heap=None):
    keys,hdr=compile_keys(payer,ixs); idx={k:i for i,k in enumerate(keys)}
    mask=0; cfg=b''
    if prio is not None: mask|=3; cfg+=struct.pack('<Q',prio)
    if cul is not None: mask|=4; cfg+=struct.pack('<I',cul)
    if lads is not None: mask|=8; cfg+=struct.pack('<I',lads)
    if heap is not None: mask|=16; cfg+=struct.pack('<I',heap)
    m=bytes([0x81])+bytes(hdr)+struct.pack('<I',mask)+b58d(lifetime)+bytes([len(ixs),len(keys)])+b''.join(b58d(k) for k in keys)+cfg
    m+=b''.join(struct.pack('<BBH',idx[ix.prog],len(ix.metas),len(ix.data)) for ix in ixs)
    m+=b''.join(bytes(idx[k] for k,_,_ in ix.metas)+ix.data for ix in ixs)
    tx=m+b'\0'*64*hdr[0]
    return tx,keys,hdr
def shortvec(n):
    out=b''
    while True:
        b=n&0x7f; n>>=7
        if n: out+=bytes([b|0x80])
        else: return out+bytes([b])
def legacy_tx(payer,ixs,lifetime):
    keys,hdr=compile_keys(payer,ixs); idx={k:i for i,k in enumerate(keys)}
    m=bytes(hdr)+shortvec(len(keys))+b''.join(b58d(k) for k in keys)+b58d(lifetime)+shortvec(len(ixs))
    for ix in ixs:
        m+=bytes([idx[ix.prog]])+shortvec(len(ix.metas))+bytes(idx[k] for k,_,_ in ix.metas)+shortvec(len(ix.data))+ix.data
    return shortvec(hdr[0])+b'\0'*64*hdr[0]+m,keys,hdr
def cb_limit(n): return Ix('ComputeBudget111111111111111111111111111111',[],bytes([2])+struct.pack('<I',n))
def cb_price(n): return Ix('ComputeBudget111111111111111111111111111111',[],bytes([3])+struct.pack('<Q',n))
def cb_lads(n): return Ix('ComputeBudget111111111111111111111111111111',[],bytes([4])+struct.pack('<I',n))
def advance_nonce(nonce,auth): return Ix(SYS,[(nonce,False,True),(NONCE_SYSVAR,False,False),(auth,True,False)],struct.pack('<I',4))
def transfer(a,b,lam): return Ix(SYS,[(a,True,True),(b,False,True)],struct.pack('<IQ',2,lam))
def create_ata_idem(payer,owner,mint,tp): return Ix(ATA_PROG,[(payer,True,True),(ata(owner,tp,mint),False,True),(owner,False,False),(mint,False,False),(SYS,False,False),(tp,False,False)],bytes([1]))
GLOBAL='4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf'; FEE_CONFIG='8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt'; EVT='Ce6TQqeHC9p8KetsN6JsjHK7UTZk7nasjjnr7XxXp9F1'
def buy_v3(user,mint,buyback,amount,max_cost,disc='07051dc4f5176550',tp=T22):
    bc=pda([b'bonding-curve',b58d(mint)],PUMP)[0]
    metas=[(GLOBAL,0,0),(mint,0,0),(WSOL,0,0),(tp,0,0),(TOK,0,0),(bc,0,1),(ata(bc,tp,mint),0,1),(ata(bc,TOK,WSOL),0,1),(user,1,1),(ata(user,tp,mint),0,1),(ata(user,TOK,WSOL),0,1),
           (pda([b'user_volume_accumulator',b58d(user)],PUMP)[0],0,1),(FEE_CONFIG,0,0),(buyback,0,1),(SYS,0,0),(EVT,0,0),(PUMP,0,0)]
    metas=[(k,bool(s),bool(w)) for k,s,w in metas]
    return Ix(PUMP,metas,bytes.fromhex(disc)+struct.pack('<QQ',amount,max_cost)+b'\x01')
