#!/usr/bin/env python3
"""Block-0 eligibility model v2 (speed-gap-6). Stdlib only; run isolated: python3 -I deploy/stake_geo_v2.py RAW_DIR [--out data/stake_geo_v2.json]

Changes vs deploy/stake_geo.py (v1):
  (a) leader location = metro of the ADVERTISED tpuQuic IP (getClusterNodes + ipinfo.io), not the validator's city.
      The feed leg (shreds) still comes from the validator's own metro (Stakewiz ip_city).
  (b) per-leader-class batching wait w ~ U[0, W_class] (DZ validators-metadata software_client):
        JITO  = JitoLabs/Agave/Rakurai/Firedancer/Frankendancer/unknown (arrival-ordered; Firedancer must run Agave from Oct 21)
        BAM   = AgaveBam/FireBAM          (20 ms rounds per Oct-9 changelog; 50 ms per older docs page)
        HARM  = HarmonicAgave/HarmonicFiredancer (default fba = 50 ms double-buffered windows)
      Jito BE bundles add a 50 ms tick on any class if we send via sendBundle.
  (c) weights = successful pump.fun creates per leader (getSignaturesForAddress on mint authority joined to
      getLeaderSchedule), or a watched-wallet proxy set, instead of leader slots.
  (d) create position inside its block x ~ empirical distribution (getBlock index / non-vote count) instead of U[0,1].
  RTT: measured TeraSwitch looking-glass min RTT (lg.teraswitch.com, router in the candidate building -> the
  advertised tpuQuic IP) where available; otherwise the v1 metro matrix (WonderNetwork/DoubleZero/CloudPing).

P(same slot | leader, site) = P( x*S + L + w <= S ),  L = RTT_feed/2 + RTT_send/2 + c0.
"""
import collections, itertools, json, math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import stake_geo as v1  # CITY, build_rtt

RAW = sys.argv[1]
OUT = sys.argv[sys.argv.index("--out") + 1] if "--out" in sys.argv else None
J = lambda f: json.load(open(os.path.join(RAW, f)))
EXTRA_CITY = {
    "Offenbach": ("FRA", 0), "Eschborn": ("FRA", 0), "Woerden": ("AMS", 2), "Lelystad": ("AMS", 3),
    "São Paulo": ("NYC", 120), "Beauharnois": ("NYC", 10), "Erith": ("LON", 1), "Tung Chung": ("HKG", 0),
    "Hwaseong-si": ("TYO", 33), "Ellesmere Port Town": ("LON", 5), "Sunrise": ("WAS", 25), "Kent": ("PDX", 5),
    "Crumlin": ("DUB", 0), "Sunnyvale": ("LAX", 8), "Sevilla": ("PAR", 20), "Zürich": ("FRA", 7),
    "Šiauliai": ("SQQ", 0), "Helsinki": ("STO", 8), "Riga": ("SQQ", 4), "Buenos Aires": ("NYC", 140),
}
CITY = dict(v1.CITY); CITY.update(EXTRA_CITY)
CLASS = {"AgaveBam": "BAM", "FireBAM": "BAM", "HarmonicAgave": "HARM", "HarmonicFiredancer": "HARM"}
SITES = {  # site -> (metro for fallback, looking-glass location id or None)
    "FRA2-tsw": ("FRA", "frankfurt-fra2"), "AMS3-tsw": ("AMS", "amsterdam-ams3"), "LON1-tsw": ("LON", "london-lon1"),
    "TYO2-tsw": ("TYO", "tokyo-tyo2"), "EWR1-tsw": ("NYC", "newark-ewr1"), "SGP1-tsw": ("SIN", "singapore-sgp1"),
    "IAD1-tsw": ("WAS", "ashburn-iad1"), "CMH-aws": ("CMH", None), "SQQ-cherry": ("SQQ", None),
}
C0 = 5.0
SCEN = {  # max batching wait per class (ms); bundle=True adds Jito BE 50 ms tick on top for every class
    "v1_published": dict(JITO=0, BAM=0, HARM=0, own_tpu_bam=False),
    "direct_bam20": dict(JITO=0, BAM=20, HARM=50, own_tpu_bam=False),
    "direct_bam50": dict(JITO=0, BAM=50, HARM=50, own_tpu_bam=False),
    "bundle_all50": dict(JITO=50, BAM=50, HARM=50, own_tpu_bam=False),
    "bam_maintenance": dict(JITO=0, BAM=0, HARM=50, own_tpu_bam=True),
}

def main():
    nodes = {x["pubkey"]: x for x in J("nodes.json")["result"]}
    sw = {v["identity"]: v for v in J("sw.json")}
    ipinfo = J("tpu_ipinfo.json")
    votes = J("votes.json")["result"]; id2vote = {v["nodePubkey"]: v["votePubkey"] for v in votes["current"] + votes["delinquent"]}
    meta = {m["vote_account"]: m["software_client"] for m in J("dzmeta.json")}
    lg = J("lg_matrix.json") if os.path.exists(os.path.join(RAW, "lg_matrix.json")) else {}
    metros, R = v1.build_rtt()
    slot_ms = 1000 * sum(x["samplePeriodSecs"] for x in J("perf.json")["result"]) / sum(x["numSlots"] for x in J("perf.json")["result"])

    def metro_of_city(c):
        return CITY.get(c, (None, 0))

    leaders = {}
    for ident in set(J("sched.json")["result"]) | set(J("sched1056.json")["result"]):
        n = nodes.get(ident, {}); v = sw.get(ident, {})
        vm = metro_of_city(v.get("ip_city"))
        tip = (n.get("tpuQuic") or "").rsplit(":", 1)[0] or None
        im = metro_of_city(ipinfo.get(tip, {}).get("city")) if tip else (None, 0)
        if im[0] is None: im = vm
        if vm[0] is None: vm = im
        cl = CLASS.get(meta.get(id2vote.get(ident)), "JITO")
        leaders[ident] = dict(tpu=tip, ing=im, val=vm, cls=cl, gossip=(n.get("gossip") or "").rsplit(":", 1)[0])

    def rtt_metro(sm, m):
        if m[0] is None or m[0] not in R: return R[sm]["FRA"] + 40
        return R[sm][m[0]] + m[1]
    measured = [0, 0]
    def rtt_send(site, L, own):
        sm, loc = SITES[site]
        if own:   # BAM maintenance: own TPU in the validator's metro
            return rtt_metro(sm, L["val"])
        if loc and L["tpu"]:
            r = lg.get(f"{loc}|{L['tpu']}")
            if r and r.get("min") is not None:
                measured[0] += 1; return r["min"]
        measured[1] += 1
        return rtt_metro(sm, L["ing"])
    def rtt_feed(site, L, own):
        if L["val"][0] == L["ing"][0] and not own: return rtt_send(site, L, False)
        return rtt_metro(SITES[site][0], L["val"])

    # positions (d)
    pos = J("positions.json")
    X_emp = sorted(p["pos_nv"] for p in pos)
    X_uni = [(i + 0.5) / 200 for i in range(200)]

    def p_same(Lms, W, X):
        # P(x*S + L + w <= S), w~U[0,W], x~empirical
        tot = 0.0
        for x in X:
            slack = slot_ms * (1 - x) - Lms
            if slack <= 0: continue
            tot += 1.0 if W == 0 else min(1.0, slack / W)
        return tot / len(X)

    # weights (c)
    def sched_index(path, first):
        s = J(path)["result"]; idx = {}
        for ident, sl in s.items():
            for i in sl: idx[first + i] = ident
        return idx
    e57 = J("epoch.json")["result"]; first57 = e57["absoluteSlot"] - e57["slotIndex"]; first56 = first57 - 432000
    s57 = sched_index("sched.json", first57); s56 = sched_index("sched1056.json", first56)
    def leader_at(slot): return s57.get(slot) or s56.get(slot)
    W = {}
    W["leader_slots_e1057"] = collections.Counter({k: len(v) for k, v in J("sched.json")["result"].items()})
    cr = collections.Counter()
    ncr = 0
    for f in ("creates.json", "creates1056.json"):
        if not os.path.exists(os.path.join(RAW, f)): continue
        for c in J(f):
            if c["err"] or c["slot"] < first56: continue
            ld = leader_at(c["slot"])
            if ld: cr[ld] += 1; ncr += 1
    W["pump_creates"] = cr
    wt = collections.Counter(); nwt = 0
    if os.path.exists(os.path.join(RAW, "watched.json")):
        for slot in J("watched.json"):
            ld = leader_at(slot)
            if ld: wt[ld] += 1; nwt += 1
        W["watched_proxy"] = wt

    def evaluate(combo, sc, weights, X):
        cfg = SCEN[sc]; tot = sum(weights.values()); ps = 0; lsum = 0; cov2 = 0; percls = collections.Counter(); wcls = collections.Counter()
        for ident, w in weights.items():
            L = leaders.get(ident)
            if not L: continue
            own = cfg["own_tpu_bam"] and L["cls"] == "BAM"
            best = min(rtt_feed(s, L, own) / 2 + rtt_send(s, L, own) / 2 for s in combo)
            Wc = cfg["JITO"] if own else cfg[L["cls"]]
            p = p_same(best + C0, Wc, X)
            ps += w * p; lsum += w * best; cov2 += w * (best <= 1.0)
            percls[L["cls"]] += w * p; wcls[L["cls"]] += w
        return dict(sites="+".join(combo), p_same=round(ps / tot, 4), mean_oneway_ms=round(lsum / tot, 1),
                    share_le_1ms_oneway=round(cov2 / tot, 4),
                    by_class={k: round(percls[k] / wcls[k], 3) for k in wcls})

    res = dict(slot_ms=round(slot_ms, 1), c0_ms=C0, n_positions=len(X_emp),
               pos_quantiles={q: round(X_emp[int(q * (len(X_emp) - 1))], 3) for q in (0.1, 0.25, 0.5, 0.75, 0.9)},
               n_creates=ncr, n_watched=nwt, weights_share={}, results={})
    # weight shares by ingress metro / class
    for wn, wts in W.items():
        t = sum(wts.values()); m = collections.Counter(); c = collections.Counter()
        for ident, w in wts.items():
            L = leaders.get(ident)
            if L: m[L["ing"][0]] += w / t; c[L["cls"]] += w / t
        res["weights_share"][wn] = dict(ingress_metro={k: round(100 * v, 2) for k, v in m.most_common(12)},
                                        cls={k: round(100 * v, 2) for k, v in c.most_common()})
    sites = list(SITES)
    combos = [(s,) for s in sites] + list(itertools.combinations(sites, 2)) + list(itertools.combinations(sites, 3)) + \
             [("FRA2-tsw", "AMS3-tsw", "EWR1-tsw", "TYO2-tsw"), ("FRA2-tsw", "AMS3-tsw", "TYO2-tsw", "SGP1-tsw"), tuple(sites)]
    for wn in W:
        for sc in SCEN:
            for xn, X in (("emp", X_emp), ("uni", X_uni)):
                if xn == "uni" and not (wn == "leader_slots_e1057"): continue
                rows = sorted((evaluate(c, sc, W[wn], X) for c in combos), key=lambda r: -r["p_same"])
                res["results"][f"{wn}|{sc}|{xn}"] = rows
    res["lookups"] = dict(measured=measured[0], fallback=measured[1])
    if OUT: json.dump(res, open(OUT, "w"), indent=1)
    # print summary
    print(f"slot_ms={slot_ms:.1f} creates={ncr} watched={nwt} positions={len(X_emp)} q={res['pos_quantiles']}")
    for wn, d in res["weights_share"].items(): print(wn, d)
    keyset = ["FRA2-tsw", "AMS3-tsw", "LON1-tsw", "TYO2-tsw", "CMH-aws", "FRA2-tsw+AMS3-tsw", "FRA2-tsw+TYO2-tsw",
              "AMS3-tsw+TYO2-tsw", "FRA2-tsw+EWR1-tsw", "FRA2-tsw+SGP1-tsw", "FRA2-tsw+AMS3-tsw+TYO2-tsw",
              "FRA2-tsw+TYO2-tsw+EWR1-tsw", "FRA2-tsw+AMS3-tsw+EWR1-tsw+TYO2-tsw"]
    for k, rows in res["results"].items():
        d = {r["sites"]: r for r in rows}
        print(f"\n== {k}  best3: " + "; ".join(f"{r['sites']} {r['p_same']}" for r in rows[:3]))
        for s in keyset:
            if s in d: r = d[s]; print(f"  {s:38s} P={r['p_same']:.3f}  one-way={r['mean_oneway_ms']:5.1f}  <=1ms={r['share_le_1ms_oneway']:.3f}  {r['by_class']}")
    print("lookups", res["lookups"])

if __name__ == "__main__":
    main()
