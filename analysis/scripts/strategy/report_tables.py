"""Print markdown tables for docs/research/strategy-sweep.md from the cached JSON results."""
import json
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parents[3]
OUT = REPO / "data" / "reports" / "strategy"


def pct(x, d=1):
    return "n/a" if x is None else f"{x*100:+.{d}f}%"


def ci(b):
    if not b or not b.get("n"):
        return "n/a"
    return f"{pct(b['ev'])} [{pct(b['lo'])}, {pct(b['hi'])}]"


def sweep():
    R = json.load(open(OUT / "sweep_results.json"))
    m = R["meta"]
    print("### meta\n", json.dumps({k: m[k] for k in ("configs_evaluated", "n_exit_policies", "bh_q10", "bh_q05", "min_p", "bonferroni_p", "share_configs_positive", "split_sizes")}, indent=0))
    print("\n### oracle\n| Entry | Triggers (of 675) | Launches/day | Hold to 900 s | Oracle: sell at max | Oracle: sell before biggest dump |\n|---|---|---|---|---|---|")
    for k, v in R["oracle"].items():
        print(f"| {k} | {v['n']} | {v['per_day']:,.0f} | {pct(v['hold900'])} | {pct(v['orc_max'])} | {pct(v['orc_dump'])} |")
    for nm, sel, ev in (("A_time", "train", "test"), ("B_evenodd", "even", "odd")):
        print(f"\n### {nm}\n| # | Entry | Filter | Exit | {sel} EV (n) | {ev} EV [90% CI] (n) | p | BH q (20) | deep holdout (n) | {ev} at +3.6% prem | {ev} at 0.01 SOL |\n|---|---|---|---|---|---|---|---|---|---|---|")
        for i, r in enumerate(R["sets"][nm], 1):
            b = r["base"]; d = r.get("deep") or {}
            st = r.get("stress_test", {})
            s36 = (st.get("S0.25_p0.036_c0.004_l1") or {}).get("test" if ev == "test" else "test")
            s01 = (st.get("S0.25_p0.011_c0.01_l1") or {}).get("test")
            print(f"| {i} | {r['entry']} | {r['filter']} | `{r['exit']}` | {pct(b[sel]['ev'])} ({b[sel]['n']}) | {ci(b[ev])} ({b[ev]['n']}) | {b[ev]['p']:.3f} | {r.get('q_set', float('nan')):.2f} | {pct(d.get('ev'))} ({d.get('n', 0)}) | {pct(s36) if nm=='A_time' else 'n/a'} | {pct(s01) if nm=='A_time' else 'n/a'} |")
    print("\n### C_testtop\n| # | Entry | Filter | Exit | Train EV (n) | Test EV [90% CI] (n) | p (normal) | BH q over all configs | Even / Odd | Deep holdout (n) |\n|---|---|---|---|---|---|---|---|---|---|")
    for i, r in enumerate(R["sets"]["C_testtop"], 1):
        b = r["base"]; d = r.get("deep") or {}
        print(f"| {i} | {r['entry']} | {r['filter']} | `{r['exit']}` | {pct(b['train']['ev'])} ({b['train']['n']}) | {ci(b['test'])} ({b['test']['n']}) | {r['p_test_normal']:.1e} | {r['q_test_allgrid']:.2f} | {pct(b['even']['ev'])} / {pct(b['odd']['ev'])} | {pct(d.get('ev'))} ({d.get('n', '-')}) |")
    print("\n### entry_best_train_all\n| Entry | Best train exit (filter all) | Train EV (n) | Test EV [90% CI] (n) |\n|---|---|---|---|")
    for r in R["sets"]["entry_best_train_all"]:
        b = r["base"]
        print(f"| {r['entry']} | `{r['exit']}` | {pct(b['train']['ev'])} ({b['train']['n']}) | {ci(b['test'])} ({b['test']['n']}) |")


def refine():
    R = json.load(open(OUT / "refine_results.json"))
    print("\n### refine sets", R["sets"], "configs", R["frozen"]["configs_searched"])
    base = "p0.011_c0.004_l1"
    print("| # | Size | Filter | Exit | Train | Gap-3 test | Odd days | Post-Oct-9 | Deep (Oct 12-14) | Fresh holdout (Oct 12-14) | Launches/day |\n|---|---|---|---|---|---|---|---|---|---|---|")
    for i, r in enumerate(R["per_policy"], 1):
        b = r["eval"][base]
        print(f"| {i} | {r['size']} | {r['filter']} | `{r['exit']}` | {pct(b['gap3_train']['ev'])} ({b['gap3_train']['n']}) | {ci(b['gap3_test'])} ({b['gap3_test']['n']}) | {ci(b['gap3_odd'])} | {ci(b['gap3_post'])} ({b['gap3_post']['n']}) | {ci(b['deep'])} ({b['deep']['n']}) | {ci(b['fresh'])} ({b['fresh']['n']}) | {b['per_day']:,.0f} |")
    print("\nhit/avg win/avg loss (fresh, gap3 test)")
    for i, r in enumerate(R["per_policy"], 1):
        b = r["eval"][base]
        for s in ("gap3_test", "fresh", "deep"):
            x = b[s]
            if x.get("n"):
                print(i, s, f"hit {x['hit']:.2f} win {pct(x['avg_win'])} loss {pct(x['avg_loss'])}")
    print("\n### stress (EV on gap-3 test / deep / fresh)\n| # | Scenario | Gap-3 test | Deep | Fresh |\n|---|---|---|---|---|")
    for i, r in enumerate(R["per_policy"], 1):
        for k, b in r["eval"].items():
            print(f"| {i} | {k} | {pct(b['gap3_test'].get('ev'))} | {pct(b['deep'].get('ev'))} | {pct(b['fresh'].get('ev'))} |")
    print("\n### BH over frozen x OOS sets")
    for x in R["bh"]:
        print(x)


if __name__ == "__main__":
    {"sweep": sweep, "refine": refine}[sys.argv[1]]()
