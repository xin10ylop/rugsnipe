# "He did not show up" as an exit signal: his attempts land (or fail) in the create slot or the next (his slot-window
# guard), and the bot sees them on the copied-wallet stream. On the newest bot's fires, the first spot's value path when
# he did not try vs when he tried: sell at +2/+3 (right after we know) vs hold to +6/+10.
import json, collections, statistics as st
A = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"; US = "4g49xGbgHZk3QYR1o6QSfDUbHmJZFwN6iDp6Vco1skff"
R = json.load(open("an_rows.json"))
def path(xs, k): return [x[f"v{k}"] for x in xs if x.get(f"v{k}") is not None]
for lab, f in (("<=40", lambda r: (r.get("pb") or 0) <= 40), (">40", lambda r: (r.get("pb") or 0) > 40)):
    fired = [r for m, r in R.items() if f(r) and (r.get("rep") or {}).get("fired") and r.get("v10") is not None]
    for who, g in (("he did not try", [r for r in fired if not (r.get("him") or {}).get("att")]), ("he tried", [r for r in fired if (r.get("him") or {}).get("att")])):
        print(f"{lab:4} {who:15} fires {len(g):5d}  " + "  ".join(f"v{k} {100*st.mean(path(g, k)):+5.1f}%/{100*st.median(path(g, k)):+5.1f}%" for k in (1, 2, 3, 6, 10)) +
              f"  dumps(v10<-25%) {sum(1 for r in g if r['v10'] < -0.25)}")
    # our live fills on these
    for who, g in (("he did not try", [r for r in fired if not (r.get("him") or {}).get("att")]), ("he tried", [r for r in fired if (r.get("him") or {}).get("att")])):
        ours = [r for r in g if (r.get("us") or {}).get("ok")]
        if ours:
            print(f"      our live fills, {who}: {len(ours)}  net {sum(r['us']['net'] for r in ours)/1e9:+.3f} SOL  v2 {100*st.mean(path(ours, 2)):+.1f}%  v10 {100*st.mean(path(ours, 10)):+.1f}%  "
                  f"v10-v2 median {100*st.median([a['v10'] - a['v2'] for a in ours if a.get('v2') is not None]):+.1f}")
