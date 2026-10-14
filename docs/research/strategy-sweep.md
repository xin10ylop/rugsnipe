# S1: policy sweep on bundled pump.fun launches

**Question.** On bundled pump.fun launches, is there an entry/exit policy with a real, out-of-sample, positive expectancy after every cost? The candidates are the guru's rules, ours, or a mix. Every number here comes from a walk-forward split or a holdout. Every sweep reports how many configurations were tried and applies a multiple-testing correction.

*Measured 2026-10-14. Code: `analysis/rugsnipe_analysis/strategy/{tapes,sim,stats}.py` and `analysis/scripts/strategy/`. Machine-readable results: `data/reports/strategy/*.json`.*

## Answer in brief

1. **The guru's method, as he describes it, loses money.** We bought right after the bundle (rank 1) and sold at +100% or held. That loses **-10.5% per trade on the test split [-13.9, -6.9]** and **-21% on an independent holdout**. "Buy on dev sell" loses -13%, and the "snipe delay" version (enter at the end of slot+1) loses -17%. His bot's protective features (phantom SL, sell on big buy, sell on no activity) raise his rule to about **-1%**, but no higher. VERIFIED (model on real tapes).
2. **The broad sweep finds no edge that survives multiple testing.** We tried **25,397,064 configurations**: 27 entries × 49,896 exit policies × 19 filters. None has a Benjamini-Hochberg q ≤ 0.10 for "test EV > 0". The smallest p is 7.9e-7; Bonferroni at 5% needs 2.0e-9.
   - The 15 best policies ranked on the test set all have q = 1.00. They are small-n artefacts: negative on train and negative on the deep holdout.
   - Survivor entries (enter after the insiders sold, if the price holds) and dip entries lose **-2% to -9%** out of sample, whatever the exit.
3. **One family sits near break-even everywhere: a rank-1 micro-scalp.** It buys right after the bundle and leaves within 1-3 s. It works because the other slot-0/1 snipers buy after us and lift the price.
4. **We refined that family on the train split only and froze 5 policies.** We then tested them on three sets: the time-split test, the deep sample, and a **fresh holdout of 244 launches pulled after the freeze**. One frozen policy is positive on all three:

   > **Lead: E1 rank-1 entry right after the bundle, multi-wallet bundles only, 0.5 SOL. Sell at the first of: realised +15%, any bundle-wallet sell, 1 s with no trade, price −15% after 1 s, or the end of slot+6 (exit fires at slot+5, fills one slot later).**
   >
   > | Set | EV per trade [90% CI] | n |
   > |---|---|---|
   > | train | **+5.3%** | 288 |
   > | gap-3 test | **+3.5%** [+1.4, +5.8], BH q = 0.04 | 200 |
   > | deep holdout | **+3.2%** [-1.0, +7.5] | 77 |
   > | fresh holdout | **+3.7%** [+0.7, +6.6] | 150 |
   > | pooled out-of-sample | **+3.5%** [+1.9, +5.2], p = 0.0002 | 427 |
   >
   > The base scenario is a +1.1% entry premium, 0.004 SOL per round trip and a 1-slot exit latency. The policy is positive on 16 of 17 days. All 108 neighbouring parameter settings are positive out of sample (+2.3% to +4.2%).

5. **The edge is the rank-1 slot itself, and pro snipers already contest it.**
   - **Break-even entry premium:** about **+4.4%** at 0.004 SOL per round trip, **+3.3%** at 0.01 SOL, **+1.4%** at 0.02 SOL.
   - **Rank-3 fill:** gap-2 measured a median +10.8% premium for the third reactive buyer. At that premium the policy loses about **-6%**.
   - **Snipers that fail to land:** it survives losing 25-50% of the other slot-0 snipers (+0.4% to +2.2%). It reaches about -2% to 0% if all of them disappear.
   - **What this means:** it works only if we land first, or nearly first, behind the bundle, and only with a slippage cap that refuses worse fills.
6. **Verdict: a candidate edge exists, but it is not proven for live trading.** It passes every out-of-sample test we can run on historical tapes. It is still a simulation:
   - Our own effect on the other snipers is modelled only by the drop stress.
   - The family was chosen after we had looked at the stage-1 test and deep results. Only the fresh holdout is clean of every selection step. There it gives +3.7%, p = 0.017, and BH q = 0.05-0.11, depending on how many tests are counted.
7. **Recommended next step: a capped live test.**
   - About 200 trades at 0.25-0.5 SOL, with a hard slippage cap of +2% over the post-bundle price.
   - Measure our real landing rank and premium.
   - Stop if the mean is below 0 after 100 trades. If the true EV is +3.5%, that happens by chance with about 4% probability.

## Data and method

**Tapes**

| Set | Source | Launches | Used for |
|---|---|---|---|
| **gap-3** | `data/cache/gap3/` | 675 bundled or dev ≥ 1 SOL launches (488 multi-wallet), Sep 28 – Oct 14, trades to +900 s | Selection (train) and evaluation (test, odd/even days) |
| **Deep sample** | random arm of `data/tokens_sample.jsonl` | 133 qualifying launches (77 multi-wallet), Oct 12-14, full coverage | Holdout only. Its results for stage-1 finalists were seen before stage 2, so it is not clean for stage 2 |
| **Fresh holdout** | `analysis/scripts/strategy/pull_holdout.py` → `data/cache/strategy/holdout_trades.jsonl` | 244 qualifying launches (150 multi-wallet) out of 573 pulled, Oct 12-14, first 150 s | Untouched holdout |

- **gap-3 details.** Weights follow gap-3: w = 5 for launches that gap-1 subsampled 1 in 5 because ATH < $5.5k. The bundle definition and the launch filter are re-implemented exactly. The loader reproduces gap-3's post-bundle mcap and wallet count on all 675 launches with 0 mismatches.
- **Deep sample details.** It comes from the random arm of `data/tokens_sample.jsonl` and was loaded from cache with no network access. Weight 1. It is a separate random draw from the same 48 h create index.
- **Fresh holdout details.**
  - Source: light-sample launches that are neither in gap-3 nor in the deep sample (non-mayhem, SOL-quoted). Their first 150 s were pulled **after** the stage-2 policies were frozen.
  - Order of events: `frozen_policies.json` was written at 11:34:40 UTC; the pull finished at 11:53:18.
  - Its launches are used here and nowhere else.

**Simulator** (`sim.py`, exact curve math)

- **Curve and our position.**
  - The tape is the history without us.
  - Our buy of S SOL at the entry state takes q tokens out of the standard curve (K = 30 SOL × 1.073e9 tokens).
  - While we hold, the curve is the historical state with our q tokens still outstanding, so our SOL stays in the curve.
  - Sell proceeds are (K / (vt_h − q) − vs_h) × (1 − 1.25%), which includes our own impact.
  - Fees are 1.25% on each side.
- **Validation.**
  - The tapes' token flows reproduce the API's mcap path to a median relative error of 1e-9, so the curve is exact.
  - The simulator reproduces gap-3's policy table within 1-2 points. The gap is explained by gap-3 booking TP at exactly +100% and ignoring a TP that came after D50.
- **Entry premium p.** It models others filling ahead of us (gap-2): we receive q0 / (1 + p) tokens. The values are 0, **+1.1% (base, the rank-1 median)**, +3.6% (rank-1 on CEX-funded bundled launches) and +10.8% (rank-3 median). This is conservative: gap-2's +1.1% already includes the buyer's own impact, which we also simulate.
- **Fixed cost per landed round trip.** **0.004 SOL at base**, 0.01 SOL when competitive, plus 0.02 SOL as a stress.
- **Exit latency.** A signal seen in slot s fills after every historical trade of slots ≤ s + 1 (gap-3's "E1"), so other sellers in those slots land first. We also tested 0, 2 and 4 slots. Fills are capped at the data horizon.
- **Entries** (27).
  - **E1**: rank 1 right after the bundle in slot 0.
  - **E2**: end of slot+1.
  - **INS**: the slot after the first insider sell. This is the guru's "buy on dev sell".
  - **SURV_N_m**: end of slot+N, N ∈ {5, 10, 15, 25, 50}. Enter only if price ≥ post-bundle × (1 + m) for m ∈ {0, 5, 10, 20}%, and only if a bundle or creator wallet has already sold.
  - **DIP_x**: after the first insider sell, buy at the first slot end within 60 s where price ≤ post-bundle × (1 + x), x ∈ {0, 5, 10, 20}%.
- **Exits** (39 components). A policy fires at the earliest of its components.
  - TP (realised value) at 20/30/40/50/60/80/100/150%.
  - Any insider sell: bundle or creator, bundle only, creator only.
  - Any sell of ≥ 1/2/5% of supply.
  - Idle (no trade) for 2/3/5/10 s.
  - Trailing stop of 10/15/20/30%, after activation at 10/20/30%.
  - Phantom SL after 3 or 10 s at −15% or −25%.
  - Auto-sell on a single buy of ≥ 2 or 5 SOL.
  - Max hold of 60/300/900 s.
- **Exit grid.** Every policy has a TP or none, up to 3 of the 6 signal groups, and one max hold. That gives **49,896 exit policies**.
- **Filters** (19).
  - Post-bundle mcap ≤ 36/40/45/50 SOL, and > 50.
  - Dev-only, and bundle wallets 1 / 2-3 / 4+.
  - Dev buy < 1, 1-2, 2-4, ≥ 4 SOL.
  - Pre or post Oct 9.
  - Third-party slot-0 buyers 0 / ≥ 1 / ≥ 2. These are look-ahead at E1 and are excluded there.
- **Splits.** The time split trains on the first 60% of launches by create time (405, to Oct 7 09:13 UTC) and tests on the last 40% (270, including all 183 post-Oct-9 launches). The second split is even vs odd UTC days.
- **Statistics.**
  - Weighted mean EV per trade, net of all costs, as a fraction of S.
  - The grid uses the normal approximation with weighted standard errors, which feed the p-values for BH over every configuration.
  - Finalists get bootstrap 90% CIs (2,000 resamples of launches).
- **Trades per day.** Σ weight × 781,123 / 9,947 / 16 days. This scales the gap-1 sample (1.27% of creates) to the population: 1 unit of weight ≈ 4.9 launches/day. The full gap-3 class is ≈ 6,260 launches/day.
- **Selection protocols.** All are written before the corresponding test numbers were computed.
  - **A (time).** Pick the best exit per (entry, filter) on train by lower 90% bound, with n_train ≥ 25 and n_test ≥ 15. Take the top 20 pairs and evaluate on test, with BH over those 20.
  - **B.** The same with even days → odd days.
  - **C.** The top 15 on **test** across the whole grid, as requested, with BH over all 25.4M configurations.
  - **Stage 2.** A finer grid around the one family that came close: 659,232 configurations, E1 only. Selection uses gap-3 train only; the top 5 by lower bound are frozen to `frozen_policies.json`, then evaluated once on test, deep and fresh.

Runtime: the full sweep takes 196 s, stage-2 selection 35 s, and evaluation 18 s on 4 cores. The holdout pull took 20 min, mostly because the swap-api rate limit is shared.

## 1. The ceiling: oracle bounds per entry (0.25 SOL, +1.1%, 0.004 SOL)

| Entry | Triggers (of 675) | Launches/day | Hold to 900 s | Oracle: sell at max | Oracle: sell before the biggest one-slot fall |
|---|---|---|---|---|---|
| **E1** (rank 1 after bundle) | 675 | 6,258 | -20.9% | **+42.2%** | +14.4% |
| E2 (end of slot+1) | 675 | 6,258 | -26.3% | +30.6% | +6.0% |
| INS (after first insider sell) | 654 | 5,978 | -21.4% | +19.8% | +1.1% |
| SURV_5_0 / 5_10 / 5_20 | 73 / 52 / 32 | 535 / 275 / 157 | -33% / -38% / -41% | +36% / +56% / +63% | +5% / +10% / +4% |
| SURV_15_0 / 15_20 | 176 / 90 | 1,001 / 442 | -41% / -45% | +36% / +50% | -1% / -1% |
| SURV_25_0 / 25_20 | 226 / 127 | 1,306 / 623 | -40% / -46% | +39% / +49% | -2% / -8% |
| SURV_50_0 / 50_20 | 229 / 148 | 1,340 / 766 | -40% / -47% | +38% / +47% | 0% / -3% |
| DIP_0 / DIP_20 | 555 / 608 | 5,433 / 5,733 | -17% / -20% | +14% / +19% | +0.3% / +1.6% |

**Reading.**
- Even selling at the exact top, the average E1 trade makes only +42%. Most of that is concentrated in a few launches.
- Every later entry has a lower ceiling and a deeper hold loss.
- Dip entries have almost no upside (+14-19% at the oracle). After the insiders leave, there is little left to catch.
- Survivor entries keep a +36-63% oracle ceiling, but holding them loses 33-47%, because the launches that "survive" are mostly those that dump later.
- The money is at the very start, and it decays within seconds.

## 2. The main grid: 25.4M configurations

- **Search size and correction.** We evaluated 25,397,064 configurations. **0 have BH q ≤ 0.10, and 0 have q ≤ 0.05.** The minimum test p is 7.9e-7 (Bonferroni at 5%: 2.0e-9).
- **Share of configurations with positive EV.** 4.4% on train, 10.9% on test, 8.8% on even days, 5.6% on odd days. The test period (Oct 7-14) was somewhat kinder overall, so a positive number on test alone is weak evidence.
- **Best train exit per entry type** (filter = all). Only E1 comes near zero; every other entry is clearly negative out of sample.

| Entry | Best train exit | Train EV (n) | Test EV [90% CI] (n) |
|---|---|---|---|
| E1 | `tp20+idle2+trail30@10+psl10s-15+hold60` | +3.4% (405) | -0.1% [-2.0, +2.2] (270) |
| E2 | `tp40+ins_any+idle2+bigbuy2+hold60` | -5.2% (405) | -6.3% [-7.9, -4.6] (270) |
| INS | `tp20+ins_bundle+idle2+trail20@10+hold60` | -4.7% (392) | -6.2% [-7.5, -4.9] (262) |
| SURV_15_0 | `tp80+bigsell2+idle2+psl3s-15+hold60` | -2.8% (102) | -2.5% [-6.3, +2.1] (74) |
| SURV_15_20 | `tp20+bigsell5+psl3s-25+bigbuy2+hold300` | +3.9% (46) | -5.3% [-12.9, +2.8] (44) |
| SURV_25_0 | `tp20+ins_creator+bigsell1+idle2+hold60` | -5.8% (130) | -5.8% [-8.1, -3.4] (96) |
| SURV_50_0 | `tp20+ins_any+idle2+psl3s-15+hold60` | -5.0% (135) | -8.8% [-11.3, -6.4] (94) |
| DIP_0 | `tp80+idle2+trail30@10+bigbuy2+hold60` | -4.8% (329) | -6.6% [-7.8, -5.4] (226) |
| DIP_20 | `tp150+idle2+trail20@10+bigbuy2+hold60` | -4.1% (365) | -6.5% [-8.0, -5.0] (243) |

The remaining SURV and DIP variants are in `sweep_results.json` (`entry_best_train_all`). All of them are -2% to -9% on test, except SURV_5_10 at +3.3% [-9.5, +20.3] on n = 24.

### 2a. Protocol A: selected on train, evaluated on test (top 20 by train lower bound; BH over the 20)

| # | Entry | Filter | Exit | Train EV (n) | Test EV [90% CI] (n) | p | BH q | Deep (n) | Test at +3.6% premium | Test at 0.01 SOL |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | E1 | dev<1 | `tp20+ins_bundle+idle10+hold60` | +9.9% (70) | +4.2% [-0.1, +9.6] (73) | 0.057 | 0.57 | -1.8% (23) | +1.7% | +1.8% |
| 2 | E1 | nB1 | `tp150+ins_any+bigsell1+bigbuy2+hold900` | +11.6% (49) | +5.3% [+0.3, +11.6] (40) | 0.040 | 0.57 | -6.7% (18) | +2.7% | +2.9% |
| 3 | E1 | me≤50 | `tp30+idle2+trail10@30+psl3s-25+hold60` | +4.5% (314) | -0.8% [-2.9, +1.5] (199) | 0.73 | 1.00 | +1.0% (101) | -3.5% | -3.2% |
| 4 | E1 | me≤45 | `tp20+idle3+trail30@10+psl10s-15+hold60` | +4.5% (258) | -0.1% [-2.3, +2.4] (161) | 0.54 | 1.00 | -0.6% (92) | -2.2% | -2.5% |
| 5 | SURV_15_20 | nB2-3 | `tp20+bigsell5+trail15@10+bigbuy2+hold60` | +9.8% (27) | **-17.4%** [-25.7, -9.1] (21) | 1.00 | 1.00 | -7.6% (3) | -19.3% | -19.8% |
| 6 | E1 | nB2-3 | `tp20+idle2+trail20@20+psl10s-15+hold60` | +4.8% (192) | -1.7% [-5.5, +2.5] (110) | 0.74 | 1.00 | -0.8% (44) | -4.1% | -4.1% |
| 7 | E1 | pre | `tp20+idle2+trail30@10+psl10s-15+hold60` | +3.4% (405) | +2.3% [-1.2, +6.5] (87) | 0.15 | 1.00 | n/a | -0.1% | -0.1% |
| 8 | E1 | all | `tp20+idle2+trail30@10+psl10s-15+hold60` | +3.4% (405) | -0.1% [-2.0, +2.2] (270) | 0.52 | 1.00 | -0.3% (133) | -2.4% | -2.5% |
| 9 | E1 | dev2-4 | `tp30+idle2+trail10@30+psl10s-15+hold60` | +4.8% (132) | -2.6% [-6.0, +0.9] (76) | 0.89 | 1.00 | -0.7% (37) | -5.6% | -5.0% |
| 10 | E1 | me≤40 | `tp30+ins_creator+idle2+trail15@20+hold60` | +3.0% (182) | -0.2% [-2.4, +2.4] (111) | 0.56 | 1.00 | +1.5% (74) | -2.9% | -2.6% |
| 11-20 | E1 / SURV | various | see JSON | +1.7% to +8.6% | -15.3% to +1.7% | ≥ 0.29 | 1.00 | | | |

**Reading.**
- Train EVs of +3% to +12% shrink to about 0% on test. This is the usual winner's curse.
- Nothing in protocol A survives BH.
- Every survivor entry that looked good on train (SURV_15_*) is strongly negative on test, -10% to -17%.

### 2b. Protocol B: selected on even days, evaluated on odd days (BH over 20)

| # | Entry | Filter | Exit | Even EV (n) | Odd EV [90% CI] (n) | p | BH q | Deep (n) |
|---|---|---|---|---|---|---|---|---|
| 1 | E1 | dev<1 | `tp40+ins_creator+idle10+psl3s-15+hold60` | +10.0% (80) | +2.9% [-2.2, +9.1] (63) | 0.17 | 0.60 | -4.3% (23) |
| 3 | E1 | pre | `tp60+ins_creator+idle2+psl3s-15+hold60` | +4.1% (260) | +1.2% [-1.1, +3.7] (232) | 0.20 | 0.60 | n/a |
| 4 | E1 | all | `idle2+trail15@20+bigbuy2+hold300` | +3.3% (358) | +0.6% [-1.4, +2.7] (317) | 0.30 | 0.71 | -0.6% (133) |
| 5 | E1 | me≤50 | `bigsell2+idle2+bigbuy2+hold60` | +3.1% (276) | **+2.0%** [-0.1, +4.0] (237) | 0.057 | 0.60 | +1.1% (101) |
| 11 | E1 | nB2-3 | `tp20+idle3+trail30@20+hold60` | +3.3% (156) | +2.2% [-2.1, +7.0] (146) | 0.19 | 0.60 | -1.5% (44) |
| 17 | E1 | nB4+ | `tp20+ins_any+bigsell1+idle5+hold60` | +3.7% (48) | +5.1% [-1.8, +11.9] (49) | 0.11 | 0.60 | +2.9% (15) |
| 18 | SURV_25_0 | post | `tp80+ins_creator+bigsell5+hold900` | +11.0% (30) | -21.2% [-28.3, -13.7] (31) | 1.00 | 1.00 | -14.2% (24) |

Again nothing passes BH. However, the E1 short-exit policies (#4, #5, #10, #11) are mildly positive on odd days and about 0 to +1% on the deep holdout. This pattern pointed to the stage-2 family.

### 2c. Top 15 on the TEST set (as requested), with BH over all 25.4M configurations

| # | Entry | Filter | Exit | Train EV (n) | Test EV [90% CI] (n) | p (normal) | BH q | Even / Odd | Deep (n) |
|---|---|---|---|---|---|---|---|---|---|
| 1 | SURV_25_10 | dev<1 | `tp80+ins_creator+bigsell5+bigbuy2+hold60` | -14.4% (17) | +17.0% [+5.4, +30.3] (22) | 1.4e-2 | 1.00 | +7.1% / -2.2% | -12.8% (2) |
| 2 | SURV_25_10 | dev<1 | `tp80+ins_creator+trail30@20+bigbuy2+hold60` | -14.0% (17) | +18.3% [+4.8, +32.6] (22) | 1.7e-2 | 1.00 | +5.0% / +3.1% | -12.8% (2) |
| 3 | SURV_25_10 | dev<1 | `tp80+ins_creator+trail30@30+bigbuy2+hold60` | -14.0% (17) | +18.2% [+4.7, +32.6] (22) | 1.8e-2 | 1.00 | +5.0% / +3.1% | -12.8% (2) |
| 4 | SURV_25_10 | dev<1 | `tp80+ins_creator+bigbuy2+hold60` | -14.0% (17) | +18.2% [+4.7, +32.6] (22) | 1.8e-2 | 1.00 | +5.0% / +3.1% | -12.8% (2) |
| 5 | E1 | me>50 | `tp100+ins_creator+idle2+trail15@10+hold300` | -5.6% (91) | +12.2% [+4.2, +21.5] (71) | 9.1e-3 | 1.00 | +3.6% / +1.2% | -6.5% (32) |
| 6 | E1 | me>50 | `tp100+ins_creator+idle2+trail15@10+hold900` | -5.6% (91) | +12.2% [+4.2, +21.5] (71) | 9.1e-3 | 1.00 | +3.6% / +1.2% | -6.5% (32) |
| 7 | SURV_25_10 | dev<1 | `ins_creator+bigsell5+bigbuy2+hold60` | -14.4% (17) | +15.6% [+4.8, +27.9] (22) | 1.6e-2 | 1.00 | +5.8% / -2.2% | -12.8% (2) |
| 8 | SURV_25_10 | dev<1 | `tp100+ins_creator+bigsell5+bigbuy2+hold60` | -14.4% (17) | +15.6% [+4.8, +27.9] (22) | 1.6e-2 | 1.00 | +5.8% / -2.2% | -12.8% (2) |
| 9 | SURV_25_10 | dev<1 | `tp150+ins_creator+bigsell5+bigbuy2+hold60` | -14.4% (17) | +15.6% [+4.8, +27.9] (22) | 1.6e-2 | 1.00 | +5.8% / -2.2% | -12.8% (2) |
| 10 | E1 | me>50 | `tp100+idle10+trail20@10+psl3s-15+hold900` | -11.4% (91) | +13.7% [+4.3, +25.1] (71) | 1.3e-2 | 1.00 | -0.3% / +0.2% | -17.1% (32) |
| 11 | E1 | me>50 | `tp100+idle10+trail20@10+psl3s-15+hold300` | -11.4% (91) | +13.7% [+4.3, +25.1] (71) | 1.3e-2 | 1.00 | -0.3% / +0.2% | n/a |
| 12 | SURV_25_10 | dev<1 | `tp60+ins_creator+bigsell5+bigbuy2+hold60` | -14.4% (17) | +15.5% [+4.7, +27.8] (22) | 1.6e-2 | 1.00 | +6.0% / -2.5% | n/a |
| 13 | E1 | me>50 | `tp150+ins_creator+idle2+trail15@10+hold300` | -5.6% (91) | +12.5% [+4.2, +22.6] (71) | 1.1e-2 | 1.00 | +3.2% / +2.0% | n/a |
| 14 | E1 | me>50 | `tp150+ins_creator+idle2+trail15@10+hold900` | -5.6% (91) | +12.5% [+4.2, +22.6] (71) | 1.1e-2 | 1.00 | +3.2% / +2.0% | n/a |
| 15 | E1 | me>50 | `tp150+ins_creator+idle2+trail10@20+hold300` | -5.5% (91) | +12.4% [+4.2, +22.4] (71) | 1.1e-2 | 1.00 | +3.8% / +1.4% | n/a |

**Reading.**
- Ranking on the test set returns small, lucky subsets: 22 launches of SURV_25_10 with dev < 1, and 71 launches above 50 SOL post-bundle mcap.
- **Each one is negative on train, and negative on the deep holdout (-6% to -17%).**
- Each has BH q = 1.00. This table shows how much ranking on the test set inflates; it is not a strategy list.

## 3. Reference: the guru's rules and gap-3's combos (fixed a priori, 0.25 SOL, base costs)

| Reference policy | Entry | Train EV | Test EV [90% CI] | Even / Odd | Deep (n) | Hit rate |
|---|---|---|---|---|---|---|
| Guru: TP +100%, no stop (natural floor) | E1 | -8.6% | **-10.5%** [-13.9, -6.9] | -10.2% / -8.7% | **-21.1%** (133) | 14% |
| Guru bot: TP +100% + phantom SL 10 s −25% + sell on a 2 SOL buy + no-activity 10 s | E1 | -0.4% | -0.8% [-3.5, +2.2] | -0.4% / -0.9% | -4.6% (133) | 32% |
| Guru: buy on dev/insider sell, TP +100% | INS | -14.0% | -13.1% [-15.9, -10.0] | -13.3% / -14.0% | -22.0% (129) | 9% |
| Guru: snipe delay (end of slot+1), TP +100% | E2 | -15.8% | -16.9% [-20.2, -13.5] | -16.5% / -16.1% | -28.5% (133) | 11% |
| gap-3 best: TP +100% + idle 3 s + sell ≥ 2% of supply | E1 | +1.1% | +1.5% [-0.5, +3.7] | +1.6% / +0.9% | -2.3% (133) | 32% |
| gap-3 recommendation: TP +40% + insider sell + idle 3 s | E1 | +2.5% | +0.2% [-1.6, +2.3] | +1.8% / +1.2% | -0.2% (133) | 31% |

**Reading.**
- His TP and hold rule is clearly negative everywhere.
- His exit *features* are the right idea: they move EV from -10% to about -1%.
- What is missing is speed of exit. The profitable horizon is 1-3 seconds, not minutes.

## 4. Stage 2: the rank-1 micro-scalp, frozen on train and tested on three holdouts

**Grid.** The entry is E1. The exit grid is 54,936 policies:
- TP ∈ {5, 7.5, 10, 15, 20, 30}% or none.
- Up to 3 signal groups:
  - flow: insider, bundle or creator sell; first sell by another slot-0 sniper; any non-insider sell ≥ 0.5/1/2 SOL;
  - sell ≥ 0.5/1/2% of supply;
  - idle 1/1.5/2/3 s;
  - big buy ≥ 1/2/3 SOL;
  - phantom SL after 1 or 3 s at −10% or −15%.
- One cap: exit at slot+1/2/3/5/10, or a max hold of 10/20/30/60 s.

Combined with 6 filters and 2 sizes, that gives **659,232 configurations, selected on gap-3 train only.** The top 5 (one per size and filter) were frozen before any out-of-sample number was computed.

| # | Size | Filter | Exit (frozen) | Train | Gap-3 test | Odd days | Post-Oct-9 | Deep holdout | **Fresh holdout** | Launches/day |
|---|---|---|---|---|---|---|---|---|---|---|
| 1 | 0.5 | me≤45 | `tp20+idle3+bigbuy3+psl1s-10+hold20` | +5.5% (258) | +0.6% [-1.5, +3.0] (161) | +3.4% [+1.0, +5.9] | -0.2% [-2.8, +2.1] (103) | +0.9% [-2.9, +5.7] (92) | +3.1% [+0.5, +5.8] (171) | 4,903 |
| 2 | 0.5 | me≤50 | `tp20+idle2+bigbuy3+hold10` | +5.3% (314) | +0.1% [-1.9, +2.2] (199) | +2.8% [+0.6, +5.2] | -0.5% [-2.6, +2.1] (129) | +2.2% [-2.0, +7.0] (101) | +3.0% [+0.5, +5.7] (199) | 5,443 |
| **3** | **0.5** | **multi** | **`tp15+ins_bundle+idle1+psl1s-15+slot+5`** | **+5.3%** (288) | **+3.5% [+1.4, +5.8]** (200) | **+5.1% [+2.8, +7.5]** | **+3.6% [+1.0, +6.7]** (133) | **+3.2% [-1.0, +7.5]** (77) | **+3.7% [+0.7, +6.6]** (150) | 3,750 |
| 4 | 0.25 | me≤45 | `tp20+idle3+bigbuy3+psl1s-10+hold20` | +4.6% (258) | -0.2% [-2.3, +2.2] (161) | +2.6% [+0.1, +5.1] | -1.0% [-3.6, +1.3] (103) | +0.1% [-3.7, +4.8] (92) | +2.3% [-0.3, +4.9] (171) | 4,903 |
| 5 | 0.25 | me≤50 | `tp20+idle2+bigbuy3+hold10` | +4.5% (314) | -0.7% [-2.7, +1.4] (199) | +2.0% [-0.3, +4.3] | -1.3% [-3.4, +1.3] (129) | +1.4% [-2.8, +6.2] (101) | +2.2% [-0.4, +4.9] (171) | 5,443 |

**Multiple testing.**
- **Across the 5 frozen policies × 3 OOS sets (15 tests):** policy #3 has gap-3 test p = 0.0025 (**q = 0.037**), fresh p = 0.017 (q = 0.11) and deep p = 0.10 (q = 0.21).
- **On the fresh holdout alone:** all 5 frozen policies are positive. BH over those 5 gives q = 0.048 for #1-#3 and 0.082 for #4-#5.
- **Pooled OOS for #3:** gap-3 test + deep + fresh, with gap-3 weights rescaled to a mean of 1, gives **+3.5% [+1.9, +5.2], n = 427, bootstrap p = 0.0002.**

**Hit rate, average win and average loss (#3).**

| Set | Hit rate | Avg win | Avg loss |
|---|---|---|---|
| gap-3 test | 40% | +18.2% | -6.4% |
| fresh holdout | 42% | +19.9% | -8.1% |
| deep holdout | 38% | +21.0% | -7.5% |

Across the pooled OOS set, the per-trade SD is 19.8%. The median trade is -1.2%, the 10th percentile -9%, the 90th +29%, and the worst -66%.

**Regimes (#3).** Pre-Oct-9 is +4.8% (n = 355); post-Oct-9 (200 ms slots) is +3.6% (n = 133). EV is positive on **16 of 17 days**, with daily EV between -1.5% and +14.8% (n = 18-139 per day).

### 4a. Is #3 a spike or a plateau? (post-hoc sensitivity, not used for selection)

**Neighbourhood.** We varied TP ∈ {10, 15, 20}%, idle ∈ {1, 1.5, 2} s and cap ∈ slot+{3, 5, 10}, with or without the bundle-sell exit and with or without the phantom SL. That is 108 policies at 0.5 SOL on multi-wallet bundles. **Every one is positive on pooled OOS: median +3.5%, range +2.3% to +4.2%.** Their train median is +4.7%. The edge is the structure (enter rank 1, leave within a few slots), not one tuned threshold.

**Size and filter.**

| Size | Multi-wallet bundles | All | me ≤ 50 |
|---|---|---|---|
| 0.1 SOL | +0.2% [-1.4, +1.9] | -0.4% | -0.6% |
| 0.25 SOL | +2.7% [+1.0, +4.3] | +2.1% [+0.9, +3.3] | +1.8% [+0.5, +3.2] |
| 0.5 SOL | +3.5% [+1.9, +5.2] | +2.9% [+1.7, +4.2] | +2.6% [+1.4, +4.0] |
| 1.0 SOL | +4.0% [+2.3, +5.6] | +3.3% | +3.1% |

- At 0.1 SOL the fixed 0.004 SOL cost (4%) eats the edge.
- Larger sizes look better in the model only because fixed cost per SOL falls. In reality a bigger rank-1 buy knocks out more of the snipers we sell to (§5), and the guru says ruggers dump on big buys. We do **not** recommend more than 0.5 SOL.

## 5. Stress tests for the lead (#3, 0.5 SOL, multi-wallet bundles)

**Entry premium × fixed cost, pooled OOS EV.**

| Premium → | 0% | +1% | +2% | +3% | +4% | +5% | +6% | +8% | +10.8% (rank 3) |
|---|---|---|---|---|---|---|---|---|---|
| 0.004 SOL/round trip | +4.7% | +3.6% | +2.6% | +1.5% | +0.4% | -0.5% | -1.5% | -3.4% | -6.0% |
| 0.01 SOL | +3.5% | +2.4% | +1.4% | +0.3% | -0.8% | -1.7% | -2.7% | -4.6% | -7.2% |
| 0.02 SOL | +1.5% | +0.4% | -0.6% | -1.7% | -2.8% | -3.7% | -4.7% | -6.6% | -9.2% |

**Exit latency.** Results on gap-3 test / deep / fresh:

| Latency | Gap-3 test | Deep | Fresh |
|---|---|---|---|
| 0 slots | +3.4% | +2.4% | +4.2% |
| 1 slot (base) | +3.5% | +3.2% | +3.7% |
| 2 slots | +3.3% | +1.9% | +2.8% |
| 4 slots | +2.9% | +2.5% | +1.2% |

**Sniper drop.** This counterfactual removes a random fraction of the third-party slot-0 wallets with all their trades, and re-prices the curve from the token flows. It stands for snipers whose slippage limit fails because our rank-1 buy moved the price, or who were never there.

| Snipers removed | Gap-3 test | Deep | Fresh |
|---|---|---|---|
| 0% (base) | +3.5% | +3.2% | +3.7% |
| 25% | +2.2% | +1.8% | +1.9% |
| 50% | +0.7% | +0.4% | +0.6% |
| 100% | -1.8% | +0.0% | -1.8% |

**Reading.**
- The policy is robust to costs up to about 0.01-0.015 SOL per round trip, to any exit latency we tested, and to losing a quarter of the snipers.
- It is **not** robust to being third. Entry rank is everything.

## 6. Why it works (mechanism, gap-3, descriptive)

**Where the PnL comes from** (#3 on all multi-wallet launches, n = 488, EV +4.4%; median hold 1.6 s, 90th percentile 2.4 s):

| Exit reason | Share of trades | Average return | Contribution to EV |
|---|---|---|---|
| TP +15% | 22% | **+31%** | **+6.9 points** |
| 1 s idle | 46% | -1.8% | -0.8 points |
| Forced exit at slot+5 | 22% | -2.4% | -0.5 points |
| Bundle-wallet sell | 8% | -8.3% | -0.7 points |
| Phantom SL | 2% | -22% | -0.4 points |

- **The mechanism.** On about one launch in five, the slot-0/1 snipers who buy after us lift the curve 15-40% within two or three slots, and we sell into them. On the rest, we leave within about a second for roughly the round-trip cost.
- **The trap it avoids.** The insiders' first sell comes at a median 9 s (gap-3). By then this policy has been out for several seconds, which is why it avoids the -20% to -40% hold losses.
- **The other frozen policies work the same way.** Their TP +20% leg fires on about 20% of entries and averages +34-36%; idle exits average -3% to -5%.

## 7. Verdict

- **Is there a positive out-of-sample policy after costs? Yes, as a candidate, under one strict condition: we must be the first buyer after the bundle.**
  - The rank-1 micro-scalp (#3) is positive on train, test, odd days, post-Oct-9, the deep sample, the fresh holdout and 16 of 17 days.
  - It sits on a plateau of 108 neighbouring settings.
  - Pooled OOS it makes **+3.5% per trade [+1.9, +5.2]**, or ≈ 0.017 SOL per 0.5 SOL trade.
- **How confident are we?**
  - It does **not** survive a correction over the 26 million configurations of the whole S1 search (stage 1 plus stage 2), so the search alone cannot certify it.
  - What certifies it is the protocol. It was frozen on train, it is positive on a holdout pulled after the freeze (p = 0.017; BH q ≤ 0.11 under every count we tried), and the pooled OOS p is 0.0002.
  - Our honest label is **LIKELY real in the historical data, UNPROVEN live.**
- **His method versus ours.**
  - The guru's thesis ("buy the rugger's launch at block 0, sell at +100% before the dump") is wrong at +100%: -10% on test, -21% on the deep holdout.
  - His instinct about speed and protective exits is right.
  - The version that pays is a mix: his block-0 entry behind the bundle, then our exit after 1-3 seconds instead of minutes. It sells to the other snipers, not to the retail he describes.
- **Capacity.**
  - Multi-wallet bundled launches are ≈ 3,750/day in the population.
  - Pro snipers win slot 0 on about half of bundled launches (gap-2), so we will not be first on most of them.
  - If we win rank 1 on 10-20% of them at ≤ +2% premium, that is 375-750 trades/day × 0.5 SOL × ≈ +2.6-3.6%. This is an upper-bound illustration, not a forecast.
  - Realised PnL depends on the landing rate and premium, which only a live test can measure.

**If you take one thing from this report:** the money is in the first 1-3 seconds after the bundle, and only for whoever is first behind it. Everything slower, including the guru's +100% target, survivor entries, dip-buying and copy-the-dev-sell, loses money out of sample.

## 8. What must happen before money moves

1. **Bot logic, matching the frozen policy exactly.**
   - **Entry filter:** only launches where the create's bundle contains at least one non-creator wallet ("multi").
   - **Entry:** 0.25-0.5 SOL, sent to land right behind the bundle, with **`max_sol_cost` capped at post-bundle price +2%**. A fill at rank 3 or later fails cheaply instead of filling at a loss.
   - **Exit:** whichever comes first of realised +15%, any bundle-wallet sell, 1 s with no trade, a −15% price stop after 1 s, or a hard exit by slot+5.
   - **Exit transactions:** pre-signed, priority fee at most about 0.005 SOL.
2. **Shadow mode first (no money).**
   - Record every qualifying launch, our would-be landing index, and the simulated fill.
   - Confirm that the shadow EV matches the tape simulation on launches where our tx would have been first.
3. **Capped live test.**
   - About 200 trades at 0.25 SOL. At the measured SD of 19.8%, 200 trades give an SE of 1.4%.
   - **Kill switch:** stop if the mean is below 0 after 100 trades (≈ 4% chance if the true EV is +3.5%), or if the median entry premium exceeds +3%.
   - Track the realised landing rank and premium, the share of snipers behind us, and the realised exit slot.
4. **Re-run this sweep weekly on fresh tapes.** Alpenglow (≈ Oct 21) changes slot timing and may move this edge.

## 9. Caveats

- **This is a simulation on historical tapes.**
  - The model assumes others' trades stay the same in token terms when we are present. The sniper-drop stress bounds this, but the reflexive response is unknown: snipers or insiders may react to a consistent rank-1 buyer, for example with the guru's "farming".
  - Fills assume we see our own landing and can sell one slot later.
- **Rank 1 is assumed and then stressed through the premium.** gap-2 shows pros take slot 0 on about half of bundled launches. Our infrastructure's rank is unmeasured; that is the single biggest unknown.
  - Creates in the last 5-15% of a block cannot be reached in slot 0. The slippage cap should turn those into cheap failures, but the costs of failed attempts are not in the EV.
- **Selection history.**
  - The stage-2 family was chosen after we had seen stage-1 test and deep results, so the gap-3 test and deep numbers for #3 are not fully clean.
  - The fresh holdout is clean, but it covers only 150 qualifying launches over 48 h (Oct 12-14), the same period as the deep sample and the end of the gap-3 test window. That is one market regime.
- **Sampling.**
  - gap-3 is a 1.27% sample with stage-3 weights.
  - Trades per day are population-scaled class sizes, not achievable trade counts.
- **Fees** are the 1.25% per side observed on 2026 TradeEvents. Any fee change moves the edge one-for-one.
- **Partial exits** (the guru's 5 TP levels) are not modelled separately. EV is linear in the position split, so splitting a position between two policies yields the average of their EVs, up to second-order impact effects. It can lower variance, not raise EV.
- **Tools and data not used in this task:** mayhem coins, operator-conditioned selection and copy-trading are covered by sibling tasks. The consolidation signal was not re-tested (gap-3: too rare to matter).

## Reproduce

```
.venv/bin/python -I analysis/scripts/strategy/sweep.py            # stage 1 grid + protocols A/B/C (≈ 3.5 min)
.venv/bin/python -I analysis/scripts/strategy/reference.py        # guru / gap-3 reference rows
.venv/bin/python -I analysis/scripts/strategy/refine.py select    # stage 2, train only -> frozen_policies.json
.venv/bin/python -I analysis/scripts/strategy/pull_holdout.py     # fresh holdout tapes (swap-api, ~20 min, cached)
.venv/bin/python -I analysis/scripts/strategy/refine.py evaluate  # frozen set on test / deep / fresh + stress
.venv/bin/python -I analysis/scripts/strategy/lead_check.py       # pooled OOS, size/filter, 108-neighbour plateau
.venv/bin/python -I analysis/scripts/strategy/lead_breakeven.py   # premium x cost grid, per-day stability
.venv/bin/python -I analysis/scripts/strategy/mechanism.py        # exit-reason breakdown
.venv/bin/python -I analysis/scripts/strategy/report_tables.py sweep|refine
```

Results are in `data/reports/strategy/`: `sweep_results.json`, `frozen_policies.json`, `refine_results.json`, `lead_check.json`, `lead_breakeven.json`, `reference.json`, `mechanism.json` and `sweep_log.txt`. Per-entry simulation caches are in `data/cache/strategy/`.
