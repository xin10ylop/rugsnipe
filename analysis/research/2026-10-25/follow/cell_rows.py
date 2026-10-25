# Per-coin rows for every operator of last night (Oct 24 12:00 - Oct 25 09:13): his action, fee routing, the create and
# bundle shape, the first slots, the operator's sequence. -> cell/rows.json
import json, os, collections
N = "/tmp/claude-0/-home-user-rugsnipe/972c43cf-f494-5af4-b87a-fe32117a83be/scratchpad/dec/night"
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
ops = json.load(open(N + "/ops.json")); him = json.load(open(N + "/him.json"))
rows = []
for o, cs in ops.items():
    seq = sorted(cs.items(), key=lambda kv: kv[1]["slot"])
    prev = None
    for k, (m, c) in enumerate(seq):
        p = f"{N}/coins/{m}.json"
        t = json.load(open(p)) if os.path.exists(p) else None
        h = him.get(m) or {}
        r = dict(op=o, mint=m, k=k, slot=c["slot"], nw=len(c["wallets"]), bsz=round(sum(c["sizes"]), 3), vb=c["vb"],
                 he=int(h.get("att", 0) > 0), he_ok=int(h.get("att", 0) > h.get("failed", 0)),
                 pnl=(h.get("got", 0) - h.get("spent", 0)) / 1e9 if h.get("buy_slot") else None,
                 gap=c["slot"] - prev["slot"] if prev else None, prev_he=prev["he"] if prev else None)
        if t and t.get("create"):
            cr = t["create"]; cs0 = t["cs"]
            r["dev_fees"] = int(cr["creator"] in (cr["signer"], cr["user"]))
            r["idx"] = cr["idx"]; r["cs"] = cs0; r["bslot"] = c["slot"] - cs0
            ev = [(row[0], row[1], e) for row in t["rows"] for e in row[3]]
            dev = sum(e[2] for s, i, e in ev if s == cs0 and e[0] == cr["user"] and e[1]) / 1e9
            r["dev"] = round(dev, 3)
            ws = set(c["wallets"])
            b = [(s, i, e) for s, i, e in ev if e[0] in ws and e[1]]
            r["b_same"] = int(all(s == cs0 for s, i, e in b)) if b else None
            last = max(b, key=lambda x: (x[0], x[1])) if b else None
            r["pb"] = round(last[2][4] / 1e9, 2) if last else None
            others = [(s, i, e) for s, i, e in ev if e[0] not in ws and e[0] != cr["user"] and e[0] != A and e[1]]
            r["sn0"] = sum(1 for s, i, e in others if s == cs0); r["sn1"] = sum(1 for s, i, e in others if s == cs0 + 1)
            r["sn0_sol"] = round(sum(e[2] for s, i, e in others if s == cs0) / 1e9, 2)
            # an insider sell in the first slots (the dump clock)
            sells = [(s - cs0) for s, i, e in ev if e[0] in ws and not e[1]]
            r["dump"] = min(sells) if sells else None
            vs = [e[4] for s, i, e in ev]
            r["peak"] = round(max(vs) / 1e9, 2) if vs else None
            r["end"] = round(vs[-1] / 1e9, 2) if vs else None
            r["pos4"] = cs0 % 4
        rows.append(r); prev = r
json.dump(rows, open(os.path.dirname(__file__) + "/rows.json", "w"))
print(len(rows), "rows", sum(r["he"] for r in rows), "taken", sum(1 for r in rows if "dev_fees" in r), "with tape")
