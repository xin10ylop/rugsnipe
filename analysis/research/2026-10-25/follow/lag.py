# The same test when the wallet records only know his attempts on coins created LAG seconds or more before
# (a refresh every hour knows his attempts up to an hour old).
import json, os, sys, collections, heapq
exec(open(os.path.dirname(os.path.abspath(__file__)) + "/ev.py").read().split('if __name__ == "__main__":')[0])
def run_lag(K, R, LAG):
    n = collections.Counter(); h = collections.Counter(); pend = []; out = []
    for s, i, m in coins:
        r = D[m]; bt = cr[m]["bt"]
        while pend and pend[0][0] <= bt:
            _, mm = heapq.heappop(pend)
            for w in BW[mm]["bw"]: h[w] += D[mm]["he"]
        ws = BW[m]["bw"]
        out.append((m, any(h[w] >= K and n[w] and h[w] / n[w] >= R for w in ws)))
        for w in ws: n[w] += 1
        heapq.heappush(pend, (bt + LAG, m))
    return out
if __name__ == "__main__":
    for LAG in (0, 600, 3600, 6 * 3600):
        res = score(run_lag(2, 0.5, LAG)); line = []
        for per in ("23b", "24a"):
            for c in ("out", "rule"):
                f = res[(per, c, True)]; nf = res[(per, c, False)]; tot = f["he"] + nf["he"]
                line.append(f"{per} {c}: fol {f['n']:4d} his {100*f['he']/max(1,f['n']):3.0f}% cover {100*f['he']/max(1,tot):3.0f}%")
        print(f"lag {LAG:6d}s: " + " | ".join(line))
