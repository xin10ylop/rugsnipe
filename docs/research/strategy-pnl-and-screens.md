# Daily P&L model, and quick screens of three untested angles (task G3)

Date 2026-10-14. Companion pages:
* `strategy-ledger.md`: every angle tested so far, with its best out-of-sample number.
* `docs/04-strategy.md`: the LEAD policy itself.

**Code**
* `analysis/rugsnipe_analysis/strategy/pnl_model.py`: the model.
* `analysis/rugsnipe_analysis/strategy/screens.py`: the screen engines.
* `analysis/scripts/strategy/pnl_run.py`, `screens.py`, `screens_grad.py`, `screens_pull_grad.py`: the runners.

**Results**
* `data/reports/strategy/pnl_model.json`
* `data/reports/strategy/screens_results.json`
* `data/reports/strategy/screens_preregistration.json`: written 15:28:02 UTC, before any out-of-sample number of the screens existed.

Nothing was committed.

## 0. Answer in one screen

**Is +2.5% to +3.7% per trade "too low", and is it based on many trades per day?**

The per-trade figure is small because each trade is a 1-2 second round trip that sells to the snipers who land behind us. It is meant to be multiplied by a large count:

* About **6,200 qualifying launches per day.** These are multi-wallet bundled, SOL-quoted, non-mayhem launches: 12.5% of the 49,300 creates per day.
* About **5,750 of them are reachable** in the creation slot.

What we earn per day is set by three things:
1. **How many of those we land at rank 1 inside the +2% cap.**
2. **What a failed attempt costs.**
3. **Which launches we attempt at all.**

It is not set by the per-trade number. Squeezing the per-trade number with more exit tuning does not work. Every post-entry refinement tried so far is at or below the LEAD (ledger 50-55, 73). A pure time exit at slot+1 earns the same.

### P&L at a 20% rank-1 landing rate

Base assumptions:
* 0.003 SOL per failed on-chain attempt; 80% of lost attempts land and fail;
* +1.1% premium when landed; 0.004 SOL per landed round trip;
* 24 h traded.

Brackets are 5-95% of a day, including parameter uncertainty and daily noise.

| Policy | EV per landed trade | Landed / day | **SOL / day** | Break-even landing rate |
|---|---|---|---|---|
| LEAD 0.5 SOL, every qualifying launch (pooled OOS +3.5%) | +3.5% | 1,150 | **+9.0** [−5.0, +20.3] | **12.1%** |
| LEAD 0.5 SOL, window classes (quiet −0.9%, busy +9.4%) | +2.5% | 1,150 | +3.2 [−7.9, +10.7] | 16.2% |
| LEAD 0.25 SOL (pooled +2.7%) | +2.7% | 1,150 | **−3.4** [−13.2, +2.5] | **26.5%** |
| Busy third only, perfect label, 0.5 SOL (+9.4%) | +9.4% | 377 | +14.1 [+7.9, +19.6] | 4.9% |
| "Busy half at +9%" as specified (hypothetical; above what a perfect label gives on the window), 0.5 SOL | +9.0% | 575 | +20.3 [+11.6, +27.9] | 5.1% |
| Busy half at +9%, 0.25 SOL | +8.2% | 575 | +6.2 [+1.2, +9.6] | 10.5% |
| Top half by a slot-0 predictor of AUC 0.75, 0.5 SOL (simulated: +4.2%) | +4.2% | 575 | +6.4 [−1.1, +12.4] | 10.3% |

**Reading**

* **Failed attempts decide the business.**
  * At 0.003 SOL per failure, the LEAD needs a 12% landing rate at 0.5 SOL and 26% at 0.25 SOL just to break even.
  * If a failure costs 0.0005 SOL (bid as an in-transaction tip that reverts), the break-even rate falls to 2.2% and 5.7%.
  * The bid path (§1.4) is worth as much as any strategy refinement.
* **0.25 SOL is not the safe choice.** It cuts the SOL earned per landed trade by more than half (2.7% of 0.25 against 3.5% of 0.5), but the failure cost per attempt stays the same.
* **Selection is a landing-rate hedge.**
  * Skipping quiet launches halves the attempts, and therefore the failure costs.
  * At low landing rates a selective policy wins: at 10%, LEAD −2.4 SOL/day against +4.8 for a perfect busy label.
  * At high landing rates the unselected LEAD wins on volume: at 50%, +43 against +42 SOL/day.
  * A realistic predictor (AUC 0.75) only gets to about +4.2% per trade. +9% per trade needs a near-perfect label (§1.3).
* **The pessimistic fill model makes the LEAD negative.** The critic's "combined realistic" model is −1.07% per trade. At a 20% landing rate it gives −17 SOL/day. The live probe exists to decide between that model and the tape model.

### Quick screens (pre-registered, time-split)

| Screen | Verdict |
|---|---|
| (a) Graduation momentum: scalp the first PumpSwap trades after the curve completes | **LOSES**: −2.6% [−3.7, −1.5] pooled, test half −3.2% (53 valid pools); 0 of 18 configs positive on either half. |
| (b) "Second wave" after a LEAD take-profit | **LOSES**: pooled −0.9% [−4.5, +3.2] (n = 61), window **−5.6%** [−8.1, −3.1] (n = 86). The pullback is the unwind. |
| (c) 18-21 UTC and create-rate conditioning | **NOT REAL.** The 18-21 UTC hours are +11.0% on the two sets that suggested them (n = 69), but 6 points *worse* than the other hours on the independent sets (train −6.05 [−13.1, +1.7], deep −6.04). Create rate: no test has BH q below 0.28. |

## 1. The daily P&L model

### 1.1 Structure

`pnl_model.py` treats each qualifying launch we fire at (an *attempt*) as one of three outcomes:

| Outcome | Probability | P&L |
|---|---|---|
| **Rank 1 inside the +2% cap** | L1 (times a per-class multiplier) | S × ev_class(prem, c_rt) |
| **Rank 2 inside the cap** | L2 | S × ev2 (the critic measured +0.09%, about 0) |
| **Lost** | the rest | a share f_pay lands and fails on chain and pays c_fail; the rest drop and pay nothing |

```
ev_class(prem, c_rt) = ev_class(base) − slope × (prem − 1.1%) − (c_rt − 0.004)/S
SOL/day = Σ_hours opp[h] × reach × Σ_classes share_k × traded_k ×
          [L1_k·S·ev_k + L2·S·ev2 − (1 − L1_k − L2)·f_pay·c_fail]
```

* The **break-even landing rate** solves SOL/day = 0 for L1.
* **Uncertainty** is a Monte Carlo over:
  * the per-class EV (normal with its bootstrap SE);
  * c_fail, uniform over 0.002-0.005 SOL at base;
  * the landed premium;
  * the opportunity count (±15%);
  * day-to-day noise (per-trade SD 0.21 × S × √landed).

### 1.2 Inputs and where each comes from

| Input | Value | Source |
|---|---|---|
| Creates per day, by hour | 49,325/day; peak 2,780/h at 20 UTC, trough 1,392/h at 08 UTC | gap-1 create index (789,186 successful creates, 16 days) |
| Qualifying share | 12.5% of creates (1,069 of 8,531 in the window; 915 with complete tapes) | dense window `creates.jsonl` + `trades_meta` s0 blocks |
| Opportunities per day | 6,181 (docs/04 used 3,750 from the gap-3 population scaling; the definitions differ by the slot-0 dev-buy requirement) | product of the two rows above |
| Reachable share | 0.93 (6.8% of creates sit in the last 5% of the block) | gap-2 §6 |
| EV per landed trade, base | 0.5 SOL: pooled +3.49% [+1.9, +5.2]; window quiet −0.89%, busy +9.40%. 0.25 SOL: pooled +2.66%; quiet −1.71%, busy +8.53% | re-measured here, identical to docs/04 |
| EV slope per premium point | 1.04 pooled (window 1.03 busy, 1.10 quiet) | re-measured at premiums 0 / 1.1 / 2.0 / 3.6% |
| Fixed cost per landed round trip | 0.004 SOL base; 0.01 competitive | docs/03 §6 (landed block-0 buy median 0.004, p75 0.014) |
| Failed-attempt cost | 0.003 SOL base (range 0.002-0.005); 0.0005 if the bid is a reverting in-transaction tip | gap-2 §4 (first-winner bid p50 0.0034 SOL); gap-5 (a failing slippage buy burns ~49k CU, and v1's fixed priority fee is paid on failure); docs/03 §6 |
| Share of lost attempts that pay | 0.8 | gap-2 §3: of slot-0 attempts, 22-27% fail on slippage on chain and 29% are guard aborts; a late transaction still lands in slot+1 and fails on the cap |
| Rank-2 fills inside the cap | L2 = 0 base, 15% as a variant; ev2 = +0.09% | `critic_audit.json` (rank 2 with a +2% cap: 69% land, EV +0.09%) |
| Per-trade SD | 0.21 | pooled OOS |

### 1.3 What a busy-launch predictor is worth (simulated on the window's own trades)

We give each launch a score with a binormal model of the stated AUC and trade the top half. EV per trade (window, 0.5 SOL; unselected +2.48%):

| AUC | 0.55 | 0.60 | 0.65 | 0.70 | 0.75 | 0.85 | 0.95 | perfect |
|---|---|---|---|---|---|---|---|---|
| Top half | +2.8% | +3.1% | +3.5% | +3.8% | +4.2% | +4.8% | +5.6% | +5.9% |
| Top third | +2.9% | +3.4% | +3.9% | +4.4% | +4.9% | +6.0% | +7.6% | **+9.3%** |

**"+9% on the busy half" is not available on this window, even with a perfect label.**
* Only a third of the window's launches are busy.
* A perfect label gives +9.3% on that third.
* The top half with a perfect label gives +5.9%.
* G2 measured the post-entry follow-through proxy at AUC 0.58-0.65.

The 154 multi-wallet launches whose tapes were too hot to download are probably busy. So the true busy share may be nearer 40%, and these numbers are slightly conservative.

### 1.4 Sensitivity (LEAD 0.5 SOL, pooled OOS EV, 24 h; SOL/day)

| Premium when landed | c_fail (SOL) | Break-even L1 | L = 5% | 10% | 15% | 20% | 30% | 50% |
|---|---|---|---|---|---|---|---|---|
| 0% | 0.0005 | 1.7% | +4.5 | +11.2 | +18.0 | +24.8 | +38.3 | +65.4 |
| 0% | 0.003 | 9.4% | −6.5 | +0.9 | +8.2 | +15.6 | +30.3 | +59.7 |
| 0% | 0.01 | 25.7% | −37.0 | −28.1 | −19.1 | −10.2 | +7.7 | +43.6 |
| **+1.1%** | 0.0005 | 2.2% | +2.8 | +8.0 | +13.1 | +18.2 | +28.5 | +49.0 |
| **+1.1%** | **0.003** | **12.1%** | −8.1 | −2.4 | +3.3 | **+9.0** | +20.4 | +43.3 |
| +1.1% | 0.01 | 31.4% | −38.7 | −31.4 | −24.0 | −16.7 | −2.1 | +27.2 |
| +2.0% | 0.003 | 15.8% | −9.4 | −5.1 | −0.7 | +3.7 | +12.4 | +29.8 |
| +3.6% | 0.0005 | 8.2% | −0.9 | +0.5 | +1.9 | +3.3 | +6.1 | +11.7 |
| +3.6% | 0.003 | 34.9% | −11.8 | −9.8 | −7.9 | −5.9 | −1.9 | +6.0 |

**Same table for the other policies, at base (+1.1%, 0.003 SOL)**

| Policy | Break-even L1 | L = 5% | 10% | 20% | 30% | 50% |
|---|---|---|---|---|---|---|
| LEAD 0.25 SOL | 26.5% | −11.2 | −8.6 | −3.4 | +1.8 | +12.2 |
| LEAD 0.5, window classes | 16.2% | −9.5 | −5.3 | +3.2 | +11.7 | +28.8 |
| Busy third (perfect), 0.5 | 4.9% | +0.1 | +4.8 | +14.1 | +23.4 | +42.0 |
| Busy half at +9%, 0.5 | 5.1% | −0.1 | +6.7 | +20.3 | +34.0 | +61.2 |
| Busy half at +9%, 0.25 | 10.5% | −3.6 | −0.3 | +6.2 | +12.8 | +25.9 |
| Predictor AUC 0.75, top half, 0.5 | 10.3% | −3.6 | −0.2 | +6.4 | +13.1 | +26.5 |

**Other scenarios (LEAD 0.5 SOL, L1 = 20%)**

| Scenario | SOL/day | Break-even L1 |
|---|---|---|
| Base | +9.0 | 12.1% |
| "Competitive": 0.01 SOL per round trip, landed premium 1.1-3.6% | −3.2 [−22.1, +7.1] | 26.1% |
| 15% of attempts fill at rank 2 inside the cap | +11.5 | 9.9% |
| Contested busy launches (landing rate on busy launches × 0.6; window classes) | −4.2 | 28.8% |
| Critic "combined realistic" fills (EV −1.07%) | −17.2 | none |
| Hours traded: 12-24 UTC only / 00-12 UTC only | +5.4 / +3.7 | 12.1% |

EV per trade does not depend on the hour (§3c), so traded hours scale SOL/day by the opportunity count.

* **Contested busy launches.** Busy launches attract more slot-0 snipers: 62% have a third-party slot-0 buyer, against 48% of quiet ones. If that cuts our landing rate on them by 40%, the 0.5 SOL LEAD on window classes falls from +3.2 to −4.2 SOL/day at L1 = 20%. The busy-only policy falls from +14.1 to +6.7.

## 2. The ledger

See `docs/research/strategy-ledger.md`: 80 rows, plus a ranked list of 11 untested angles with the data each needs.

## 3. Quick screens

Every rule, grid and pass criterion below was written to `screens_preregistration.json` at 15:28:02 UTC. That was before any out-of-sample number of (b) or (c) existed, and before the graduation tapes finished downloading.

Common settings:
* 0.5 SOL;
* +1.1% premium;
* 0.004 SOL per round trip;
* 1-slot exit latency;
* 1.25% fee per side.

### 3a. Graduation momentum: LOSES

**Rule (pre-registered)**
* Universe: the 81 graduations of `data/cache/graduated_window.json`, Oct 13 12:11 to Oct 14 07:21 UTC.
* Entry `G_MIG`: react to the migrate transaction and land first in the next slot.
* Entry `G_FIRST`: react to the first PumpSwap trade and land first in the next slot.
* Primary exit: the first of realised +15%, 1 s with no trade, −15% after 1 s, or landing slot + 5. There is no bundle-wallet leg on a pool.
* Grid: TP {10, 15, 30}% × cap {slot+5, 10 s, 60 s} × 2 entries = 18 configurations, BH over 18.
* Split: first half by completion time = look; second half = test.

**Data**
* 199 swap-api requests in total (≤ 300 cap, ≤ 36/min, 35 of them rate-limit retries). 52 of them were a first pass that tried 3 pages at +120 s and hit busy coins.
* Per coin: one page ending at migrate + 15 s; if that did not reach back to the completion, one ending at + 5 s; for quiet coins, one ending at + 120 s.
* Completion and migrate slots come from gap-5's `grad.json`.

**Exclusions**
* 10 coins have no known migrate time.
* 8 are mayhem completions.
* 2 had more than 100 trades in the first 5 s, so we never reached the completion. These are the hottest coins, so this is a bias against the strategy, of at most 2 coins.
* **8 have a non-standard pool.** Their curve and pool trades price at 0.1-9 SOL of market cap, below the 27.96 SOL curve floor. They have no CompleteEvent, and their implied constant-product K is 45-9,000 times too small. These are most likely non-SOL-quoted or non-standard-supply coins that the coin API lists with a SOL quote.

  **Honest record:** this exclusion was added after a first pass over 60 coins in which those 8 pools made `G_FIRST` look like +1.6% to +2.0% pooled (q = 0.75). It is a data-validity rule (the pool must match the pre-registered pool model), not an outcome filter. Without it every `G_MIG` cell was still −7.4% to −9.5%.

That leaves **53 coins**.

**Pool model check.** On the 53 valid pools, the constant product implied by consecutive post-trade prices and each trade's token amount has a median of 1.7585e10 (1,629 trade pairs), against 1.7584e10 assumed (84.99 SOL × 206.9M tokens). The interquartile range is ±0.1%, so the model is exact.

| Entry | TP | Cap | look | test | pooled [90% CI] | BH q |
|---|---|---|---|---|---|---|
| G_MIG | 15% | slot+5 (primary) | −2.0% (26) | **−3.2%** (27) | **−2.59%** [−3.7, −1.5] | 1.00 |
| G_MIG | 10-30% | 10 s / 60 s | −1.7% to −3.3% | −4.4% to −4.5% | −3.0% to −4.0% | 1.00 |
| G_FIRST | 15% | slot+5 | −2.4% (26) | −2.6% (27) | −2.53% [−3.6, −1.4] | 1.00 |
| G_FIRST | 10-30% | 10 s / 60 s | −1.8% to −3.3% | −3.7% to −4.3% | −2.8% to −3.8% | 1.00 |

* **No configuration is positive on either half.** The +15% TP fired on 0-3 of 53 coins.
* **Why it loses.**
  * From the post-migration state, the pool drifts up a median +0.2% after 1 s and +0.9% after 5 s (83-85% of coins are higher). The drift is real.
  * But a round trip costs about 4.4%: 2 × 1.25% fees, the 1.1% premium, 0.8% fixed cost, and about 1% of own impact on an 85-SOL pool.
  * The large moves come later and in both directions: the mean at 60 s is −18% (n = 12).
  * There is no sniper wave to sell into. The migration is a known event that every graduation bot already fires at, and the pool opens with 85 SOL of real liquidity, about 3× the depth of a fresh curve.
* **Pass rule** (primary test half > 0 and pooled lower bound > 0): **fails.**
* For live use, the metadata/IPFS latency is irrelevant here (no metadata is needed). The `CompleteEvent` → migrate gap is 0-5 slots (gap-5 §7).


### 3b. "Second wave" after a LEAD take-profit: LOSES

**Rule**
* Only on LEAD trades that exit at the +15% TP.
* Trigger: the first slot end within W seconds of our TP fill where the price is at least d below the running maximum since our entry, and no bundle wallet or creator has sold.
* Entry: buy 0.5 SOL first in the next slot, with the LEAD exits.
* Primary: d = 10%, W = 30 s.
* Grid: d ∈ {5, 10, 15, 20}% × W ∈ {10, 30} s.
* Look set: gap-3 train. Out-of-sample sets: test, deep, fresh, window.

| d / W | train (look) | test | deep | fresh | window | pooled OOS |
|---|---|---|---|---|---|---|
| 5% / 10-30 s | −6.2% (58) | −3.6% (38) | −3.3% (9) | −5.6% (23) | −6.8% (119) | −4.3% (70) |
| **10% / 30 s (primary)** | −0.6% (47) | +2.4% (31) | −7.4% (9) | −2.0% (21) | **−5.6%** [−8.1, −3.1] (86) | **−0.9%** [−4.5, +3.2] (61) |
| 15% / 30 s | −0.1% (36) | +3.4% (24) | −11.9% (5) | −3.4% (14) | −4.9% (48) | −1.0% (43) |
| 20% / 10 s | +2.0% (22) | +8.4% (11) | n/a | −2.4% (6) | −3.5% (25) | +3.8% (17) |
| 20% / 30 s | +0.8% (27) | +5.8% (14) | −4.2% (1) | −3.9% (7) | −3.5% (25) | +1.9% (22) |

Brackets in parentheses are n.

* **Multiple testing.** Every one of the 32 out-of-sample tests has BH q = 1.00.
* **Stress on the primary.**
  * One slot slower: −5.8% pooled, −6.8% window.
  * +3.6% premium: −3.4% / −7.9%.
* **Added SOL per LEAD trade** (the second wave fires on about a third of TP launches): between −0.9 and +0.35 points of the LEAD's EV across sets, negative on the window in every cell.
* **Why it loses.** The re-entry hits its own +15% TP on only 4-17% of entries (the LEAD hits it on 20-30%). Most re-entries end at the slot+5 cap or the idle exit. After the snipers have taken their 15-40%, a pullback is the unwind, not a dip to buy. This matches G2's dip re-entry (−2% to −16% per re-entry leg).

### 3c. Hour of day and create rate: NOT REAL

**The claim.** "The LEAD makes about +15% at 18-21 UTC." On the sets where it was noticed (gap-3 test + fresh), the 18-21 UTC hours give **+11.0% [+6.0, +17.3], n = 69**.

**H1.** 18-21 UTC against the other hours, on the sets that did not form the claim (the window has no 18-21 UTC launches):

| Set | 18-21 UTC | Other hours | Difference [90% CI] | p | BH q |
|---|---|---|---|---|---|
| gap-3 train | +0.05% (41) | +6.10% (247) | **−6.05** [−13.1, +1.7] | 0.91 | 0.91 |
| deep | −1.99% (11) | +4.05% (66) | **−6.04** [−15.1, +2.3] | 0.87 | 0.91 |

**LEAD EV by 3-hour block** (n in brackets). The blocks jump around from set to set: test 03-06 +25% (n = 8), deep 09-12 −12% (n = 6). This is the noise of n = 10-40 cells at a per-trade SD of 21%.

| UTC | 00-03 | 03-06 | 06-09 | 09-12 | 12-15 | 15-18 | 18-21 | 21-24 |
|---|---|---|---|---|---|---|---|---|
| train | +6.7 (31) | +7.4 (31) | −0.8 (24) | +12.7 (25) | +7.9 (40) | +4.4 (56) | **+0.0** (41) | +7.5 (40) |
| test | +2.7 (25) | +25.2 (8) | +1.1 (10) | +1.8 (21) | −0.7 (26) | −0.9 (35) | **+9.7** (37) | +4.5 (38) |
| deep | +11.3 (7) | +0.8 (10) | +15.1 (4) | −12.3 (6) | +4.7 (13) | −2.1 (11) | **−2.0** (11) | +10.4 (15) |
| fresh | −0.9 (16) | +6.3 (12) | +10.6 (5) | +0.8 (15) | +4.3 (24) | −3.0 (25) | **+12.3** (32) | +0.1 (21) |
| window | +1.4 (151, 02-03 only) | +3.4 (466) | +1.6 (298) | | | | | |

**H2 / H3: create-rate conditioning**
* Rate = creates per hour in the 60 min (H2) or 10 min (H3) before the launch, from the full gap-1 index.
* Threshold = the gap-3-train median (2,153/h and 2,118/h).
* In-sample on train, high rate is worse: −2.3 and −1.7 points.

| Test | Set | High rate | Low rate | Difference [90% CI] | p | BH q |
|---|---|---|---|---|---|---|
| H2 60 min | test | +3.77% (121) | +3.00% (79) | +0.77 [−4.1, +5.1] | 0.39 | 0.78 |
| H2 | deep | +1.54% (36) | +4.64% (41) | −3.10 [−11.0, +5.7] | 0.73 | 0.91 |
| H2 | fresh | +5.01% (82) | +2.05% (68) | +2.97 [−2.3, +8.6] | 0.18 | 0.66 |
| H2 | window (all below the threshold) | n/a | n/a | not testable | | |
| H2 within-window median split | window | +2.33% (460) | +2.64% (455) | −0.31 [−2.1, +1.4] | 0.61 | 0.91 |
| H3 10 min | test | +3.90% (120) | +2.87% (80) | +1.03 [−3.8, +5.2] | 0.35 | 0.78 |
| H3 | deep | +0.57% (36) | +5.49% (41) | −4.92 [−13.0, +3.8] | 0.84 | 0.91 |
| H3 | fresh | +5.19% (86) | +1.63% (64) | +3.56 [−1.8, +9.3] | 0.13 | 0.66 |
| H3 | window | +2.98% (36) | +2.46% (879) | +0.52 [−3.8, +4.9] | 0.43 | 0.78 |
| H3 within-window median split | window | +3.44% (469) | +1.48% (446) | +1.95 [+0.3, +3.7] | 0.025 | 0.28 |

Over 11 tests nothing passes BH, and the signs disagree across sets. The window's busy share is 29% in its high-rate half and 36% in its low-rate half, so the create rate does not even predict "busy". **Do not condition the LEAD on the hour or the create rate.** For the P&L, this means EV per trade can be treated as constant over the day: trading 24 h is strictly better, and the peak hours carry 59% of the opportunities.

## 4. What this changes in the plan

1. **The live question is the landing rate and the failure cost, not the exit.**
   * The shadow mode (docs/04 §7) must log, per attempt: the would-be rank, the realised premium, and whether the transaction would land and fail on chain.
   * Without a measured landing rate above about 12% (0.5 SOL, 0.003 SOL per failure), the LEAD does not make money at any per-trade EV we have measured.
2. **Bid path.**
   * Where the leader class allows it, put the bid in an in-transaction tip that reverts with the failed buy. Gap-5: a failed v1 transaction still pays its fixed priority fee and burns about 49k CU.
   * Use the priority-fee path only where ordering requires it (BAM, Harmonic: docs/03 §6).
   * The model values this at about +9 SOL/day at L1 = 20%: from 0.003 to 0.0005 SOL per failure, the 0.5 SOL LEAD goes from +9.0 to +18.2 SOL/day.
3. **Size.** Keep 0.5 SOL. At 0.25 SOL the break-even landing rate (26.5%) is above anything gap-2 suggests an unstaked bot gets on contested launches.
4. **Selection is the one per-trade lever left.**
   * The slot-0 busy predictor (G1) should be judged by AUC on held-out launches.
   * AUC 0.75 is worth about +1.7 points per trade on the selected half, and it halves the failed attempts.
   * Below an AUC of about 0.65 it is not worth the complexity.
5. **Drop:** the second wave, hour-of-day and create-rate conditioning, and graduation momentum.

## 5. Caveats

* **The opportunity count** combines the window's qualifying share (one 6-hour block, 02-08 UTC) with the 16-day hour profile. docs/04's 3,750/day is a narrower definition. SOL/day scales linearly with it (±15% is in the Monte Carlo).
* **Per-class EVs come from one 6-hour window at low-activity hours.** Its busy label is the API page count, which is not observable at entry.
* **Failure costs and f_pay are modelled, not measured.** That is the first thing the shadow mode must measure.
* **Reflexivity is not in the model.** A persistent rank-1 buyer at a high landing rate takes the slot-0 flow from others and may provoke dumps. The sniper-drop and first-follower stresses (ledger 45-47) are the only handle on it.
* **The screens use small n.** (b) has n = 61 pooled and 86 on the window. The (c) cells are 11-121 per group. They can rule out large effects, not small ones.
