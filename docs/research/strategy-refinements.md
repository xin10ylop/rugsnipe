# Post-entry refinements of the LEAD micro-scalp (task G2)

Date 2026-10-14. Question: can information that arrives **after our fill** (the first 1-3 slots) raise the
expectancy of the LEAD (docs/04-strategy.md §2: 0.5 SOL rank-1 entry behind the creator bundle, exit at
the first of realised +15%, bundle-wallet sell, 1 s idle, -15% phantom stop after 1 s, landing slot + 5;
base scenario +1.1% premium, 0.004 SOL per round trip, 1-slot exit latency)?

**Short answer: no.** None of the five ideas we were asked to try raises EV beyond the LEAD out of
sample. The best variant of each idea was chosen on the training split only and evaluated once, and
every one of them is at or below the LEAD on the pooled out-of-sample set and on the dense window. The
LEAD's own EV stays positive throughout. The reason is structural. The edge is the sniper flow of slots
0-1, and the LEAD already collects it: simply selling at the end of slot+1 earns about the same. What
comes after our take-profit is, on average, a fall.

Code: `analysis/rugsnipe_analysis/strategy/postentry.py` (engine),
`analysis/scripts/strategy/refinements{,_data,_diag,_gridsum}.py`. Results:
`data/reports/strategy/frozen_refinements.json` (the freeze), `refinements_results.json` (one-shot
evaluation, latency/cost/premium/sniper-drop stress), `refinements_diag.json` (mechanisms, ceilings),
`refinements_grid_summary.json` (the whole grid out of sample, diagnostic only).

## 0. Answer in one screen

| Idea | Frozen variant (best on train) | Pooled OOS vs LEAD [90% CI] | Window vs LEAD [90% CI] | Verdict |
|---|---|---|---|---|
| (1) Scale-out + runner | sell 75% at +15%, runner trails 10% from the max, idle 2 s, cap slot+10 | **-0.27** [-0.4, -0.1] | **-0.40** [-0.5, -0.3] | **loses**: 0 of 288 configs beat the LEAD on any set |
| (2) Follow-through gate | at end of slot 0: price >= +5% and >= 5 third-party buys → TP 50% + trail 15%, cap slot+5; else scalp | -0.02 [-0.3, +0.3] | -0.15 [-0.3, -0.0] | **no gain**: 2% of 18,330 configs beat the LEAD on both pooled and window |
| (3) Sell on third-party selling | LEAD with TP 20% + exit on first slot-0-sniper sell | **-0.35** [-0.7, -0.0] | +0.12 [-0.1, +0.3] | **loses** pooled; small gain only on busy window launches |
| (4) Re-entry on a dip | after a non-insider exit, re-buy 0.25 SOL on a 10% dip below our exit within 3 s | **-0.15** [-0.3, -0.0] | **-0.17** [-0.3, -0.1] | **loses**: the re-entry leg itself averages -2% to -16% |
| (5) Size ladder 0.25/0.5/1.0 | by post-bundle mcap terciles → 0.25 / 0.5 / 0.25 SOL | -0.26 [-0.8, +0.3] (vs a flat stake of the same mean size: +0.22 [-0.3, +0.8]) | -0.61 (vs matched flat: -0.12) | **no information**; flat 1.0 SOL looks +0.46 better only because the fixed cost per SOL halves |
| (6) Exit latency 0..4 slots | all of the above | no variant overtakes the LEAD at any latency on both sets | | latency hurts every variant about equally |

LEAD itself, same runs: train +5.30%, gap-3 test +3.47% [+1.4, +5.8] (n=200), deep +3.19% [-1.0, +7.5]
(n=77), fresh +3.67% [+0.7, +6.6] (n=150), pooled OOS +3.49% [+1.9, +5.1] (n=427), window +2.48%
[+1.6, +3.4] (n=915). These reproduce docs/04 exactly, which validates the new engine.

**Multiple testing.** 18,772 configurations were scored on train (plus 30 more in a superseded first
freeze, §1). The Benjamini-Hochberg q-value for "improves on the LEAD" is >= 0.9998 for every one of
them. None is distinguishable from the LEAD even in-sample. For the 48 non-flat ladders, the minimum
q for "beats a flat stake of the same mean size" is 0.77. Out of sample, BH over 5 variants × 4 sets
(20 tests) gives q = 1.00 for an improvement over the LEAD in all 20. The variants' own EV > 0 keeps
q <= 0.05 (0.002-0.043) on test, fresh and window, and q = 0.12-0.24 on deep (n=77).

**Why the LEAD's +2.5-3.7% per trade is not "under-tuned".** Post-entry tuning has very little room
left (§5):
- **A pure time exit matches the LEAD.** Selling at the end of slot+1 regardless of anything earns
  +3.35 / +2.84 / +4.02 / +2.49% on test / deep / fresh / window, against the LEAD's
  +3.47 / +3.19 / +3.67 / +2.48%. The exit legs only reshuffle the same slot-0/1 sniper flow.
- **After our +15% fill the price falls.** On the window it is -7% at 10 slots and -11% at 20 slots
  on average. Only 29-45% of take-profit launches are higher 10 slots later, and the median best price
  over the next 20 slots is only +1% to +8% above our fill. A runner has little upside to catch and
  pays the drawdown.
- **The per-trade lever is which launches we enter, not how we leave.** Skipping quiet launches with a
  perfect label would lift EV to +9.4% per trade on a third of the trades. A perfect post-entry bail on
  quiet launches lifts the window only from +2.48% to +3.44%. The observable post-entry proxy is weak:
  third-party buys behind us give AUC 0.58 at the end of slot 0 and 0.65 at slot+1 for busy versus
  quiet.

On "is it based on a lot of trades per day": yes. The figure is a per-trade expectancy on a
high-frequency class of about 3,750 qualifying launches per day (population-scaled; docs/04 §4). At
0.5 SOL, +3.5% is about 0.0175 SOL per trade. What we earn per day is set by how many of those
launches we land at rank 1 within the +2% slippage cap, not by the per-trade number.

## 1. Protocol

- **Universe.** Multi-wallet bundled launches (the LEAD universe) from four sets: gap-3 (675 bundled
  launches Sep 28-Oct 14; multi-wallet: 288 train = first 60% by create time, 200 test), deep sample
  (77), fresh holdout (150), dense window Oct 14 02:05-08:05 UTC (915; 614 "quiet" = whole 15-min history
  in one API page, 300 "busy", 1 other). Gap-3 sampling weights are kept (w=5 for the 1-in-5 subsampled
  stratum). Pooled OOS = test + deep + fresh, with gap-3 weights rescaled to mean 1.
- **Engine.** `postentry.py` reuses `sim.py`'s curve, fill and leg semantics. A signal in slot f fills
  after every trade of slots <= f + latency, at the historical curve with our outstanding tokens still
  out. Partial sells use the outstanding-token algebra
  (K/(vt - q_out) - K/(vt - q_out + q_sell)) × (1 - fee). It reproduces the LEAD to the last digit on all
  five sets. Identity checks: scale-out with x = 100% equals the LEAD, an always-pass gate that keeps the
  scalp equals the LEAD, and a flow variant with no extra leg equals the LEAD.
- **Costs.** 1.25% fee per side, own impact, +1.1% entry premium, 0.004 SOL per round trip, 0.002 SOL for
  each extra sell transaction (scale-out), and a full 0.004 SOL round trip plus the premium for a
  re-entry. Stressed below.
- **Selection.** Train only. Score = weighted mean of the per-launch paired difference
  (variant - LEAD) - 1.645 SE. A variant must change the outcome on at least 20 train launches. Ladders
  are scored by their gain over a flat stake of the same train-mean size. One variant per idea was frozen
  (`frozen_refinements.json`, 15:16:03 UTC) before any out-of-sample number existed. The frozen set was
  then evaluated once.
- **Revision note (honest record).** The first freeze (`frozen_refinements_v1_degenerate.json`, same
  data, train only) had two artefacts. Its "best" re-entry never fired: a difference of exactly 0 with
  SE 0 beat every re-entry that did fire. Its "best ladder" was the flat 1.0 SOL stake, which is the
  fixed-cost effect, not a ladder. Its second ladder feature (total bundle SOL) was also the same
  variable as post-bundle mcap. These were fixed before any out-of-sample number was computed: the
  activity >= 20 rule, flat maps moved to references, scoring against a matched flat stake, and bundle
  wallet count as the second feature. The gate grid was also widened at that point (pass-regime cap 5
  and TP 20%, fail-branch TP 8%/10%). Both files are kept.

### The grid (18,772 configurations)

| Family | What varies | Configs |
|---|---|---|
| Scale-out | sell x ∈ {0, 25, 50, 75}% at the +15% TP; runner trail {10,15,20,30}% from the running max, activation {0,10,20}%; runner idle {2,3} s; runner cap slot+{10,20,60}; runner also exits on bundle sell and the phantom stop | 288 |
| Gate | gate at end of slot {0, +1}; price >= +{5,10,20}% of entry; >= {0,1,2,3,5} third-party buys behind us; pass → keep the scalp or loose regime (TP {20,30,50,100}% or none × trail {none,15,20,30}% × idle {1,2} s × cap {5,10,20,60}); fail → scalp, exit now, or scalp with TP 8% / 10%. With a slot+1 gate the +15% TP is deferred to the gate | 18,330 |
| Flow exits | LEAD with TP {15,20,30}% × cap {5,10} + one of: first slot with net third-party selling (from slot 0 or slot+1), first third-party sell >= {0.3, 0.5, 1.0} SOL, first sell by a slot-0 sniper, net-sell + 0.5 SOL | 42 |
| Re-entry | after the scalp exit, if no insider has sold: re-buy 0.25 SOL on an end-of-slot dip >= {10,15,20,30}% below the running max or our exit price, within {3,5} s, after any non-insider exit or after TP only; re-exit by the LEAD rules with TP {10,15}% | 64 |
| Size ladder | stake per tercile of post-bundle mcap, or per bundle-wallet group (1 / 2-3 / 4+), each ∈ {0.25, 0.5, 1.0} SOL; flat maps excluded | 48 |

## 2. Frozen variants, one-shot evaluation (base scenario)

EV per trade as % of the stake (ladders: SOL P&L per SOL deployed). "d" = paired difference to the LEAD
on the same launches, with a 90% bootstrap CI by launch.

| Variant | train | test | deep | fresh | window (quiet / busy) | pooled OOS |
|---|---|---|---|---|---|---|
| **LEAD** | +5.30 | +3.47 [+1.4, +5.8] | +3.19 [-1.0, +7.5] | +3.67 [+0.7, +6.6] | +2.48 [+1.6, +3.4] (-0.89 / +9.40) | **+3.49** [+1.9, +5.1] |
| Scale-out x75% trail10 cap10 | +4.85 (d -0.45) | +3.27 (d -0.21) | +2.82 (d -0.37) | +3.37 (d -0.30 [-0.5, -0.0]) | +2.09 (d -0.40 [-0.5, -0.3]) (q -0.42 / b -0.35) | +3.22 (d **-0.27** [-0.4, -0.1]) |
| Gate s+0, p>=5%, k>=5 → TP50+trail15 cap5 | +5.26 (d -0.04) | +3.31 (d -0.16) | +3.32 (d +0.13) | +3.76 (d +0.09 [-0.5, +0.9]) | +2.33 (d -0.15 [-0.3, -0.0]) (q -0.21 / b -0.02) | +3.47 (d -0.02 [-0.3, +0.3]) |
| Flow: TP20 + sniper-sell exit | +5.37 (d +0.07) | +3.40 (d -0.07) | +1.93 (d **-1.26** [-3.0, -0.0]) | +3.41 (d -0.25) | +2.60 (d +0.12 [-0.1, +0.3]) (q +0.07 / b +0.22) | +3.14 (d **-0.35** [-0.7, -0.0]) |
| Re-entry 0.25 SOL on 10% dip within 3 s | +5.45 (d +0.15) | +3.30 (d -0.17) | +3.09 (d -0.11) | +3.51 (d -0.16) | +2.31 (d -0.17 [-0.3, -0.1]) (q -0.17 / b -0.18) | +3.34 (d **-0.15** [-0.3, -0.0]) |
| Ladder by mcap: 0.25 / 0.5 / 0.25 SOL | +5.36 (d +0.07) | +3.13 (d -0.34) | +3.13 (d -0.06) | +3.42 (d -0.25) | +1.87 (d -0.61 [-0.9, -0.3]) | +3.23 (d -0.26 [-0.8, +0.3]) |
| *ref: flat 0.25 SOL* | +4.46 | +2.64 | +2.36 | +2.84 | +1.65 | +2.66 (d -0.83) |
| *ref: flat 1.0 SOL* | +5.72 | +3.93 | +3.65 | +4.13 | +2.92 | +3.95 (d +0.46) |
| *ref: flat 0.3175 SOL (ladder's mean stake)* | +4.81 | +2.99 | +2.71 | +3.19 | +1.99 | +3.01 |

Ladder versus its matched flat stake: test +0.14, deep +0.42, fresh +0.23, window -0.12; pooled +0.22
[-0.3, +0.8]. None is significant, and the sign flips on the window (busy window launches: -0.67
[-1.2, -0.1]).

## 3. Idea by idea: what was tried, what happened, why

### 3.1 Scale-out with a runner: loses everywhere

All 288 configurations are worse than the LEAD on train, on the pooled OOS and on the window. The
median difference is -1.04 / -0.54 / -1.06 points. The loss grows monotonically with the runner share:
selling 75% at the TP costs a median -0.28 pooled, selling 0% costs -0.87. A longer runner cap does not
help (cap 10 / 20 / 60: -0.63 / -0.48 / -0.57 pooled). Train and OOS rankings agree (Spearman +0.93),
so this is a stable effect, not noise.

Why: the +15% realised TP fires when the snipers who landed behind us are still buying. Our fill (one
slot later) is close to the local top. After it, the price falls on average:

| After our TP fill | test | deep | fresh | window |
|---|---|---|---|---|
| mean price change at +5 / +10 / +20 slots | -3% / -0% / -1% | -4% / -5% / -10% | -3% / -6% / -8% | -5% / -7% / -11% |
| share higher at +10 slots | 45% | 29% | 37% | 29% |
| median best price in the next 20 slots | +8% | +8% | +4% | +1% |

A trailing stop on that path gives back more than the occasional run adds, and the extra sell
transaction (0.002 SOL) is a further small drag.

### 3.2 Follow-through gating: the signal is real, but the LEAD already harvests it

The post-entry signal itself is strong. Third-party buys landing behind us predict the LEAD's return on
every set:

| third-party buys behind us by end of slot+1 | 0 | 1 | 2 | 3-4 | 5+ |
|---|---|---|---|---|---|
| train | -2.6% | -2.7% | +1.6% | +13.4% | +19.5% |
| gap-3 test | -4.1% | -1.7% | +5.3% | +4.8% | +20.4% |
| fresh | -5.3% | +1.0% | +4.3% | -1.4% | +30.0% |
| window (share of launches) | -5.5% (18%) | -2.6% (19%) | +0.5% (13%) | +4.2% (27%) | +12.1% (23%) |

But it does not convert into a better exit. Each branch fails for its own reason:

- **Holding longer when the launch is busy** (pass → looser TP or trail) loses. The TP is already
  reached quickly on busy launches, and what follows is the same post-TP fall (§3.1). Median difference
  by pass-regime TP 20 / 30 / 50 / 100% / none: -0.13 / -0.21 / -0.47 / -0.49 / -0.42 pooled.
- **Bailing out when nobody follows** (fail → exit at the gate) cannot recover the round trip. Fees,
  premium and the fixed cost (about 4.4% of the stake) are already sunk. Pass-lead / fail-exit at slot 0
  has a median -0.16 pooled, and +1.33 on quiet window launches but -2.55 on busy ones. A slot+1 gate
  is worse than a slot-0 gate (median -0.48 vs -0.13 pooled) because deferring the +15% TP by a slot
  sells into the fall.
- **A tighter TP on the fail branch** (8% / 10%) is the least bad sub-family (median -0.07 pooled) but
  is negative on the window (-0.41).
- **Train-to-OOS instability.** The frozen gate is -0.02 pooled and -0.15 on the window. The best gate
  in hindsight is +1.07 pooled but -0.39 on the window: no configuration generalises to both. The
  train ranking predicts the pooled ranking only loosely (Spearman +0.50).

### 3.3 Selling on third-party selling: loses pooled, helps a little on busy launches only

Out of 42 configurations, 12% beat the LEAD on train and 12% on the pooled OOS, but 62% on the window.
The window gain comes only from busy launches (median +0.55 busy, -0.27 quiet). The frozen variant
(TP 20% + exit on the first slot-0-sniper sell) is -0.35 [-0.7, -0.0] pooled, -1.26 on deep, and +0.12
[-0.1, +0.3] on the window. Why: in the first second, third-party sells are other snipers taking their
own +10-20%. That is the same moment our TP fires, so the extra leg mostly sells a slot earlier at a
lower price, or fires on noise in quiet launches. Net third-party selling in a slot and large (>= 0.3
to 1 SOL) third-party sells behave the same way.

### 3.4 Re-entry on a dip: the dip is the dump

0 of 64 configurations beat the LEAD on the pooled OOS. The frozen re-entry triggers on 1-12% of
launches (deep 1%, window 5%, test 9%, fresh 12%), and its own 0.25 SOL leg averages **-2.1% (test, n=17), -16% (deep, n=1), -2.7% (fresh, n=18),
-6.4% (window, n=49)**, with hit rates of 22-35%. After the snipers' exits, a 10-30% dip is the start of
the unwind, not a pullback. There is nobody left to sell the second position to.

### 3.5 Size ladder: no information beyond the fixed-cost effect, and size is fragile to sniper attrition

- **Larger stakes look better for one reason only.** The 0.004 SOL fixed cost is 0.8% of a 0.5 SOL
  stake and 0.4% of 1.0 SOL. Flat 1.0 SOL is +0.46 pooled over 0.5 SOL (+0.44 window). The own-impact
  model is in these numbers: a larger buy pays more slippage in and out, but at these sizes that is
  smaller than the fixed-cost saving.
- **Ladders add no information.** Mcap terciles and bundle-wallet groups were tested. Against a flat
  stake of the same mean size, the frozen ladder is +0.22 [-0.3, +0.8] pooled and -0.12 on the window
  (busy window launches -0.67 [-1.2, -0.1]). On train, the minimum BH q over the 48 ladders is 0.77.
  The post-bundle mcap is set by the bundle's SOL, so mcap and total bundle SOL are one feature.
- **Sniper-drop stress.** Dropping a random 25% / 50% of the third-party slot-0 wallets, with the curve
  repriced and averaged over 5 seeds, gives:

| sniper drop | 0.25 SOL | 0.5 SOL (LEAD) | 1.0 SOL |
|---|---|---|---|
| 0% | +2.64 / +1.65 | +3.47 / +2.48 | +3.93 / +2.92 |
| 5% | +2.22 / +1.38 | +3.04 / +2.22 | +3.49 / +2.65 |
| 10% | +1.72 / +1.18 | +2.54 / +2.01 | +2.99 / +2.44 |
| 25% | +0.82 / +0.55 | +1.64 / +1.37 | +2.08 / +1.80 |
| 50% | -0.47 / -0.59 | +0.33 / +0.23 | +0.76 / +0.65 |

(gap-3 test / window EV, %.) Every 5% of snipers lost costs about 0.4-0.5 points at any size. The 1.0
SOL "gain" of +0.45 is therefore erased if a 1.0 SOL rank-1 buy knocks out about 5% more of the slot-0
snipers than a 0.5 SOL buy would, through their slippage limits. That is a very plausible amount, and
we cannot measure it in historical tapes. **Keep the 0.5 SOL cap.** Sizing is a capacity decision, not
a source of edge.

### 3.6 Exit latency 0..4 slots, every variant

Pooled OOS EV / window EV (%):

| latency (slots) | LEAD | scale-out | gate | flow | re-entry | ladder |
|---|---|---|---|---|---|---|
| 0 | +3.52 / +2.60 | +3.15 / +2.16 | +3.36 / +2.48 | +3.31 / +2.63 | +3.45 / +2.49 | +3.47 / +1.81 |
| 1 (base) | +3.49 / +2.48 | +3.22 / +2.09 | +3.47 / +2.33 | +3.14 / +2.60 | +3.34 / +2.31 | +3.23 / +1.87 |
| 2 | +2.87 / +2.53 | +2.70 / +2.19 | +2.90 / +2.44 | +2.61 / +2.65 | +2.72 / +2.30 | +2.48 / +1.92 |
| 3 | +2.65 / +2.43 | +2.50 / +2.04 | +2.71 / +2.32 | +2.68 / +2.13 | +2.40 / +2.22 | +2.29 / +1.72 |
| 4 | +2.25 / +1.93 | +2.07 / +1.62 | +2.25 / +1.86 | +2.36 / +1.58 | +2.09 / +1.77 | +1.93 / +1.18 |

Fresh holdout, LEAD: +4.24 / +3.67 / +2.82 / +1.99 / +1.21 for latency 0-4, matching docs/04. No
variant overtakes the LEAD on both sets at any latency. The gate and flow variants trade places with it
by ±0.1, which is within noise. Pooled, latency costs about 0.3 points per slot. On the window it
matters little until 4 slots.

### 3.7 Cost and premium stress (pooled / window)

| scenario | LEAD | scale-out | gate | flow | re-entry | ladder | flat 1.0 |
|---|---|---|---|---|---|---|---|
| 0.01 SOL per round trip (0.005 per extra tx) | +2.29 / +1.28 | +1.92 / +0.78 | +2.27 / +1.13 | +1.94 / +1.40 | +2.04 / +1.05 | +1.39 / +0.05 | +3.35 / +2.32 |
| +3.6% entry premium | +0.90 / -0.23 | +0.70 / -0.56 | +0.85 / -0.34 | +0.57 / -0.03 | +0.65 / -0.47 | +0.67 / -0.87 | +1.34 / +0.18 |
| 0.004 SOL per extra sell tx | +3.49 / +2.48 | +3.15 / +2.01 | (unchanged) | | | | |

The ranking does not change under stress. Higher fixed costs widen the flat 1.0 SOL advantage
mechanically, for the reason in §3.5.

## 4. Ceilings: how much room post-entry management has at all

| (EV, %) | test | deep | fresh | window |
|---|---|---|---|---|
| LEAD | +3.47 | +3.19 | +3.67 | +2.48 |
| sell at end of slot+1 regardless (no signal at all) | +3.35 | +2.84 | +4.02 | +2.49 |
| sell at end of slot+2 / +3 / +4 / +6 | +3.34 / +3.31 / +3.29 / +2.75 | +2.46 / +2.11 / +2.56 / +3.34 | +3.27 / +3.43 / +3.28 / +2.72 | +2.62 / +2.82 / +2.51 / +1.42 |
| hindsight-perfect exit within slot+5 | +7.18 | +6.41 | +7.00 | +5.52 |
| hindsight-perfect exit within slot+20 | +14.86 | +12.05 | +14.58 | +11.19 |
| window: perfect bail on quiet launches (label oracle), LEAD on busy | | | | +3.44 |
| window: perfect skip of quiet launches (pre-entry oracle) | | | | **+9.38 per trade on 33% of trades** |

Three things follow from this table:
1. The exit legs are worth almost nothing over a fixed sell at slot+1. The LEAD is a "sell to the snipers
   who land in slots 0-1" trade.
2. A hindsight-perfect exit within slot+5 doubles EV. But the post-entry signals we can observe (price,
   follow-through, flow) do not predict the turn well enough to capture any of that gap after costs
   (§3.1-3.3).
3. The large lever is pre-entry: not entering quiet launches. Post-entry, a perfect quiet/busy label is
   worth about +1 point. The observable proxy is weak (AUC 0.58 at slot 0, 0.65 at slot+1). Even with
   k = 5+ buys behind us by slot+1 only 49% of window launches are "busy"; with k = 0 it is 15%. Raising
   the per-trade EV requires a slot-0 or pre-create predictor of activity (the separate predictor
   task), not exit tuning.

**Observation for pre-registration, not a result.** On launches with no third-party buy behind us in
slot 0 (about half), selling at slot+1 beat the LEAD on every recent set: test -1.12 vs -1.23, fresh
-0.91 vs -1.33, window -0.67 vs -1.46. On train (mostly pre-Oct-9, 400 ms slots) it was the reverse:
-0.29 vs +1.86. The train-only protocol therefore did not select it, and we cannot claim it. The total
effect would be at most +0.1 to +0.4 points. It belongs in the weekly freeze-then-holdout re-run as a
pre-registered variant ("k@slot0 = 0 → sell at slot+1, else LEAD"), together with the simplification
"sell at slot+1 always". The latter is not better, but it is equally good and has fewer moving parts
(no TP math, no idle timer) for the hot path.

## 5. What this means for the strategy

- The LEAD stays as specified in docs/04 §2. No post-entry change is recommended. Scale-out, runners,
  re-entry and flow-based early exits should **not** be added to the bot; they lower EV.
- The +2.5% to +3.7% per trade is not a tuning shortfall. It is close to what the slot-0/1 sniper flow
  pays after a 4.4% round-trip cost (fees + premium + fixed cost at 0.5 SOL). The two remaining levers
  are:
  - **Entry selection**: predict busy launches before or at slot 0. The ceiling is +9.4% per trade on a
    third of the launches.
  - **Execution**: rank 1 at <= +2% premium, a pre-signed exit, and latency of 0-1 slots. These move EV
    by 1-3 points, as much as any rule change tested here.
- Size stays capped at 0.5 SOL. 1.0 SOL's apparent +0.46 is fixed-cost arithmetic, and it is erased by
  about 5% extra sniper attrition, which a larger rank-1 buy plausibly causes.

## 6. Caveats

- The tapes hold other traders fixed in token terms. A runner or re-entry that changes our footprint
  could change their behaviour, but that is second order given these results.
- The window is a single 6-hour block on Oct 14. The "busy" label is the API page count (more than 100
  trades in 15 min), which is a proxy. One window launch has a third mode (prefix60).
- The deep set is small (n=77); its CIs span about ±4 points.
- Train (Sep 28-Oct 8) is mostly the 400 ms-slot regime and the OOS sets are mostly 217 ms. Several
  in-sample orderings flip between them (the bail rule, §4). Selections on train are more conservative
  than they might be in the current regime, in both directions.
- Costs for an extra sell transaction (0.002 SOL) and for a re-entry (a full 0.004 round trip plus
  premium) are assumptions. The ranking is unchanged at 0.004 / 0.005 SOL per extra transaction.
