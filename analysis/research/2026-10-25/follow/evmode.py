# Ex ante, Oct 23 12-24 + Oct 24 00-12: the follow key with the coin's mode (plain / holder reward / creator fee bps > 0)
# instead of, or with, the fee routing; and the mode as a skip rule. Precision = his rate; value = live policy, net 1.2%.
import os, json, collections
__file__ = os.path.abspath("final.py")
src = open("final.py").read().rsplit('if __name__ == "__main__":', 1)[0]
exec(compile(src, "final5", "exec"))
MODE = json.load(open(SP + "/dec/cell/modes2324.json"))
def devfees(m):
    c = cr[m]; return c["creator"] == c["signer"] or c["creator"] == c["user"]
def mode(m):
    t = MODE.get(m) or ""
    hr = (len(t) >= 22 and t[-2:] == "01") or t == "000001"
    bps = int.from_bytes(bytes.fromhex(t[4:20]), "little") if len(t) >= 20 else 0
    return ("holder" if hr else "plain") + ("+bps" if bps else "")
c = collections.Counter((mode(m), devfees(m)) for s, i, m in coins)
print("mode x fee routing:", dict(c))
KEYS = {"(wallet, fee routing) [built]": lambda m: [(w, devfees(m)) for w in BW[m]["bw"]],
        "(wallet, mode)": lambda m: [(w, mode(m)) for w in BW[m]["bw"]],
        "(wallet, fee routing, mode)": lambda m: [(w, devfees(m), mode(m)) for w in BW[m]["bw"]]}
for lab, kf in KEYS.items():
    for skip_bps in (False, True):
        n = collections.Counter(); h = collections.Counter(); fol = {}
        for s, i, m in coins:
            ks = kf(m)
            fol[m] = any(h[k] >= 2 and h[k] / n[k] >= 0.3 for k in ks) and not (skip_bps and mode(m).endswith("+bps"))
            for k in ks: n[k] += 1; h[k] += D[m]["he"]
        sel = hit = tot = 0; V_ = []
        for m, f in fol.items():
            r = D[m]
            if r["per"] == "23a" or cls(r) == "low": continue
            tot += r["he"]
            if not f: continue
            sel += 1; hit += r["he"]
            if (BW[m]["pb"] or 99) <= 40:
                x = pol(m)
                if x is not None: V_.append(x)
        print(f"   {lab:32s}{' + skip creator-fee coins' if skip_bps else '':26s} coins {sel:5d} his rate {100*hit/max(1,sel):3.0f}% cover {100*hit/max(1,tot):3.0f}% | value {BT.fmt(BT.stats(V_, 0.012))} n {len(V_)} sum {100*sum(x - 0.012 for x in V_):+.0f}")
