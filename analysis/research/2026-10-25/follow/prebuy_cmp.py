# Inside each (operator, fee routing) cell of last night with mixed takes: does any pre-buy feature separate his takes
# from his skips? Each feature's value -> take mapping is learned on the cell's first half (in time) and scored on the
# second half, against the cell's own majority (the baseline). Also each feature's take rates pooled over mixed cells.
import json, collections, statistics as st
from urllib.parse import urlparse
S = json.load(open("prebuy.json"))
def b(x, edges):
    if x is None: return None
    for e in edges:
        if x < e: return f"<{e}"
    return f">={edges[-1]}"
def feats(d):
    c = d.get("create") or {}; bt = d.get("bundle_txs") or []
    f = {}
    f["host"] = urlparse(c.get("uri") or "").netloc or None
    f["cu_price"] = c.get("cu_price"); f["cu_limit"] = c.get("cu_limit")
    f["tip_route"] = "+".join(sorted(c.get("tips") or {})) or ("none" if c else None)
    f["tip"] = c.get("tip"); f["n_ix"] = c.get("n_ix"); f["n_sig"] = c.get("n_sig"); f["alts"] = c.get("alts")
    f["v0"] = c.get("v0"); f["pump_ix"] = "+".join(c.get("pump_ix") or []) or None
    f["name_len"] = b(len(c["name"]), [6, 10, 16, 24]) if c.get("name") else None
    f["sym_upper"] = c["sym"].isupper() if c.get("sym") else None
    f["name_eq_sym"] = (c["name"].lower().replace(" ", "") == c["sym"].lower()) if c.get("name") and c.get("sym") else None
    f["n_bundle_tx"] = len(bt)
    f["bundle_in_create_tx"] = any(x["same_as_create"] for x in bt)
    f["bundle_cu"] = "+".join(sorted({str(x["cu_price"]) for x in bt})) or None
    f["bundle_tip"] = "+".join(sorted({"+".join(sorted(x["tips"])) or "none" for x in bt})) or None
    f["bundle_multisig"] = any(x["n_sig"] > 1 for x in bt)
    idxs = sorted(x["idx"] for x in bt if x["idx"] is not None)
    f["bundle_gap"] = (idxs[0] - d["idx"]) if idxs and d.get("idx") is not None else None
    f["bundle_spread"] = (idxs[-1] - idxs[0]) if idxs else None
    f["dev_reused"] = d.get("dev_capped") or (d.get("dev_prior_txs") or 0) > 20
    f["dev_prior"] = b(d.get("dev_prior_txs"), [1, 3, 10, 50]) if not d.get("dev_capped") else "3000+"
    f["fund_src"] = (d["fund"][0][0][:6] if d.get("fund") else "none")
    f["fund_amt"] = b(d["fund"][0][1], [0.05, 0.5, 2, 5]) if d.get("fund") else "none"
    f["fund_gap"] = b(d["fund"][0][2], [50, 500, 5000, 50000]) if d.get("fund") else "none"
    f["signer_is_creator"] = d["signer"] == d["creator"]
    f["idx"] = b(d.get("idx"), [5, 20, 100, 300, 800])
    f["slot_in_leader"] = d["cs"] % 4
    f["his_busy"] = b(d.get("his_prev50"), [1, 2, 4])
    return f
cell = collections.defaultdict(list)
for m, d in S.items(): cell[(d["op"], d["dev_fees"])].append((d["cs"], m, d))
mixed = {k: sorted(v) for k, v in cell.items() if len(v) >= 6 and 0.1 <= sum(x[2]["he"] for x in v) / len(v) <= 0.9}
print("mixed cells", len(mixed), "launches", sum(len(v) for v in mixed.values()), "taken", sum(x[2]["he"] for v in mixed.values() for x in v))
F = {m: feats(d) for v in mixed.values() for _, m, d in v}
names = list(next(iter(F.values())).keys())
base_hit = base_n = 0; res = {}
for f in names:
    hit = n = 0; bh = 0
    for k, v in mixed.items():
        h = len(v) // 2; tr, te = v[:h], v[h:]
        maj = int(sum(x[2]["he"] for x in tr) * 2 >= len(tr))
        mp = collections.defaultdict(lambda: [0, 0])
        for _, m, d in tr: mp[F[m][f]][0] += 1; mp[F[m][f]][1] += d["he"]
        for _, m, d in te:
            val = F[m][f]
            pred = int(mp[val][1] * 2 >= mp[val][0]) if val in mp and mp[val][0] >= 2 else maj
            hit += pred == d["he"]; bh += maj == d["he"]; n += 1
    res[f] = (hit, bh, n)
for f, (hit, bh, n) in sorted(res.items(), key=lambda kv: -(kv[1][0] - kv[1][1])):
    print(f"   {f:20s} second-half accuracy {100*hit/n:5.1f}% vs cell majority {100*bh/n:5.1f}%  ({hit - bh:+d} of {n})")
print("\npooled take rates in mixed cells by value (n >= 15):")
for f in ("cu_price", "tip_route", "bundle_tip", "bundle_cu", "dev_reused", "fund_src", "his_busy", "host", "pump_ix", "n_bundle_tx", "bundle_gap"):
    c = collections.defaultdict(lambda: [0, 0])
    for v in mixed.values():
        for _, m, d in v: c[F[m][f]][0] += 1; c[F[m][f]][1] += d["he"]
    print(f"   {f}: " + " | ".join(f"{k}: {100*h/n:.0f}% of {n}" for k, (n, h) in sorted(c.items(), key=lambda kv: -kv[1][0]) if n >= 15))
