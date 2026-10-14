import sys,json,base64,struct; sys.path.insert(0,'.')
from rpc import rpc; from sol import *
idl=json.load(open('/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/g5/docs/pump.json'))
T={t['name']:t for t in idl['types']}
def rd(d,o,t):
    if t=='bool': return bool(d[o]),o+1
    if t=='u8': return d[o],o+1
    if t=='u64': return struct.unpack_from('<Q',d,o)[0],o+8
    if t=='i64': return struct.unpack_from('<q',d,o)[0],o+8
    if t=='pubkey': return b58e(d[o:o+32]),o+32
    if isinstance(t,dict) and 'array' in t:
        out=[]; 
        for i in range(t['array'][1]): v,o=rd(d,o,t['array'][0]); out.append(v)
        return out,o
    raise Exception(t)
g=pda([b'global'],PUMP)[0]; print('global',g)
d=base64.b64decode(rpc('getAccountInfo',[g,{'encoding':'base64'}])['result']['value']['data'][0]); o=8; G={}
for f in T['Global']['type']['fields']:
    G[f['name']],o=rd(d,o,f['type'])
for k in ['initial_virtual_token_reserves','initial_virtual_sol_reserves','initial_real_token_reserves','token_total_supply','fee_basis_points','pool_migration_fee','creator_fee_basis_points','buyback_basis_points','initial_virtual_quote_reserves','whitelisted_quote_mints','mayhem_mode_enabled','max_curve_depth','enable_migrate','buyback_fee_recipients','reserved_fee_recipients','reserved_fee_recipient']:
    print(k,G[k])
print('len',len(d),'parsed to',o)
