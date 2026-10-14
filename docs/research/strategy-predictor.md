# G1: can slot 0 tell a busy launch from a quiet one, and does selecting on it raise the LEAD's EV?

*Measured 2026-10-14. Code: `analysis/rugsnipe_analysis/strategy/predictor.py` (table, features), `predictor_models.py` (design matrix, folds, models, statistics), scripts `analysis/scripts/strategy/predictor_{build,meta,funding,pool_fetch,wf,diag,volume,ablation}.py`. Table: `data/cache/predictor/table.jsonl`. Results: `data/reports/strategy/predictor_{preregistration,dev,frozen,holdout,full,diag,volume,ablation}.json` (+ `predictor_frozen_models.pkl`, sha256 in the frozen file).*

The policy is the LEAD micro-scalp of docs/04 §2 throughout (0.5 SOL, entry right behind the creator bundle in the creation slot, exits +15% / bundle-wallet sell / 1 s idle / −15% after 1 s / slot+5). Base scenario: +1.1% premium, 0.004 SOL per round trip, 1-slot exit latency. Every EV is per trade, net of all costs.

## Answer in brief

1. **Yes, busy and quiet can be partly told apart in slot 0. But predicting "busy" is the wrong target.**
   - P(busy) models reach an out-of-sample AUC of **0.65-0.72** for "busy" (more than 100 trades in 15 min). The result is the same on every set and in every model family.
   - Almost all of that comes from **the size of the bundle**: post-bundle market cap and bundle SOL alone give AUC 0.63-0.73 on dev, window, deep and gap-3-post.
   - **Big bundles are busy, but they do not hit the take-profit.** The AUC of bundle size for TP is 0.40-0.51. The 20% biggest bundles make −0.1% per trade out of sample.
   - The busy-versus-quiet EV gap (window +9.4% vs −0.9%) comes from **followers**: independent snipers who land behind us. Busy launches have them more often. The part of "busy" we can predict is mostly the insiders' own volume.
   - So selecting on P(busy) adds only **+0.8 to +2.1 points** of EV, with q = 0.13-0.20 on the Oct 12-14 folds.
2. **Predicting the take-profit directly works better. Most of the lift comes from operator history, not from slot-0 structure.**
   - **Clean test (frozen on pre-Oct-12, evaluated once on Oct 12-14):** the frozen P(TP) model (rule 3) ranks the 1,207 Oct 12-14 launches with **TP AUC 0.70**.
     - Its top tertile (51% of launches) makes **+4.7% [+3.3, +6.1]**, against +2.7% for all launches; the bottom tertile makes −0.5%.
     - The ranking works through operator history. On launches whose signer has earlier launches in our data, the AUC is **0.80**. On first-seen signers it is **0.57-0.59**.
   - **Walk-forward, retrained hourly on all data before each fold:** the best configurations keep the top 20% at **+9.4% [+7.3, +11.6]** (q < 0.001) and the top 40-60% at **+5.5% to +6.5%**.
     - This comes from 1,130 configurations; 201 have q < 0.05 on the Oct 12-14 folds.
     - Recent data matters here: the pre-Oct-12 models are much weaker on first-seen signers.
3. **What selection costs in volume, and what it buys:**

   | 0.5 SOL per trade (Oct 12-14, population-scaled) | Trades/day at 100% landing | EV/trade | SOL/day at 10% / 20% / 50% landing |
   |---|---|---|---|
   | Every multi-wallet bundle (today's LEAD) | 3,750 | +2.7% | **−5.1 / +1.0 / +19.5** |
   | Frozen P(TP), top tertile (clean, frozen before Oct 12) | 1,920 | +4.7% | −0.7 / +4.4 / +19.7 |
   | Retrained P(TP), top 20% (walk-forward) | 760 | +9.4% | +1.5 / +5.3 / +16.7 |
   | Retrained P(E[ret]), top 40% (walk-forward) | 1,530 | +6.5% | +0.9 / +6.3 / +22.7 |

   Failed attempts cost 0.003 SOL each (docs/04 §3).
   - **At a 50% landing rate, total SOL/day is about the same with or without selection.** We trade a third to a half as often for 2-3.5x the EV per trade.
   - **At 10-20% landing rates, selection is what makes the policy positive.** It cuts the number of failed attempts by 50-80%. Each trade is also further from break-even: the +3.6% premium stress keeps +2.0% to +6.8%, where the unselected policy is at 0.0%.
4. **Sizing by P is not a free lunch.**
   - Frozen rule 3 (skip the bottom tertile, 0.5 SOL in the middle, 1.0 SOL in the top) earns **+8.8 mSOL per launch more than the same size mix assigned at random** (p < 0.001 on Oct 12-14). That gain is real.
   - But its headline SOL/day (+48.8 at 50% landing) relies on 1.0 SOL tickets. docs/04 §4 caps tickets at 0.5 SOL because the simulator does not model the snipers a bigger rank-1 buy knocks out.
   - Capped at 0.5 SOL, the same frozen score gives **+3.3% to +4.7% per trade at about the same SOL/day as no selection**.
5. **Verdict: selection is LIKELY real, and it raises EV per trade about 2-3.5x. It does not raise total profit unless landing is the bottleneck.**
   - The clean frozen test supports **+4.7% at half the volume**.
   - The retrained walk-forward supports **+6.5% at 40% and +9.4% at 20% of the volume**, if we retrain continuously on a full feed.
   - **"Can we pick the busy ones?"** Partly, but busy is not the target. **"Can we pick the ones where snipers follow us?"** Yes, mostly by knowing the operator.

## 1. Data: one table of every multi-wallet bundled launch on disk

| Source | Multi-wallet launches (after mint dedupe) | Period | Tape | "busy" label |
|---|---|---|---|---|
| gap-3 random sample | 488 (413 before Oct 12) | Sep 28 - Oct 14 | to 900 s (10 truncated) | trades in 15 min > 100; truncated tapes = busy |
| deep sample | 77 | Oct 12-14 | 900 s | same |
| fresh holdout | 144 (+6 kept under the window, which has the longer tape) | Oct 12-14 | 150 s | none (n10/n60 only) |
| dense window (workflow C) | 911 | Oct 14 02:05-08:05 | 900 s ("full" mode = busy) | same |
| **random-sample table** | **1,620** | | | 1,475 labelled, 32% busy |
| operator pools of strategy-operators.md (exp, opA, opB, ops) | 1,345 (1,123 before Oct 12) | Sep 28 - Oct 14 | 2-25 s | none; **training rows and operator history only, never tested** |

- **Dedupe.** The longest horizon wins; ties go to gap-3, then deep, fresh, window. LEAD returns per launch come from the shared simulator (`sim.fire_slots` / `fill_index` / `value`). Pooled EVs reproduce docs/04: gap-3 +4.4%, deep +3.2%, fresh +3.3% (+3.7% on the original 150), window +2.5%.
- **Labels (outcomes, never features):**
  - LEAD return under six scenarios: base, 0.25 SOL, 1.0 SOL, +3.6% premium, 0.01 SOL cost, 2-slot exit latency.
  - TP hit (the +15% leg fires first).
  - busy900.
  - n10 / n60 (trades in the first 10 / 60 s).
  - s0_third: independent slot-0 buyers who land after the bundle, i.e. after us.

**Features: only what is observable at the decision instant** (84 columns, `predictor_models.feature_row`):

| Group | Features | Source / live availability |
|---|---|---|
| bundle in front of us | wallets, bundle SOL, dev SOL, post-bundle mcap, dev share, per-wallet min/mean/max, CV, roundness of amounts, equal-token buys, descending sizes, % of supply taken | the create tx + the bundle shreds (the entry needs them anyway) |
| create tx | in-block index, Jito tip present / size, priority fee, CU used / limit / price, top-level ixs, tx version, signers, creator != signer, fee payer != signer | decoded from the cached `getTransaction` (`createtx.rows_from_tx`); all are in the shred |
| constants (dropped) | launcher program (1,617 / 1,620 direct), creator_fee_bps (30 everywhere), cashback (false), mayhem (false), token program | |
| name / symbol / uri | lengths, caps share, emoji, non-ASCII / CJK, "ticker-like" symbol, $ prefix, name = symbol, digits; uri host (j7tracker 556, ipfs.io 481, uxento 256, mypinata 125, rapidlaunch 80, other) | in the create ix |
| metadata JSON | twitter / telegram / website present, tweet link, description length, createdOn | fetched for the study (§6) |
| signer funding | class (serial ≥ 1,000 prior txs, CEX-fresh, CEX-old, mother-fresh, mother-old, swap service), prior tx count, funding amount, lag, round amount | caches: gap-1 (by create sig), window, analytics.db; 134 deep/fresh signers and every pool launch fetched here (one `getSignaturesForAddress` before the create sig + ≤ 3 decodes); all point-in-time |
| operator history (signer key, point in time) | prior launches in the gap-1 1.27% sample; prior table launches; prior LEAD hit rate, LEAD EV and mean followers (known 10 s after each launch); prior busy rate (known 15 min after) | `op_tab_*`: random-sample table only; `op_all_*`: + operator pools |
| time / market | hour (sin / cos), weekend, regime (post Oct 9), creates in the previous 60 s and 600 s (gap-1 index of every create) | |

## 2. Method

- **Walk-forward folds:**
  - Dev: test windows of 2 days from Oct 2 to Oct 12.
  - Full: the same, then Oct 12, Oct 13, Oct 14 00:00-02:05, and six 1-hour folds over the window.
  - Training uses every row created before the fold start minus a 15-minute embargo, so every busy label is known when used.
  - Weights follow gap-3 (w = 5 for the 1-in-5 low-ATH subsample).
- **Configurations (1,130 per run).**
  - *Univariate threshold rules (828 + 302):* every feature, keep ≥ or ≤ the training 25/50/75% quantile, plus inter-quantile bands. The threshold is recomputed in each fold.
  - *Models (2 training variants × 2 feature sets × 4 models × 5 targets × 6 policies):*
    - Training variants: random rows only / + operator pools.
    - Feature sets: all features / a 27-feature core set.
    - Models: logistic or ridge at C = 0.05 and 0.5; gradient-boosted trees of depth 2 and 3.
    - Targets: busy, TP hit, return, log followers, log n10.
    - Policies: keep the top 80/60/40/20% by training-quantile threshold; size by tertile (0.25 / 0.5 / 1.0 SOL); size with skip (0 / 0.5 / 1.0).
- **Statistics.**
  - Lift = kept EV − EV of all test launches, with a one-sided normal p (SE scaled by √(1 − keep share)).
  - Benjamini-Hochberg q over all 1,130 configurations of the run.
  - Sizing is compared with **the same size mix assigned at random**, so that bigger tickets' cheaper fixed cost per SOL does not count as prediction.
- **Freeze protocol.**
  - The criterion was written to `predictor_preregistration.json` (15:34 UTC). That was after the random-sample-only dev grid had been seen, but before the operator-pool dev grid and before any feature-level Oct 12-14 number.
  - Criterion: among dev configurations with ≥ 40 test launches and ≥ 20% kept, take the best univariate rule and the best model rule by the dev lower 90% bound, and the best sizing rule by its p against the random-mix control.
  - The three rules were refitted on every pre-Oct-12 row (16:12 UTC) and evaluated **once** on the Oct 12-14 sets (16:12).
  - The full walk-forward and the ablation were run after that.

## 3. Busy vs quiet: what slot 0 can and cannot see

**Descriptive** (no fitting; the direction is known from dev):

| Set | busy rate | EV busy / quiet (oracle) | AUC for busy: post-bundle mcap | bundle SOL | wallets | AUC for TP hit: post-bundle mcap |
|---|---|---|---|---|---|---|
| dev (gap-3 < Oct 12) | 0.29 | +13.2% / +1.3% | 0.73 | 0.73 | 0.63 | 0.51 |
| window | 0.33 | +9.4% / −0.9% | 0.64 | 0.63 | 0.56 | 0.44 |
| deep | 0.31 | +15.5% / −2.4% | 0.69 | 0.73 | 0.55 | 0.51 |
| gap-3 Oct 12-14 | 0.35 | +9.1% / −0.4% | 0.71 | 0.67 | 0.52 | 0.40 |

**Dev, by quartile of post-bundle mcap:**
- busy rate: 0.11 / 0.39 / 0.42 / 0.48;
- EV of the busy launches inside each quartile: +19.6 / +15.9 / +9.7 / +8.6%;
- EV of the quiet ones: +1.4 / +3.2 / +1.2 / −2.7%.

Busy launches win at every size. But the busy share rises with size while the busy launches' EV falls with it, so size is a busy predictor and not an EV predictor. The outcome that carries the EV is **followers**. With ≥ 2 independent slot-0 buyers behind the bundle, EV is +8.4% to +13.9%; with fewer, −0.6% to +1.4%. The share with followers is 27-35%, about the same as the busy share.

**Model AUCs for busy, out of sample:**
- dev: 0.64-0.82;
- full walk-forward: 0.65-0.72 (the best is n10-target GBT, 0.70-0.72);
- frozen rule 2 on Oct 12-14: 0.64 on the window, 0.73 deep, 0.71 gap-3-post.

**What that buys in EV:**

| P(busy) selection (full walk-forward, Oct 12-14 folds) | keep | busy kept / dropped | EV kept | lift | q |
|---|---|---|---|---|---|
| core GBT-2, busy target, top 40% | 51% | 0.44 / 0.22 | +3.5% | +0.8 | 0.20 |
| core GBT-3, busy target, top 20% (pool training) | 21% | 0.51 / 0.28 | +4.8% | +2.1 | 0.13 |
| frozen rule 2 (n10 GBT, top 40% of dev) | 55% | 0.42 / 0.23 | +2.7% | −0.0 | p = 0.51 |

## 4. The frozen test (pre-Oct-12 only, then once on Oct 12-14)

**Frozen rules** (`predictor_frozen.json`):

| # | Rule | Dev evidence (walk-forward Oct 2-12, n = 281) |
|---|---|---|
| 1 | Create in-block index ≤ 295 (first quartile of dev) | 21% kept, +8.7% vs +4.8%, p = 0.06 |
| 2 | Pool-trained core GBT-2 on log n10 (trades in 10 s), keep top 40% (score ≥ 2.89) | 33% kept, +9.3%, p = 0.016 |
| 3 | Pool-trained all-feature GBT-3 on TP hit; skip P < 0.145, 0.5 SOL for P < 0.243, else 1.0 SOL | +12.0 mSOL/launch over the random-mix control, p = 0.026 |

None of the 1,130 dev configurations survives BH (minimum q = 0.69): the dev set is 413 launches, effective n ≈ 230.

| Rule | Set | Launches | Kept | EV kept [90% CI] | EV all | EV dropped | Lift [90% CI] | p |
|---|---|---|---|---|---|---|---|---|
| 1 tx index | fresh | 150 | 33% | +8.7% [+3.1, +14.7] | +3.7% | +1.1% | +5.1 [+0.7, +9.8] | 0.03 |
| | window | 915 | 32% | +2.8% [+1.1, +4.5] | +2.5% | +2.4% | +0.3 [−1.0, +1.6] | 0.36 |
| | deep | 77 | 32% | +2.8% | +3.2% | +3.4% | −0.4 | 0.54 |
| | gap-3 post | 75 | 38% | −3.0% | +2.9% | +6.5% | −5.9 | 0.99 |
| | **all Oct 12-14** | 1,207 | 32% | +2.8% [+1.2, +4.4] | +2.7% | +2.6% | **+0.1** [−1.2, +1.3] | 0.47 |
| 2 n10 GBT | **all Oct 12-14** | 1,207 | 55% | +2.7% [+1.5, +3.9] | +2.7% | +2.7% | **−0.0** [−0.8, +0.7] | 0.51 |
| 3 P(TP) sizing | **all Oct 12-14** | 1,207 | 78% traded | 28.4 mSOL/launch vs control 19.5 | | | **+8.8 mSOL**, p < 0.001; window +8.4 (p < 0.001), deep +18.3 (p = 0.03), fresh +8.4 (p = 0.11), gap-3 post +5.4 (p = 0.30) | |

By the pre-registered success criterion, **rules 1 and 2 fail and rule 3 passes**.
- Rule 1's fresh-holdout result (+5.1 points, p = 0.03) is the kind of fluke a single set produces: it is flat on the window and negative on gap-3-post.
- Rule 3 passes, and its frozen score is the useful object:

| Frozen P(TP), by frozen tertile cut points | Low | Mid | High |
|---|---|---|---|
| all Oct 12-14: share / EV / TP rate / followers | 22% / −0.5% / 0.08 / 0.78 | 26% / +1.4% / 0.14 / 1.26 | **51% / +4.7% [+3.3, +6.1]** / 0.28 / 1.56 |
| window | −0.5% | +1.1% | +4.4% [+3.0, +5.8] |
| deep | −2.0% | −0.2% | +8.1% [+0.7, +16.3] |
| fresh | +0.2% | +3.5% | +5.3% [+1.6, +9.4] |
| gap-3 post | +1.5% | +2.0% | +4.8% |
| high tertile at +3.6% premium / 0.01 SOL cost | | | +2.0% / +3.5% (all launches: 0.0% / +1.5%) |

**Where the frozen score's skill comes from** (TP AUC on Oct 12-14):

| Subset | Launches | TP AUC |
|---|---|---|
| all | 1,207 | 0.70 |
| signer seen before (operator history) | 568 | **0.80** |
| first-seen signer | 639 | 0.59 |

**Permutation importance** (AUC drop): prior LEAD hit rate of the operator 0.031, operator's prior followers 0.020, mean bundle-wallet size 0.016, operator's prior LEAD EV 0.012, smallest bundle wallet 0.010, creates in the last 10 min 0.007, prior count in the gap-1 sample 0.007, CU limit 0.005. Nothing else is above 0.005.

## 5. The full walk-forward (retrained before every fold, through the end of the window)

1,130 configurations. All OOS test launches: 1,488 (base +3.3%); Oct 12-14 folds: 1,207 (base +2.7%). q < 0.05 for 255 configurations on all folds and 288 on the Oct 12-14 folds.

| Configuration (Oct 12-14 folds) | Kept | EV [90% CI] | Lift | q | +3.6% premium | 0.01 SOL | busy kept / dropped |
|---|---|---|---|---|---|---|---|
| random rows, all features, GBT-3, TP, top 20% | 20% | **+9.4%** [+7.3, +11.6] | +6.7 | < 0.001 | +6.8% | +8.2% | 0.41 / 0.31 |
| random rows, core, logistic, TP, top 20% | 25% | +7.8% [+5.7, +9.8] | +5.1 | < 0.001 | +5.3% | +6.6% | 0.41 / 0.30 |
| + pools, all, ridge, return, top 40% | 41% | +6.5% [+4.9, +8.2] | +3.9 | < 0.001 | +3.8% | +5.3% | 0.38 / 0.30 |
| + pools, all, ridge, return, top 60% | 61% | +5.5% [+4.3, +6.7] | +2.8 | < 0.001 | +2.7% | +4.3% | 0.37 / 0.27 |
| random rows, all, GBT-3, TP, top 40% | 49% | +5.0% [+3.6, +6.3] | +2.3 | < 0.001 | +2.3% | +3.7% | 0.33 / 0.33 |

**Top univariate rules** (Oct 12-14 folds, one per feature; dev walk-forward lift in brackets):

| Rule | Kept | EV | Lift | q | dev lift |
|---|---|---|---|---|---|
| uri host = meta.uxento.io | 18% | +6.4% | +3.7 | < 0.001 | [+10.4] |
| mean bundle-wallet size ≤ q25 | 24% | +5.7% | +3.0 | 0.002 | [−0.2] |
| create priority fee ≤ q25 | 32% | +4.8% | +2.1 | 0.007 | [+3.8] |
| operator's prior LEAD EV ≥ q75 | 19% | +4.7% | +2.1 | 0.08 | [+2.3] |
| metadata lists a website | 70% | +3.9% | +1.2 | 0.004 | [+2.4] |

The worst univariate rules: operator's prior LEAD EV ≤ q25 −1.7% (lift −4.4); bundle SOL ≥ q75 −0.1% (−2.7); no website −0.0% (−2.7).

**Ablation** (same folds; TP target; Oct 12-14 folds; `predictor_ablation.json`):

| Feature set (random rows) | TP AUC, all / window / other sets | Top 20% EV |
|---|---|---|
| all | 0.71 / 0.75 / 0.61 | +9.4% |
| bundle only (GBT) | 0.68 / 0.70 / 0.63 | +7.8% |
| all minus operator history | 0.66 / 0.70 / 0.55 | +7.8% |
| operator history only | 0.63-0.67 | +4.5% to +4.8% |
| create tx only | 0.59-0.62 | +5.5% to +6.6% |
| metadata only | 0.58-0.60 | +3.4% to +5.8% |
| name / uri only | 0.53-0.59 | +4.3% to +4.6% |
| funding only | 0.49-0.56 | +3.2% to +4.4% |
| time / create rate only | 0.49-0.50 | +2.0% to +5.0% |

**Reading the walk-forward:**
- Once the model is retrained on recent data (for the window folds, the preceding hours of the same window), the bundle's fine structure becomes a strong TP signal: a non-linear model of bundle sizes alone reaches AUC 0.70. Logistic on the same features gets 0.60.
- Frozen on pre-Oct-12 data, the same structure is weak (AUC 0.57-0.59 on first-seen signers).
- Our reading: **bundle shape works as an operator fingerprint**. It is learnable only while the operators who use that shape keep launching, which is the operator finding of strategy-operators.md seen through slot-0 structure. It decays within days.
- Funding class, create rate, hour, name/symbol style and social links add little or nothing once the bundle and operator are known.

## 6. Metadata JSON and other live-availability notes

- **Metadata fetch.** 1,597 of 1,620 fetched. Latency p50 **449 ms**, p90 763 ms, p99 2.9 s.
  - ipfs.io answered 403/429 to this box. 482 of its CIDs were fetched through ipfs.filebase.io / 4everland instead.
  - That latency exceeds a slot (200 ms post Oct 9), so the JSON **cannot be read before the slot-0 decision**.
  - Live, the only usable form is an operator cache (uri host / template → the operator's usual socials) or nothing.
  - The ablation says nothing is lost: all-minus-metadata matches all features (AUC 0.71). The "website present" rule, +1.2 points at 70% kept, would be the only casualty.
  - The **uri host** is in the create instruction itself and needs no fetch.
- **Everything else is in the create tx or the bundle shreds.** That covers the bundle, the in-block index, tip, priority, CU, name, symbol and uri. Funding and operator history need a background watcher that keeps per-signer state current: docs/06 already has the funding watcher; operator history comes from our own tape of past LEAD outcomes.

## 7. What did not work

| Angle | Result |
|---|---|
| Predicting "busy" (> 100 trades in 15 min) and trading only P(busy)-high launches | AUC 0.65-0.72, but EV lift +0.8 to +2.1 points, q ≥ 0.13. The predictable part of busy is bundle size, which does not predict the TP. |
| Bundle size as a filter | Bigger bundles are busier and worse: top-quartile bundle SOL makes −0.1% per trade OOS. |
| Frozen rule 1 (early in-block create) | +0.1 point on Oct 12-14 (flat on the window, −5.9 on gap-3-post) despite +5.1 on the fresh holdout. |
| Frozen rule 2 (predicted trades in 10 s) | −0.0 point on Oct 12-14. It picks busy launches (0.42 vs 0.23) and earns nothing. |
| Funding class (CEX-fresh / mother / serial) | Funding-only TP AUC 0.49-0.56; no lift. |
| Time of day, weekend, regime, current create rate | AUC 0.49-0.50 alone; nothing. |
| Name / symbol style, emoji, ticker-like symbols | AUC 0.53-0.59 alone; small, inconsistent. |
| Launcher program, creator_fee_bps, cashback, mayhem | Constant on multi-wallet bundles (1,617 / 1,620 direct creates): no information. |
| Anything frozen on pre-Oct-12 data for first-seen signers | TP AUC 0.57-0.59: almost no slot-0 signal survives two days without retraining. |
| 1.0 SOL tickets on high-P launches | The simulated gain is mostly fixed-cost amortisation plus unmodelled reflexivity; do not use (docs/04 §4). |

## 8. Caveats

- **Search size.**
  - 1,130 configurations per walk-forward run, run twice (dev and full).
  - Plus 60 ablation configurations and 3 frozen rules.
  - Earlier development runs of subsets of the same grid (636 and 818 configurations) on dev only.
  - The dev grid had no BH survivor. The full-walk-forward q-values are over its own 1,130.
- **The operator pools.** strategy-operators.md chose them with full-period launch counts, so a signer having pool history partly encodes "heavy bundler over the 16 days". The best walk-forward rows use **random rows only** (no pools) and do as well or better, so the result does not depend on the pools. The frozen rule 3 does use them.
- **The window dominates the Oct 12-14 numbers** (915 of 1,207 launches, one 6-hour block, the same operators repeating). On the other Oct 12-14 sets:
  - TP AUC is lower: 0.61 retrained, 0.64-0.74 frozen.
  - The top-20% EV is lower: +7.9% vs +9.9%.
- **Sample biases.** The fresh holdout is depleted of big winners (docs/04 §3). The window misses the very busiest launches (> 15 API pages).
- **Population scaling.** 3,750 multi-wallet bundled launches per day is an estimate, and trades/day assume we land on all selected launches. Landing rank, premium and reflexivity are unchanged from docs/04 §3 and still unmeasured.
- **Selection may change the rank race.** The launches we select are the ones other snipers also want (followers 1.56 vs 0.78 in the bottom tertile). The +1.1% rank-1 premium may be optimistic for exactly these launches. At +3.6% the selected EV stays positive (+2.0% frozen top tertile, +6.8% retrained top 20%).

## 9. Recommendation

- **Shadow mode** (docs/04 §7) should log the P(TP) score for every qualifying launch, alongside the would-be fill. Two versions:
  - the frozen model (`predictor_frozen_models.pkl`, rule 3; its tertile cut points);
  - an hourly-retrained logistic / GBT-3 on all LEAD outcomes so far.
- **The probe** should use the score in one way only: **skip the bottom tertile** (−0.5% per trade, 22% of launches) and keep 0.5 SOL elsewhere.
  - Frozen, that is +3.6% per trade on 78% of launches, against +2.7% on all.
  - Tighten to the top tertile or quintile once the shadow data confirm the retrained skill (+4.7% frozen, +9.4% retrained).
- **Size stays at 0.5 SOL.**
- **What gates the business is still the landing rate.** Selection matters most if it is low: at a 20% landing rate it is the difference between +1 SOL/day and +4 to +6 SOL/day.
