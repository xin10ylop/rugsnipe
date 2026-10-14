# S3: operator-conditioned arming of the bundled-launch policies

**Question.** Take the best S1 entry/exit policies and trade them only on operators that are "armed". An operator is armed at time t if a point-in-time posterior of its expected value under the policy has a 20th percentile above 0 and it has at least 5 scored launches. The posterior uses empirical-Bayes shrinkage toward the family mean and only launches before t. Operators are keyed three ways: creator wallet, reused bundle-wallet component, and bundle-shape fingerprint. Score the armed operators' launches until the next re-arming, roll t over the 16-day window in 2-day steps, and compare against two things: the guru's own arming rule (at least 3 of the last 10 hit the TP) and a matched random control. Does any (policy, arming rule) pair have a positive out-of-sample lower bound after all costs, and at what volume?

*Measured 2026-10-14. Code: `analysis/rugsnipe_analysis/strategy/arming.py`, `analysis/scripts/strategy/operator_walkforward.py`, `analysis/scripts/strategy/operator_pull.py`. Results: `data/reports/strategy/operator_walkforward.json` (log: `operator_walkforward_log.txt`). New tapes: `data/cache/strategy/op_tapes.jsonl`, listings: `op_listing.json`.*

## Answer in brief

1. **Yes: a lower bound above zero, for one family only, and it is our exit with his idea of tracking operators.** We arm the S1 lead (rank-1 micro-scalp, 0.5 SOL, multi-wallet bundles) per creator wallet with the empirical-Bayes (EB) rule. On every scored launch that was not used to select the S1 policy ("policy-OOS"), it makes:

   | Arming (lead policy, 0.5 SOL) | n trades | EV per trade | 90% CI, operator x day clusters | 90% CI, operator clusters | Operators | Matched control | Armed launches/day |
   |---|---|---|---|---|---|---|---|
   | **creator, EB, skip rule, no mcap cap** | 383 | **+9.0%** | [+7.1, +11.0] | [+6.6, +11.5] | 41 | +3.4% | **≈ 430** (≈ 780 extrapolated) |
   | creator, EB, skip rule, cap ≤ 45 SOL (*pre-declared spec*) | 187 | +8.0% | [+5.5, +10.6] | [+4.3, +11.0] | 30 | +2.5% | ≈ 180 (≈ 380) |
   | creator, guru's rule (≥ 3 of last 10 reached +100%) | 175 | +8.9% | [+5.8, +12.7] | [+4.8, +13.9] | 24 | +2.2% | ≈ 175 (≈ 355) |
   | creator, guru's rule on the policy's own TP (+15%) | 266 | +9.3% | [+7.2, +11.4] | [+6.3, +12.0] | 29 | +2.6% | ≈ 230 (≈ 510) |
   | bundle-wallet component, EB | 327 | +8.8% | [+6.7, +11.0] | [+6.2, +11.3] | 38 | +3.0% | ≈ 400 (≈ 720) |
   | **no arming (every multi-wallet bundle)** | 1,625 | +3.6% | [+2.8, +4.5] | [+2.5, +4.8] | 377 | n/a | ≈ 4,250 |

   - **Multiple testing.** We ran 700 configurations: 7 policies × (3 keys × 4 rules × skip on/off × cap on/off × 2 timings, plus the unarmed baseline). Every row above has a BH q of 1e-12 to 1e-4 over all 700.
   - **The lift comes from the arming itself.** Against a matched random control (same 2-day window, same 5-SOL post-bundle mcap band, same UTC hour, unarmed launches), every EB, guru-rule and guru-TP configuration on the creator or bundle-component key beats the control by 5-9 points (p ≤ 0.005, BH q ≤ 0.05).
   - **Repeat operators alone do not help.** "Arm any operator with ≥ 5 launches" (R0) scores +4.0% against a control of +5.2%. The selection on track record does the work.
2. **Operator skill at this scalp persists.** We split each operator's launches in half by time (74 creators with ≥ 10 scored launches):
   - The two halves' EVs rank-correlate at **Spearman 0.49** (permutation p = 0.0005).
   - The top third goes from +13.0% in the first half to +9.1% in the second; the bottom third goes from -5.0% to -0.6%.
   - The EB fit agrees: between-operator SD of EV **τ ≈ 7.5-10%** around a family mean of +3-4%. That is real heterogeneity, not noise.
3. **Why it works.** Armed operators' launches draw more follow-on snipers: 1.64 vs 1.13 independent slot-0 buyers. The +15% take-profit fires on **38%** of armed entries vs 17%. Bundle wallets sell within 3 slots on 3% of armed launches vs 9%.
4. **It loosens the speed constraint, but it does not remove it.** All stresses keep the arming decisions fixed (creator / EB, no cap):

   | Stress | EV |
   |---|---|
   | Base (+1.1% premium, 0.004 SOL) | +9.0% |
   | +3.6% premium (rank-1, CEX-funded) | +6.6% |
   | +5% premium | +5.0% |
   | +8% premium | +2.3% |
   | +10.8% premium (rank-3 median) | -0.5% |
   | 0.01 SOL round trip at +1.1% | +7.8% |
   | Exit latency 0 / 2 / 4 slots | +8.5% / +9.4% / +9.3% |
   | 25% / 50% / 100% of slot-0 snipers removed | +8.1% / +6.3% / +3.3% |

   - **Break-even premium** is about **+10.5%**, against +4.4% for the unarmed lead.
5. **The guru's own policy cannot be rescued by arming.** TP +100%, hold to 900 s, armed by his own rule: **-20.5%** [-37.5, +0.9] (n = 27, 12 operators). Unarmed it is -12.3%. His bot's protective version and gap-3's TP +40% also have too few operators with 900-s tapes to test (3-5 armed operators, n ≤ 27). No long-horizon policy passes with at least 10 operators. The 20-s policies F1/F2 lose -1.3% unarmed; with creator EB they reach +7.5% and +5.6% from 8-9 operators, but the operator-level CIs cross zero (q = 0.10-0.20).
6. **Verdict: LIKELY real in the historical data, UNPROVEN live.** It is the strongest result in the S-series, and it survives every check here except one:
   - **Where the lift does not show:** in the random-sample subset (gap-3 test, deep, fresh), armed launches score +2.9% [-1.1, +7.0] (n = 50, 20 operators). That is no better than the unarmed +3.6%.
   - **Where it does:** the lift is carried by the two listing-sampled operator sets, +9.4% and +10.7% vs about +1% unarmed.
   - **Reading:** the true lift may be smaller than +5.6 points.
   - **Next step:** it is cheap to verify live in shadow mode (§8).

## Data and method

**Launches with tapes** (after de-duplication by mint; the longest tape wins):

| Source | Launches | Multi-wallet | Tape | Operator selection | Role |
|---|---|---|---|---|---|
| gap-3 random sample | 675 | 488 | to 900 s | random 1.27% of creates, gap-3 weights | history + scoring; its first 60% by time (S1 train) is excluded from scoring |
| gap-3 operator pull (`ops`) | 79 | 51 | 900 s | 7 operators hand-picked in gap-3 | history + scoring |
| deep sample | 133 | 77 | 900 s | random, Oct 12-14 | history + scoring |
| fresh holdout | 244 | 150 | 150 s | random, Oct 12-14 | history + scoring |
| `exp`: gap-1 expanded wallets, ATH ≥ $5.5k coins (cached) | 713 | 632 | 2 s | 42 expanded wallets with ≥ 3 bundled launches | history + scoring |
| **`opA` (new)**: the same 42 wallets' low-ATH coins | 329 | 233 | 25 s (4 s if busy) | completes `exp` | history + scoring |
| **`opB` (new)**: 35 new heavy bundlers | 472 | 440 | 25 s (4 s if busy) | creators of gap-3 multi-wallet launches with ≥ 5 launches in the gap-1 sample (≈ 470-1,570 launches each in 16 d), not covered before | history + scoring |

- **Why `opA` was necessary.** gap-1 s6 pulled only coins with ATH ≥ $5.5k, which is an outcome filter. Those coins alone score the lead at +5.9%; their low-ATH complement scores -5.0%. Without `opA`, every expanded wallet's history would be inflated. We therefore use expanded wallets *only* where `opA` completed them.
- **Pull budget.** 1,490 requests by `operator_pull.py` (hard cap 1,500; 299 of them were HTTP 429 retries on the shared swap-api limit), plus 1 manual header probe. Breakdown: 170 listing pages (frontend-api, ≤ 0.8 req/s) and 929 coins (swap-api, ≤ 1.3 req/s, i.e. ≤ 78/min). All 929 coins came back complete within their horizon.
- **Truncation is harmless for the lead.** Cut to 2 s, 4 s and 25 s, the full gap-3 tapes give *identical* lead returns on all 488 multi-wallet launches. The lead exits by slot+6, and 671/675 launches have every trade of slots ≤ S0+6 within create+2 s. Mints present in two sources (28 pairs) give identical tapes and returns.
- Policies needing longer tapes are scored only where the tape is long enough: F2 ≥ 12 s, F1 ≥ 25 s, the guru / gap-3 references ≥ 900 s.

**Policies** (fixed in advance, from S1; base scenario +1.1% entry premium, 0.004 SOL per round trip, 1-slot exit latency, exact curve, 1.25% fee per side):

| Name | Size | Filter | Exit (first of) |
|---|---|---|---|
| LEAD_0.5 / LEAD_0.25 | 0.5 / 0.25 SOL | multi-wallet bundle | realised +15%, bundle-wallet sell, 1 s idle, -15% after 1 s, slot+5 |
| F1_0.5 (frozen #1) | 0.5 | post-bundle mcap ≤ 45 | +20%, idle 3 s, buy ≥ 3 SOL, -10% after 1 s, 20 s |
| F2_0.5 (frozen #2) | 0.5 | ≤ 50 | +20%, idle 2 s, buy ≥ 3 SOL, 10 s |
| GURU_TP100 | 0.25 | all | +100%, else hold 900 s |
| GURU_BOT | 0.25 | all | +100%, -25% after 10 s, buy ≥ 2 SOL, idle 10 s, 900 s |
| G3_TP40 | 0.25 | all | +40%, insider sell, idle 3 s, 900 s |

**Operator keys.**

| Key | Definition |
|---|---|
| `creator` | The create signer. |
| `bcomp` | Union-find over non-creator bundle wallets. It is rebuilt at each re-arming time from history only. A launch joins the component of any of its bundle wallets already seen; otherwise it is its own, unarmable, operator. |
| `shape` | gap-1's fingerprint: bundle wallets (capped at 5), dev buy to 0.01 SOL, bundle SOL to 1 SOL. |

**Arming rules.** Each needs ≥ 5 scored launches in the history, except G100, which needs ≥ 5 launches with a known +100% outcome.

| Rule | Arms the operator when |
|---|---|
| **R0** | Always (any repeat operator). |
| **EB** | Normal-normal posterior of the operator's EV clears zero at its 20th percentile. The prior is the family mean of all history launches of the policy. The between-operator variance τ² comes from DerSimonian-Laird; σ² is the pooled within-operator variance. The posterior variance includes the family mean's operator-clustered variance. Weights follow gap-3. |
| **GTP** | The guru's rule on the policy's own TP: ≥ 3 of the last 10 launches exited at the TP. |
| **G100** | The guru's rule as he states it: ≥ 3 of the last 10 reached +100% for a 0.5 SOL rank-1 entry, using the lifetime ATH (gap-1's optimistic definition). An ATH counts only after its timestamp; if the timestamp is unknown, 1 day after the create. |

**Modifiers.**
- `skip13` drops operators whose history shows a bundle-wallet sell in slots 0-3 on at least half of ≥ 3 bundled launches (gap-3 §2).
- `cap45` scores only post-bundle mcap ≤ 45 SOL.

**Timing.**
- `2d`: history = launches created before t. Re-arm at t = day 2, 4, …, 14, counted from 2026-09-28 08:58:50 UTC, and score [t, t+2 d). The last window runs to the end of the data.
- `cont`: per-launch history up to 60 s before the launch. Family parameters and bundle components are still taken from before t.
- `stale7`: arm at t and score [t, t+7 d). This variant is reported, not selected.
- Each launch is scored once.

**Statistics.**
- **Headline set: "policy-OOS".** All scored launches from day 2 on, except the gap-3 train split that selected the S1 policies.
- **CIs.** 90% cluster bootstrap with 2,000 resamples, with operator × day clusters (as specified) and operator-only clusters (more conservative, because operator effects persist).
- **p-values.** Normal approximation with the cluster-bootstrap SE; BH and Bonferroni over all 700 configurations.
- **Matched control.** For each armed trade, draw an unarmed launch from the same window, mcap band and hour (fallback: any hour). 200 replications. The p-value is the share of replications whose control EV is ≥ the armed EV.

**Volume ("armed launches/day").**
- This is the qualifying launches/day of the operators that are armed. Gap-3 rows count 1 / 1.27%. Listing-sampled rows count the wallet's 16-day launch count divided by the number of coins sampled from it. Days 2-16 are 14 days.
- "Extrapolated" scales the expanded-wallet part by 920/200: the 200 expanded wallets are a random 22% of the 920 mid-size wallets (gap-1).
- These are **upper bounds on trades**. Each one still needs a rank-1 fill.

## 1. Unarmed baselines (policy-OOS)

| Policy | n | EV [90% CI] | TP hit rate |
|---|---|---|---|
| LEAD_0.5 | 1,638 | **+3.6%** [+2.8, +4.5] | 22% |
| LEAD_0.25 | 1,638 | +2.8% [+2.0, +3.7] | 22% |
| F1_0.5 | 895 | -1.3% [-2.4, -0.2] | 17% |
| F2_0.5 | 1,070 | -1.3% [-2.3, -0.3] | 17% |
| GURU_TP100 | 466 | -12.3% [-15.4, -9.2] | 11% |
| GURU_BOT | 466 | -1.8% [-4.2, +0.8] | 2% |
| G3_TP40 | 466 | +0.9% [-0.8, +2.7] | 11% |

- **The lead replicates on operators never used anywhere before.** On the 35 new heavy bundlers (`opB`), it scores **+3.4%** (n = 440). On the expanded bundlers' complete coin sets (`exp` + `opA`), it scores about +3.0% (n = 865). Both are close to S1's +3.5%.
- **F1/F2 do not replicate.** On the new operators they lose -3% to -8%, which confirms S1's finding that only the lead was positive everywhere.

## 2. Arming results for the lead (0.5 SOL, skip rule on, `2d` timing, policy-OOS)

| Cap | Key | Rule | n | EV | CI op x day | CI operator | TP hit | Ops | Control | q (700) | Launches/day studied / extrap. |
|---|---|---|---|---|---|---|---|---|---|---|---|
| - | none | none | 1,625 | +3.6% | [+2.8, +4.5] | [+2.5, +4.8] | 22% | 377 | n/a | 1e-10 | 4,256 / 5,284 |
| - | creator | R0 | 911 | +4.0% | [+2.8, +5.2] | [+2.3, +5.7] | 24% | 76 | +5.2% | 1e-07 | 984 / 1,786 |
| - | creator | **EB** | 383 | **+9.0%** | **[+7.1, +11.0]** | [+6.6, +11.5] | 38% | 41 | +3.4% | 2e-12 | **434 / 778** |
| - | creator | GTP | 266 | +9.3% | [+7.2, +11.4] | [+6.3, +12.0] | 50% | 29 | +2.6% | 9e-12 | 232 / 512 |
| - | creator | G100 | 175 | +8.9% | [+5.8, +12.7] | [+4.8, +13.9] | 41% | 24 | +2.2% | 9e-05 | 175 / 356 |
| - | bcomp | R0 | 851 | +3.5% | [+2.4, +4.7] | [+1.9, +5.2] | 23% | 85 | +4.7% | 2e-06 | 1,010 / 1,781 |
| - | bcomp | EB | 327 | +8.8% | [+6.7, +11.0] | [+6.2, +11.3] | 38% | 38 | +3.0% | 5e-10 | 402 / 718 |
| - | bcomp | GTP | 242 | +8.3% | [+6.2, +10.5] | [+5.2, +11.2] | 47% | 28 | +2.2% | 7e-09 | 255 / 539 |
| - | bcomp | G100 | 155 | +8.9% | [+5.6, +13.3] | [+4.5, +14.5] | 41% | 22 | +2.2% | 3e-04 | 159 / 345 |
| - | shape | R0 | 774 | +4.2% | [+3.1, +5.4] | [+2.8, +5.4] | 24% | 64 | +4.5% | 2e-08 | 1,122 / 1,807 |
| - | shape | EB | 410 | +5.8% | [+4.1, +7.5] | [+3.8, +7.7] | 26% | 34 | +3.6% | 1e-07 | 492 / 888 |
| - | shape | GTP | 299 | +4.9% | [+3.3, +6.7] | [+2.5, +7.3] | 33% | 24 | +4.3% | 8e-06 | 323 / 652 |
| - | shape | G100 | 117 | +7.2% | [+3.8, +12.3] | [+3.6, +13.9] | 34% | 11 | +3.0% | 1e-02 | 123 / 233 |
| ≤ 45 | none | none | 880 | +3.5% | [+2.5, +4.6] | [+2.1, +4.9] | 23% | 235 | n/a | 2e-07 | 2,757 / 3,351 |
| ≤ 45 | creator | **EB** (pre-declared) | 187 | **+8.0%** | [+5.5, +10.6] | [+4.3, +11.0] | 44% | 30 | +2.5% | 7e-07 | 178 / 377 |
| ≤ 45 | creator | GTP | 136 | +10.1% | [+7.3, +13.2] | [+6.2, +13.2] | 53% | 18 | +2.0% | 6e-08 | 120 / 268 |
| ≤ 45 | creator | G100 | 77 | +10.5% | [+6.3, +16.6] | [+4.9, +17.3] | 52% | 13 | +1.5% | 2e-03 | 49 / 139 |
| ≤ 45 | bcomp | EB | 170 | +8.9% | [+6.5, +11.7] | [+5.4, +11.8] | 46% | 23 | +1.1% | 1e-07 | 164 / 352 |
| ≤ 45 | shape | EB | 277 | +4.8% | [+3.1, +6.6] | [+2.7, +6.6] | 29% | 21 | +3.2% | 2e-05 | 308 / 593 |

- **The skip rule changes almost nothing.** It removes 7-13 launches. Operators that dump within 3 slots are rare among repeat bundlers, and EB already refuses them.
- **The ≤ 45 SOL cap halves the volume and does not raise the EV of EB arming.** Gap-3 recommended it for +100% holds, not for a 2-second scalp.
- **Continuous re-arming (`cont`)** gives the same picture: creator EB +8.2% [+6.4, +10.0] (n = 456), and bundle-component EB +8.9% [+7.2, +10.9] (n = 393, the best p overall).
- **Creator wallet and bundle-wallet component are equivalent keys.** The shape fingerprint is weaker, because it merges unrelated operators who use the same amounts.
- **Time stability** (creator EB, `2d`):

  | Window starting day | 2 | 4 | 6 | 8 | 10 | 12 | 14 |
  |---|---|---|---|---|---|---|---|
  | n | 7 | 13 | 59 | 58 | 67 | 68 | 111 |
  | EV | +3.1% | +12.1% | +11.2% | +4.0% | +10.0% | +10.5% | +9.0% |

  - Positive on **13 of 14 days**.
  - Median trade +5.4% (unarmed lead: -1.2%), 10th percentile -4.3%, 90th percentile +31%, worst -38%, SD 22%.
- **Stale arming** (arm once, trade 7 days): creator EB scores +5.5% to +10.8% in every window from day 4 on. Arming can therefore lag by days, not only hours.

**Where the lift comes from** (creator EB, no cap; armed vs not armed, same scoring set, operator-clustered 90% CIs):

| Source group | Armed | Not armed |
|---|---|---|
| Expanded bundlers (`exp` + `opA`, complete coin sets) | **+9.4%** [+6.0, +12.4], n = 212, 20 ops | +0.8% [-0.5, +2.3], n = 548 |
| New heavy bundlers (`opB`, never used before) | **+10.7%** [+5.4, +16.8], n = 103, 17 ops | +0.9% [-0.5, +2.6], n = 307 |
| Random sample (gap-3 test, deep, fresh) | +2.9% [-1.1, +7.0], n = 50, 20 ops | +3.6% [+1.9, +5.4], n = 377 |
| gap-3 operator pull | +18.6%, n = 18, 3 ops | +15.7%, n = 23 |

- **The two operator samples agree.** They were chosen in different ways: `exp` partly through ATH-tinted bundling counts, `opB` through slot-0 structure only. In both, unarmed repeat bundlers are roughly break-even and armed ones make about +10%.
- **The random-sample subset does not show the lift.** It is weighted toward gap-3's w = 5 low-ATH launches; unweighted, it reads armed +6.2% vs unarmed +5.3%. The gap from about +10% is about 2.3 SE.
- **Two readings:**
  - The listing-sampled groups overstate the lift, so the true armed EV is nearer +5-7%.
  - Or the 50 random-sample trades are an unlucky draw.
- **We cannot separate the two with this data.** We count this as the main reason for the "unproven" label.

## 3. Persistence: is operator skill at the scalp real? (split-half, all scorable launches)

| Policy / key | Operators (≥ 10 launches) | Spearman (1st half vs 2nd half) | Permutation p | Top third: 1st → 2nd half | Bottom third: 1st → 2nd half |
|---|---|---|---|---|---|
| LEAD / creator | 74 (1,483 launches) | **0.49** | 0.0005 | +13.0% → +9.1% | -5.0% → -0.6% |
| LEAD / bundle component | 67 | 0.49 | 0.0005 | +13.0% → +8.4% | -4.9% → -0.9% |
| LEAD / shape | 54 | 0.26 | 0.03 | +10.6% → +6.4% | -3.4% → 0.0% |
| F2 / creator | 37 | 0.57 | 0.0005 | +9.9% → +2.2% | -8.4% → -8.3% |
| GURU_TP100 / creator | 6 | 0.43 | 0.21 | n/a | n/a |

- **The EB family parameters are stable over time.** At each re-arming time (creator key): mean +2.6% to +4.2%, between-operator SD τ = 7.5-9.8%, and within-operator SD σ = 15-19% per trade.
- **What that means:** after about 10 launches, an operator's record carries roughly as much weight as the family prior.

## 4. Mechanism (creator EB, policy-OOS; armed vs not armed)

| | Armed (n = 383) | Not armed (n = 1,255) |
|---|---|---|
| EV | +9.0% | +2.1% |
| Exit at TP +15% | **38%** | 17% |
| Exit at 1 s idle | 35% | 50% |
| Exit on a bundle-wallet sell | 1.7% | 8.4% |
| Independent slot-0 buyers (after us) | **1.64** | 1.13 |
| Bundle sells within 3 slots | 3% | 9% |
| Post-bundle mcap | 46.6 SOL | 44.6 SOL |
| Bundle wallets / dev buy | 2.9 / 4.0 SOL | 2.7 / 3.0 SOL |

- **What the armed operators share:** their launches reliably attract follow-on sniper flow, and their bundles do not dump immediately.
- **This is consistent with copy-trading bots that follow specific dev wallets.** The guru's bot has "copy-trading pump wallets" as a feature. Each such bot that follows an operator becomes the exit liquidity for whoever is first behind that operator's bundle.
- **The catch:** these launches also have about 45% more competing snipers in slot 0. Winning rank 1 is harder exactly where the edge is.

## 5. The guru's way, our way, and the mix

- **His selection idea works, but only with our exit.** Arming operators by "≥ 3 of the last 10 hit +100%" picks the same kind of operator our EB rule picks. On the lead it scores +8.9% [+5.8, +12.7] (n = 175).
- **His target and hold lose even on the operators he would pick.** TP +100% held to 900 s on G100-armed operators scores -20.5% [-37.5, +0.9] (n = 27, 12 operators); on G100 bundle components, -34%.
- **His protective bot version and gap-3's TP +40% cannot be judged at operator level.** Only 3-5 operators have enough 900-s tapes. Their armed results (+16% to +39%, n = 9-27) rest on the 7 hand-picked gap-3 operators, and the operator-level CIs are meaningless at 3 clusters.
- **The version that pays is the mix.** Keep his habit of trading only operators with a track record. Measure the track record on *our* 2-second scalp, not on +100%.

## 6. Multiple testing

- **Family.** 700 configurations: 7 policies × [3 keys × 4 rules × 2 skip × 2 cap × 2 timing + 4 unarmed baselines (2 skip × 2 cap)].
  - BH q ≤ 0.05: 216 configurations.
  - Bonferroni ≤ 0.05: 156.
  - Not in this family: 12 stale-window variants and 9 persistence tests. These are diagnostics, not selected.
- **With ≥ 10 armed operators and an operator-clustered lower bound > 0, 192 configurations pass. All are LEAD_0.5 or LEAD_0.25.** That includes the unarmed and R0 rows, because the lead is positive unconditionally.
- **Arming lift vs matched control.** 467 armed configurations had ≥ 20 trades. BH over all 672 armed configurations gives q ≤ 0.05 for 151, including all 8 variants of LEAD × {EB, GTP, G100} × {creator, bcomp}.
  - **For the lead, R0 never significantly beats its control.** 23 of its 24 variants score below it.
  - For F1, F2 and the long-horizon policies, the "lifts" rest on 1-9 operators.
- **Smallest p.** Bundle-component EB `cont`: q = 3e-13. Creator EB `2d`: q = 2e-12.

## 7. Verdict

- **Does any (policy, arming rule) pair have a positive out-of-sample lower bound? Yes:** the S1 lead armed per operator.
  - The answer holds for creator or bundle-wallet component as the key, and for the EB, guru-on-TP or guru-+100% rule.
  - Lower bound: **+6.6% to +7.1%** per trade (operator-clustered or operator x day clustered 90% CI) on 383 policy-OOS trades from 41 operators.
  - Point estimate +9.0%, about 2.5 times the unarmed lead's +3.6%.
  - The pre-declared variant with the ≤ 45 SOL cap: +8.0% [+5.5, +10.6] (operator-clustered [+4.3, +11.0]).
- **Volume:**
  - About **430 armed qualifying launches/day** among the 84 bundling operators we could study (41 of them ever armed). Scaled by the expanded-wallet sampling rate, that is about **780/day**.
  - These are launches we may *try*. At 0.5 SOL and +9%, every 100 landed rank-1 trades make about 4.5 SOL.
  - If we land rank 1 on 10-20% of attempts, that is 40-160 trades/day. This is an illustration, not a forecast.
- **No other policy works with or without arming:**
  - the guru's TP +100% (-12% unarmed, -20% armed by his rule);
  - his protective bot;
  - gap-3's TP +40%;
  - S1's frozen F1/F2.
- **Honest label: LIKELY real in historical data, UNPROVEN live.**
  - The edge is a simulation on real tapes, at an assumed rank-1 fill.
  - One independent subset, the random sample, does not reproduce the lift (n = 50).
  - The armed launches are the ones other snipers crowd. The break-even premium rises to about +10.5%, but we still need to be first or second behind the bundle.

## 8. What to build next

1. **Shadow-score every multi-wallet bundled launch the bot sees.**
   - Simulate the lead on the live feed and keep per-creator and per-bundle-component posteriors, with the same EB as `arming.eb_params` / `eb_lower`.
   - The family parameters to start from: mean +3.5%, τ 8%, σ 16%.
   - The bot sees 100% of launches, not 1.27%. A heavy bundler (60-95 launches/day) reaches 5 scored launches within hours.
2. **Arm** at `n ≥ 5` and posterior 20th percentile > 0.
   - Re-arm continuously.
   - Disarm when the posterior lower bound falls below 0.
   - Log the guru's 3-of-10 rule alongside it as a second opinion.
3. **Live test only on armed operators.**
   - About 200 trades at 0.25-0.5 SOL.
   - `max_sol_cost` capped at post-bundle +5% (armed break-even is about +10.5%; leave half as margin).
   - Kill switch: stop if the mean is below 0 after 100 trades.
   - Report the armed vs unarmed shadow EV weekly. That is the live replication of §2's lift, and the cleanest answer to the random-sample doubt.
4. **Track the landing rank on armed launches separately.** Competition is about 45% heavier there.

## 9. Caveats

- **Simulation, rank-1 assumption.** Same model and caveats as S1: fills, our own impact, sniper behaviour held fixed except in the sniper-drop stress. The reflexive risk is larger here. Copy-trading snipers that follow these operators may also learn to follow *us*, or the operators may notice a consistent rank-1 buyer.
- **Operator universe.** 84 studied bundling operators: 42 expanded, 35 new heavy, 7 hand-picked.
  - **Expanded wallets.** Their inclusion counted bundled coins among ATH ≥ $5.5k coins, a mildly outcome-tinted criterion. Excluding expanded wallets entirely, creator EB scores **+8.6%** [+5.4, +12.2] (n = 171, 26 operators).
  - **Gap-3 random-sample creators.** Their history comes only from the 1.27% sample. They rarely reach 5 scored launches, so arming is under-tested on the long tail of operators.
- **The random-sample subset shows no lift** (§2). This is the main unresolved inconsistency.
- **Creator wallets rotate.** The bundle-component key recovers rotating operators only through reused bundle wallets. Gap-3 shows that some operators reuse the same 3 wallets on every launch.
- **Time.** 16 days, including the Oct 9 change to 200 ms slots (both regimes positive). Alpenglow (≈ Oct 21) may change slot-0 structure.
- **G100 uses lifetime ATH.** 96% of +100% hits happen within an hour of the create, and each one counts only after its timestamp. Leakage is negligible.
- **Volume figures are upper bounds on attempts.** They mix a random-sample HT estimate with listing-based per-wallet rates. The extrapolation factor (920/200) applies only to the expanded part.
- **Fees** are 1.25% per side. Any change moves the edge one for one.

## Reproduce

```
.venv/bin/python -I analysis/scripts/strategy/operator_pull.py AB 35 14     # ~45 min on the shared limit; cached, resumable, hard cap 1,500 requests
.venv/bin/python -I analysis/scripts/strategy/operator_walkforward.py      # ~2.5 min: grid, BH, controls, persistence, stress
```

Inputs: `data/cache/gap3/` (sel.json, trades.jsonl, ops_*), `data/cache/gap1/` (expanded.jsonl, creates.jsonl, outcomes*.jsonl), `data/tokens_sample.jsonl`, `data/tokens_light.jsonl`, `data/cache/strategy/holdout_trades.jsonl`, the pumpapi caches, and the new `data/cache/strategy/op_tapes.jsonl` / `op_listing.json`.

Output: `data/reports/strategy/operator_walkforward.json`. It contains every configuration row (`rows`), the BH/Bonferroni values, `primary` (stress, mechanism, per-source lift, armed operator list), `persistence`, `eb_params`, `stale7` and `truncation`.
