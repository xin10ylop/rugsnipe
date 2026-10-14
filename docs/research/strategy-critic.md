# S4: adversarial review of the bundled-launch strategy results (S1 lead, S3 arming)

**Question.** Two results in the strategy series claim a positive out-of-sample expectancy after costs:

* **S1** (`strategy-sweep.md`): the rank-1 micro-scalp "lead", **+3.5% per trade**.
* **S3** (`strategy-operators.md`): the same lead, traded only on operators armed by their track record, **+9.0% per trade**.

S2 (mayhem) and S5 (copy-trading) found no edge, and S5 already has its own critic. We attacked S1 and S3 for:

* look-ahead;
* truncated tapes and swap-api paging gaps;
* survivorship and weighting errors;
* multiple testing;
* fill realism: landing rank, the rank premium, our own impact, failed attempts;
* regime mixing between 400 ms and 217 ms slots.

We then ran the top three policies on a **fresh holdout**: every pump.fun create between 08:43 and 13:43 UTC today, all at least 60 minutes old when pulled. The policies and the armed operator lists were frozen before any holdout tape was pulled.

*Measured 2026-10-14, 14:43-15:50 UTC.*

* **Code:**
  * `analysis/rugsnipe_analysis/strategy/critic.py`: rank-r entries, slippage cap, SOL-fixed own impact, token-flow gap check, first-follower drop.
  * `analysis/scripts/strategy/critic_audit.py`: the audit.
  * `critic_freeze.py`: the pre-registration.
  * `critic_holdout.py`: stages `index`, `decode` and `pull`.
  * `critic_holdout_eval.py`: the holdout evaluation.
* **Results:** `data/reports/strategy/critic_audit.json`, `critic_preregistration.json` and `critic_holdout.json`.
* **Caches:** `data/cache/strategy_holdout/`.
* **Fix applied to S3:** `analysis/rugsnipe_analysis/strategy/arming.py` (4 lines). `operator_walkforward.py` was re-run, so `operator_walkforward.json` and its log are the corrected versions.
* Nothing was committed.

## Verdict in brief

1. **The S1 lead survives every data and look-ahead attack, and it replicates on the fresh holdout, in simulation.**
   * Pre-registered random holdout: **+3.2% per trade [90% CI +1.5, +4.9]**, n = 222, creator-clustered [+1.3, +5.2], BH q = 0.005.
   * All 485 clean post-history launches (three windows, 05:55-13:43 UTC): **+2.5% [+1.3, +3.8]**.
   * The 631 scored holdout tapes have no paging gaps. The look-ahead probes find nothing.
2. **S3's operator arming does not replicate. Its +9% was a historical artefact.**
   * Frozen EB-armed creators made **+2.9% [+0.8, +5.0]** (n = 132, 15 creators).
   * The unarmed launches in the same holdout made +3.2%.
   * **The arming lift is -0.2 points [-2.9, +2.5], p = 0.54.**
   * The two busiest armed creators, with 69 of the 132 launches, scored -0.4% and -1.8%.
3. **The guru's own selection rule, P3, adds nothing.** "≥ 3 of the last 10 reached +100%" gives +2.1% [-2.0, +6.0] (n = 67, 7 creators, q = 0.29).
4. **The edge belongs entirely to whoever lands first behind the bundle.** Charging realistic fills drives it to about zero:
   * **Rank 2** (one historical sniper ahead of us): **+0.0%** on the 1,342 earlier out-of-sample launches and +0.8% on the holdout.
   * **Rank 3:** -1.3% on the earlier launches.
   * **Adverse selection:** all of the money is on the ~1/3 of launches where the first follower's buy lifts the price by more than 2%. There the lead makes **+11.7%** (holdout: +9.1%); on the rest it makes **-0.5%** (holdout: +0.3%). Those hot launches are exactly where the rank-1 race is hardest.
   * **Combined realistic scenario.** It applies all of these together:
     * gap-2's *mean* rank-1 premium instead of the median;
     * buyers after us keep their SOL amounts, not their token amounts;
     * 0.01 SOL per landed round trip;
     * the first follower fails its slippage check one time in three.

     Result: **-1.1%** on the earlier out-of-sample set, **-0.4% [-2.0, +1.3]** on the holdout random set and **-1.2% [-2.3, +0.1]** on all clean launches. That is before the cost of failed attempts.
5. **Recommendation: deploy P1 in paper (shadow) only. Move no money.** Drop operator arming as a filter and keep it only as a logged field.
   * The simulated edge is real and stable enough to be worth measuring.
   * Whether it exists for us depends on one number no tape can provide: how often our transaction lands ahead of the first follower on the hot launches.
   * If shadow mode shows rank 1 on fewer than about 1 in 3 hot launches at a premium of +2% or less, the answer is **no edge**.

| Policy (frozen 14:46:56 UTC) | Holdout set | n (clusters) | EV per trade | 90% CI (launch) | 90% CI (creator-clustered) | p | BH q (H1-H4) |
|---|---|---|---|---|---|---|---|
| **P1** lead, unarmed (S1 #3) | random launches (frame + copy holdout) | 222 (156) | **+3.17%** | [+1.48, +4.93] | [+1.31, +5.24] | 0.001 | **0.005** |
| P1 | frame, population-weighted (armed enumerated, rest sampled) | 260 (112) | +5.57% | [+3.46, +7.83] | [+3.06, +8.16] | 0.0005 | n/a |
| **P2** lead on EB-armed creators (S3 headline) | every armed launch in the frame, plus copy holdout | 132 (15) | **+2.93%** | [+0.81, +5.03] | [+0.74, +7.76] | 0.009 | **0.018** |
| **P3** lead on G100-armed creators (the guru's rule) | same | 67 (7) | **+2.08%** | [-1.97, +5.99] | [-1.88, +10.8] | 0.22 | 0.29 |
| **H4** arming lift: P2 minus unarmed | two-sample bootstrap | 132 vs 219 | **-0.24 pts** | [-2.93, +2.49] | n/a | 0.54 | 0.54 |
| *secondary*: lead on GTP-armed creators | same | 43 (9) | +9.14% | [+4.70, +13.3] | [+5.72, +15.5] | 0.0002 | not in the family |

## 1. Attacks and what they found

| Attack | How we tested it | Result | Effect |
|---|---|---|---|
| **Look-ahead in the lead's entry or exits** | Read `sim.py`. TP fires at the trade that crosses the threshold and fills after every trade of the next slot. Idle fires N slots after the last trade. The phantom SL activates after 1 s. A bundle sell fills one slot later. Re-priced every set with independent code (`critic.lead_trade`). | No peak-based fills. Fills always land after the trigger slot. The independent code reproduces S1 exactly: fresh holdout +3.67% (n = 150). | None. |
| **Is the "multi-wallet" filter knowable at entry?** | The bundle is the create tx plus contiguous slot-0 buys at most 5 tx indices after it, so E1 enters after every trade the filter uses. We also checked whether "bundle wallets" are really outside snipers. | Knowable, as long as we enter after those txs. Only 146 of 2,071 multi-wallet launches have a bundle wallet seen under ≥ 3 different creators. | None. The bot must use the same definition, including skipping launches whose create tx contains no buy. |
| **Peak-based hits** | G100 arming uses the lifetime ATH, gap-1's "+100%" definition. | Used only as an arming signal, gated by the ATH timestamp (unknown: +1 day). It is never a P&L. | None. |
| **Survivor and dip entries peeking at later prices** | `entry_point`: SURV and DIP use state up to the end of slot n and land first in slot n+1. | No peeking, but the fill is optimistic: we are first in the next slot. They lose 2-9% anyway. | Strengthens S1's negatives. |
| **Tapes truncated at +900 s or +2 s** | The lead exits by slot+6 (1.3 s at 217 ms). We re-scored all 488 gap-3 multi-wallet tapes cut to 2 s, including 56 flagged as possibly clipped (fill slot beyond 2 s). | Identical returns on all 488, because block times are coarse. | None. |
| **swap-api paging gaps** | New `tape_gaps`: every trade's token flow must equal the change in virtual token reserves implied by consecutive post-trade prices, over the first 25 s. | Clean everywhere the lead is scored: 0 of the gap-3 test, deep and fresh tapes, 1 of 1,425 window tapes, 0 of the 631 scored holdout tapes. **11 of 79 `ops` tapes and 1 gap-3 tape fail.** They are non-standard curves priced below the 27.96 SOL floor (mayhem-like coins of operator `EhrQmJ…`), and 4 were armed and scored in S3. | **Bug, fixed.** `arming.load_launches` now skips them. S3 re-run: creator-EB **+9.04% (n = 383) → +9.15% (n = 376)**; BH q ≤ 0.05 configs 216 → 221. Immaterial. |
| **Survivorship: hot launches never paged to the create** | Counted incomplete tapes and checked whether the drops are outcome-related. | Window: 262 incomplete (the busiest launches) are missing. Copy holdout: 4 of 422. **Our frame: 0 of 573** (paged to 4 pages for the first 4 s). | The window understates the lead. The fresh frame has no such hole. |
| **Survivorship: outcome-filtered operator histories** | S3's `exp` coins were pulled only if ATH ≥ $5.5k. Checked whether `opA` really completes the rest. | 98% of high-ATH and 75% of low-ATH coins reach the scored set. All 108 low-ATH drops are structural: 98 dev-only buys under 1 SOL, 9 with no dev buy in the first slot, 1 with no trades. | None. |
| **Weighting** | gap-3's w = 5 restores gap-1's 1-in-5 subsample of low-ATH launches. | Correct. It explains S3's "random-sample subset shows no lift": its armed launches score **+2.9% weighted, +6.2% unweighted** (unarmed: +3.6% / +5.3%). | Not a bug. The weighted number was the right one, and it was the warning. |
| **Own impact** | S1 holds other traders' *token* amounts fixed while we hold. Alternative: buyers after us keep their *SOL* amounts and the curve is replayed with our tokens outstanding. | Pooled out-of-sample: +2.80% → **+2.46%**. Holdout: +3.17% → +2.84%. | -0.35 points. |
| **Rank premium** | S1 charges gap-2's *median* rank-1 premium (+1.1%). EV is linear in the premium, so the *mean* is what matters. gap-2's bundled rank-1 quantiles are 0.4 / 1.6 / 4.6 / 8.4% (p25 / p50 / p75 / p90). | Mean **3.2%** including the buyer's own impact; about **2.4%** net of it. EV: +2.8% → **+1.4%** (+0.5% if own impact is not netted). Holdout: +3.2% → +1.8%. | -1.4 points. |
| **Landing rank** | New entry: we land after the first r-1 *historical* third-party slot-0 buyers, at the natural premium, with an optional slippage cap (fail = no position plus the priority fee). | §2. **Rank 2: +0.0%. Rank 3: -1.3%.** | Decisive. |
| **The first follower is knocked out** | Our 0.5 SOL buy moves the price about +2.6% at 45 SOL. Followers with tight slippage fail (gap-2: 27% of bundled slot-0 attempts, 38% on repeat-creator launches). We removed only the *first* follower, which is targeted, unlike S1's random drop. | Never lands: **-0.2%**. Fails 27% / 38% of the time: +2.0% / +1.7%. | -0.8 to -1.1 points. |
| **Failed attempts** | Per-attempt EV = landing rate × EV per landed trade − (1 − landing rate) × failure fee. | §2. At a 20% landing rate with a 0.003 SOL failure fee: +0.08%. | Depends on the landing rate. |
| **Regime mixing (400 vs 217 ms slots)** | The lead's caps are in slots: slot+5 is 2.0 s pre-Oct-9 and 1.1 s after; idle-1 s is 3 or 5 slots. We measured today's slot time from the holdout index (217 ms over 109k slots). | Out-of-sample sets are post-Oct-9 except 67 gap-3 test launches (pre +3.3%, post +3.6%). The holdout is entirely post. | None. |
| **Multiple testing: S1** | Recomputed BH from the stored p-values. | Identical. Lead on the fresh set: q = 0.048 over the 5 frozen policies, Bonferroni 0.087. The full S1 search was 26,056,296 configurations. Only the protocol (freeze, then holdout) certifies it, not the search. | The holdout here adds q = 0.005. |
| **Multiple testing: S3** | Recomputed BH over the 700 configurations, then re-tested the arming lift. | BH recomputes identically. But the family tests "EV > 0", which the unarmed lead passes on its own (q ≈ 1e-10). The matched-control p (min 1/201) ignores the armed group's own sampling variance. Re-tested with an operator-cluster bootstrap of armed minus control: **+5.7 [+2.7, +8.8] points, p = 0.003** (creator EB); 0.002-0.007 for GTP, G100 and bcomp. | Still significant in sample. It fails on the holdout (§4). |

## 2. Fill realism: the edge is the rank-1 slot itself

**Pooled out-of-sample set.** This is everything S1 and the orchestrator used out of sample, and nothing used in selection:

* gap-3 test: n = 200;
* deep sample: n = 77;
* fresh sample: n = 150;
* the orchestrator's dense window, Oct 14 02:05-08:05: n = 915.

Total **1,342 multi-wallet bundled launches**. At S1's base fills they give **+2.80% [+2.02, +3.60]**. gap-3 weights are rescaled to a mean of 1, as in `lead_check.py`.

| Entry (0.5 SOL, lead exits, 0.004 SOL per landed round trip) | Lands | EV per landed trade | EV per attempt | 90% CI |
|---|---|---|---|---|
| Rank 1, +1.1% fixed premium (S1 base) | 100% | +2.80% | +2.80% | [+2.02, +3.60] |
| Rank 1, gap-2 mean premium net of own impact (+2.4%) | 100% | +1.38% | +1.38% | [+0.59, +2.17] |
| **Rank 2**: after the first historical third-party buyer, natural premium | 100% | **+0.03%** | +0.03% | [-0.64, +0.74] |
| Rank 2, slippage cap +2%, 0.003 SOL per failure | 69% | +0.09% | -0.12% | [-0.64, +0.42] |
| Rank 3, natural premium | 100% | -1.33% | -1.33% | [-1.98, -0.68] |
| Rank 3, cap +2%, 0.003 SOL per failure | 66% | -0.06% | -0.24% | [-0.76, +0.27] |
| Rank 1, first follower never lands | 100% | -0.20% | -0.20% | [-0.86, +0.48] |
| **Combined realistic** (mean premium, SOL-fixed impact, 0.01 SOL, first follower fails 1 in 3) | 100% | **-1.07%** | -1.07% | without follower failures: -0.13% [-0.90, +0.63] |

**Where the money is.** Split the 1,342 launches by whether the first third-party buyer after the bundle lifted the curve more than 2% above post-bundle:

| | Launches | Rank-1 EV |
|---|---|---|
| Lifted > 2% ("hot") | 431 (32%) | **+11.7%** |
| Lifted ≤ 2% ("quiet") | 911 (68%) | **-0.5%** |
| Holdout, hot | 33% | +9.1% |
| Holdout, quiet | 67% | +0.3% |

* The policy is a bet on being directly in front of a big sniper. On a quiet launch it is a fast, small loss.
* Nothing in slot 0 before our buy tells the two apart. S1 swept post-bundle mcap, bundle wallets and dev buy, and found no filter.
* gap-2 shows where the competition is: rival bots key on the visible bundle, the first slot-0 winner's median bid is 0.0034 SOL (p90 0.068), and the earlier winner had the higher bid in 73% of contested pairs.
* So the launches that pay are the ones where we are least likely to be first, and most likely to have to bid 0.02-0.07 SOL (4-14% of a 0.5 SOL position) to get there.

**Per-attempt EV at rank 1.** S1 base EV per landed trade, minus failed attempts:

| Landing rate | Failure costs 0.0002 SOL | Failure costs 0.003 SOL |
|---|---|---|
| 100% | +2.80% | +2.80% |
| 50% | +1.38% | +1.10% |
| 30% | +0.81% | +0.42% |
| 20% | +0.53% | +0.08% |
| 10% | +0.24% | -0.26% |

This table *assumes* the landed trades are a random subset of launches. Given the adverse selection above, they will not be: we will land more easily on the quiet launches.

## 3. S3-specific checks

* **The lift lived in the listing-sampled operators.**
  * Armed creator-EB launches: **+10.5%** in the listing samples (`exp`, `opA`, `opB`; n = 326), against **+2.9%** in the random sample (n = 50, weighted).
  * Unarmed: +1.2% and +3.6% respectively.
  * **The same operators look worse in the random sample.** 19 armed operators appear in both. Their random-sample launches average **+6.7%** (n = 46, unweighted), their listing-sampled launches **+11.3%** (n = 180). The paired per-operator difference is **-4.7 points [90% CI -10.1, -0.1]** (`critic_audit.json`, `s3.paired_19_ops_random_vs_listing`).
  * So the listing samples overstate the very operators that get armed. Two explanations, which this data cannot separate:
    * the frontend `/coins?creator=` listing, paged at four offsets, is not an outcome-neutral sample of a creator's launches;
    * regression to the mean in a small, heavily searched universe (84 operators, 700 configurations).
  * Either way, it predicts the holdout result in §4.
* **The armed set is more rank-robust in sample, and that would have mattered if arming were real.** S3's armed trades keep:

  | Stress | In-sample EV |
  |---|---|
  | Rank 2 | +4.4% |
  | Rank 3 | +2.2% |
  | First follower never lands | +5.6% |
  | +3.6% premium and 0.01 SOL | +5.5% |

  That is why the holdout test of arming was the decisive one. It failed.
* **Data defect fixed:** the 10 non-standard-curve `ops` coins (§1). With them removed, S3's numbers move by under 0.2 points.

## 4. Fresh holdout

### Pre-registration

`critic_preregistration.json` was written at **14:46:56 UTC**, before the first holdout tape was pulled (14:47:37). It holds:

* the three policies;
* the frozen armed creator sets (creator key, skip13, no cap), computed from every launch in `arming.load_launches()`. The history ends at 2026-10-14 05:54:34 UTC.

| Rule | Armed creators |
|---|---|
| EB (posterior 20th percentile > 0, n ≥ 5) | 35 |
| GTP (the guru's rule on the policy's TP) | 26 |
| G100 (the guru's rule: ≥ 3 of the last 10 reached +100%) | 25 |

* EB family parameters: mean +3.85%, τ = 7.1%, σ = 16.4%.
* Hypotheses:
  * H1: P1 > 0 on the random holdout launches;
  * H2: P2 > 0;
  * H3: P3 > 0;
  * H4: P2 minus unarmed > 0.

  BH applies over H1-H4.

### Frame and pull

* **Frame.**
  * `getSignaturesForAddress` on the pump mint authority (`TSLvdd…`), anchored at 14:43 UTC. The frame is 6,991 signatures with anchor - 6 h ≤ blockTime ≤ anchor - 60 min, i.e. 08:43-13:43 UTC.
  * Every one was decoded with `dataset.creation_record` on api.mainnet-beta: 8,900 calls including 1,910 rate-limit retries.
  * Result: 6,938 creates, of which 1,980 mayhem (28.5%). **3,764 are eligible**: non-mayhem, SOL-quoted, one create per tx, not a slot-0 graduation.
* **Armed launches.** 164 of the eligible creates are by frozen-armed creators: 132 EB launches from 13 creators, 80 G100 launches from 8 creators, 45 GTP. **All 164 were pulled** (enumeration, not a sample).
* **Random sample.** 409 more launches in seeded random order.
* **Tapes.**
  * swap-api, first 25 s (4 s with up to 4 pages if busy).
  * **795 requests** against the cap of 800, including 54 shared-limit retries, at ≤ 1.2 req/s (72/min).
  * Every tape is complete back to the create, with no token-flow gaps.
  * Armed: 164 pulled, 18 dev-only, **146 multi-wallet**.
  * Random: 409 pulled, 39 with no trade at all, 193 outside the bundled / dev ≥ 1 SOL class, 63 dev-only, **114 multi-wallet**.
* **Second holdout (copy).** The copytrade critic's random tapes for 06:12-10:45 UTC, deduplicated: 385 pulled, 108 multi-wallet. They were never used by S1 or S3.
* **Secondary set (not pre-registered).** The orchestrator's dense window after the history cut, 05:55-08:05 UTC: 263 multi-wallet launches.

### Results

Headline table above. The stresses below are EV per trade; n is P1 = 222, P2 = 132, P3 = 67.

| Stress | P1 random | P2 EB-armed | P3 G100-armed |
|---|---|---|---|
| Base (+1.1%, 0.004 SOL, 1-slot exit) | +3.17% | +2.93% | +2.08% |
| +3.6% premium | +0.41% | +0.02% | -1.10% |
| gap-2 mean premium net of own impact (+2.4%) | +1.84% | +1.60% | +0.85% |
| 0.01 SOL per round trip | +1.97% | +1.73% | +0.88% |
| SOL-fixed own impact | +2.84% | +2.65% | +1.74% |
| Exit latency 2 slots | +2.91% | +2.29% | +1.83% |
| **Rank 2, natural premium** | **+0.80%** | +0.90% | -0.44% |
| Rank 2, cap +2%, 0.003 SOL per failure | +0.56% | +0.85% | +0.21% |
| First follower never lands | +0.84% | +0.97% | n/a |
| **Combined realistic** | **-0.39%** [-1.99, +1.27] | -0.47% [-2.50, +1.51] | n/a |

**The lead is strongly time-varying.** P1 on each clean set:

| Set | Window (UTC) | n | EV |
|---|---|---|---|
| Dense window, after the history cut | 05:55-08:05 | 263 | +1.98% [+0.18, +3.85] |
| Copy holdout | 06:12-10:45 | 108 | +0.10% [-2.07, +2.24] |
| Our frame, random launches | 08:43-13:43 | 114 | +6.08% [+3.61, +8.71] |

By UTC hour (all 485 clean launches), EV ranges from -0.1% (11:00, n = 18) to +8.9% (13:00, n = 21). The 07:00 hour (n = 166) is +1.2%. A single 4-5 hour window can show anything from 0% to +6%. Evaluations shorter than a day are noise.

**Armed creators regress.** EB-armed launches by creator:

| Creator | Launches | EV |
|---|---|---|
| `GeUnv1jm` | 38 | -0.4% |
| `5FqUo9aB` | 31 | -1.8% |
| `Aqje5DsN` | 17 | +2.9% |
| `9ncm3Mfx` | 14 | +6.6% |
| `6gAgy3F1` | 12 | +7.2% |
| 10 creators combined | 1-4 each | |

The two heaviest armed creators were armed on +5% to +10% histories and now sit at about zero.

## 5. Verdict and recommendation

* **Which policies survive?**
  * **P1 (the S1 lead, unarmed) survives on historical and fresh tapes as a simulation.** It is positive on every out-of-sample set and on the fresh holdout (+3.2%, q = 0.005). Across 485 clean post-history launches it makes +2.5% [+1.3, +3.8].
  * **P2 (S3 arming) is positive only because P1 is.** The arming adds nothing (-0.2 points). The S3 claim of +9% with a 5.7-point lift is refuted.
  * **P3 (the guru's selection rule on our exit)** is not significant (+2.1%, q = 0.29).
  * GTP arming scored +9.1% on 43 launches from 9 creators. It was pre-registered as a *secondary* test, outside the BH family. Its in-sample lift was the same as EB's, which failed. With 9 clusters and the S3 failure, treat it as noise until it replicates on a week of shadow data; it is not a lead.
* **Is it a deployable edge?** **Not yet, and possibly never for us.**
  * The tape edge is the premium paid by the first big sniper behind us. It disappears at rank 2 (+0.0% / +0.8%) and under realistic fill costs (-0.4% to -1.2%).
  * Our landing rank is unmeasured, and the hot launches are where pro snipers bid hardest.
* **Recommendation: deploy P1 in paper (shadow) mode only. Move no money.**
  1. On every multi-wallet bundled launch, send nothing. Record the would-be landing index (the time from the create shred to our signed tx) against the actual index of the first follower, and the post-bundle price and premium.
  2. Score the lead on the real tape, at our would-be position, split hot / quiet.
  3. **Go/no-go rule, set now:**
     * Shadow EV per attempt at our measured rank and premium must have a 90% lower bound above 0 over ≥ 1,000 qualifying launches (≥ 1 day; the hourly swings demand it).
     * We must be ahead of the first follower on at least about 1/3 of hot launches, at a premium of ≤ +2%.
     * Otherwise: no edge, stop.
  4. **Drop operator arming as a filter.** Keep the creator and bundle-component posteriors as logged fields only. They cost nothing and can be re-tested on a week of shadow data.

## 6. Single most promising lead

**The S1 rank-1 micro-scalp (P1), on the hot subset, if and only if the bot can be first behind the bundle.**

* Spec: E1 entry right after the bundle on multi-wallet bundles, 0.5 SOL. Exit at the first of realised +15%, a bundle-wallet sell, 1 s idle, -15% after 1 s, or slot+5.
* Its value per landed rank-1 trade is +2.5% to +3.2% on fresh data, about +9-12% on the third of launches where a big sniper follows us, and about 0% on the rest.
* The whole question is now an engineering measurement:
  * our create-to-landing latency in tx-index terms, against the fastest repeat snipers (gap-2: 5-9 txs after the create; first winner at a median of 149);
  * the bid it takes to win, on exactly the hot launches.
* If those two numbers come out where we need them, this is the edge. If not, there is no edge in this data: every slower variant, the guru's +100% rule, operator arming, mayhem and copy-trading has been tested and lost.

## Reproduce

```
.venv/bin/python -I analysis/scripts/strategy/critic_audit.py                  # section 1-3 audit, no network (~1 min)
.venv/bin/python -I analysis/scripts/strategy/critic_freeze.py                 # pre-registration (refuses to overwrite)
.venv/bin/python -I analysis/scripts/strategy/critic_holdout.py index          # RPC create index (anchor = now; cached)
.venv/bin/python -I analysis/scripts/strategy/critic_holdout.py decode 3.5     # every create in the frame (~45 min)
.venv/bin/python -I analysis/scripts/strategy/critic_holdout.py pull both 800  # armed first, then random; cap 800 requests
.venv/bin/python -I analysis/scripts/strategy/critic_holdout_eval.py           # pre-registered tests, stresses
.venv/bin/python -I analysis/scripts/strategy/operator_walkforward.py          # S3 re-run with the gap fix
```

The index, decode and pull stages are cached and resumable under `data/cache/strategy_holdout/`. A re-run uses the stored anchor. The hot/quiet split and the combined-realistic numbers for the holdout are stored in `critic_holdout.json` under `combined_realistic_and_hot_split`.
