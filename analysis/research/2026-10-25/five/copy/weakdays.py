# Making the copy hold up on his weak days: ex-ante filters on the listed-signer copies (sold at H-1, our measured
# landing distances, 0.5 SOL, bound +10%), judged by total AND by day. Everything a filter uses is known before our buy:
# his tip and size (in his own transaction), the operator's recent results for him (his coins on the signer whose last
# sell came before this buy), his own recent realized P&L, the price move a copy lands into (D = 90 vs his fill), and a
# stop-loss inside the hold (sell at the end of the first slot the position is below -SL).
import json, sys, collections, bisect
SP = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad"
src = open(SP + "/dec/copy/exitopt.py").read().split("for sl, sel in")[0]
exec(src)
# when each coin's result was known to him (his last sell), and his realized net on it
last_sell = {}
for r in W[A]:
    if r["kind"] == "sell" and r["ok"]:
        last_sell[r["m"]] = max(last_sell.get(r["m"], 0), r["bt"])
done = sorted((last_sell[m], m) for m in his_net if m in last_sell)
done_t = [t for t, _ in done]
def his_recent(bt, hours):
    i = bisect.bisect_left(done_t, bt); j = bisect.bisect_left(done_t, bt - hours * 3600)
    return sum(his_net[m] / 1e9 for _, m in done[j:i])
by_signer = collections.defaultdict(list)
for r in sorted(rows, key=lambda r: his_buy[r["m"]]["bt"]):
    by_signer[r["signer"]].append(r)
def signer_recent(r, k):
    bt = his_buy[r["m"]]["bt"]
    prev = [x for x in by_signer[r["signer"]] if last_sell.get(x["m"], 1e18) < bt]
    return [x["his"] for x in prev[-k:]]
def copy_v(r, sl=None):
    vs = []
    for D in dec:
        en = entry_at(r, D)
        if not en: continue
        if (en[0] / en[1]) / r["hisp"] - 1 > PREM: vs.append(-0.0001); continue
        h = r["H"] - 1; v = None
        if sl:
            for k in range(1, h):
                if r["s"] is not None and r["cs"] + k >= r["s"]["slot"]: break
                x = value_at(r, en, k)
                if x is not None and x <= -sl * 0.5: v = x; break
        if v is None: v = value_at(r, en, h)
        if v is not None: vs.append(v)
    return sum(vs) / len(vs) if vs else None
L = [r for r in rows if r["listed"]]
for r in L:
    r["v"] = copy_v(r); r["v_sl20"] = copy_v(r, 0.20); r["v_sl35"] = copy_v(r, 0.35)
    e = entry_at(r, 90); r["prem90"] = (e[0] / e[1]) / r["hisp"] - 1 if e else None
    r["sig3"] = signer_recent(r, 3); r["h6"] = his_recent(his_buy[r["m"]]["bt"], 6); r["h2"] = his_recent(his_buy[r["m"]]["bt"], 2)
DAYS = ["10-21", "10-22", "10-23", "10-24", "10-25"]
def show(lab, f, key="v"):
    sel = [r for r in L if r[key] is not None and f(r)]
    byd = collections.defaultdict(float)
    for r in sel: byd[r["day"]] += r[key]
    pos = sum(1 for d in DAYS if byd[d] > 0)
    print(f"   {lab:62} n {len(sel):4} total {sum(byd.values()):+6.2f}  days+ {pos}/5  " + " ".join(f"{byd[d]:+.2f}" for d in DAYS))
print("listed signers, sold at H-1; by day " + " ".join(DAYS))
show("baseline: his 0.7 SOL+ buys", lambda r: r["sol"] >= 0.7)
show("all his buys", lambda r: True)
print(" his conviction (his own transaction):")
for t in (4e6, 8e6, 12e6, 20e6):
    show(f"his tip >= {t/1e6:.0f}M", lambda r, t=t: r["tip"] >= t)
for s_ in (1.0, 1.5):
    show(f"his buy >= {s_} SOL", lambda r, s_=s_: r["sol"] >= s_)
show("his tip >= 4M and buy >= 0.7 SOL", lambda r: r["tip"] >= 4e6 and r["sol"] >= 0.7)
show("his tip >= 8M and buy >= 0.7 SOL", lambda r: r["tip"] >= 8e6 and r["sol"] >= 0.7)
print(" the operator is hot for him (his coins on the signer, results known before this buy):")
show("0.7 SOL+, his last coin on the signer won", lambda r: r["sol"] >= 0.7 and r["sig3"] and r["sig3"][-1] > 0)
show("0.7 SOL+, his last 3 coins on the signer net > 0", lambda r: r["sol"] >= 0.7 and r["sig3"] and sum(r["sig3"]) > 0)
show("tip >= 4M, his last 3 coins on the signer net > 0", lambda r: r["tip"] >= 4e6 and r["sig3"] and sum(r["sig3"]) > 0)
print(" his own recent run (realized):")
for hh, th in ((2, 0.0), (2, 0.5), (6, 0.0), (6, 1.0), (6, 2.0)):
    k = "h2" if hh == 2 else "h6"
    show(f"0.7 SOL+, his last {hh}h net > {th}", lambda r, k=k, th=th: r["sol"] >= 0.7 and r[k] > th)
print(" the price a copy lands into (D = 90 over his fill):")
for b in (0.0, 0.01, 0.02, 0.05):
    show(f"0.7 SOL+, landing price <= his +{b:.0%}", lambda r, b=b: r["sol"] >= 0.7 and r["prem90"] is not None and r["prem90"] <= b)
print(" stop-loss inside the hold:")
show("0.7 SOL+, stop-loss -20%", lambda r: r["sol"] >= 0.7, "v_sl20")
show("0.7 SOL+, stop-loss -35%", lambda r: r["sol"] >= 0.7, "v_sl35")
