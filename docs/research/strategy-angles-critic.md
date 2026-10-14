# G4: skeptic of the three angle studies, and a second fresh holdout

**Question.** G1 (`strategy-predictor.md`), G2 (`strategy-refinements.md`) and G3 (`strategy-pnl-and-screens.md`) tried to raise the LEAD micro-scalp above its +2.5% to +3.7% per trade. Do their positive claims survive an attempt to break them, and what raises the edge beyond the LEAD on launches none of them has seen?

*Measured 2026-10-14, 16:44-17:43 UTC. Code: `analysis/scripts/strategy/angles_critic.py` (audits on the existing table), `angles_prereg.py` (freeze), `angles_holdout.py` (index, decode, chain-only prescreen, pull), `angles_eval.py` (funding, metadata, rows, one-shot evaluation, coverage, competition). Caches: `data/cache/strategy_holdout2/`. Results: `data/reports/strategy/angles_{prereg.json,prereg_models.pkl,critic_frozen,critic_fills,critic_wf,holdout,compete,hb_coverage}.json`. One fix: `analysis/rugsnipe_analysis/strategy/predictor.py` (`Funding`, 10 lines, §2.2). Nothing was committed.*

Policy throughout: the LEAD of docs/04 §2 (0.5 SOL, rank 1 behind the creator bundle in the creation slot; exits +15% realised, any bundle-wallet sell, 1 s idle, −15% after 1 s, slot+5), base scenario +1.1% premium, 0.004 SOL per round trip, 1-slot exit latency. EV is per trade, net.

## Verdict in brief

1. **Is +2.5-3.7% "too low", and is it a many-trades-a-day number?** Yes to both. On 480 fresh launches the LEAD makes **+3.2% [+1.9, +4.5]**, about 0.016 SOL per trade. The opportunity count is larger than G3 assumed: in a dense 40-minute afternoon block, **18% of all creates** are multi-wallet bundled launches in the tested class (307 of 1,707), about **11,000 a day at that rate** against G3's 6,181 (from a 02-08 UTC window). Per-day profit is set by the landing rate, not by the per-trade number.
2. **What raises the edge beyond the LEAD on fresh data: entry selection with a model of P(take-profit) / E[return] that uses operator history and the bundle's shape. Nothing else.** All pre-registered, frozen before the data were pulled, BH over 8 primary tests:

   | Rule (frozen) | Kept | EV kept vs all (n = 480) | Lift [90% CI, signer-clustered] | q | HA / HB / HC lift |
   |---|---|---|---|---|---|
   | G1 frozen P(TP), **skip the bottom tertile** | 74% | **+4.6%** vs +3.2% | **+1.4** [+0.7, +2.1] | 0.001 | +1.5 / +1.6 / +0.5 |
   | G1 frozen P(TP), top tertile | 45% | **+6.7%** | +3.5 [+2.1, +5.0] | 0.001 | +3.5 / +4.7 / **−1.3** |
   | G1 config refitted on the whole table: GBT-3 P(TP), top 20% | 15% | **+9.7%** | +6.5 [+3.3, +9.8] | 0.001 | +3.8 / +7.1 / +8.3 |
   | G1 config refitted: ridge E[return], top 40% | 30% | **+7.9%** | +4.7 [+2.6, +7.0] | 0.001 | +3.3 / +5.7 / +2.3 |

   Three of the four pass the pre-registered bar (q < 0.10 and lift ≥ 0 on every set with ≥ 40 launches). **The top tertile fails it**: strong on HA and HB, negative on the small HC set (59 launches, 08:05-10:45 UTC, where the LEAD itself is −0.4%).
3. **What does not raise it, on fresh data as on old:** predicting "busy" (frozen n10 model: −0.3 points), the early-in-block rule (−1.1), every post-entry refinement (gate −0.09, re-entry +0.02, flow +0.05, scale-out **−0.29 [−0.49, −0.09]**, "bail at slot+1 if nobody followed" −0.26, "always sell at slot+1" −0.30), the size ladder (+0.5 per SOL against a flat stake of equal mean size, q = 0.053, but **0.011 SOL per launch against the LEAD's 0.016**: it makes less money), and the G3 screens (all lost; nothing to carry).
4. **The catch: selection concentrates our attempts exactly where rank 1 is most contested.**
   * On the dense block (HB), the launches the frozen P(TP) and ridge rules keep carry **5.6-6.2 failed slot-0 transactions** on their curve against **3.0** for all launches. 36% of multi-wallet launches are flooded (≥ 1,000 curve transactions within ~5 s, 68% of them failed) and those make **+10.7%**; all the others make −0.5% to −3.3% (+2.3% for the 18 with ≥ 5 failures).
   * gap-2 says the same from the bid side: where ≥ 2 independent snipers win slot 0 (the launches that pay), the first winner bid **0.010 SOL** median (p75 0.050) against 0.0019 where only one wins.
   * The selection keeps its sign under every fill stress (+3.6% premium, 0.01 SOL, 2-slot latency, rank 2, median-bid cost), but our *landing rate* on selected launches is not measured. With a crude competition haircut (chance of being first ∝ 1 / (1 + followers + failed rivals)), the top tertile's SOL/day at a 20% landing rate falls from +12.4 to +5.9 and the ridge rule's from +10.4 to +4.6, below the unselected LEAD (+7.4). Skip-bottom-tertile stays above it (+8.2). The GBT-3 top 20% is *not* more contested (+6.7), but it trades too rarely to beat the LEAD's SOL/day at 20% landing; it wins only when landing is scarce (10%: +2.3 against −3.2).
5. **G1's positive claims survive the audits** (no look-ahead that matters, clustering by operator changes nothing, the walk-forward winner's curse is about half of the top number in one direction only). Two of G3's statements are wrong in the details and one G1 input had a small look-ahead (fixed). §2.
6. **Recommendation.** Adopt **"skip the bottom tertile of the frozen P(TP) score"** as the LEAD's entry filter for shadow mode and the probe: it passed every pre-registered check, drops 26% of attempts that lose (−0.8% pooled, negative on every set), and does not concentrate competition much. Log the GBT-3 and ridge scores (refitted hourly) on every qualifying launch. **Do not** tighten to the top tertile or top 20% until shadow mode measures our landing rate on selected versus unselected launches. No exit refinement, no size ladder. Re-freeze a live version without the metadata JSON and `cu_used`, which cannot be read at slot 0 (it does as well: top tertile +6.4% vs +6.7%).

## 1. The claims under attack

| Study | Positive claim | Status after this review |
|---|---|---|
| G1 | Frozen P(TP) (rule 3), on Oct 12-14: top tertile +4.7% vs +2.7%; skipping the bottom tertile +3.6% | **Survives** the audits; on fresh data +6.7% (fails on HC) and +4.6% (passes) |
| G1 | Retrained walk-forward: top 20% +9.4%, top 40-60% +5.5% to +6.5% | **Survives**; the best number is inflated in one half (§2.1). Fresh: +9.7% (top 15%), +7.9% (top 30%) |
| G1 | The lift comes from operator history (AUC 0.80 seen vs 0.57-0.59 first-seen) | Partly. Fresh TP AUC 0.72: 0.77 with history, **0.67 first-seen** (better than G1 saw) |
| G1 | Predicting "busy" adds little | **Confirmed** (frozen n10 model −0.3 points on fresh data) |
| G2 | No post-entry refinement beats the LEAD | **Confirmed** on fresh data; scale-out is significantly worse |
| G3 | ~6,200 qualifying launches/day; the 154 window tapes missing are "too hot" | **Count understated** for the afternoon (~11,000/day); the "too hot" story is wrong (§2.2) |
| G3 | Graduation, second wave, hour/create-rate screens lose | Nothing to refute |

## 2. Attacks on the existing evidence (no new data)

### 2.1 What survived

| Attack | How it was tested | Result | Effect |
|---|---|---|---|
| **Operator history includes the launch itself, or outcomes not yet known** | Read `predictor_build.py`. | History uses rows with `bt < launch bt`; LEAD outcomes only from launches ≥ 10 s older; busy labels only from launches ≥ 900 s older. Correct. | None. |
| **Labels leaking into features** | Same code. | `busy900`, `s0_third`, `n10` are labels only; `op_busy` / `op_s0t` are point in time. | None. |
| **Operator pools chosen with full-period launch counts** | Re-scored frozen rule 3 on Oct 12-14 with the pool-free history in the `op_all_*` slots. | Pool-only history touches 7 of 1,207 test launches. TP AUC 0.699 → 0.690; top tertile +4.70% → +4.70%. | None. |
| **Metadata JSON fetched long after the launch** (p50 449 ms; j7tracker, uxento and rapidlaunch are mutable HTTP hosts) **and `cu_used`** (an execution result, not in the shred) | Zeroed every metadata feature; set `cu_used` to its training median. | Top tertile +4.70% (51% kept) → +4.30% (43%) without metadata; +4.54% (36%) without either. On fresh data: +6.39% (40%) vs +6.70% (45%). | Small. A live model must be refrozen without them. |
| **The same operator launching many times (clustered outcomes)** | Bootstrap over signers. | Top tertile 90% CI [+3.3, +6.1] by launch and by signer (438 signers, 621 launches). Lift [+1.2, +2.9], p = 0.0005; skip-bottom lift [+0.5, +1.3]. The top 5 signers carry 20% of the top tertile's P&L (48% on the 296 non-window launches). | None pooled; the non-window part is concentrated. All fresh-data CIs below are signer-clustered. |
| **Winner's curse in "+9.4% at top 20%"** | Re-ran G1's grid (970 selection configs, same code), split the Oct 12-14 OOS folds into alternate halves (576 / 631 launches), picked the top 10 by lower bound on one half, read them on the other. | Picked on A, read on B (all +2.29%): top-1 +11.8% → **+4.7%**, top-10 mean **+5.0%**. Picked on B, read on A (all +3.10%): top-1 is the GBT-3 / TP / top-20% config, +9.2% → **+9.6%**, top-10 mean **+7.5%**. | The single best number shrinks by about half in one direction. Honest in-period value: +2 to +6 points over all launches. |
| **Multiple testing** | Recomputed BH over the 1,130 configs of G1's full run from the stored p-values. | 288 with q < 0.05, identical to G1. | None. |
| **Exit latency on selected launches** | Stored 2-slot returns. | Top tertile +4.33% vs all +2.53%. Fresh: +6.5% vs +3.4%. | None. |

### 2.2 What did not survive, or survives only under an assumption

| Attack | How it was tested | Result | Effect |
|---|---|---|---|
| **Funding features as of a later create** | `predictor.Funding.get` preferred the window's by-wallet record (as of the signer's first window create, Oct 14) over the deep / fresh row's own create; the analytics.db record can be as of a later mint. | **61-62 test rows** (deep / fresh, Oct 12-14) carried funding from after their launch. Blanking it: top tertile +4.70% → +4.52%; on the 296 non-window launches **+5.72% → +5.00%**. | Small real look-ahead. **Fixed** in `Funding` (window records only for launches after the signer's first window create; db records only if their as-of mint is older). The table on disk predates the fix; rebuilding it would change G1's frozen inputs, so the effect is quantified here instead. Holdout rows are unaffected (their funding is fetched as of their own create). |
| **"The busiest launches are missing from the window" (docs/04 §3; G3 §1.3: "154 tapes too hot to download, probably busy")** | Rebuilt every window multi-wallet launch absent from the table. | Only **6 of 154** are hot (first 60 s not reachable). **101** have no trade in the create transaction, so `tapes._raw_bundle` cannot anchor the bundle and they are outside the tested class; **47** are slot-0 graduations or non-standard. They have fewer followers (0.97 vs 1.39). | The survivorship story is wrong; the bias is small. The tested universe excludes ~9% of multi-wallet launches (dev buy outside the create tx), EV unknown. G3's 6,181/day counts them (the tested class is 10.7% of window creates, 5,290/day). |
| **Rank 1 is assumed on exactly the launches where it is most contested** | Re-simulated Oct 12-14 at rank 2 (behind the first follower), rank 2 with the +2% cap, the critic's "combined realistic" fills, and a fixed cost priced from gap-2's first-winner bids by follower count. | gap-2 (570 bundled launches): first independent winner's bid **0.0019 SOL** median with one winner, **0.010** (p75 0.050, p90 0.145) with ≥ 2 winners, 3.7 failed rivals per launch vs 1.5. Top tertile: 1.56 followers vs 0.78 in the bottom. **Top tertile at rank 2: −0.97%** (all −1.29%); rank-2 premium +4.9% vs +1.0%. Rank 2 with cap: −0.07%. Combined realistic: top +1.75%, all −0.24%. Median-bid cost: top +4.15%, all +2.21%. Mean-bid cost: top −0.91%, all −2.12%. | **The selection lift is conditional on still landing first where competition is highest.** It keeps its sign under each fill model, but nothing measures our landing rate there (§4.4). |

### 2.3 G2 and G3 (negative claims: did they miss something?)

* **G2.** The frozen variants were re-scored on fresh data (§4.3) together with G2's own pre-registration note ("no third-party buy behind us at slot 0 → sell at slot+1"). Nothing beats the LEAD; the note's variant is −0.26 pooled and −1.53 [−2.9, −0.1] on HA.
* **G3.** The P&L arithmetic is right. Its opportunity count is time-varying (above). Its "contested busy launches" stress (landing × 0.6 on busy launches) is supported by gap-2 and by HB's failed-transaction counts (§4.4). The screens were pre-registered and lost; nothing is carried.

## 3. The fresh holdout

**Pre-registration** (`angles_prereg.json`, 16:59:46 UTC; amendment 17:01:05 UTC adding HC, before any holdout feature, score or outcome was computed). Primary family of 8 tests: G1 frozen rule 3 top tertile and skip-bottom-tertile; G1's two best walk-forward configs (`rand:all:gbt_3:tp:keep0.2`, `pool:all:lr_0.5:ret:keep0.4`) **refitted on the whole predictor table and frozen** (`angles_prereg_models.pkl`, sha256 in the JSON); G2 frozen gate, re-entry, ladder and the k@slot0 = 0 bail. G3 had no positive screen. Statistic: pooled lift (selection) or paired difference (refinement), one-sided bootstrap over signers, 4,000 draws, BH over the 8. Pass: q < 0.10 and lift ≥ 0 on every set with ≥ 40 launches. Ten secondary rules were also scored once (G2 scale-out, flow and slot+1 exit; G1 rules 1 and 2, the uxento host, the no-metadata score; two HB-only refits). They are reported without a pass/fail. Nothing else was run on the holdout.

| Set | What | Launches scored |
|---|---|---|
| **HA** | The S4 critic's holdout, Oct 14 08:43-13:43 UTC, random group (its armed-operator group, 146 launches, is used only as operator history). Never used by G1/G2/G3. 25 s tapes. | 114 |
| **HB** (new) | **Every** multi-wallet bundled launch created Oct 14 **15:20:04-16:00:14 UTC** (≥ 45 min old at the 16:45 index). Creates from `getSignaturesForAddress` on the mint authority, decoded with `dataset.creation_record`; multi-wallet status from the chain only (curve signatures with a later create's signature as the `before` cursor, then the slot-0 transactions right after the create under `_raw_bundle`'s rule); tapes from swap-api (10 s ×1 page, 10 s ×3, 4 s ×3). | 307 |
| **HC** | The copy-trade critic's random holdout after the predictor table's last row (08:05-10:45 UTC). | 59 |

* **HB coverage:** 1,707 creates in the block, 967 eligible (non-mayhem, SOL-quoted, single create, mint in no earlier cache), 347 prescreened as multi-wallet or unknown and pulled: **307 multi-wallet**, 30 dev-only, 10 slot-0 graduations / non-standard; none incomplete. The prescreen was validated on 60 HA launches of known class: 18 of 18 multi-wallet launches flagged multi or unknown (0 missed), 41 of 42 others rejected.
* **Budget:** swap-api 497 requests including 102 rate-limit retries, ≤ 36/min (cap 500). RPC at ≤ 3.6 req/s configured.
* **Honest record.**
  * My pull walked newest-first through prescreened launches and jumped past a gap in the prescreen at 15:30, pulling 10 launches from the sparse random decode below it. I stopped it, added a contiguity frontier, and HB is the verified contiguous block (every eligible create decoded and prescreened, every multi/unknown launch pulled). The 10 are excluded.
  * A debug run of the evaluation code on partial data (HB's first 75 launches) ran at 17:09 before HB was complete. No rule, threshold or set definition changed after it; the final run added only diagnostics (competition, coverage).

## 4. Results on the fresh holdout

### 4.1 The LEAD itself

| Set | n | EV [90% CI] | TP hit | +3.6% premium | 0.01 SOL | 2-slot latency | rank 2 |
|---|---|---|---|---|---|---|---|
| HA (08:43-13:43) | 114 | **+6.1%** [+3.1, +8.9] | 30% | +3.5% | +4.9% | +6.0% | +3.1% |
| HB (15:20-16:00, dense) | 307 | **+2.8%** [+1.2, +4.6] | 22% | +0.3% | +1.6% | +3.3% | −0.3% |
| HC (08:05-10:45) | 59 | −0.4% [−3.0, +2.3] | 19% | −2.9% | −1.6% | −0.9% | −4.4% |
| **pooled** | 480 | **+3.2%** [+1.9, +4.5] | 23% | +0.6% | +2.0% | +3.4% | +0.0% |

On HB, the 30 launches with more than 100 trades in their first 10 s make **+15.6%**, the other 277 **+1.5%**. Any data set that loses hot launches understates the LEAD.

### 4.2 G1: entry selection

| Rule | Kept | EV kept | EV dropped | Lift [90% CI] | q | HA | HB | HC | Pass? |
|---|---|---|---|---|---|---|---|---|---|
| **Frozen P(TP), skip bottom tertile** | 74% (355) | **+4.6%** | −0.8% | **+1.4** [+0.7, +2.1] | 0.001 | +1.5 | +1.6 | +0.5 | **yes** |
| Frozen P(TP), top tertile | 45% (217) | **+6.7%** | +0.3% | +3.5 [+2.1, +5.0] | 0.001 | +3.5 | +4.7 | **−1.3** [−3.5, +0.6] | **no** (HC) |
| Refitted GBT-3 P(TP), top 20% | 15% (71) | **+9.7%** | +2.1% | +6.5 [+3.3, +9.8] | 0.001 | +3.8 (15 kept) | +7.1 | +8.3 (7 kept) | **yes** |
| Refitted ridge E[return], top 40% | 30% (144) | **+7.9%** | +1.2% | +4.7 [+2.6, +7.0] | 0.001 | +3.3 | +5.7 | +2.3 | **yes** |
| *secondary:* frozen top tertile without metadata | 40% | +6.4% | +1.1% | +3.2 [+1.6, +4.8] | | +4.0 | +3.7 | −1.8 | |
| *secondary:* frozen n10 ("busy") model, top 40% | 54% | +2.9% | +3.6% | −0.3 [−1.6, +0.9] | | −0.6 | +0.3 | −2.2 | |
| *secondary:* create in-block index ≤ 295 | 29% | +2.1% | +3.7% | −1.1 [−2.8, +0.8] | | +1.7 | −2.6 | −1.0 | |
| *secondary:* uri host = meta.uxento.io | 11% | +6.7% | +2.8% | +3.5 [+0.3, +6.7] | | −0.0 | +4.9 | +4.6 | |
| *secondary, HB only:* GBT-3 / ridge refitted again on table + HA + HC | 15% / 37% | +10.0% / +8.4% | | +7.1 / +5.6 | | | | | |

* **Score skill.** TP AUC of the frozen score: **0.72** pooled (HA 0.75, HB 0.71, HC 0.72); **0.77** with operator history (219 launches), **0.67** first-seen. G1 had 0.80 / 0.57-0.59; on fresh data the bundle structure carries more of the skill.
* **Frozen tertiles, pooled:** low −0.8% (26%), mid +1.4% (29%), high +6.7% (45%). HA + HB: −0.6 / +1.0 / +8.3%.
* **Stress, pooled, kept vs all:**

| Rule | +3.6% premium | 0.01 SOL | 2-slot latency | rank 2 | rank 2, +2% cap | median-bid cost |
|---|---|---|---|---|---|---|
| LEAD (all) | +0.6% | +2.0% | +3.4% | +0.0% | −0.3% | +2.8% |
| skip bottom tertile | +2.0% | +3.4% | +5.0% | +0.8% | +0.2% | +4.1% |
| top tertile | +4.1% | +5.5% | +6.5% | +2.2% | +1.5% | +6.2% |
| GBT-3 top 20% | +6.6% | +8.5% | +9.8% | +3.8% | +4.7% | +9.2% |
| ridge top 40% | +5.2% | +6.7% | +7.9% | +2.9% | +2.0% | +7.4% |

### 4.3 G2: post-entry refinements (frozen `frozen_refinements.json`)

| Variant | Pooled EV | vs LEAD [90% CI] | p | HA / HB / HC |
|---|---|---|---|---|
| gate s+0 p≥5% k≥5 → TP50 + trail15 | +3.12% | −0.09 [−0.25, +0.06] | 0.84 | +0.07 / **−0.18** [−0.35, −0.05] / +0.08 |
| re-entry 0.25 SOL on a 10% dip | +3.24% | +0.02 [−0.10, +0.15] | 0.40 | +0.12 / +0.02 / −0.15 |
| scale-out 75% + trail 10% | +2.92% | **−0.29** [−0.49, −0.09] | 0.99 | −0.19 / −0.36 / −0.12 |
| flow: TP 20% + sniper-sell exit | +3.27% | +0.05 [−0.19, +0.29] | 0.35 | +0.22 / +0.04 / −0.21 |
| k@slot0 = 0 → sell at slot+1 (G2's note) | +2.96% | −0.26 [−0.90, +0.39] | 0.76 | **−1.53** / +0.05 / +0.59 |
| always sell at slot+1 | +2.91% | −0.30 [−1.08, +0.45] | 0.75 | −2.10 / +0.01 / +1.51 |
| ladder 0.25/0.5/0.25 SOL by mcap tercile, per SOL vs flat 0.329 SOL | +3.31% vs +2.78% | +0.53 [+0.06, +0.99] | 0.033 | +1.32 / +0.31 / +0.24 |

The ladder passes its own pre-registered test (q = 0.053), but it deploys 0.33 SOL per launch on average: **0.0109 SOL per launch against 0.0161 for the flat 0.5 SOL LEAD**. Attempts, not capital, are the scarce resource, so it lowers SOL/day.

**BH over the 8 primary tests:** q = 0.001 for the four G1 rules; ladder 0.053; re-entry 0.53; gate and k0-bail 0.84.

### 4.4 Competition on the selected launches (HB, chain-only)

| HB, kept vs all multi-wallet launches | flooded (≥ 1,000 curve txs in ~5 s) | failed slot-0 txs on the curve (non-flooded) | followers |
|---|---|---|---|
| all | 36% | 2.99 | 1.05 |
| skip bottom tertile | 41% | 3.76 | 1.17 |
| top tertile | 43% | **5.57** | 1.18 |
| ridge top 40% | 38% | **6.15** | 1.21 |
| GBT-3 top 20% | 31% | 2.62 | 1.37 |

| HB launches by competition | n | LEAD EV |
|---|---|---|
| flooded (68% of their transactions fail; 1,000 signatures span ~20 slots) | 109 | **+10.7%** |
| ≥ 5 failed slot-0 txs | 18 | +2.3% |
| 1-4 failed | 91 | −3.3% |
| 0 failed | 89 | −0.5% |

**The LEAD's money is on the launches hundreds of bots fire at.** The frozen and ridge selections roughly double the failed rivals per launch; the GBT-3 top 20% does not.

### 4.5 Trades and SOL per day

pnl_model accounting (G3): 6,181 qualifying launches/day × 0.93 reachable, 0.5 SOL, 0.003 SOL per failed attempt (80% of lost attempts land and fail), fresh pooled EVs. "Contested" scales the kept launches' landing rate by the HB ratio of 1 / (1 + followers + failed rivals), kept vs all: top tertile 0.65, ridge 0.60, skip 0.85, GBT-3 1.01.

| Policy | EV/trade | Trades/day at 20% landing | SOL/day at 10% / **20%** / 50% | 20%, contested | Break-even landing |
|---|---|---|---|---|---|
| LEAD (every launch) | +3.2% | 1,150 | −3.2 / **+7.4** / +39.3 | +7.4 | 13.0% |
| skip bottom tertile | +4.6% | 850 | +0.6 / **+11.5** / +44.0 | **+8.2** | 9.4% |
| top tertile | +6.7% | 520 | +3.1 / **+12.4** / +40.4 | +5.9 | 6.7% |
| GBT-3 top 20% | +9.7% | 170 | +2.3 / **+6.6** / +19.6 | +6.7 | 4.7% |
| ridge top 40% | +7.9% | 345 | +3.1 / **+10.4** / +32.1 | +4.6 | 5.7% |

At HB's afternoon rate (~11,000 qualifying launches/day) every row scales by about 1.8 (LEAD +13.3 SOL/day at 20%; skip-bottom +20.4; top tertile +22.1, contested +10.5). These are expected values under the tape model; the critic's combined-realistic fill model still makes the unselected LEAD negative (docs/04 §3).

## 5. What works, what does not

| Angle | Fresh-data verdict |
|---|---|
| Entry selection by a P(TP) / E[return] model (operator history + bundle shape) | **Works per trade**: +1.4 (skip bottom tertile, 74% kept) to +6.5 points (top 15%). Three of four pre-registered rules pass. |
| ... and per day | Only if our landing rate does not collapse on the selected launches, which are measurably more contested. With the crude competition haircut only skip-bottom-tertile stays above the unselected LEAD at 20% landing; the tight cuts win only at low landing rates (≤ ~10%), where cutting failed attempts matters most. |
| Predicting "busy" / trades in 10 s | Does not work (−0.3). Busy is mostly the insiders' own volume. |
| Create position in the block | Does not work (−1.1). |
| Post-entry exits (gate, flow, re-entry, scale-out, bail, time exit) | Do not work: −0.3 to +0.05; scale-out significantly worse. |
| Size ladder | Better per SOL, worse per launch; no gain in SOL/day. |
| Graduation momentum, second wave, hour / create rate (G3) | Lost before; not carried. |
| Bigger stakes | Not tested here; G2's sniper-drop stress still applies (keep 0.5 SOL). |

## 6. Recommendation

1. **Entry filter for shadow mode and the probe: skip the bottom tertile of the frozen P(TP) score** (`predictor_frozen_models.pkl`, rule 3, cut point t1 = 0.145). Fresh data: +4.6% vs +3.2%, dropped launches −0.8%, lift on every set, competition only modestly higher. It needs a live operator-history store (our own LEAD outcomes per signer, 10 s after each launch), which docs/06's funding watcher can host.
2. **Re-freeze a live-feasible version** without the metadata JSON (449 ms, after the slot-0 decision) and `cu_used`, on every row known now (table + HA + HB + HC). On fresh data the no-metadata frozen score keeps its skill (AUC 0.72, top tertile +6.4%).
3. **Log, do not trade on, the tighter cuts**: frozen top tertile, refitted GBT-3 top 20% and ridge top 40%, refitted hourly. Shadow mode must record our would-be landing index **separately for selected and unselected launches**. The tighter cuts should go live only if the measured landing rate on the selected launches is at least **0.73×** (top tertile) or **0.80×** (ridge) the rate on all launches. Below that, their per-trade gain no longer pays for the lost volume at a 20% landing rate. The GBT-3 top 20% needs 1.09×, so at 20% landing it is a low-volume hedge, not an upgrade. Skip-bottom-tertile needs 0.81×; HB's crude estimate is 0.85×.
4. **No exit, re-entry, scale-out or ladder changes.** The LEAD's exits stay as in docs/04 §2.
5. **Correct the record:**
   * docs/04 §3 / G3 §1.3: the window's missing launches are not "too hot" (6 of 154 are). The tested class excludes multi-wallet launches whose dev buy is not in the create transaction (~9%).
   * The qualifying count is ~11,000/day in the afternoon block measured here, against 6,181 from the 02-08 UTC window. SOL/day estimates should use the hour profile of the multi-wallet share, not one window's.
6. **Still unmeasured, and still the whole business:** our landing rate, now split by selection. The flooded launches carry the EV and have ~1,000 curve transactions in 5 s.

## 7. Caveats

* **Small, short sets.** HB is 40 minutes of one afternoon and HC 59 launches. The LEAD itself ranges from −0.4% (HC) to +6.1% (HA) across these sets. The lifts are within-set comparisons and are less exposed to that swing, but the top tertile's HC failure shows a 59-launch set can flip a sign.
* **Operator history on the holdouts is sparser than live.** It comes from the table, the pools and earlier holdout launches. A live bot sees every launch, which should help the history-driven rules (AUC 0.77 with history vs 0.67 without).
* **The refitted models were refit on the whole table**, which includes the dense window. Their thresholds come from in-sample training predictions, so they kept 15% and 30% instead of 20% and 40%.
* **Competition figures are chain counts, not our rank.**
  * Failed curve transactions are rivals that lost, by slippage or bot guards. Flooded launches have no slot-0 count, because the signature page never reaches slot 0.
  * The landing haircut (1 / (1 + rivals)) is a crude model, not a measurement.
* **Fill model unchanged.** Rank 1 at +1.1%, tokens held fixed for others (docs/04 §3). Every EV above inherits the tape model's optimism about reflexivity.
