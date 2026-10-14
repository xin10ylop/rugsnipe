# On-chain backtest of the guru's rugger snipe (Task C)

Generated 2026-10-14T13:07:32Z by `analysis/scripts/backtest.py` from `data/reports/backtest_results.json`. Discovery: `analysis/scripts/discover_ruggers.py` (`data/rugger_clusters.json`, `data/reports/discovery_summary.json`).

## Verdict

**This sample shows no edge for the guru's snipe, and it gives no sign that picking ruggers would create one.**

> **Verification caveat (Task D, see the last section).** The window sample misses the hottest bundled launches: those with more than 200 trades in their first minute never had slot 0 read. Those launches carry most of the TP hits. Corrected for them, the guru's rule still loses, but about half as much: roughly −11% to −17% per snipe at a 13–16% TP-hit rate. Treat the numbers below as biased against the strategy. The sign of the verdict holds.

Dense window: every pump.fun create from 2026-10-14 02:05 to 08:05 UTC. That is 8,593 creates, of which 7,049 are SOL-quoted. 985 bundled launches were identified and 982 have a contiguous trade path from slot 0 (949 of them for the full 15 minutes). 50 of these completed their curve inside the creation slot. That leaves no curve to snipe, so they are skipped, which leaves 932 snipes.

**The guru's rule** is: buy 0.25 SOL at the end of the creation slot, take profit at +100%, no stop (floor), sell after 150 s without trades. On the 932 snipes it:
- hit TP on **6.0%** of snipes (95% CI 4.7–7.7%);
- **lost 0.063 SOL per snipe, or −25.1%** (CI −27% to −23%).

Task A's random arm (Oct 12–14, n=80 bundled) replicates this: −31%.

**Win/loss ratio.** The rule does not reach the guru's 3:1:
- On TP hits the trade makes about +99%.
- The median loss is about −34%, because the bundle has already lifted the entry to a median 41.8 SOL market cap, so the floor is far below.
- Wins vs. losses come out at 2.5:1. Wins include small idle-exit profits.
- TP hits make +99% and every other exit −33% on average. Break-even is therefore a **25% TP-hit rate**. The guru's "3 of 10" would make about +7% per snipe. The measured rate is 6%.

**Best implementable variants** (out of 172; tested filters, stops, trailing stops, big-buy exits, slot+1 entry, and size/cost):
- The best is "enter only if the end-of-slot-0 market cap is ≤ 35 SOL": n=131, −9.3% per snipe, CI −14% to −5%.
- Every variant with n ≥ 30 has a negative expectancy.
- Choosing the best variant on the first half of the window and testing it on the second half gives −11.0% (CI −17% to −5%).

**Even perfect foresight barely pays.**
- Selling every snipe at its best price within 15 minutes (oracle) makes only +23% per snipe.
- Selling just before the biggest dump slot makes +4%.
- Both bounds are too low, because the censored hot launches are missing (see Verification).
- The money is in the few launches that run, and the entry gives no information about which ones those are.

**Competition makes it worse.**
- Filling 5%, 15% or 30% above the post-bundle price takes the base rule to −29%, −36% and −44%.
- Gap-2 measured that the 3rd reactive buyer pays a median +10.8%.

**Ruggers are real and identifiable, but in this sample they are not better than average.**
- 396 clusters of ≥ 3 launches. Signals used: same signer (d), same mother wallet + amount (a), shared bundle wallets (c), and identical bundle shape (b).
- 60 clusters have ≥ 5 snipes (853 snipes in total). Their pooled TP hit rate is 6.3%.
- Exactly **1** cluster reaches the guru's 33%. The expected number from chance alone is **1.0**.
- 5 clusters have a binomial p < 0.05, against 3 expected by chance.
- These tests lack the censored hot launches, so they say little.
- Walk-forward "follow the good rugger" (snipe a cluster's launch only after ≥ 2/3/5 earlier snipes closed with ≥ 33% TP hits) traded 15–35 launches. It hit 9–13% and lost 23–27% per snipe.

**What would change the verdict** (see the end of this page): re-pulling the censored hot launches; a multi-day dense window, so that operators have ≥ 10 earlier launches; live paper-trading fills at our real landing position; and a selection signal that is visible at entry and moves the TP hit rate above ~25%. In this sample the hit rate is ~6%, or ~15% after the censoring correction.


## Samples

| sample | launches with a contiguous trade path | bundled (tradable) |
|---|---|---|
| window | 982 | 982 |
| clusters | 960 | 960 |
| taskA | 362 | 80 |

Look-ahead guard: 6912 entry decisions re-run on a truncated and on a scrambled future; mismatches: 0.

## Assumptions

- Entry state = curve after the last recorded trade of the creation slot (eos0) or of slot+1 (eos1); the entry decision uses slot-0 trades only.
- Our buy is overlaid on the recorded path: impact from the constant-product curve, our SOL stays in the curve while we hold (recorded state shifted by our net SOL).
- Fees 1.25 % per side (0.95 % protocol + 0.30 % creator, as observed on 2026 TradeEvents) instead of the 1 % in the brief; a fixed cost (priority fee + tip) is paid on the buy AND on the sell.
- Exit triggers are evaluated on each recorded post-trade price; the sell executes after all recorded trades of the next slot (exit latency 1 slot) unless stated.
- Natural SL = no stop: the position is held until TP, 150 s without trades (idle), or the 900 s horizon.
- Competition is not simulated except through the entry_premium variants (fill 5/15/30 % above the post-bundle price, cf. gap-2: 3rd reactive buyer pays a median +10.8 %, 5th +15.8 %). Failed attempts (22 % slippage failures in slot 0) are not charged.
- Bundled = >= 1 non-dev wallet in the contiguous run of txs right after the create tx (or inside it) and >= 1 SOL of dev + bundle buys in slot 0.

## Sample `window`

Gain potential (no costs): 55/982 bundled launches (5.6%, CI 4.3%-7.2%) traded at >= 2x the end-of-slot-0 price within the horizon.

Guru base rule and the best variants (ranked by expectancy, n >= min-n):

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 932 | 6.0% | +0.2147 | -0.0845 | 2.54 | -0.0627 (-25.1%) | [-0.0682, -0.0571] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.1725 | -0.0394 | 4.371 | -0.0233 (-9.3%) | [-0.0350, -0.0115] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.1890 | -0.0450 | 4.196 | -0.0300 (-12.0%) | [-0.0414, -0.0186] |
| eos0 0.25SOL c0.002 TP100 SL50 idle150 B<=3 | 140 | 4.3% | +0.1890 | -0.0450 | 4.196 | -0.0300 (-12.0%) | [-0.0414, -0.0186] |
| eos0 0.25SOL c0.002 TP150 SLfloor idle150 B<=3 | 140 | 2.9% | +0.2025 | -0.0451 | 4.493 | -0.0309 (-12.4%) | [-0.0427, -0.0192] |
| eos0 0.25SOL c0.002 TP150 SL50 idle150 B<=3 | 140 | 2.9% | +0.2025 | -0.0451 | 4.493 | -0.0309 (-12.4%) | [-0.0427, -0.0192] |
| eos0 0.25SOL c0.002 TP100 SL30 idle150 B<=3 | 140 | 3.6% | +0.1792 | -0.0449 | 3.993 | -0.0321 (-12.8%) | [-0.0427, -0.0215] |
| eos0 0.25SOL c0.002 TP150 SL30 idle150 B<=3 | 140 | 2.9% | +0.1950 | -0.0449 | 4.345 | -0.0329 (-13.2%) | [-0.0439, -0.0219] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=3 | 140 | 4.3% | +0.2407 | -0.0478 | 5.037 | -0.0334 (-13.3%) | [-0.0448, -0.0219] |
| eos0 0.25SOL c0.002 TP100 SL50 B<=3 | 140 | 4.3% | +0.2407 | -0.0478 | 5.037 | -0.0334 (-13.3%) | [-0.0448, -0.0219] |
| eos0 0.25SOL c0.002 TP200 SLfloor idle150 B<=3 | 140 | 1.4% | +0.1771 | -0.0447 | 3.959 | -0.0336 (-13.4%) | [-0.0452, -0.0220] |

Upper bounds with perfect foresight (not implementable): `oracle:max` sells at the best price seen in the horizon, `oracle:pre_dump` sells right before the slot with the largest sell volume.

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 oracle:max | 932 | 0.0% | +0.0974 | -0.0086 | 11.362 | +0.0587 (+23.5%) | [+0.0502, +0.0671] |
| eos1 0.25SOL c0.002 oracle:max | 932 | 0.0% | +0.0959 | -0.0091 | 10.556 | +0.0512 (+20.5%) | [+0.0429, +0.0594] |
| eos0 0.25SOL c0.002 oracle:pre_dump | 932 | 0.0% | +0.0721 | -0.0264 | 2.727 | +0.0110 (+4.4%) | [+0.0046, +0.0174] |
| eos1 0.25SOL c0.002 oracle:pre_dump | 932 | 0.0% | +0.0775 | -0.0273 | 2.842 | +0.0051 (+2.0%) | [-0.0011, +0.0113] |

Out-of-sample check: best variant on the first half in time (n=491 launches) was `eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35` with expectancy -0.0183 SOL; on the second half (n=491) it made -0.0275 SOL per snipe (CI [-0.04142, -0.01361], 71 trades, hit 4.2%).

Size / cost sensitivity (base and top variants):

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 | 932 | 6.0% | +0.0882 | -0.0362 | 2.435 | -0.0275 (-27.5%) | [-0.0298, -0.0253] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 | 932 | 6.0% | +0.0787 | -0.0520 | 1.515 | -0.0435 (-43.5%) | [-0.0458, -0.0413] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 932 | 6.0% | +0.2147 | -0.0845 | 2.54 | -0.0627 (-25.1%) | [-0.0682, -0.0571] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 | 932 | 6.0% | +0.2117 | -0.1001 | 2.115 | -0.0787 (-31.5%) | [-0.0842, -0.0732] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 | 932 | 6.1% | +0.4251 | -0.1638 | 2.595 | -0.1208 (-24.2%) | [-0.1317, -0.1100] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 | 932 | 6.1% | +0.4282 | -0.1792 | 2.39 | -0.1368 (-27.4%) | [-0.1477, -0.1260] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 | 932 | 6.2% | +0.8292 | -0.3192 | 2.598 | -0.2342 (-23.4%) | [-0.2556, -0.2128] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 | 932 | 6.2% | +0.8132 | -0.3352 | 2.426 | -0.2502 (-25.0%) | [-0.2716, -0.2288] |
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.0844 | -0.0180 | 4.693 | -0.0117 (-11.7%) | [-0.0165, -0.0070] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.0949 | -0.0336 | 2.823 | -0.0277 (-27.7%) | [-0.0325, -0.0230] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.1725 | -0.0394 | 4.371 | -0.0233 (-9.3%) | [-0.0350, -0.0115] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.2273 | -0.0543 | 4.185 | -0.0393 (-15.7%) | [-0.0510, -0.0275] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.3434 | -0.0743 | 4.623 | -0.0424 (-8.5%) | [-0.0656, -0.0192] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.4108 | -0.0889 | 4.62 | -0.0584 (-11.7%) | [-0.0816, -0.0352] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 mc<=35 | 131 | 5.3% | +0.6827 | -0.1425 | 4.792 | -0.0732 (-7.3%) | [-0.1202, -0.0262] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 mc<=35 | 131 | 5.3% | +0.6667 | -0.1585 | 4.207 | -0.0892 (-8.9%) | [-0.1362, -0.0422] |
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.0954 | -0.0202 | 4.719 | -0.0144 (-14.4%) | [-0.0191, -0.0098] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.0947 | -0.0360 | 2.627 | -0.0304 (-30.4%) | [-0.0350, -0.0258] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.1890 | -0.0450 | 4.196 | -0.0300 (-12.0%) | [-0.0414, -0.0186] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.2641 | -0.0599 | 4.409 | -0.0460 (-18.4%) | [-0.0574, -0.0346] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.3762 | -0.0854 | 4.404 | -0.0557 (-11.2%) | [-0.0783, -0.0332] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.4649 | -0.1000 | 4.65 | -0.0717 (-14.3%) | [-0.0943, -0.0492] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 B<=3 | 140 | 5.0% | +0.7416 | -0.1645 | 4.507 | -0.0998 (-10.0%) | [-0.1455, -0.0541] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 B<=3 | 140 | 5.0% | +0.7256 | -0.1805 | 4.019 | -0.1158 (-11.6%) | [-0.1615, -0.0701] |
| eos0 0.1SOL c0.002 TP100 SL50 idle150 B<=3 | 140 | 4.3% | +0.0954 | -0.0202 | 4.719 | -0.0144 (-14.4%) | [-0.0191, -0.0098] |
| eos0 0.1SOL c0.01 TP100 SL50 idle150 B<=3 | 140 | 4.3% | +0.0947 | -0.0360 | 2.627 | -0.0304 (-30.4%) | [-0.0350, -0.0258] |
| eos0 0.25SOL c0.002 TP100 SL50 idle150 B<=3 | 140 | 4.3% | +0.1890 | -0.0450 | 4.196 | -0.0300 (-12.0%) | [-0.0414, -0.0186] |
| eos0 0.25SOL c0.01 TP100 SL50 idle150 B<=3 | 140 | 4.3% | +0.2641 | -0.0599 | 4.409 | -0.0460 (-18.4%) | [-0.0574, -0.0346] |
| eos0 0.5SOL c0.002 TP100 SL50 idle150 B<=3 | 140 | 4.3% | +0.3762 | -0.0854 | 4.404 | -0.0557 (-11.2%) | [-0.0783, -0.0332] |
| eos0 0.5SOL c0.01 TP100 SL50 idle150 B<=3 | 140 | 4.3% | +0.4649 | -0.1000 | 4.65 | -0.0717 (-14.3%) | [-0.0943, -0.0492] |
| eos0 1SOL c0.002 TP100 SL50 idle150 B<=3 | 140 | 5.0% | +0.7416 | -0.1645 | 4.507 | -0.0998 (-10.0%) | [-0.1455, -0.0541] |
| eos0 1SOL c0.01 TP100 SL50 idle150 B<=3 | 140 | 5.0% | +0.7256 | -0.1805 | 4.019 | -0.1158 (-11.6%) | [-0.1615, -0.0701] |

Selected exits and filters:

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=3 | 140 | 4.3% | +0.1890 | -0.0450 | 4.196 | -0.0300 (-12.0%) | [-0.0414, -0.0186] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=5 | 392 | 6.4% | +0.2157 | -0.0621 | 3.475 | -0.0401 (-16.0%) | [-0.0482, -0.0320] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=10 | 848 | 5.1% | +0.2117 | -0.0800 | 2.645 | -0.0615 (-24.6%) | [-0.0668, -0.0562] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=3 | 140 | 4.3% | +0.2407 | -0.0478 | 5.037 | -0.0334 (-13.3%) | [-0.0448, -0.0219] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=5 | 392 | 6.4% | +0.2375 | -0.0647 | 3.672 | -0.0431 (-17.2%) | [-0.0512, -0.0350] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=10 | 848 | 5.1% | +0.2238 | -0.0823 | 2.72 | -0.0639 (-25.5%) | [-0.0692, -0.0586] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail30/20 | 932 | 0.0% | +0.0651 | -0.0787 | 0.827 | -0.0489 (-19.6%) | [-0.0537, -0.0441] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail30/20 | 932 | 2.8% | +0.0716 | -0.0788 | 0.91 | -0.0473 (-18.9%) | [-0.0523, -0.0423] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/30 | 932 | 0.0% | +0.0969 | -0.0821 | 1.18 | -0.0598 (-23.9%) | [-0.0653, -0.0543] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/30 | 932 | 5.4% | +0.1246 | -0.0822 | 1.516 | -0.0558 (-22.3%) | [-0.0612, -0.0503] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/20 | 932 | 0.0% | +0.0966 | -0.0838 | 1.152 | -0.0566 (-22.6%) | [-0.0616, -0.0515] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/20 | 932 | 3.5% | +0.1062 | -0.0840 | 1.265 | -0.0548 (-21.9%) | [-0.0600, -0.0496] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail100/30 | 932 | 0.0% | +0.1694 | -0.0844 | 2.008 | -0.0667 (-26.7%) | [-0.0723, -0.0611] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail100/30 | 932 | 6.0% | +0.2147 | -0.0845 | 2.54 | -0.0627 (-25.1%) | [-0.0682, -0.0571] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 932 | 2.5% | +0.0865 | -0.0757 | 1.142 | -0.0423 (-16.9%) | [-0.0475, -0.0372] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy5 | 932 | 5.3% | +0.1848 | -0.0834 | 2.215 | -0.0610 (-24.4%) | [-0.0664, -0.0556] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy10 | 932 | 6.0% | +0.2128 | -0.0845 | 2.519 | -0.0625 (-25.0%) | [-0.0680, -0.0569] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 lat0 | 932 | 6.0% | +0.2142 | -0.0845 | 2.535 | -0.0627 (-25.1%) | [-0.0682, -0.0572] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 lat2 | 932 | 6.0% | +0.2130 | -0.0845 | 2.52 | -0.0628 (-25.1%) | [-0.0683, -0.0573] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem5 | 932 | 5.0% | +0.2183 | -0.0922 | 2.368 | -0.0729 (-29.1%) | [-0.0782, -0.0676] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem15 | 932 | 4.2% | +0.2188 | -0.1057 | 2.07 | -0.0893 (-35.7%) | [-0.0943, -0.0843] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem30 | 932 | 2.7% | +0.2095 | -0.1221 | 1.715 | -0.1108 (-44.3%) | [-0.1150, -0.1065] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 nomayhem | 931 | 6.0% | +0.2147 | -0.0843 | 2.545 | -0.0625 (-25.0%) | [-0.0680, -0.0570] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35 | 131 | 4.6% | +0.1725 | -0.0394 | 4.371 | -0.0233 (-9.3%) | [-0.0350, -0.0115] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=45 | 647 | 5.1% | +0.2147 | -0.0704 | 3.049 | -0.0524 (-20.9%) | [-0.0582, -0.0466] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=60 | 887 | 6.0% | +0.2224 | -0.0813 | 2.737 | -0.0600 (-24.0%) | [-0.0655, -0.0545] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=140 | 928 | 5.9% | +0.2143 | -0.0841 | 2.549 | -0.0625 (-25.0%) | [-0.0680, -0.0570] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=3 | 793 | 6.3% | +0.2185 | -0.0915 | 2.389 | -0.0684 (-27.4%) | [-0.0745, -0.0623] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=5 | 544 | 5.7% | +0.2137 | -0.1004 | 2.129 | -0.0790 (-31.6%) | [-0.0862, -0.0719] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=10 | 87 | 14.9% | +0.2259 | -0.1340 | 1.686 | -0.0761 (-30.4%) | [-0.1053, -0.0469] |

## Sample `clusters`

Gain potential (no costs): 58/960 bundled launches (6.0%, CI 4.7%-7.7%) traded at >= 2x the end-of-slot-0 price within the horizon.

Guru base rule and the best variants (ranked by expectancy, n >= min-n):

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 939 | 6.3% | +0.2305 | -0.0848 | 2.717 | -0.0624 (-24.9%) | [-0.0679, -0.0568] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.2178 | -0.0408 | 5.331 | -0.0204 (-8.2%) | [-0.0359, -0.0048] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.2355 | -0.0495 | 4.755 | -0.0297 (-11.9%) | [-0.0446, -0.0148] |
| eos0 0.25SOL c0.002 TP100 SL50 idle150 B<=3 | 115 | 6.1% | +0.2355 | -0.0495 | 4.755 | -0.0297 (-11.9%) | [-0.0446, -0.0148] |
| eos0 0.25SOL c0.002 TP100 SL30 idle150 B<=3 | 115 | 5.2% | +0.2309 | -0.0485 | 4.758 | -0.0315 (-12.6%) | [-0.0453, -0.0178] |
| eos0 0.25SOL c0.002 TP150 SLfloor idle150 B<=3 | 115 | 3.5% | +0.2729 | -0.0496 | 5.497 | -0.0328 (-13.1%) | [-0.0477, -0.0179] |
| eos0 0.25SOL c0.002 TP150 SL50 idle150 B<=3 | 115 | 3.5% | +0.2729 | -0.0496 | 5.497 | -0.0328 (-13.1%) | [-0.0477, -0.0179] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=3 | 115 | 6.1% | +0.2690 | -0.0525 | 5.123 | -0.0329 (-13.2%) | [-0.0478, -0.0181] |
| eos0 0.25SOL c0.002 TP100 SL50 B<=3 | 115 | 6.1% | +0.2690 | -0.0525 | 5.123 | -0.0329 (-13.2%) | [-0.0478, -0.0181] |
| eos0 0.25SOL c0.002 TP150 SL30 idle150 B<=3 | 115 | 3.5% | +0.2765 | -0.0487 | 5.683 | -0.0345 (-13.8%) | [-0.0483, -0.0207] |
| eos0 0.25SOL c0.002 TP100 SL30 B<=3 | 115 | 5.2% | +0.2691 | -0.0515 | 5.228 | -0.0348 (-13.9%) | [-0.0485, -0.0210] |

Upper bounds with perfect foresight (not implementable): `oracle:max` sells at the best price seen in the horizon, `oracle:pre_dump` sells right before the slot with the largest sell volume.

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 oracle:max | 939 | 0.0% | +0.0996 | -0.0085 | 11.748 | +0.0649 (+26.0%) | [+0.0563, +0.0736] |
| eos1 0.25SOL c0.002 oracle:max | 939 | 0.0% | +0.0955 | -0.0090 | 10.639 | +0.0557 (+22.3%) | [+0.0475, +0.0638] |
| eos0 0.25SOL c0.002 oracle:pre_dump | 939 | 0.0% | +0.0767 | -0.0293 | 2.615 | +0.0138 (+5.5%) | [+0.0072, +0.0205] |
| eos1 0.25SOL c0.002 oracle:pre_dump | 939 | 0.0% | +0.0784 | -0.0309 | 2.541 | +0.0066 (+2.6%) | [+0.0002, +0.0130] |

Out-of-sample check: best variant on the first half in time (n=480 launches) was `eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35` with expectancy -0.0215 SOL; on the second half (n=480) it made -0.0190 SOL per snipe (CI [-0.0405, 0.00259], 45 trades, hit 6.7%).

Size / cost sensitivity (base and top variants):

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 | 939 | 6.3% | +0.0921 | -0.0365 | 2.528 | -0.0274 (-27.4%) | [-0.0297, -0.0252] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 | 939 | 6.3% | +0.0813 | -0.0522 | 1.557 | -0.0434 (-43.4%) | [-0.0457, -0.0412] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 939 | 6.3% | +0.2305 | -0.0848 | 2.717 | -0.0624 (-24.9%) | [-0.0679, -0.0568] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 | 939 | 6.3% | +0.2180 | -0.1008 | 2.164 | -0.0784 (-31.3%) | [-0.0839, -0.0728] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 | 939 | 6.4% | +0.4564 | -0.1645 | 2.775 | -0.1202 (-24.0%) | [-0.1312, -0.1092] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 | 939 | 6.4% | +0.4472 | -0.1803 | 2.48 | -0.1362 (-27.2%) | [-0.1472, -0.1252] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 | 939 | 6.5% | +0.8889 | -0.3205 | 2.774 | -0.2329 (-23.3%) | [-0.2545, -0.2113] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 | 939 | 6.5% | +0.8729 | -0.3365 | 2.594 | -0.2489 (-24.9%) | [-0.2705, -0.2273] |
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.0980 | -0.0186 | 5.261 | -0.0106 (-10.6%) | [-0.0168, -0.0043] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.0974 | -0.0344 | 2.833 | -0.0266 (-26.6%) | [-0.0328, -0.0203] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.2178 | -0.0408 | 5.331 | -0.0204 (-8.2%) | [-0.0359, -0.0048] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.2327 | -0.0564 | 4.125 | -0.0364 (-14.6%) | [-0.0519, -0.0208] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.4326 | -0.0771 | 5.614 | -0.0367 (-7.3%) | [-0.0673, -0.0061] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.4774 | -0.0922 | 5.18 | -0.0527 (-10.5%) | [-0.0833, -0.0221] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 mc<=35 | 101 | 6.9% | +0.8392 | -0.1481 | 5.668 | -0.0601 (-6.0%) | [-0.1219, +0.0018] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 mc<=35 | 101 | 6.9% | +0.8232 | -0.1641 | 5.018 | -0.0761 (-7.6%) | [-0.1379, -0.0142] |
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.1062 | -0.0221 | 4.803 | -0.0143 (-14.3%) | [-0.0203, -0.0083] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.0902 | -0.0381 | 2.367 | -0.0303 (-30.3%) | [-0.0363, -0.0243] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.2355 | -0.0495 | 4.755 | -0.0297 (-11.9%) | [-0.0446, -0.0148] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.2530 | -0.0651 | 3.888 | -0.0457 (-18.3%) | [-0.0606, -0.0308] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.4677 | -0.0943 | 4.96 | -0.0552 (-11.0%) | [-0.0846, -0.0258] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.5176 | -0.1094 | 4.732 | -0.0712 (-14.2%) | [-0.1006, -0.0418] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 B<=3 | 115 | 7.0% | +0.9005 | -0.1824 | 4.937 | -0.0976 (-9.8%) | [-0.1569, -0.0384] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 B<=3 | 115 | 7.0% | +0.8845 | -0.1984 | 4.459 | -0.1136 (-11.4%) | [-0.1729, -0.0544] |
| eos0 0.1SOL c0.002 TP100 SL50 idle150 B<=3 | 115 | 6.1% | +0.1062 | -0.0221 | 4.803 | -0.0143 (-14.3%) | [-0.0203, -0.0083] |
| eos0 0.1SOL c0.01 TP100 SL50 idle150 B<=3 | 115 | 6.1% | +0.0902 | -0.0381 | 2.367 | -0.0303 (-30.3%) | [-0.0363, -0.0243] |
| eos0 0.25SOL c0.002 TP100 SL50 idle150 B<=3 | 115 | 6.1% | +0.2355 | -0.0495 | 4.755 | -0.0297 (-11.9%) | [-0.0446, -0.0148] |
| eos0 0.25SOL c0.01 TP100 SL50 idle150 B<=3 | 115 | 6.1% | +0.2530 | -0.0651 | 3.888 | -0.0457 (-18.3%) | [-0.0606, -0.0308] |
| eos0 0.5SOL c0.002 TP100 SL50 idle150 B<=3 | 115 | 6.1% | +0.4677 | -0.0943 | 4.96 | -0.0552 (-11.0%) | [-0.0846, -0.0258] |
| eos0 0.5SOL c0.01 TP100 SL50 idle150 B<=3 | 115 | 6.1% | +0.5176 | -0.1094 | 4.732 | -0.0712 (-14.2%) | [-0.1006, -0.0418] |
| eos0 1SOL c0.002 TP100 SL50 idle150 B<=3 | 115 | 7.0% | +0.9005 | -0.1824 | 4.937 | -0.0976 (-9.8%) | [-0.1569, -0.0384] |
| eos0 1SOL c0.01 TP100 SL50 idle150 B<=3 | 115 | 7.0% | +0.8845 | -0.1984 | 4.459 | -0.1136 (-11.4%) | [-0.1729, -0.0544] |

Selected exits and filters:

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=3 | 115 | 6.1% | +0.2355 | -0.0495 | 4.755 | -0.0297 (-11.9%) | [-0.0446, -0.0148] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=5 | 420 | 7.4% | +0.2313 | -0.0679 | 3.403 | -0.0430 (-17.2%) | [-0.0515, -0.0345] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=10 | 879 | 5.8% | +0.2271 | -0.0823 | 2.761 | -0.0615 (-24.6%) | [-0.0670, -0.0560] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=3 | 115 | 6.1% | +0.2690 | -0.0525 | 5.123 | -0.0329 (-13.2%) | [-0.0478, -0.0181] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=5 | 420 | 7.4% | +0.2446 | -0.0705 | 3.471 | -0.0457 (-18.3%) | [-0.0542, -0.0372] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=10 | 879 | 5.8% | +0.2350 | -0.0845 | 2.78 | -0.0638 (-25.5%) | [-0.0693, -0.0583] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail30/20 | 939 | 0.0% | +0.0638 | -0.0785 | 0.813 | -0.0457 (-18.3%) | [-0.0505, -0.0409] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail30/20 | 939 | 3.0% | +0.0705 | -0.0785 | 0.898 | -0.0441 (-17.6%) | [-0.0491, -0.0390] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/30 | 939 | 0.0% | +0.0919 | -0.0828 | 1.111 | -0.0595 (-23.8%) | [-0.0649, -0.0541] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/30 | 939 | 5.4% | +0.1233 | -0.0829 | 1.488 | -0.0550 (-22.0%) | [-0.0605, -0.0495] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/20 | 939 | 0.0% | +0.0932 | -0.0847 | 1.101 | -0.0561 (-22.4%) | [-0.0610, -0.0511] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/20 | 939 | 3.6% | +0.1051 | -0.0848 | 1.241 | -0.0540 (-21.6%) | [-0.0593, -0.0488] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail100/30 | 939 | 0.0% | +0.1864 | -0.0848 | 2.199 | -0.0660 (-26.4%) | [-0.0718, -0.0602] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail100/30 | 939 | 6.3% | +0.2305 | -0.0848 | 2.717 | -0.0624 (-24.9%) | [-0.0679, -0.0568] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 939 | 2.2% | +0.0895 | -0.0801 | 1.117 | -0.0424 (-16.9%) | [-0.0477, -0.0370] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy5 | 939 | 5.6% | +0.1983 | -0.0843 | 2.353 | -0.0605 (-24.2%) | [-0.0660, -0.0550] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy10 | 939 | 6.3% | +0.2305 | -0.0848 | 2.717 | -0.0624 (-24.9%) | [-0.0679, -0.0568] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 lat0 | 939 | 6.3% | +0.2296 | -0.0848 | 2.706 | -0.0624 (-25.0%) | [-0.0680, -0.0568] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 lat2 | 939 | 6.3% | +0.2311 | -0.0848 | 2.724 | -0.0623 (-24.9%) | [-0.0679, -0.0567] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem5 | 939 | 5.3% | +0.2291 | -0.0928 | 2.469 | -0.0726 (-29.0%) | [-0.0779, -0.0672] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem15 | 939 | 4.2% | +0.2217 | -0.1062 | 2.087 | -0.0902 (-36.1%) | [-0.0950, -0.0853] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem30 | 939 | 2.7% | +0.2148 | -0.1229 | 1.748 | -0.1117 (-44.7%) | [-0.1159, -0.1076] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 nomayhem | 939 | 6.3% | +0.2305 | -0.0848 | 2.717 | -0.0624 (-24.9%) | [-0.0679, -0.0568] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35 | 101 | 5.9% | +0.2178 | -0.0408 | 5.331 | -0.0204 (-8.2%) | [-0.0359, -0.0048] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=45 | 639 | 5.8% | +0.2303 | -0.0719 | 3.204 | -0.0520 (-20.8%) | [-0.0582, -0.0459] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=60 | 910 | 6.3% | +0.2327 | -0.0830 | 2.803 | -0.0608 (-24.3%) | [-0.0664, -0.0552] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=140 | 939 | 6.3% | +0.2305 | -0.0848 | 2.717 | -0.0624 (-24.9%) | [-0.0679, -0.0568] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=3 | 824 | 6.3% | +0.2298 | -0.0898 | 2.56 | -0.0669 (-26.8%) | [-0.0729, -0.0610] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=5 | 523 | 5.3% | +0.2297 | -0.0980 | 2.344 | -0.0780 (-31.2%) | [-0.0851, -0.0709] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=10 | 61 | 13.1% | +0.2554 | -0.1251 | 2.041 | -0.0752 (-30.1%) | [-0.1084, -0.0421] |

## Sample `taskA`

Gain potential (no costs): 5/80 bundled launches (6.2%, CI 2.7%-13.8%) traded at >= 2x the end-of-slot-0 price within the horizon.

Guru base rule and the best variants (ranked by expectancy, n >= min-n):

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 80 | 6.2% | +0.2674 | -0.1009 | 2.65 | -0.0779 (-31.2%) | [-0.0992, -0.0566] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.0751 | -0.0920 | 0.817 | -0.0565 (-22.6%) | [-0.0744, -0.0386] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.0640 | -0.0974 | 0.657 | -0.0590 (-23.6%) | [-0.0777, -0.0403] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.0592 | -0.0974 | 0.608 | -0.0602 (-24.1%) | [-0.0780, -0.0423] |
| eos0 0.25SOL c0.002 TP100 SL30 idle150 B<=10 | 50 | 4.0% | +0.2856 | -0.0750 | 3.81 | -0.0605 (-24.2%) | [-0.0811, -0.0399] |
| eos0 0.25SOL c0.002 TP100 SL30 B<=10 | 50 | 4.0% | +0.2856 | -0.0754 | 3.789 | -0.0609 (-24.4%) | [-0.0815, -0.0403] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=45 | 38 | 2.6% | +0.2836 | -0.0728 | 3.895 | -0.0634 (-25.4%) | [-0.0830, -0.0439] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=10 | 50 | 4.0% | +0.2856 | -0.0812 | 3.517 | -0.0665 (-26.6%) | [-0.0877, -0.0453] |
| eos0 0.25SOL c0.002 TP100 SL50 idle150 B<=10 | 50 | 4.0% | +0.2856 | -0.0812 | 3.517 | -0.0665 (-26.6%) | [-0.0877, -0.0453] |
| eos0 0.25SOL c0.002 TP150 SL30 idle150 B<=10 | 50 | 2.0% | +0.3539 | -0.0751 | 4.711 | -0.0665 (-26.6%) | [-0.0843, -0.0488] |
| eos0 0.25SOL c0.002 TP150 SL30 B<=10 | 50 | 2.0% | +0.3539 | -0.0755 | 4.685 | -0.0669 (-26.8%) | [-0.0847, -0.0492] |

Upper bounds with perfect foresight (not implementable): `oracle:max` sells at the best price seen in the horizon, `oracle:pre_dump` sells right before the slot with the largest sell volume.

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 oracle:max | 80 | 0.0% | +0.1217 | -0.0085 | 14.352 | +0.0680 (+27.2%) | [+0.0352, +0.1008] |
| eos1 0.25SOL c0.002 oracle:max | 80 | 0.0% | +0.1154 | -0.0091 | 12.693 | +0.0563 (+22.5%) | [+0.0244, +0.0881] |
| eos0 0.25SOL c0.002 oracle:pre_dump | 80 | 0.0% | +0.0978 | -0.0306 | 3.196 | +0.0079 (+3.2%) | [-0.0093, +0.0251] |
| eos1 0.25SOL c0.002 oracle:pre_dump | 80 | 0.0% | +0.0880 | -0.0326 | 2.7 | -0.0024 (-1.0%) | [-0.0173, +0.0124] |

Out-of-sample check: best variant on the first half in time (n=181 launches) was `eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail30/20` with expectancy -0.0450 SOL; on the second half (n=181) it made -0.0717 SOL per snipe (CI [-0.09231, -0.05116], 42 trades, hit 0.0%).

Size / cost sensitivity (base and top variants):

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 | 80 | 6.2% | +0.1053 | -0.0429 | 2.454 | -0.0336 (-33.6%) | [-0.0422, -0.0251] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 | 80 | 6.2% | +0.0893 | -0.0589 | 1.516 | -0.0496 (-49.6%) | [-0.0582, -0.0411] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 80 | 6.2% | +0.2674 | -0.1009 | 2.65 | -0.0779 (-31.2%) | [-0.0992, -0.0566] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 | 80 | 6.2% | +0.2514 | -0.1169 | 2.15 | -0.0939 (-37.6%) | [-0.1152, -0.0726] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 | 80 | 6.2% | +0.5324 | -0.1965 | 2.709 | -0.1510 (-30.2%) | [-0.1933, -0.1087] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 | 80 | 6.2% | +0.5164 | -0.2125 | 2.43 | -0.1670 (-33.4%) | [-0.2092, -0.1247] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 | 80 | 6.2% | +1.0442 | -0.3841 | 2.719 | -0.2948 (-29.5%) | [-0.3778, -0.2118] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 | 80 | 6.2% | +1.0282 | -0.4001 | 2.57 | -0.3108 (-31.1%) | [-0.3938, -0.2278] |
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.0297 | -0.0387 | 0.767 | -0.0251 (-25.1%) | [-0.0323, -0.0179] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.0199 | -0.0518 | 0.384 | -0.0411 (-41.1%) | [-0.0483, -0.0338] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.0751 | -0.0920 | 0.817 | -0.0565 (-22.6%) | [-0.0744, -0.0386] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.0637 | -0.1065 | 0.598 | -0.0725 (-29.0%) | [-0.0904, -0.0546] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.1525 | -0.1788 | 0.853 | -0.1084 (-21.7%) | [-0.1439, -0.0728] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.1454 | -0.1918 | 0.758 | -0.1244 (-24.9%) | [-0.1599, -0.0888] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.3019 | -0.3488 | 0.866 | -0.2105 (-21.1%) | [-0.2803, -0.1407] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.2859 | -0.3648 | 0.784 | -0.2265 (-22.6%) | [-0.2964, -0.1567] |
| eos0 0.1SOL c0.002 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.0234 | -0.0415 | 0.564 | -0.0261 (-26.1%) | [-0.0336, -0.0186] |
| eos0 0.1SOL c0.01 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.0372 | -0.0497 | 0.749 | -0.0421 (-42.1%) | [-0.0496, -0.0346] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.0640 | -0.0974 | 0.657 | -0.0590 (-23.6%) | [-0.0777, -0.0403] |
| eos0 0.25SOL c0.01 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.0671 | -0.1052 | 0.638 | -0.0751 (-30.0%) | [-0.0937, -0.0563] |
| eos0 0.5SOL c0.002 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.1305 | -0.1895 | 0.688 | -0.1135 (-22.7%) | [-0.1506, -0.0764] |
| eos0 0.5SOL c0.01 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.1145 | -0.2055 | 0.557 | -0.1295 (-25.9%) | [-0.1666, -0.0924] |
| eos0 1SOL c0.002 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.2557 | -0.3701 | 0.691 | -0.2215 (-22.1%) | [-0.2942, -0.1488] |
| eos0 1SOL c0.01 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.2397 | -0.3861 | 0.621 | -0.2375 (-23.8%) | [-0.3102, -0.1648] |
| eos0 0.1SOL c0.002 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.0215 | -0.0415 | 0.517 | -0.0266 (-26.6%) | [-0.0337, -0.0194] |
| eos0 0.1SOL c0.01 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.0320 | -0.0497 | 0.643 | -0.0425 (-42.5%) | [-0.0497, -0.0354] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.0592 | -0.0974 | 0.608 | -0.0602 (-24.1%) | [-0.0780, -0.0423] |
| eos0 0.25SOL c0.01 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.0605 | -0.1052 | 0.576 | -0.0762 (-30.5%) | [-0.0940, -0.0583] |
| eos0 0.5SOL c0.002 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.1210 | -0.1895 | 0.638 | -0.1158 (-23.1%) | [-0.1512, -0.0803] |
| eos0 0.5SOL c0.01 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.1050 | -0.2055 | 0.511 | -0.1318 (-26.4%) | [-0.1672, -0.0963] |
| eos0 1SOL c0.002 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.2372 | -0.3701 | 0.641 | -0.2259 (-22.6%) | [-0.2954, -0.1564] |
| eos0 1SOL c0.01 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.2212 | -0.3861 | 0.573 | -0.2419 (-24.2%) | [-0.3114, -0.1724] |

Selected exits and filters:

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=3 | 8 | 0.0% | +0.0000 | -0.0486 | 0.0 | -0.0486 (-19.4%) | [-0.0600, -0.0372] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=5 | 20 | 0.0% | +0.0000 | -0.0624 | 0.0 | -0.0624 (-25.0%) | [-0.0728, -0.0520] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=10 | 50 | 4.0% | +0.2856 | -0.0812 | 3.517 | -0.0665 (-26.6%) | [-0.0877, -0.0453] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=3 | 8 | 0.0% | +0.0000 | -0.0500 | 0.0 | -0.0500 (-20.0%) | [-0.0618, -0.0381] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=5 | 20 | 0.0% | +0.0000 | -0.0632 | 0.0 | -0.0632 (-25.3%) | [-0.0737, -0.0526] |
| eos0 0.25SOL c0.002 TP100 SLfloor B<=10 | 50 | 4.0% | +0.2856 | -0.0823 | 3.472 | -0.0675 (-27.0%) | [-0.0888, -0.0463] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.0592 | -0.0974 | 0.608 | -0.0602 (-24.1%) | [-0.0780, -0.0423] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.0640 | -0.0974 | 0.657 | -0.0590 (-23.6%) | [-0.0777, -0.0403] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/30 | 80 | 0.0% | +0.1074 | -0.0958 | 1.121 | -0.0755 (-30.2%) | [-0.0928, -0.0582] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/30 | 80 | 3.8% | +0.1235 | -0.0958 | 1.289 | -0.0739 (-29.6%) | [-0.0924, -0.0553] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/20 | 80 | 0.0% | +0.0911 | -0.0987 | 0.923 | -0.0726 (-29.0%) | [-0.0901, -0.0551] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/20 | 80 | 2.5% | +0.0995 | -0.0987 | 1.008 | -0.0715 (-28.6%) | [-0.0898, -0.0531] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail100/30 | 80 | 0.0% | +0.1832 | -0.1009 | 1.815 | -0.0832 (-33.3%) | [-0.1006, -0.0657] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail100/30 | 80 | 6.2% | +0.2674 | -0.1009 | 2.65 | -0.0779 (-31.2%) | [-0.0992, -0.0566] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.0751 | -0.0920 | 0.817 | -0.0565 (-22.6%) | [-0.0744, -0.0386] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy5 | 80 | 3.8% | +0.2000 | -0.1005 | 1.99 | -0.0780 (-31.2%) | [-0.0980, -0.0579] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy10 | 80 | 5.0% | +0.2689 | -0.1009 | 2.665 | -0.0778 (-31.1%) | [-0.0992, -0.0564] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 lat0 | 80 | 6.2% | +0.2835 | -0.1009 | 2.81 | -0.0769 (-30.8%) | [-0.0990, -0.0548] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 lat2 | 80 | 6.2% | +0.2592 | -0.1009 | 2.569 | -0.0784 (-31.4%) | [-0.0995, -0.0573] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem5 | 80 | 6.2% | +0.2427 | -0.1082 | 2.244 | -0.0862 (-34.5%) | [-0.1066, -0.0659] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem15 | 80 | 5.0% | +0.2303 | -0.1207 | 1.909 | -0.1031 (-41.2%) | [-0.1215, -0.0847] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 prem30 | 80 | 3.8% | +0.2246 | -0.1368 | 1.641 | -0.1233 (-49.3%) | [-0.1398, -0.1067] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 nomayhem | 80 | 6.2% | +0.2674 | -0.1009 | 2.65 | -0.0779 (-31.2%) | [-0.0992, -0.0566] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=35 | 6 | 0.0% | +0.0000 | -0.0418 | 0.0 | -0.0418 (-16.7%) | [-0.0496, -0.0341] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=45 | 38 | 2.6% | +0.2836 | -0.0728 | 3.895 | -0.0634 (-25.4%) | [-0.0830, -0.0439] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=60 | 69 | 4.3% | +0.2773 | -0.0930 | 2.983 | -0.0769 (-30.8%) | [-0.0960, -0.0577] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=140 | 77 | 6.5% | +0.2674 | -0.0986 | 2.712 | -0.0748 (-29.9%) | [-0.0965, -0.0532] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=3 | 72 | 6.9% | +0.2674 | -0.1071 | 2.495 | -0.0811 (-32.5%) | [-0.1047, -0.0576] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=5 | 63 | 7.9% | +0.2674 | -0.1135 | 2.356 | -0.0833 (-33.3%) | [-0.1101, -0.0564] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B>=10 | 30 | 10.0% | +0.2553 | -0.1360 | 1.877 | -0.0969 (-38.7%) | [-0.1412, -0.0525] |

## Guru rule by cluster type (window sample)

| launches of ... | n | TP hit | 95% CI | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|
| shared_bundle_wallets | 380 | 5.5% | 3.6%-8.3% | -0.0640 (-25.6%) | [-0.07201, -0.05591] |
| no_cluster | 262 | 6.5% | 4.1%-10.1% | -0.0614 (-24.5%) | [-0.07253, -0.05017] |
| serial_signer | 170 | 5.9% | 3.2%-10.5% | -0.0714 (-28.6%) | [-0.08477, -0.05799] |
| same_bundle_shape | 63 | 1.6% | 0.3%-8.5% | -0.0676 (-27.1%) | [-0.07881, -0.05643] |
| rotating_signer | 21 | 0.0% | 0.0%-15.5% | -0.0605 (-24.2%) | [-0.06927, -0.05168] |
| mother_funded | 17 | 17.6% | 6.2%-41.0% | -0.0238 (-9.5%) | [-0.08732, 0.03965] |
| cex_funded_same_shape | 11 | 9.1% | 1.6%-37.7% | -0.0435 (-17.4%) | [-0.09962, 0.01261] |
| same_round_bundle_shape | 8 | 37.5% | 13.7%-69.4% | +0.0645 (+25.8%) | [-0.05794, 0.18687] |

## Hits after the horizon (window sample)

The simulation sees 900 s of trades. Launches whose all-time ATH (coin API) reached the TP multiple of the end-of-slot-0 price but that did not hit TP in the simulation are re-scored as TP wins at the average simulated TP payoff (an upper bound: it ignores the idle exit and the dump before a late ATH).

| variant | n | TP in horizon | late ATH hits | expectancy | if late hits were TP | hit-rate upper bound |
|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 932 | 56 | 9 | -0.0627 | -0.0605 | 7.0% |
| eos0 0.25SOL c0.002 TP150 SLfloor idle150 | 932 | 28 | 10 | -0.0676 | -0.0652 | 4.1% |
| eos0 0.25SOL c0.002 TP200 SLfloor idle150 | 932 | 17 | 8 | -0.0702 | -0.0675 | 2.7% |

## Clusters

Walk-forward 'follow the good rugger': a cluster's launch is sniped only after at least `min_prev` earlier snipes of the same cluster have closed with a TP hit rate >= `min_hit`.

| variant | min_prev | min_hit | clusters traded | n | TP hit | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 2 | 0.33 | 11 | 35 | 8.6% | -0.0664 (-26.6%) | [-0.10163, -0.03124] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 3 | 0.33 | 10 | 27 | 11.1% | -0.0571 (-22.8%) | [-0.10204, -0.0121] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 5 | 0.33 | 6 | 15 | 13.3% | -0.0572 (-22.9%) | [-0.12147, 0.00705] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 3 | 0.5 | 2 | 3 | 0.0% | -0.1022 (-40.9%) | [-0.14623, -0.05821] |
| eos0 0.25SOL c0.002 TP50 SLfloor idle150 | 2 | 0.33 | 32 | 199 | 22.6% | -0.0397 (-15.9%) | [-0.05264, -0.02683] |
| eos0 0.25SOL c0.002 TP50 SLfloor idle150 | 3 | 0.33 | 29 | 173 | 23.1% | -0.0371 (-14.9%) | [-0.05094, -0.02333] |
| eos0 0.25SOL c0.002 TP50 SLfloor idle150 | 5 | 0.33 | 18 | 133 | 22.6% | -0.0377 (-15.1%) | [-0.05346, -0.02199] |
| eos0 0.25SOL c0.002 TP50 SLfloor idle150 | 3 | 0.5 | 13 | 46 | 17.4% | -0.0514 (-20.6%) | [-0.07528, -0.0276] |
| eos0 0.25SOL c0.002 TP100 SL30 idle150 | 2 | 0.33 | 10 | 26 | 3.9% | -0.0651 (-26.1%) | [-0.09409, -0.03615] |
| eos0 0.25SOL c0.002 TP100 SL30 idle150 | 3 | 0.33 | 9 | 19 | 5.3% | -0.0566 (-22.6%) | [-0.09562, -0.01755] |
| eos0 0.25SOL c0.002 TP100 SL30 idle150 | 5 | 0.33 | 6 | 9 | 0.0% | -0.0761 (-30.4%) | [-0.08583, -0.0664] |
| eos0 0.25SOL c0.002 TP100 SL30 idle150 | 3 | 0.5 | 2 | 3 | 0.0% | -0.0863 (-34.5%) | [-0.10032, -0.07228] |
| eos1 0.25SOL c0.002 TP100 SLfloor idle150 | 2 | 0.33 | 10 | 28 | 10.7% | -0.0608 (-24.3%) | [-0.10454, -0.01701] |
| eos1 0.25SOL c0.002 TP100 SLfloor idle150 | 3 | 0.33 | 9 | 20 | 15.0% | -0.0456 (-18.3%) | [-0.10573, 0.01444] |
| eos1 0.25SOL c0.002 TP100 SLfloor idle150 | 5 | 0.33 | 4 | 10 | 20.0% | -0.0351 (-14.0%) | [-0.13017, 0.05994] |
| eos1 0.25SOL c0.002 TP100 SLfloor idle150 | 3 | 0.5 | 1 | 1 | 0.0% | -0.1464 (-58.5%) | [-0.14638, -0.14638] |

Are some clusters really better? Among 60 clusters with >= 5 snipes (853 snipes, pooled TP hit 6.3%), 1 reach the guru's 33 % hit rate; 1.04 would by pure chance if every cluster had the pooled rate. 5 clusters have a binomial p < 0.05 (~3.0 expected false positives).

Per cluster, guru base rule (in-sample, top 25 by expectancy):

| cluster | score | members (tradable) | edges | n | TP hit | W/L | expectancy | total SOL | guru 33% | guru 3:1 |
|---|---|---|---|---|---|---|---|---|---|---|
| C322 | 4.992 | 3 (2) | {"d": 3} | 2 | 50.0% | 10.277 | +0.1400 | +0.280 | True | True |
| C111 | 9.867 | 3 (3) | {"d": 3, "c": 3, "b_weak": 2} | 3 | 66.7% | 2.104 | +0.1268 | +0.381 | True | False |
| C081 | 11.364 | 9 (2) | {"d": 9, "c": 2, "b_weak": 2} | 2 | 50.0% | 2.59 | +0.0941 | +0.188 | True | False |
| C151 | 8.288 | 3 (2) | {"d": 3, "c": 2} | 2 | 50.0% | 2.644 | +0.0790 | +0.158 | True | False |
| C127 | 9.377 | 8 (8) | {"a_cex_shape": 7, "b_weak": 8, "b_strong": 7} | 8 | 37.5% | 4.471 | +0.0645 | +0.516 | True | True |
| C029 | 15.178 | 10 (7) | {"d": 10, "c": 7, "b_weak": 5} | 7 | 28.6% | 4.862 | +0.0352 | +0.246 | False | True |
| C099 | 10.315 | 4 (5) | {"a_cex_shape": 2, "c": 4, "b_strong": 4} | 5 | 20.0% | 3.65 | -0.0047 | -0.024 | False | True |
| C080 | 11.368 | 5 (10) | {"d": 2, "a_cex": 3, "a_cex_shape": 2, "c": 4} | 10 | 30.0% | 2.034 | -0.0112 | -0.112 | False | False |
| C034 | 14.816 | 19 (19) | {"d": 4, "c": 19} | 19 | 21.1% | 2.851 | -0.0163 | -0.309 | False | False |
| C019 | 16.952 | 9 (9) | {"d": 5, "c": 9, "b_weak": 2, "b_strong": 7} | 9 | 22.2% | 2.61 | -0.0172 | -0.154 | False | False |
| C049 | 13.301 | 7 (16) | {"d": 5, "c": 7, "b_weak": 6} | 16 | 25.0% | 2.297 | -0.0181 | -0.289 | False | False |
| C062 | 12.355 | 9 (9) | {"d": 2, "c": 9, "b_weak": 9} | 9 | 11.1% | 3.981 | -0.0284 | -0.256 | False | True |
| C324 | 4.826 | 3 (1) | {"d": 3} | 1 | 0.0% | 0.0 | -0.0285 | -0.029 | False | False |
| C104 | 10.057 | 11 (34) | {"c": 11, "b_strong": 2} | 34 | 14.7% | 3.105 | -0.0293 | -0.997 | False | True |
| C076 | 11.461 | 4 (9) | {"a_cex_shape": 4, "c": 4, "b_strong": 4} | 9 | 11.1% | 3.773 | -0.0300 | -0.270 | False | True |
| C025 | 15.967 | 9 (26) | {"a_mother": 9, "c": 9, "b_weak": 9} | 26 | 23.1% | 2.134 | -0.0314 | -0.817 | False | False |
| C135 | 9.063 | 5 (5) | {"c": 5, "b_strong": 5} | 5 | 0.0% | 0.036 | -0.0323 | -0.161 | False | False |
| C175 | 7.513 | 7 (20) | {"a_cex": 2, "c": 7} | 20 | 10.0% | 3.343 | -0.0345 | -0.689 | False | True |
| C204 | 6.81 | 7 (1) | {"d": 7} | 1 | 0.0% | 0.0 | -0.0346 | -0.035 | False | False |
| C004 | 21.862 | 34 (2) | {"d": 34, "a_mother": 34} | 2 | 0.0% | 0.0 | -0.0357 | -0.071 | False | False |
| C006 | 20.312 | 21 (28) | {"a_cex_shape": 15, "c": 21, "b_strong": 19} | 28 | 0.0% | 0.0 | -0.0362 | -1.014 | False | False |
| C038 | 14.481 | 19 (27) | {"c": 19, "b_strong": 19} | 27 | 11.1% | 3.356 | -0.0364 | -0.982 | False | True |
| C033 | 14.817 | 6 (5) | {"d": 6, "c": 5, "b_strong": 5} | 5 | 0.0% | 0.0 | -0.0366 | -0.183 | False | False |
| C014 | 18.429 | 6 (28) | {"d": 3, "a_mother": 6, "c": 4, "b_strong": 5} | 28 | 7.1% | 4.624 | -0.0377 | -1.056 | False | True |
| C140 | 8.731 | 14 (3) | {"d": 14} | 3 | 0.0% | 0.0 | -0.0410 | -0.123 | False | False |

## Discovery: what the clusters look like

`data/rugger_clusters.json` lists 396 clusters with ≥ 3 launches. Each entry has its tokens, slot-0 shape, funding and outcome, and, for 30 clusters, up to 25 extra launches from the 72 h expansion (286 launches). `data/reports/discovery_summary.json` has the aggregates.

**Population (6 h, 7,049 SOL-quoted launches)**
- 6,354 launches have a traced signer. 21.6% of these are fresh wallets (≤ 10 prior txs).
- Fresh creators are funded by:
  - unlabeled "mother" wallets: 813
  - labeled exchanges: 463 (Binance 1 alone: 324)
  - exchange-like unlabeled wallets (> 200 tx/h): 63
  - the Relay bridge solver: 31
- Median time from funding to create: 372 s.
- 23% of the non-mayhem launches with a known slot 0 are bundled. Counting the 262 launches whose slot 0 was never read (see Verification), it is ≈ 29%. Bundle-wallet counts: 1 (209), 2 (279), 3 (360), 4+ (137). Median bundle 5.9 SOL (p10 2.4, p90 11.0); median end-of-slot-0 market cap 41.8 SOL.

**Signal (a), same funder + amount ±0.02 SOL**
- Exchange-funded fresh creators: 2,136 matching pairs observed vs 1,560 expected by chance (permutation p = 0.005). That is only 1.4×, and it is driven by round withdrawals (1.999 SOL: 44 launches; 9.999: 11).
- Exchange + amount alone is therefore weak. It is used only together with the same bundle shape (`a_cex_shape`).
- Mother wallets: 1,466 pairs vs 69 expected. Mothers really do send fixed amounts. Most of these are coin farms seeding wallets with dust (0.024, 0.047, 0.007 SOL), and those coins stay at the floor.

**Signal (c), shared bundle wallets**
- 334 bundle wallets recur across launches. 26 more wallets show up both as bundle wallets and as third-party slot-0 buyers (sniper bots) and are excluded.

**Signal (d), same signer**
- 76 signers launch for ≥ 3 different `creator` keys. These are launch services, and they are keyed by `creator` instead.

**Examples of the guru's scheme found on-chain**
- **C001**: 48 launches in 6 h, 98% fresh creators, 35 of them funded directly by Binance 1, and 44 with the identical bundle "dev 0.99 SOL + 3 wallets 1.11 / 0.99 / 0.95 SOL". This is exactly the guru's rugger, and trivially trackable. Typical peak: 1.2–1.5× the entry. 1 of 48 snipes hit +100%; −23% per snipe.
- **C025**: mother `JAmGVx91` sends exactly 2.0 SOL to fresh creators, who launch with 4 bundle wallets.
  - In the window, 3 of 9 launches reached 2× (one reached 70×).
  - After expansion to 26 launches: 6 TP hits (23%), −12.6% per snipe. The small in-window sample regressed toward the mean.
- **C014**: mother `ET69Tz` (the 0.9 SOL scheme found in Task B). 28 launches, 2 TP hits, −15%.
- **C127**: Binance 1 sends 0.685 SOL; 2-wallet bundles of ~0.88 SOL. 8 launches, 3 TP hits, +26% per snipe. This is the one cluster that passes both guru tests. It is consistent with chance (see the heterogeneity test above) and should be re-tested forward before any money goes on it.

## Method notes

- **Window and sampling.** Every create in the window was decoded over RPC.
  - 1,652 SOL-quoted launches were skipped for trades: their all-time high (at a conservative SOL price of $104) proves slot 0 held < 1 SOL of buys, so they were never bundled.
  - Trades were pulled for all 3,006 non-mayhem candidates and for a random 606 of 2,391 mayhem candidates.
  - However, 262 non-mayhem candidates had more than 200 trades in their first 60 s. For them slot 0 was never read, and they were stored as not bundled (see Verification). The other 1,785 mayhem launches were not pulled because of the shared swap-api rate limit. The bundled sample is therefore essentially all non-mayhem (981 of 982).
- **Trade paths.** Paths come from swap-api, using a crafted cursor 900 s after creation and paging back to the first trade. If 15 pages were not enough, the 300 s and then 60 s windows were used. The simulator only uses the contiguous prefix from slot 0.
- **Instant graduations.** 50 of the 982 loaded launches completed their curve inside the creation slot. They are skipped (`why = graduated`), because their later trades are AMM trades, which the curve model cannot price.
- **Holds past the covered path.** 14 positions were still open at the end of their covered path and were closed at the last price: 11 at 900 s and 3 at a 60 s coverage limit. 9 launches reached 2× of the slot-0 price only after the simulation had closed them. Counting all of them as TP wins moves the base rule from −0.063 to −0.061 SOL per snipe.
- **Look-ahead guard.** All 6,912 entry decisions (3 entry variants × all samples) were recomputed from truncated and from scrambled futures. There were zero mismatches. Task D also recomputed every slot-0 feature this way: zero differences.
- **Not modelled.**
  - Failed buy attempts. Gap-2 measured 22% slippage failures in slot 0; each costs the tip without opening a position.
  - Sell-side competition when the dump is under way.
  - Jito bundle ordering inside the slot: the entry is the last recorded trade of the slot, which is optimistic for a reactive bot.

## What more data is needed

0. **Re-pull the censored hot launches** (see Verification). This comes before anything else on this page.
1. **A longer dense window, at least 3–7 days.** In 6 h, only 61 clusters had ≥ 5 snipes, and walk-forward could only trade 15–35 launches. The guru's rule ("≥ 10 earlier tokens, ≥ 3 of 10 hits") needs operator histories that a 6 h window, plus a 25-launch expansion, cannot give. This is mostly a rate-limit problem: a paid RPC with Geyser or a dedicated trade indexer removes it.
2. **Live paper trading.** Record our own would-be landing index and fill price in slot 0 / slot+1 for every bundled launch from tracked funders, instead of assuming the last recorded trade.
3. **A pre-entry signal that separates the ~6% of runners.** Candidates: metadata and socials, the dev's earlier runners, how fast bundle wallets were funded, and slot-1 buy pressure. Each must be tested out of sample against the ~25% break-even TP-hit rate.
4. **Mayhem launches.** They had to be dropped from slot-0 coverage because of the rate limit. The guru's floor-stop argument does not hold for them in any case.

## Verification (Task D, adversarial check, 2026-10-14)

**Trust level: LOW for the published magnitudes, MEDIUM for the sign of the verdict.**
- The simulator's arithmetic is right: units, decimals, fees, sell impact and look-ahead all check out.
- The sample is censored in a way that removes most of the winners. Corrected, the guru's base rule still loses money, even at the favourable end of the correction.
- Not reliable as published: the size of the loss, the TP-hit rates, the variant ranking, the oracle bounds and the cluster tests.

**How it was checked.** `analysis/scripts/verify_backtest.py` runs offline in about 10 s. `--probe 48` adds at most 96 swap-api requests and `--spot 5` adds 10. Output: `data/reports/backtest_verification.json`.

Before the fixes below, the selftest and `backtest.py` were re-run from the cache with 0 network calls, and they reproduced every published number.

**What holds up**
- **Units and decimals.** Each window trade was re-derived from the previous trade's curve state (102k trade pairs).
  - `priceSol × 1e9` is the post-trade market cap in SOL on k = 30 SOL × 1.073e15 (median error 3e-11).
  - `amountSol` is net of fees: token amounts at 6 decimals match to 3e-9. Treating it as gross would be off by 1.2%.
- **SOL vs USD.** The coin API's `ath_market_cap` and `usd_market_cap` are USD, and the code treats them as USD everywhere.
  - API ATH ÷ the trades' SOL-USD equals the trade-path maximum (median difference 2e-9; 0 of 982 below it by more than 0.5%).
  - SOL traded at $106.9–108.5 in the window, so the $104 ATH prefilter is conservative. It skipped no bundled launch.
- **Fees.** All 944 bundled launches with in-tx TradeEvents pay 95 + 30 bps, so 1.25% per side is right for this sample.
  - The buy fee is added on top of the SOL amount and the sell fee is deducted.
  - Sell impact is included: buying and then selling on an unchanged curve returns the size minus both fees.
- **Curve.** All 7,049 SOL-quoted creates in the window start from the standard 30 SOL / 1.073e15 reserves.
- **Look-ahead.** Beyond the decision check, every slot-0 feature (roles, bundle wallets and sizes, eos0) was recomputed from a truncated and from a scrambled future on 1,062 launches. There were 0 differences.
- **Creation slot and timestamps.**
  - Every loaded launch has its first trade in the create slot.
  - All 4,948 slot-0 trades carry the create block time: no ms/s or timezone slip.
  - No trade row is duplicated.
  - Dev buys are not double-counted: swap-api dev buys match the RPC create-tx TradeEvents.
- **Failed transactions.** Swap-api lists only successful trades (256 of 256 matched on-chain in Task A). Failed creates are excluded from the window index.
- **Population frame.** The window comes from the RPC create index, not the coins API's capped pagination. Its largest gap between consecutive creates is 21 s.
- **Spot check by hand: 5 random bundled launches** (coins-v2 + DexScreener, 13:05 UTC).
  - API ATH equals the trade maximum within 0.05%, at the same second.
  - The current DexScreener price (27.95 SOL mcap on the pumpfun pair) equals API `usd_market_cap` ÷ SOL, which is the curve floor. All 5 had been rugged to the floor within 25–700 s.

**Fixed in this pass** (all numbers on this page are after these fixes)
1. **Instant graduations were priced as curve trades.**
   - 50 of the 982 loaded launches (5%) completed their curve inside the creation slot. Their later trades are PumpSwap AMM trades, which the simulator priced with the bonding-curve formula.
   - `backtest.entry_view` now skips the entry when an AMM trade happened by the entry slot, or when the curve is ≥ 98% complete. The skip reason is `graduated`, and selftest `test_skip_graduated_entry` covers it. Task A and the strategy modules already did this.
   - Effect on the window base rule: −24.8% → −25.1%, n 982 → 932.
2. **Wrong break-even.** The verdict said break-even needs a 34% hit rate, so the guru's "3 of 10" is exactly break-even.
   - 34% was the break-even *win* rate (any profitable exit), not the TP-hit rate.
   - TP hits make +99% and the other exits −33% on average. Break-even is therefore a **25% TP-hit rate**, and 3 of 10 would make about +7% per snipe.
3. **"56 positions still open at 900 s."** 42 of those were instant graduations. After fix 1 the count is 14 (11 at 900 s, 3 at a 60 s coverage limit), with 9 late-ATH launches.
4. **Per-cluster table.** It listed 19 clusters with zero tradable launches among the "top 25 by expectancy". Clusters with snipes now come first.

**Not fixed: the window sample is censored at 200 trades in the first minute (the main problem)**
- **Mechanism.**
  - `window.staged_trades` reads slot 0 from at most 2 pages (200 trades) of the first 60 s.
  - For 262 non-mayhem launches the first minute did not fit. Slot 0 was never read, and they were stored as `bundled = False`.
  - No loaded launch has more than 200 trades in its first 60 s: the sample is truncated exactly there.
- **These are the runners.**
  - Median ATH 177 SOL, against 49.7 SOL for the bundled sample.
  - 86% passed 84 SOL, against 13%.
  - 96 graduated, against 56 in the whole bundled sample.
- **Probe of 48 random censored launches** (slot 0 read live, 60 requests).
  - 43 are bundled. 13 of those graduated in slot 0, which leaves 31 normal bundled launches.
  - On those 31 the guru's rule hits TP **68–71%** of the time (CI 50–84%) and makes **+61% to +66% per snipe** (CI +27% to +99%).
  - The lower bound uses only the cached 60 s and 900 s pages. The upper bound also credits API ATHs inside 900 s.
  - Re-weighted to all 262, the window base rule becomes about **−11% to −12% per snipe at a 15–16% TP-hit rate** (n ≈ 1,100), instead of −25% at 6%.
  - At the top of the probe's CI it would still be about −6%.
- **Task A agrees, with less optimism.** Task A's random arm pulled up to 2,000 trades, so it is not censored.
  - Its 10 bundled launches with more than 200 trades in the first minute hit TP 5 times, for +27% per snipe (CI −26% to +81%).
  - The other 70 hit TP 0 times (−40%). Every TP hit in Task A comes from a hot launch.
  - Using +27% instead of +61% for the censored group puts the corrected window at about −17%.
- **What survives.** The base-rule verdict (negative) holds under both corrections.
- **What does not.**
  - "Every variant is negative" and the variant ranking: hot launches are not spread evenly over the filters. For example, both probed hot launches with eos0 ≤ 35 SOL hit TP.
  - The oracle bounds: +23% is far too low.
  - The cluster heterogeneity test and the walk-forward results: the clusters lack exactly the launches where an edge would show.
  - The "23% bundled" share: it is ≈ 29% once censored launches are counted.
  - The TP-hit rates quoted on this page: 6% becomes ≈ 15%.
- **Fix (not run).** It needs about 5–10k swap-api requests, i.e. hours under the shared limit of 1,000 requests per 5 minutes.
  1. When the 60 s prefix is incomplete, read slot 0 with a cursor at create time + 1 s (1–2 pages).
  2. If that also fails, mark the row `s0_unknown` instead of `bundled = False`.
  3. Pull the full path of bundled launches with a page budget sized to the trade count, or use `minSolAmount`-filtered pages for the price path.
  4. Re-run discovery from the cluster stage, then the backtest.

**Other limitations** (unchanged; see Method notes)
- **Optimistic entry.** Entering at the last recorded slot-0 trade is optimistic for a reactive bot. Failed attempts and sell-side competition are not charged, and the hot launches are where the slot-0 competition is heaviest. All of this works against the strategy.
- **Hindsight in walk-forward.** Cluster membership uses the whole window, so a launch can be linked to its cluster only through a later launch. This flatters "follow the good rugger".
- **Non-default creator fees.** 5.2% of all window creates use one (up to 300 bps; 80 also pay 0 protocol fee). None in the bundled sample do, but a live bot must read the fee from the curve rather than assume 1.25%.
- **The only positive pocket.** Hot launches are the one profitable group in the data; Task A and the probe agree on the sign. But "hot" is measured over the first 60 s, which is after the entry.
  - The open question for the strategy work is whether anything visible at entry predicts it: the count of third-party slot-0 snipers, the dev buy size, or slot-1 buy flow.
  - That must be tested on uncensored data.


