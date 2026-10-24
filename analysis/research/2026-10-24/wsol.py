import sys, json, time, collections
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
sys.path.insert(0, SP + "/month"); sys.path.insert(0, SP + "/rivalpnl")
import tri
W = "6anjqMeWigQFo5wpciARAunVQUXTqoat8w8fFWarCkpc"; A = "8UdL67UGWs3dAqCBLtZAyjpyxoAZD5yfpDXXVBvfSvzA"
rows = [json.loads(l) for l in open(f"{SP}/month/6anjqMeW.jsonl")]
t_first = min(r["bt"] for r in rows); t_last = max(r["bt"] for r in rows)
before = None; allsig = []
while True:
    s = tri.sigs(A, before=before)
    if not s: break
    allsig += s; before = s[-1]["signature"]
    if s[-1]["blockTime"] < t_first: break
print("WSOL account txs since", time.strftime("%m-%d %H:%M", time.gmtime(t_first)), ":", sum(1 for x in allsig if x["blockTime"] >= t_first))
first = next(x for x in reversed(allsig) if x["blockTime"] >= t_first)
def wsol_bal(sig, which):
    t = tri.tx(sig)
    for b in t["meta"].get(which) or []:
        ks = t["transaction"]["message"]["accountKeys"]; la = (t["meta"].get("loadedAddresses") or {})
        keys = ks + la.get("writable", []) + la.get("readonly", [])
        if keys[b["accountIndex"]] == A: return float(b["uiTokenAmount"]["uiAmountString"] or 0)
    return None
w0 = wsol_bal(first["signature"], "preTokenBalances")
w1 = tri.rpc("getTokenAccountBalance", [A], cache=False)["value"]["uiAmountString"]
print(f"WSOL balance at the month's first touch {w0}, now {w1}")
# W's own SOL: balance change from the month rows, external transfers (not his WSOL account, not tips, not trades)
dW = sum(r["d"] for r in rows) / 1e9
ext = collections.defaultdict(float)
for r in rows:
    if any(t[1] == W for t in r["tr"]): continue
    for s0, d0, l in r["xf"]:
        if A in (s0, d0): continue
        if d0 == W: ext[s0] += l / 1e9
        elif s0 == W: ext[d0] -= l / 1e9
net_ext = sum(ext.values())
print(f"his wallet's SOL change {dW:+.3f}; external transfers in-out {net_ext:+.3f} (in {sum(v for v in ext.values() if v > 0):.3f}, out {sum(v for v in ext.values() if v < 0):.3f})")
print(f"=> trading P&L Oct 1 - now = wallet change + WSOL change - external net = {dW + (float(w1) - (w0 or 0)) - net_ext:+.3f} SOL over {(t_last - t_first)/86400:.1f} days")
