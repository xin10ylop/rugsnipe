import hashlib
B58='123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz'
def b58d(s):
    n=0
    for c in s: n=n*58+B58.index(c)
    b=n.to_bytes((n.bit_length()+7)//8,'big') if n else b''
    return b'\0'*(len(s)-len(s.lstrip('1')))+b
def b58e(b):
    n=int.from_bytes(b,'big'); s=''
    while n: n,r=divmod(n,58); s=B58[r]+s
    return '1'*(len(b)-len(b.lstrip(b'\0')))+s
P=2**255-19; D=(-121665*pow(121666,P-2,P))%P
def on_curve(b):
    y=int.from_bytes(b,'little')&((1<<255)-1)
    if y>=P: return False
    y2=y*y%P; u=(y2-1)%P; v=(D*y2+1)%P
    x2=u*pow(v,P-2,P)%P
    if x2==0: return True
    return pow(x2,(P-1)//2,P)==1
def pda(seeds,prog):
    pb=b58d(prog)
    for bump in range(255,-1,-1):
        h=hashlib.sha256(b''.join(seeds)+bytes([bump])+pb+b'ProgramDerivedAddress').digest()
        if not on_curve(h): return b58e(h),bump
ATA_PROG='ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL'
T22='TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb'; TOK='TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'
PUMP='6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P'; WSOL='So11111111111111111111111111111111111111112'; SYS='11111111111111111111111111111111'
def ata(owner,tp,mint): return pda([b58d(owner),b58d(tp),b58d(mint)],ATA_PROG)[0]
