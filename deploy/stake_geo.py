#!/usr/bin/env python3
"""Solana stake / leader-slot geography and a region-placement model for a same-slot sniper.

Usage (run isolated, stdlib only):
    python3 -I deploy/stake_geo.py                         # fetch live (Stakewiz + public RPC)
    python3 -I deploy/stake_geo.py --stakewiz sw.json --leaders ls.json --out data/stake_geo.json

Inputs
  * https://api.stakewiz.com/validators : per-validator activated_stake, ip_city/ip_country/ip_asn/ip_org
  * getLeaderSchedule (current epoch) from a Solana RPC: real leader-slot counts per identity
  * getRecentPerformanceSamples: measured slot time
  * RTT matrix (embedded below): WonderNetwork 14-day hourly ping medians (wondernetwork.com/pings/A/B,
    fetched 2026-10-14), DoubleZero data API internet p50 (data.doublezero.xyz/api/v1/dz/metro-pairs/latency,
    24 h, 2026-10-14) and CloudPing AWS inter-region p50 1Y (cloudping.co/api/latencies). Missing pairs are
    filled by shortest path over measured pairs; measured pairs are never overridden.

Model
  A leader produces `slot_ms` blocks. A target create lands at a uniform time u in [0, slot_ms).
  Our reaction arrives at the leader after L = RTT(box, leader_metro) + overhead_ms.
  P(same slot) = max(0, 1 - L / slot_ms).   (feed one-way + send one-way = RTT; overhead = decode/sign/relay)
  With several boxes, the box with the lowest RTT to the scheduled leader is used (schedule is known per epoch).
"""
import argparse, collections, itertools, json, math, sys, urllib.request

RPC = "https://api.mainnet-beta.solana.com"
STAKEWIZ = "https://api.stakewiz.com/validators"

# ---------------------------------------------------------------- metro mapping (city -> metro, extra ms)
CITY = {
    "Frankfurt am Main": ("FRA", 0), "Russelsheim": ("FRA", 0), "Limburg an der Lahn": ("FRA", 1),
    "Hattersheim am Main": ("FRA", 0), "Strasbourg": ("FRA", 4), "Offenbach": ("FRA", 0),
    "Berlin": ("FRA", 10), "Munich": ("FRA", 6), "Ottobrunn": ("FRA", 6), "Gunzenhausen": ("FRA", 5),
    "Falkenstein": ("FRA", 7), "Falkenstein/Vogtl.": ("FRA", 7), "Prague": ("FRA", 13),
    "Vienna": ("FRA", 12), "Zurich": ("FRA", 7), "Luxembourg": ("FRA", 5), "Bratislava": ("FRA", 14),
    "Warsaw": ("FRA", 18), "Bucharest": ("FRA", 30), "Sofia": ("FRA", 30), "Kyiv": ("FRA", 35),
    "Amsterdam": ("AMS", 0), "Diemen": ("AMS", 0), "Haarlem": ("AMS", 1), "Rotterdam": ("AMS", 2),
    "Nijmegen": ("AMS", 2), "Enschede": ("AMS", 3),
    "London": ("LON", 0), "Farnham Royal": ("LON", 0), "Slough": ("LON", 0), "Manchester": ("LON", 5),
    "Edinburgh": ("LON", 9), "Gravelines": ("LON", 5), "Roubaix": ("PAR", 3),
    "Paris": ("PAR", 0), "Aubervilliers": ("PAR", 0), "Madrid": ("PAR", 17), "La Rinconada": ("PAR", 22),
    "Dublin": ("DUB", 0), "Stockholm": ("STO", 0), "Oslo": ("STO", 8), "Helsinki": ("STO", 8),
    "Siauliai": ("SQQ", 0), "Šiauliai": ("SQQ", 0), "Riga": ("SQQ", 4),
    "Tokyo": ("TYO", 0), "Osaka": ("TYO", 8), "Seoul": ("TYO", 33),
    "Singapore": ("SIN", 0), "Hong Kong": ("HKG", 0),
    "Washington": ("WAS", 0), "Ashburn": ("WAS", 0), "Charlotte": ("WAS", 8), "Atlanta": ("WAS", 13),
    "New York City": ("NYC", 0), "Newark": ("NYC", 0), "Secaucus": ("NYC", 0), "Piscataway": ("NYC", 1),
    "Englewood Cliffs": ("NYC", 0), "Toronto": ("NYC", 12), "Montreal": ("NYC", 10),
    "Pittsburgh": ("WAS", 5), "Chicago": ("CHI", 0), "Elk Grove": ("CHI", 0), "Elk Grove Village": ("CHI", 0),
    "Dallas": ("CHI", 22), "Miami": ("WAS", 25), "Los Angeles": ("LAX", 0), "San Jose": ("LAX", 8),
    "San Francisco": ("LAX", 9), "Salt Lake City": ("SLC", 0), "Ogden": ("SLC", 1), "Bluffdale": ("SLC", 0),
    "Bingham": ("SLC", 1), "Boardman": ("PDX", 0), "Anchorage": ("PDX", 45),
    "Sao Paulo": ("NYC", 120), "Buenos Aires": ("NYC", 140), "Santiago de Queretaro": ("CHI", 35),
    "Pushkino": ("FRA", 40), "Novosibirsk": ("FRA", 80),
}
REGION = {"FRA": "EU", "AMS": "EU", "LON": "EU", "PAR": "EU", "DUB": "EU", "STO": "EU", "SQQ": "EU",
          "TYO": "APAC", "SIN": "APAC", "HKG": "APAC", "WAS": "US-East", "NYC": "US-East",
          "CHI": "US-Central", "CMH": "US-Central", "LAX": "US-West", "SLC": "US-West", "PDX": "US-West"}

# ---------------------------------------------------------------- RTT matrix (ms, round trip)
# W = WonderNetwork median of hourly averages (336 samples / 14 d); D = DoubleZero internet p50 (24 h);
# C = CloudPing AWS p50 1Y. CMH = Columbus, Ohio (AWS us-east-2); WAS = DC / Ashburn (us-east-1);
# PDX = Boardman, OR (us-west-2); SQQ = Siauliai LT (Vilnius ping used as proxy where D is missing).
RTT_SRC = {
 ("CMH","FRA"):(107.9,"W"),("CMH","AMS"):(97.4,"W"),("CMH","LON"):(90.1,"W"),("CMH","TYO"):(145.8,"W"),
 ("CMH","SQQ"):(120.8,"W"),("CMH","SIN"):(233.3,"W"),("CMH","WAS"):(13.9,"C"),("CMH","NYC"):(15.9,"W"),
 ("CMH","LAX"):(60.3,"W"),("CMH","DUB"):(108.8,"W"),("CMH","STO"):(117.9,"W"),("CMH","CHI"):(12.9,"W"),
 ("CMH","HKG"):(206.4,"W"),("CMH","SLC"):(60.3,"W"),("CMH","PAR"):(95.1,"W"),("CMH","PDX"):(55.7,"C"),
 ("WAS","FRA"):(87.9,"W"),("WAS","AMS"):(87.2,"W"),("WAS","LON"):(75.3,"W"),("WAS","TYO"):(149.4,"C"),
 ("WAS","SQQ"):(118.0,"W"),("WAS","SIN"):(213.6,"W"),("WAS","NYC"):(7.5,"W"),("WAS","LAX"):(56.4,"W"),
 ("WAS","DUB"):(69.4,"C"),("WAS","STO"):(98.1,"W"),("WAS","CHI"):(19.7,"W"),("WAS","HKG"):(200.8,"W"),
 ("WAS","SLC"):(61.5,"W"),("WAS","PAR"):(78.5,"W"),("WAS","PDX"):(68.3,"C"),
 ("NYC","FRA"):(85.9,"W"),("NYC","AMS"):(76.3,"W"),("NYC","LON"):(70.0,"W"),("NYC","TYO"):(173.1,"W"),
 ("NYC","SQQ"):(106.5,"W"),("NYC","SIN"):(220.0,"W"),("NYC","LAX"):(66.4,"W"),("NYC","DUB"):(80.0,"D"),
 ("NYC","STO"):(97.2,"W"),("NYC","CHI"):(18.0,"W"),("NYC","HKG"):(212.1,"W"),("NYC","SLC"):(64.4,"W"),
 ("NYC","PAR"):(72.9,"W"),
 ("FRA","AMS"):(7.4,"D"),("FRA","LON"):(16.8,"D"),("FRA","TYO"):(230.0,"W"),("FRA","SQQ"):(24.8,"D"),
 ("FRA","SIN"):(156.9,"D"),("FRA","LAX"):(145.8,"W"),("FRA","DUB"):(25.4,"W"),("FRA","STO"):(23.9,"C"),
 ("FRA","CHI"):(106.9,"W"),("FRA","HKG"):(180.2,"W"),("FRA","SLC"):(148.6,"W"),("FRA","PAR"):(10.2,"C"),
 ("FRA","PDX"):(143.3,"C"),
 ("PAR","AMS"):(13.5,"W"),("PAR","LON"):(7.2,"W"),("PAR","TYO"):(221.0,"C"),("PAR","SQQ"):(29.8,"W"),
 ("PAR","SIN"):(150.6,"W"),("PAR","LAX"):(127.7,"W"),("PAR","DUB"):(15.4,"W"),("PAR","STO"):(27.5,"W"),
 ("PAR","CHI"):(96.6,"W"),("PAR","HKG"):(196.7,"C"),("PAR","SLC"):(133.2,"W"),("PAR","PDX"):(142.2,"C"),
 ("LON","AMS"):(6.4,"D"),("LON","TYO"):(214.2,"C"),("LON","SQQ"):(38.9,"W"),("LON","SIN"):(162.3,"W"),
 ("LON","LAX"):(124.7,"W"),("LON","DUB"):(10.8,"D"),("LON","STO"):(25.7,"W"),("LON","CHI"):(91.6,"W"),
 ("LON","HKG"):(199.2,"C"),("LON","SLC"):(131.9,"W"),("LON","PDX"):(129.3,"C"),
 ("TYO","AMS"):(235.8,"W"),("TYO","SQQ"):(285.0,"W"),("TYO","SIN"):(68.0,"W"),("TYO","LAX"):(105.1,"D"),
 ("TYO","DUB"):(203.8,"C"),("TYO","STO"):(246.3,"C"),("TYO","CHI"):(148.3,"W"),("TYO","HKG"):(52.5,"C"),
 ("TYO","SLC"):(115.7,"W"),("TYO","PDX"):(99.3,"C"),
 ("AMS","SQQ"):(43.8,"W"),("AMS","SIN"):(155.7,"W"),("AMS","LAX"):(131.9,"W"),("AMS","DUB"):(17.8,"D"),
 ("AMS","STO"):(27.7,"D"),("AMS","CHI"):(91.6,"W"),("AMS","HKG"):(191.4,"W"),("AMS","SLC"):(141.9,"W"),
 ("CHI","SQQ"):(130.7,"W"),("CHI","SIN"):(217.9,"W"),("CHI","LAX"):(52.2,"D"),("CHI","DUB"):(100.1,"W"),
 ("CHI","STO"):(115.6,"W"),("CHI","HKG"):(196.7,"W"),("CHI","SLC"):(29.7,"D"),("CHI","PDX"):(44.5,"D"),
 ("LAX","SQQ"):(157.6,"W"),("LAX","SIN"):(176.6,"D"),("LAX","DUB"):(145.2,"W"),("LAX","STO"):(161.3,"W"),
 ("LAX","HKG"):(149.7,"W"),("LAX","SLC"):(23.4,"W"),("LAX","PDX"):(27.5,"D"),
 ("SIN","SQQ"):(191.4,"W"),("SIN","DUB"):(163.9,"W"),("SIN","STO"):(180.4,"C"),("SIN","HKG"):(30.1,"D"),
 ("SIN","SLC"):(169.6,"W"),("SIN","PDX"):(167.4,"C"),
 ("DUB","SQQ"):(48.5,"W"),("DUB","STO"):(38.0,"C"),("DUB","HKG"):(258.0,"C"),("DUB","SLC"):(109.7,"W"),
 ("DUB","PDX"):(118.5,"C"),("STO","SQQ"):(33.2,"D"),("STO","PDX"):(154.6,"C"),("STO","HKG"):(211.8,"C"),
 ("HKG","PDX"):(144.8,"C"),("SLC","PDX"):(40.7,"D"),
}
INTRA_METRO_MS = 1.0   # different DCs in one metro; same DC (e.g. TeraSwitch FRA to TeraSwitch FRA) ~0.2-0.5

def build_rtt():
    metros = sorted({m for k in RTT_SRC for m in k})
    R = {a: {b: (0.0 if a == b else math.inf) for b in metros} for a in metros}
    for (a, b), (v, _) in RTT_SRC.items():
        R[a][b] = R[b][a] = min(R[a][b], v)
    F = {a: dict(R[a]) for a in metros}      # shortest-path closure, used ONLY for unmeasured pairs
    for k in metros:
        for i in metros:
            for j in metros:
                if F[i][k] + F[k][j] < F[i][j]:
                    F[i][j] = F[i][k] + F[k][j]
    for i in metros:
        for j in metros:
            if R[i][j] == math.inf:
                R[i][j] = F[i][j]
    for m in metros:
        R[m][m] = INTRA_METRO_MS
    return metros, R

def rpc(method, params=None):
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params or []}).encode()
    req = urllib.request.Request(RPC, body, {"content-type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=120))["result"]

def load(path, url=None, method=None):
    if path:
        j = json.load(open(path))
        return j.get("result", j) if isinstance(j, dict) else j
    if url:
        return json.load(urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "stake_geo"}), timeout=120))
    return rpc(method, [None]) if method == "getLeaderSchedule" else rpc(method)

CLOUD_ASN = {"AS16509": "AWS", "AS14618": "AWS", "AS15169": "GCP", "AS396982": "GCP", "AS8075": "Azure",
             "AS45102": "Alibaba", "AS31898": "Oracle", "AS24940": "Hetzner", "AS16276": "OVH",
             "AS20473": "Vultr", "AS14061": "DigitalOcean", "AS63949": "Akamai/Linode"}

def p_same(rtt, slot_ms, overhead):
    return max(0.0, 1.0 - (rtt + overhead) / slot_ms)

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--stakewiz"); ap.add_argument("--leaders"); ap.add_argument("--out")
    ap.add_argument("--slot-ms", type=float, default=None, help="default: measured via RPC, else 210")
    ap.add_argument("--overhead-ms", type=float, default=5.0, help="decode+sign+relay/scheduler overhead")
    a = ap.parse_args()

    sw = load(a.stakewiz, STAKEWIZ)
    try:
        sched = load(a.leaders, method="getLeaderSchedule")
    except Exception as e:
        print("leader schedule unavailable, weighting by stake:", e, file=sys.stderr); sched = None
    slot_ms = a.slot_ms
    if slot_ms is None:
        try:
            s = rpc("getRecentPerformanceSamples", [30])
            slot_ms = 1000 * sum(x["samplePeriodSecs"] for x in s) / sum(x["numSlots"] for x in s)
        except Exception:
            slot_ms = 210.0

    act = [v for v in sw if not v.get("delinquent") and (v.get("activated_stake") or 0) > 0]
    T = sum(v["activated_stake"] for v in act)
    by = {k: collections.Counter() for k in ("city", "country", "asn", "metro", "region", "hosting")}
    unmapped = collections.Counter(); ident_metro = {}
    for v in act:
        s = v["activated_stake"]; city = v.get("ip_city") or "?"
        metro, extra = CITY.get(city, (None, 0))
        if metro is None:
            unmapped[city] += s; metro = "OTHER"
        ident_metro[v["identity"]] = (metro, extra)
        by["city"][f'{city}, {v.get("ip_country")}'] += s
        by["country"][v.get("ip_country") or "?"] += s
        by["asn"][f'{v.get("ip_asn")} {v.get("ip_org")}'] += s
        by["metro"][metro] += s
        by["region"][REGION.get(metro, "Other")] += s
        by["hosting"][CLOUD_ASN.get(v.get("ip_asn"), "bare-metal/other")] += s

    # leader-slot weights per (metro, extra)
    lw = collections.Counter(); total_slots = 0; miss = 0
    if sched:
        for ident, slots in sched.items():
            n = len(slots); total_slots += n
            if ident in ident_metro: lw[ident_metro[ident]] += n
            else: miss += n
    else:
        for v in act: lw[ident_metro[v["identity"]]] += v["activated_stake"]
        total_slots = T
    W = {k: n / total_slots for k, n in lw.items()}
    metro_slots = collections.Counter()
    for (m, e), w in W.items(): metro_slots[m] += w

    metros, R = build_rtt()
    sites = [m for m in metros]          # every metro is a candidate deployment site
    def rtt(site, m, e):
        if m == "OTHER": return R[site]["FRA"] + 40
        return R[site][m] + e
    def score(combo):
        exp_rtt = sum(w * min(rtt(s, m, e) for s in combo) for (m, e), w in W.items())
        ps = sum(w * p_same(min(rtt(s, m, e) for s in combo), slot_ms, a.overhead_ms) for (m, e), w in W.items())
        cov = {t: sum(w for (m, e), w in W.items() if min(rtt(s, m, e) for s in combo) <= t) for t in (2, 10, 25, 50)}
        return dict(sites=list(combo), mean_rtt_ms=round(exp_rtt, 1), p_same_slot=round(ps, 4),
                    cov_le_ms={str(k): round(v, 4) for k, v in cov.items()})
    singles = sorted((score((s,)) for s in sites), key=lambda r: -r["p_same_slot"])
    best = {}
    for k in (2, 3, 4, 5):
        best[k] = sorted((score(c) for c in itertools.combinations(sites, k)), key=lambda r: -r["p_same_slot"])[:5]
    full = score(tuple(sites))

    pct = lambda c, n=None: [(k, round(100 * v / T, 2)) for k, v in c.most_common(n)]
    print(f"validators active={len(act)} stake={T:,.0f} SOL  slot_ms={slot_ms:.1f}  overhead_ms={a.overhead_ms}")
    if sched: print(f"leader schedule: {total_slots} slots, {miss} unmapped ({100*miss/total_slots:.2f}%)")
    for name, n in (("city", 25), ("country", 12), ("asn", 20), ("metro", None), ("region", None), ("hosting", None)):
        print(f"\n== stake by {name}")
        cum = 0
        for k, p in pct(by[name], n):
            cum += p; print(f"  {k[:58]:58s} {p:6.2f}%  cum {cum:6.2f}%")
    print("\n== leader-slot share by metro (current epoch)")
    for m, w in metro_slots.most_common(): print(f"  {m:6s} {100*w:6.2f}%")
    print(f"\n== single site (sorted by expected same-slot probability; slot={slot_ms:.0f} ms)")
    print(f"  {'site':6s} {'meanRTT':>8s} {'P(same)':>8s} {'<=2ms':>7s} {'<=10':>7s} {'<=25':>7s} {'<=50':>7s}")
    for r in singles:
        c = r["cov_le_ms"]
        print(f"  {r['sites'][0]:6s} {r['mean_rtt_ms']:8.1f} {r['p_same_slot']:8.3f} {c['2']:7.3f} {c['10']:7.3f} {c['25']:7.3f} {c['50']:7.3f}")
    for k, rows in best.items():
        print(f"\n== best {k}-site sets")
        for r in rows:
            c = r["cov_le_ms"]
            print(f"  {'+'.join(r['sites']):28s} meanRTT {r['mean_rtt_ms']:6.1f}  P(same) {r['p_same_slot']:.3f}  <=2ms {c['2']:.3f}  <=10ms {c['10']:.3f}")
    print(f"\n== every metro covered: meanRTT {full['mean_rtt_ms']}  P(same) {full['p_same_slot']}")
    if unmapped: print("\nunmapped cities:", pct(unmapped))

    if a.out:
        out = dict(generated_from=dict(stakewiz=STAKEWIZ, rpc=RPC, rtt_sources="WonderNetwork/DoubleZero/CloudPing, see RTT_SRC"),
                   epoch_note="leader schedule = current epoch at fetch time", slot_ms=round(slot_ms, 1),
                   overhead_ms=a.overhead_ms, validators_active=len(act), total_stake_sol=round(T),
                   leader_slots=total_slots, leader_slots_unmapped=miss,
                   stake_pct={k: dict(pct(c, 40)) for k, c in by.items()},
                   leader_slot_pct_by_metro={m: round(100 * w, 2) for m, w in metro_slots.most_common()},
                   rtt_matrix_ms={s: {m: round(R[s][m], 1) for m in metros} for s in metros},
                   single_site=singles, best_sets={str(k): v for k, v in best.items()}, all_metros=full)
        json.dump(out, open(a.out, "w"), indent=1)
        print("\nwrote", a.out)

if __name__ == "__main__":
    main()
