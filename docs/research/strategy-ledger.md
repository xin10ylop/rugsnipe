# Strategy ledger: every angle tested so far

Date 2026-10-14 (state at about 16:00 UTC). This page has one row per angle we have tested. Each row gives
the best **out-of-sample** number we have, its n, the verdict, and the source that holds the full result.

"Out of sample" means one of these:
* a time split (train = first 60% by create time);
* an even/odd-day split;
* a holdout pulled after the policy was frozen;
* a set that played no part in the selection.

An in-sample number appears only where nothing better exists, and the row says so.

Unless a row says otherwise, every return is EV per trade as a fraction of the stake, after:
* 1.25% fee per side;
* our own price impact;
* a +1.1% entry premium;
* 0.004 SOL per landed round trip;
* a 1-slot exit latency.

Brackets are 90% bootstrap CIs.

**Sets used below**

| Name | What it is |
|---|---|
| test | gap-3 time-split test |
| deep | deep random sample, Oct 12-14 |
| fresh | holdout pulled after the stage-2 freeze |
| pooled | test + deep + fresh, n = 427 |
| window | dense window, Oct 14 02:05-08:05 UTC, 915 multi-wallet launches |
| critic holdout | every create 08:43-13:43 UTC Oct 14, pulled after a freeze |

**Verdict labels**

| Label | Meaning |
|---|---|
| **LOSES** | Negative out of sample. |
| **NO GAIN** | No better than the policy it modifies. |
| **POSITIVE** | Positive out of sample, with the stated caveat. |
| **UNPROVEN** | The test cannot decide. |

## 1. The ledger

### 1a. The guru's method as he states it (block-0 entry behind the bundle, hold for a big take-profit)

| # | Angle | Best out-of-sample number | n | Verdict | Source |
|---|---|---|---|---|---|
| 1 | TP +100%, no stop (natural floor), rank-1 entry, 0.25 SOL | test **−10.5%** [−13.9, −6.9]; deep −21.1% | 270 / 133 | LOSES | strategy-sweep §3, `reference.json` |
| 2 | Same with his bot's protective exits (phantom SL 10 s −25%, sell on a 2 SOL buy, 10 s no activity) | test −0.8% [−3.5, +2.2]; deep −4.6%; new operators −1.8% | 270 / 133 / 466 | LOSES (about −1%) | strategy-sweep §3, strategy-operators §1 |
| 3 | "Buy on dev sell": enter the slot after the first insider sell, TP +100% | test −13.1%; deep −22.0% | 262 / 129 | LOSES | strategy-sweep §3 |
| 4 | "Snipe delay": enter at the end of slot+1, TP +100% | test −16.9%; deep −28.5% | 270 / 133 | LOSES | strategy-sweep §3 |
| 5 | Guru rule on the dense window: end-of-slot-0 entry, TP +100%, 150 s idle exit | −25.1% (hit 6.0%); best of 172 variants −9.3% (n = 131); half-split walk-forward −11.0% | 932 | LOSES | onchain-backtest |
| 6 | Naive end-of-slot-0 entry, TP 2×, random arm | −18.6% [−23, −14]; bundled −27.1% | 396 | LOSES | 05-validation §2 |
| 7 | TP +100%, armed by his own rule (≥3 of the last 10 reached +100%) | −20.5% [−37.5, +0.9] (unarmed −12.3%) | 27 | LOSES | strategy-operators §0 item 5 |
| 8 | "Follow the good rugger" (snipe a cluster only after ≥33% earlier hits) | −23% to −27% per snipe, hit 9-13% | 15-35 | LOSES | onchain-backtest |
| 9 | His hit-rate arming (3 of 10 at +100%), as a hit rate | 18.6-21.8% out of sample against a 43-48% break-even (top tercile 15-16%) | 32-55 per split | LOSES (2-3× short) | gap-1 §5 |

### 1b. Exit families, stage-1 grid and other entries (gap-3 tapes; 25,397,064 configurations, 0 with BH q ≤ 0.10)

| # | Angle | Best out-of-sample number | n | Verdict | Source |
|---|---|---|---|---|---|
| 10 | Single exit signals with TP +100% (bundle sell, creator sell, ≥2% supply sell, idle 3/5/10 s, trail 20/30%, consolidation) | in-sample EVnet −0.6% (idle 3 s) to −13.0% (consolidation); none positive | 675 (153) | LOSES | gap-3 §4 |
| 11 | gap-3 best combo: TP +100% + idle 3 s + sell ≥2% supply | test +1.5% [−0.5, +3.7]; deep −2.3% | 270 / 133 | NO GAIN | strategy-sweep §3 |
| 12 | gap-3 recommendation: TP +40% + insider sell + idle 3 s | test +0.2%; deep −0.2%; new operators +0.9% [−0.8, +2.7] | 270 / 133 / 466 | NO GAIN | strategy-sweep §3, strategy-operators §1 |
| 13 | Best train exit, E1 (rank-1) entry | test −0.1% [−2.0, +2.2] | 270 | NO GAIN | strategy-sweep §2 |
| 14 | E2 (end of slot+1), best train exit | test −6.3% | 270 | LOSES | strategy-sweep §2 |
| 15 | INS (after the first insider sell), best train exit | test −6.2% | 262 | LOSES | strategy-sweep §2 |
| 16 | Survivor entries SURV_N_m (slot+5..50 if the price held and insiders sold) | test −2.5% to −8.8%; SURV_15_20 −17.4% | 21-96 | LOSES | strategy-sweep §2 |
| 17 | Dip entries after the first insider sell (DIP_0..20) | test −6.5% to −6.6%; oracle ceiling only +14-19% | 226-243 | LOSES | strategy-sweep §1-2 |
| 18 | Protocol A top-20 (selected on train) | best test +4.2% [−0.1, +9.6] (n = 73) but deep −1.8%; all q ≥ 0.57 | 73 | NO GAIN | strategy-sweep §2a |
| 19 | Protocol B (even → odd days) | best odd +2.9% (n = 63), deep −4.3%; q ≥ 0.60 | 63 | NO GAIN | strategy-sweep §2b |
| 20 | Top-15 ranked on test (winner's curse) | test +12% to +18%, but train −6% to −14% and deep −6% to −17%; q = 1.00 | 22-71 | LOSES (artefact) | strategy-sweep §2c |
| 21 | Stage-2 frozen F1 (`tp20+idle3+bigbuy3+psl1s-10+hold20`, mcap ≤ 45) | test +0.6%, fresh +3.1%, deep +0.9%; new operators **−1.3%** [−2.4, −0.2] | 161 / 171 / 92 / 895 | LOSES on new operators | strategy-sweep §4, strategy-operators §1 |
| 22 | Stage-2 frozen F2 (`tp20+idle2+bigbuy3+hold10`, mcap ≤ 50) | test +0.1%, fresh +3.0%; new operators −1.3% | 199 / 199 / 1,070 | LOSES on new operators | same |
| 23 | First buyer in slot+1 with the LEAD exits | −3.3% / −1.8% / −4.2% (test / deep / fresh) | 200 / 77 / 150 | LOSES | 04-strategy §5, orchestrator_checks |

### 1c. The LEAD (rank-1 micro-scalp, 0.5 SOL, multi-wallet bundles), set by set

The rule: enter rank 1 behind the bundle, 0.5 SOL, multi-wallet bundles only. Exit at the first of TP +15%, a bundle-wallet sell, 1 s idle, −15% after 1 s, or slot+5.

| # | Set | EV | n | Verdict | Source |
|---|---|---|---|---|---|
| 24 | train (in-sample) | +5.3% | 288 | in-sample | strategy-sweep §4 |
| 25 | gap-3 test | **+3.5%** [+1.4, +5.8], q = 0.04 | 200 | POSITIVE | same |
| 26 | deep | +3.2% [−1.0, +7.5] | 77 | POSITIVE (CI crosses 0) | same |
| 27 | fresh holdout (clean of every selection step) | **+3.7%** [+0.7, +6.6], p = 0.017 | 150 | POSITIVE | same |
| 28 | pooled OOS | **+3.5%** [+1.9, +5.2] | 427 | POSITIVE | same; reproduced here |
| 29 | dense window, all | **+2.5%** [+1.6, +3.4] | 915 | POSITIVE | 04-strategy §3; reproduced here |
| 30 | window, quiet launches (≤100 trades in 15 min) | **−0.9%** [−1.9, +0.1] | 614 | LOSES | same |
| 31 | window, busy launches | **+9.4%** [+7.7, +11.0], hit 65% | 300 | POSITIVE (but the label is not known at entry) | same |
| 32 | new heavy bundlers `opB` (never used before) | +3.4% | 440 | POSITIVE | strategy-operators §1 |
| 33 | expanded bundlers' complete coin sets (`exp` + `opA`) | about +3.0% | 865 | POSITIVE | same |
| 34 | every multi-wallet launch not used to select it ("policy-OOS") | +3.6% [+2.8, +4.5] | 1,625 | POSITIVE | same |
| 35 | critic holdout, random launches (HT frame) | +6.7% [+4.5, +8.9] | 189 | POSITIVE | `critic_holdout.json`* |
| 36 | critic holdout, launches 06:12-10:45 UTC from the copy-trade holdout | **+0.4%** [−1.8, +2.5] | 113 | ≈ 0 | same* |
| 37 | critic holdout, pooled random | +2.9% [+1.2, +4.5] | 178 | POSITIVE | same* |
| 38 | LEAD at 0.25 SOL, pooled | +2.7% [+1.0, +4.3] | 427 | POSITIVE (thinner) | strategy-sweep §4a; reproduced here |
| 39 | LEAD at 0.1 SOL, pooled | +0.2% [−1.4, +1.9] | 427 | NO EDGE (the fixed cost eats it) | same |
| 40 | 108 neighbouring LEAD settings, pooled | all positive, +2.3% to +4.2% | 427 | plateau, not a spike | strategy-sweep §4a |

\* The adversarial critic (S4) was still writing `strategy-critic.md` when this page was compiled (the file holds a placeholder verdict). The numbers are from `data/reports/strategy/critic_holdout.json` as of 15:31 UTC and may change.

### 1d. What breaks the LEAD (stress, pooled OOS unless noted)

| # | Stress | EV | Source |
|---|---|---|---|
| 41 | Entry premium +3.6% (rank-1 median on CEX-fresh bundled launches) | +0.9% pooled; −0.2% window; +0.1% critic holdout | strategy-sweep §5, 04-strategy §5, `critic_holdout.json` |
| 42 | Entry premium +10.8% (rank-3 median) | −6.0% | strategy-sweep §5 |
| 43 | Rank 2 behind one real sniper, +2% slippage cap (31% of attempts fail) | **+0.06%** (filled trades +0.09%) | `critic_audit.json` |
| 44 | Rank 3 with a +2% cap | −0.04% | same |
| 45 | Our rank-1 buy makes the first follower's slippage fail | **−0.2%** (audit); +0.3% (critic holdout) | same |
| 46 | "Combined realistic": gap-2 mean premium net of own impact (+2.4%), SOL-fixed impact, 0.01 SOL per round trip, first follower fails with p = 0.33 | **−1.07%** | same |
| 47 | 25% / 50% / 100% of slot-0 snipers removed | +1.9% / +0.6% / −1.8% (fresh) | strategy-sweep §5 |
| 48 | Exit latency 0 / 2 / 4 slots | +4.2% / +2.8% / +1.2% (fresh) | strategy-sweep §5 |
| 49 | 0.01 SOL per round trip | +2.3% pooled / +1.3% window | strategy-refinements §3.7 |

### 1e. Refinements of the LEAD after entry (task G2; 18,772 configs on train, BH q ≥ 0.9998 for "beats the LEAD")

| # | Angle | Best out-of-sample number | n | Verdict | Source |
|---|---|---|---|---|---|
| 50 | Scale-out at +15% plus a trailing runner | −0.27 [−0.4, −0.1] points vs LEAD pooled; −0.40 window | 427 / 915 | LOSES | strategy-refinements §2-3.1 |
| 51 | Follow-through gate (hold longer if snipers follow) | −0.02 pooled; −0.15 window | same | NO GAIN | §3.2 |
| 52 | Sell on third-party / sniper selling | −0.35 pooled; +0.12 window (busy launches only) | same | LOSES | §3.3 |
| 53 | Re-enter on a dip after a non-insider exit (0.25 SOL) | −0.15 pooled; the re-entry leg itself −2% to −16% | same | LOSES | §3.4 |
| 54 | Size ladder by mcap / bundle wallets | −0.26 pooled; matched flat stake +0.22 [−0.3, +0.8] | same | NO GAIN | §3.5 |
| 55 | Pure time exit at the end of slot+1 | +3.35 / +2.84 / +4.02 / +2.49% (test / deep / fresh / window) | same | ≈ LEAD (the exit legs only reshuffle the same flow) | §0 |

### 1f. Operator arming (S3) and the guru's selection idea on top of the LEAD

| # | Angle | Best out-of-sample number | n | Verdict | Source |
|---|---|---|---|---|---|
| 56 | LEAD on creators armed by an empirical-Bayes track record | policy-OOS +9.0% [+7.1, +11.0] (41 ops), control +3.4%; but the random-sample subset gives +2.9% (n = 50), and on the critic holdout **+3.7% [+1.4, +6.0], lift over unarmed +0.8 [−2.1, +3.7], p = 0.32** | 383 / 112 | UNPROVEN: the lift does not replicate on the critic holdout so far | strategy-operators, `critic_holdout.json`* |
| 57 | LEAD armed by the guru's own rule (≥3 of 10 reached +100%) | S3 +8.9% (n = 175); critic holdout +3.0% [−1.8, +7.5] | 175 / 56 | UNPROVEN | same |
| 58 | LEAD armed by its own TP record (≥3 of 10 hit +15%) | S3 +9.3% (n = 266); critic holdout +9.5% [+4.7, +13.8] | 266 / 40 | UNPROVEN (n = 40, 9 operators) | same |
| 59 | Arm any repeat operator (≥5 launches) | +4.0% against a matched control of +5.2% | 911 | NO GAIN | strategy-operators §2 |
| 60 | Bundle-shape fingerprint as the key | +5.8% (EB), weaker than creator / bundle wallets | 410 | weaker key | same |

### 1g. Mayhem-mode coins (S2; 3,434 configurations)

| # | Angle | Best out-of-sample number | n | Verdict | Source |
|---|---|---|---|---|---|
| 61 | Best train config per family, time walk-forward | −10.7% to −30.7% on test; 0 of 3,434 positive even in-sample | 2,430 | LOSES | strategy-mayhem |
| 62 | Pre-registered: buy end of slot 0, hold 900 s | −21.7% [−29.2, −13.5] | 1,688 | LOSES | same |
| 63 | Guru rule on mayhem (TP +100%, hold 900 s) | −5.1% [−8.8, −1.6] (hit 27.5%) | 2,429 | LOSES | same |
| 64 | Agent micro-drift (+0.29% per step; +1.08% after a third-party sell) | real but about a quarter of the 4.4% round-trip cost | 141k steps | too small to trade | same |

### 1h. Copy-trading (S5 and its critic C5; 32,400 configurations, 0 with test p < 0.05)

| # | Angle | Best out-of-sample number | n | Verdict | Source |
|---|---|---|---|---|---|
| 65 | Guru default copy (end of slot+1, exit 1 slot after their sell), scored pump wallet | −7.0% [−9.5, −4.5] | 234 | LOSES | strategy-copytrade |
| 66 | Two scored wallets within 5 s | −6.4% | n/a | LOSES | same |
| 67 | Control: copy the first independent early buyer | −7.5% | n/a | LOSES | same |
| 68 | Operator wave wallets | −20% to −23% | 17-20 | LOSES | same |
| 69 | Best realistic config (`Sb_R3` scalp) | test −2.5% [−6.3, +1.4]; holdout A −4.9% [−8.0, −1.8]; A+B −4.9% | 171 / 150 | LOSES | strategy-copytrade-critic |
| 70 | Honest walk-forward pick | test −3.2%; holdout −3.1% / −4.8% | 185 | LOSES | same |
| 71 | Front-run upper bound (0-slot reaction to their sell) | test +0.6% [−3.1, +4.3]; holdout −2.5% / −2.4% | 89 / 65 | LOSES (and not achievable) | same |
| 72 | Copy plus auto-sell on a big buy (1,152 configs) | best test −2.0%; train-pick −2.85% | n/a | LOSES | same |

### 1i. Screens run in this task (G3; pre-registered in `screens_preregistration.json`, details in `strategy-pnl-and-screens.md`)

| # | Angle | Best out-of-sample number | n | Verdict | Source |
|---|---|---|---|---|---|
| 73 | "Second wave": a second LEAD scalp on the first ≥10% pullback within 30 s after a LEAD take-profit | pooled −0.9% [−4.5, +3.2]; window **−5.6%** [−8.1, −3.1]; best grid cell (20% dip) pooled +1.9% (n = 22) but window −3.5% | 61 / 86 | LOSES | `screens_results.json` |
| 74 | LEAD at 18-21 UTC (claimed +15% on test + fresh) | on the sets that formed it +11.0% (n = 69); on independent sets the 18-21 UTC hours are **6 points worse** than the rest (train −6.05 [−13.1, +1.7]; deep −6.04) | 41 / 11 | LOSES (selection artefact) | same |
| 75 | LEAD conditioned on the create rate (60 min or 10 min) | differences −4.9 to +3.6 points, no BH q below 0.28 over 11 tests; within-window 10-min split +1.95 [+0.3, +3.7], q = 0.28 | 77-915 | NO GAIN | same |
| 76 | Graduation momentum: scalp the first PumpSwap trades after the migrate tx or the first pool trade | primary −2.6% [−3.7, −1.5]; test half −3.2%; 0 of 18 configs positive on either half; TP fired on 0-3 of 53 | 53 | LOSES | same |
| 77 | Value of a slot-0 "busy" predictor, simulated on the window (top half) | AUC 0.65 → +3.5%; 0.75 → +4.2%; 0.85 → +4.8%; perfect → +5.9% (top third with a perfect label: +9.3%); unselected +2.5% | 915 | an upper bound on what selection can add | `pnl_model.json` |

### 1j. Rugger discovery and funding (descriptive, not P&L)

| # | Angle | Result | Verdict | Source |
|---|---|---|---|---|
| 78 | Operator clustering (funder, CEX band, bundle shape, launcher, host) | 129 operator units with ≥15 launches; none ≥43% at +100%; funder-linked groups and CEX bands hit 0-5% (farms) | no rugger edge at +100% | gap-1 §2-4 |
| 79 | CEX fresh-wallet trigger (0.9-15 SOL) | 9.7% of fresh recipients create within 60 min, ≈210 wallets/h | watchlist pre-arm only, not a buy signal | gap-4 |
| 80 | Bundle-shape fingerprint per operator | within-operator CV 4-15% (strong match key) | used as a key in S3 | gap-3 §2 |

## 2. Reading the ledger

* **Everything that holds a position for more than a few seconds loses.** That covers 1-9, 14-17, 61-63 and 65-72. The money is in the sniper flow of slots 0-1 and decays within seconds.
* **Only one family is positive out of sample: the rank-1 micro-scalp (LEAD).** It holds about +2.5% to +3.7% per trade on every independent set, and on a plateau of 108 settings. Every post-entry modification tried (50-55, 73) is at or below it.
* **The per-trade number is not under-tuned.** A pure time exit at slot+1 earns the same (55).
* **The levers that move the per-trade number:**
  * which launches we enter: busy +9.4% vs quiet −0.9% (30-31, 77);
  * whether we are really first: rank 2 ≈ 0, premium +3.6% ≈ +0.9% (41-46).
* **The levers that move SOL/day:** the landing rate and the cost of failed attempts (see the P&L model).
* **Two results are still open.**
  * Operator arming (56-58): +9% in S3, but its lift does not replicate yet on the critic holdout.
  * The critic's "combined realistic" fill model (46) at −1.1%.

  They bound the LEAD from above and from below.

## 3. Untested angles, ranked

Ranked by expected value of information, with the data each one needs.

| Rank | Angle | Why it could matter | Data needed | Status |
|---|---|---|---|---|
| 1 | **Slot-0 "busy" predictor** (features visible before our buy: funding class, launcher program, CU/tip of the create and bundle, bundle shape, metadata host, creator history) | The busy/quiet split is the largest per-trade lever (+9.4% vs −0.9%). A predictor also halves failed-attempt costs, because quiet launches are skipped. AUC 0.75 lifts the top half to about +4.2% and cuts the break-even landing rate from 12% to about 10% (P&L model). | Window `trades_meta` s0 blocks and `funding.jsonl`, gap-1 `creates.jsonl` (CU, tips, launcher), analytics.db. All on disk. | In progress (task G1, `predictor.py`) |
| 2 | **Measured landing rate, rank and premium** (shadow mode, then a probe) | SOL/day is linear in the landing rate. Break-even is 12% at 0.5 SOL and 26% at 0.25 SOL with 0.003 SOL failure costs. Rank 2 earns about 0. Nothing in the tapes can measure this. | Shadow-mode logs on the Frankfurt box: create position in block, our would-be index, the realised premium. | Planned (04-strategy §7) |
| 3 | **Bid path vs failure cost** (bid as an in-transaction tip, which reverts on failure, vs priority fee, which is paid on failure) | Moving the failure cost from 0.003 to 0.0005 SOL takes the 0.5 SOL break-even landing rate from 12% to 2% (P&L model). The fee-ordered BAM/Harmonic leaders need the priority fee (docs/03), so this is a per-leader-class trade-off. | Shadow mode plus a small live probe per leader class. | Untested |
| 4 | **LEAD on dev-only and unbundled launches** with slot-0 snipers | Stage 2's "all bundled" filter gave +2.9% against +3.5% for multi-wallet. The 75% of SOL-quoted creates without a bundle are untested, and 22% of all creates draw an independent slot-0 buyer (gap-2). | gap-1 two-second tapes (cached pumpapi pages for 9,905 creates). The LEAD only needs about 2 s (strategy-operators: identical returns on 2 s tapes). | Untested; data on disk |
| 5 | **Size by predicted busyness** (0.5 SOL on busy, skip quiet) | It is the selection lever plus capacity. Flat size adds nothing (54). | Same as 1. | After 1 |
| 6 | **Operator arming on a full feed** | The S3 lift is unconfirmed on the critic holdout. A full feed gives 5 scored launches per operator within hours instead of days. | The collector's full tapes for several days. | Needs the collector |
| 7 | **Low-frequency "pump wallets"** (the guru's 8-a-day wallets) | The 1.27% sample can only score bots, so this is untested. Priors are unfavourable: the few slow wallets we see lose. | A full-feed wallet ledger for several days. | Needs the collector |
| 8 | **USDC- and other-quote launches** (about 12% of creates) | Never simulated. The same sniper flow may exist. | swap-api tapes for non-SOL-quoted launches; a quote-aware curve. | Untested |
| 9 | **Post-Alpenglow regime** (Oct 21) | Slot timing changes, and the edge lives in slot timing. | Fresh tapes after Oct 21. | Must re-run |
| 10 | **Other launchpads** (letsbonk, believe, etc.) | Same mechanism, different competition. | New decoders and tapes. | Not started |
| 11 | **Shred-level exit timing** (sub-slot arrival of the sniper flow) | Exit latency costs about 0.3 points per slot pooled. | A shred feed with per-region timestamps. | Needs the feed |

Dropped from the list after screening in this task:
* second wave (73);
* hour of day (74);
* create rate (75);
* graduation momentum (76).
