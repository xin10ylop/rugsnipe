# On-chain backtest of the guru's rugger snipe (Task C)

Generated 2026-10-14T10:46:31Z by `analysis/scripts/backtest.py` from `data/reports/backtest_results.json`. Discovery: `analysis/scripts/discover_ruggers.py` (`data/rugger_clusters.json`, `data/reports/discovery_summary.json`).



## Samples

| sample | launches with a contiguous trade path | bundled (tradable) |
|---|---|---|
| window | 0 | 0 |
| clusters | 0 | 0 |
| taskA | 362 | 80 |

Look-ahead guard: 1086 entry decisions re-run on a truncated and on a scrambled future; mismatches: 0.

## Assumptions

- Entry state = curve after the last recorded trade of the creation slot (eos0) or of slot+1 (eos1); the entry decision uses slot-0 trades only.
- Our buy is overlaid on the recorded path: impact from the constant-product curve, our SOL stays in the curve while we hold (recorded state shifted by our net SOL).
- Fees 1.25 % per side (0.95 % protocol + 0.30 % creator, as observed on 2026 TradeEvents) instead of the 1 % in the brief; a fixed cost (priority fee + tip) is paid on the buy AND on the sell.
- Exit triggers are evaluated on each recorded post-trade price; the sell executes after all recorded trades of the next slot (exit latency 1 slot) unless stated.
- Natural SL = no stop: the position is held until TP, 150 s without trades (idle), or the 900 s horizon.
- Competition is not simulated except through the entry_premium variants (fill 5/15/30 % above the post-bundle price, cf. gap-2: 3rd reactive buyer pays a median +10.8 %, 5th +15.8 %). Failed attempts (22 % slippage failures in slot 0) are not charged.
- Bundled = >= 1 non-dev wallet in the contiguous run of txs right after the create tx (or inside it) and >= 1 SOL of dev + bundle buys in slot 0.

## Sample `taskA`

Gain potential (no costs): 5/80 bundled launches (6.2%, CI 2.7%-13.8%) traded at >= 2x the end-of-slot-0 price within the horizon.

Guru base rule and the best variants (ranked by expectancy, n >= min-n):

| variant | n | TP hit | avg win SOL | avg loss SOL | W/L | expectancy / snipe | 95% CI |
|---|---|---|---|---|---|---|---|
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 | 80 | 6.2% | +0.2674 | -0.1009 | 2.65 | -0.0779 (-31.2%) | [-0.0992, -0.0566] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 bigbuy2 | 80 | 0.0% | +0.0751 | -0.0920 | 0.817 | -0.0565 (-22.6%) | [-0.0744, -0.0386] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail30/20 | 80 | 2.5% | +0.0640 | -0.0974 | 0.657 | -0.0590 (-23.6%) | [-0.0777, -0.0403] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail30/20 | 80 | 0.0% | +0.0592 | -0.0974 | 0.608 | -0.0602 (-24.1%) | [-0.0780, -0.0423] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=45 | 38 | 2.6% | +0.2836 | -0.0728 | 3.895 | -0.0634 (-25.4%) | [-0.0830, -0.0439] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/20 | 80 | 2.5% | +0.0995 | -0.0987 | 1.008 | -0.0715 (-28.6%) | [-0.0898, -0.0531] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/20 | 80 | 0.0% | +0.0911 | -0.0987 | 0.923 | -0.0726 (-29.0%) | [-0.0901, -0.0551] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 trail50/30 | 80 | 3.8% | +0.1235 | -0.0958 | 1.289 | -0.0739 (-29.6%) | [-0.0924, -0.0553] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=140 | 77 | 6.5% | +0.2674 | -0.0986 | 2.712 | -0.0748 (-29.9%) | [-0.0965, -0.0532] |
| eos0 0.25SOL c0.002 SLfloor idle150 trail50/30 | 80 | 0.0% | +0.1074 | -0.0958 | 1.121 | -0.0755 (-30.2%) | [-0.0928, -0.0582] |
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 mc<=60 | 69 | 4.3% | +0.2773 | -0.0930 | 2.983 | -0.0769 (-30.8%) | [-0.0960, -0.0577] |

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
| eos0 0.25SOL c0.002 TP100 SLfloor idle150 B<=5 | 20 | 0.0% | +0.0000 | -0.0624 | 0.0 | -0.0624 (-25.0%) | [-0.0728, -0.0520] |
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



