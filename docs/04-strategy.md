# 04 Strategy: what works, what does not, and how we trade it

Date 2026-10-14. This document is the strategy verdict after the validation programme
(`docs/05-validation.md`, `docs/research/strategy-*.md`, `gap-1..6.md`). Every number below is
out of sample (a time split, an odd/even-day split, or a holdout pulled after the policy was frozen)
unless marked in-sample. Costs are always included: 1.25% pump.fun fee per side, a fixed cost per
landed round trip (0.004 SOL base, 0.01 SOL competitive) and the entry premium over the post-bundle
price measured in `gap-2.md` (+1.1% median for the first reactive buyer).

## 0. Answer in one screen

| Question | Answer | Status |
|---|---|---|
| Does the guru's rule work as he states it (block-0 entry behind the rugger's bundle, take profit at +100%, natural-floor stop)? | **No.** −10.5% per trade on the time-split test, −21% on an independent holdout. Buy-on-dev-sell −13%, snipe-delay −17%. | VERIFIED on real tapes |
| Do his bot's protective exits help? | Yes, they lift his rule from −10% to about −1%. Still not positive. | VERIFIED |
| Does any policy have a positive out-of-sample expectancy after costs? | **Yes, one family: the rank-1 micro-scalp.** Enter right behind the bundle in the creation slot, leave within 1-3 s. Pooled out of sample **+3.5% per trade [+1.9, +5.2], n = 427**; on a holdout pulled after the freeze **+3.7% [+0.7, +6.6], n = 150**; positive on 16 of 17 days; all 108 neighbouring settings positive. | LIKELY real in historical data, UNPROVEN live |
| Is it his method or ours? | **A mix.** His entry (block 0, right behind the bundle, on bundled launches) with our exit (seconds, not minutes). It sells to the snipers who land behind us, not to retail. | |
| What is the condition? | **We must be the first buyer behind the bundle**, with a slippage cap that refuses worse fills. Break-even entry premium is about +4.4% (0.004 SOL costs) or +3.3% (0.01 SOL). At the rank-3 premium (+10.8%) it loses about −6%. | VERIFIED by stress |
| Copy-trading pump wallets (his method 2)? | **No.** Every realistic copy variant loses 3-6% per trade on 905 fresh launches; the scored wallets add nothing over an unscored control. | VERIFIED |
| Per-rugger arming (his idea of following operators, with our exit)? | **No.** It looked like +9% per trade on the historical operator sets, but on a fresh holdout pulled after the rules were frozen the armed launches make +2.9% against +3.2% unarmed (lift −0.2 points, p = 0.54). The historical lift came from how those operators were sampled. His own "3 of the last 10 hit +100%" rule: +2.1% [−2.0, +6.0], not significant. Arming stays as logged fields only. | VERIFIED by the critic |
| Mayhem coins? | **No.** The agent is a fee-free near-random walk that stops after a median 43 s; 87.5% of mayhem coins trade below the standard floor within 15 min; every policy loses out of sample (−11% to −22%). Excluded. | VERIFIED |
| Does the edge survive realistic fills? | **Only at rank 1, and thinly.** Rank 2 (one sniper ahead of us): +0.0% to +0.8%. All of the money sits on the third of launches where the first follower lifts the price by more than 2% (+11.7%, holdout +9.1%); the rest is −0.5%. Stacking the mean premium, SOL-fixed impact, 0.01 SOL costs and one follower in three failing gives −0.4% [−2.0, +1.3] on the fresh holdout, before failed-attempt costs. | VERIFIED by the critic |
| What happens next? | Shadow mode on the Frankfurt box, no money: record our would-be landing index against the first follower's real index and the premium, score the policy at that position, split hot/quiet. Go/no-go rule fixed now (§7). Then, only if it passes, a capped probe of about 200 trades at 0.25 SOL with a kill switch. | |

## 1. His method versus ours, on the same tapes

Reference policies were fixed before any search (0.25 SOL, base costs, 675 bundled launches from
the 16-day sample; test = last 40% by time; deep = 133 random launches of Oct 12-14).

| Policy | Entry | Test EV [90% CI] | Deep holdout | Hit rate |
|---|---|---|---|---|
| Guru: TP +100%, no stop (floor) | block 0 behind bundle | **−10.5%** [−13.9, −6.9] | **−21.1%** | 14% |
| Guru bot: TP +100% + phantom SL 10 s −25% + sell on 2 SOL buy + no-activity 10 s | same | −0.8% [−3.5, +2.2] | −4.6% | 32% |
| Guru: buy on dev sell, TP +100% | slot after first insider sell | −13.1% | −22.0% | 9% |
| Guru: snipe delay (end of slot+1), TP +100% | end of slot+1 | −16.9% | −28.5% | 11% |
| gap-3 best combo: TP +100% + idle 3 s + sell ≥2% of supply | block 0 | +1.5% [−0.5, +3.7] | −2.3% | 32% |
| **Ours: rank-1 micro-scalp (§2), 0.5 SOL** | block 0 behind bundle | **+3.5%** [+1.4, +5.8] | **+3.2%** | 40% |

His thesis is right about where the money is (the first second after the bundle, on bundled
launches) and wrong about how long it lasts. The insiders' first sell comes at a median 9 s
(6 s after Oct 9); the pump he waits for rarely arrives before that.

## 2. The policy, exactly

**Universe.** pump.fun creates, SOL-quoted, not mayhem mode, where the creation slot contains a
bundle with **at least one non-creator wallet** adjacent to the create (gap-3 definition: the
create transaction's buys, the creator/signer/fee-payer buys, and the contiguous run of slot-0 buys
at index ≤ create+5 with gaps ≤ 2). About 3,750 such launches per day.

**Entry.** Buy **0.25-0.5 SOL** (never more: §4) to land **in the creation slot, first behind the
bundle**. `max_sol_cost` is capped at the post-bundle price **+2%**, so a fill at rank 3 or later
fails cheaply instead of filling at a loss.

**Exit.** Sell the whole position at the **first** of:
1. realised value ≥ **+15%** of the stake (after fees and our own impact);
2. **any bundle-wallet sell**;
3. **1 s with no trade** on the curve, counted from our landing slot;
4. price ≤ **−15%** of the post-bundle price, active from 1 s after landing (phantom stop);
5. a hard exit at **landing slot + 5** (fills one slot later).

The exit transaction is pre-built and sent on the first shred that shows the trigger; the
simulation assumes it lands one slot after the trigger slot (0, 2 and 4 slots were also tested:
+4.2%, +2.8%, +1.2% on the fresh holdout).

**Where the PnL comes from** (all multi-wallet launches, n = 488): the TP leg fires on 22% of
entries and averages **+31%** (contributing +6.9 points); idle exits (46%) average −1.8%; the slot+5
exit (22%) −2.4%; bundle-wallet-sell exits (8%) −8.3%; the phantom stop (2%) −22%. Median hold
1.6 s, 90th percentile 2.4 s. Per-trade SD 19.8%; median trade −1.2%; worst −66%.

## 3. Evidence and its limits

**Search size and selection history.** Stage 1 tried 25,397,064 configurations (27 entries ×
49,896 exits × 19 filters). Nothing survives a Benjamini-Hochberg correction over that grid; the
top-15-on-test table is a list of small-n flukes that are negative on train and on the deep
holdout. Stage 1 did show that only the block-0 entry with a very short exit sits near zero out of
sample, which pointed to the family. Stage 2 searched 659,232 exits in that family **on the
training split only** and froze 5 policies (11:34:40 UTC) before pulling a fresh holdout (finished
11:53). The family itself was chosen after stage-1 test numbers had been seen, so only the fresh
holdout is clean of every selection step.

| Set | EV per trade [90% CI] | n | Clean of selection? |
|---|---|---|---|
| train (first 60% by time) | +5.3% | 288 | in-sample |
| gap-3 test (last 40%) | +3.5% [+1.4, +5.8], BH q = 0.04 over 15 tests | 200 | family seen |
| deep holdout (random arm, Oct 12-14) | +3.2% [−1.0, +7.5] | 77 | family seen |
| **fresh holdout (pulled after the freeze)** | **+3.7% [+0.7, +6.6]**, p = 0.017 | 150 | **yes** |
| pooled out of sample | +3.5% [+1.9, +5.2], p = 0.0002 | 427 | |
| **dense window, Oct 14 02:05-08:05 UTC** (workflow C, every complete multi-wallet bundled tape, never used in any search) | **+2.5% [+1.6, +3.4]** at base; +1.3% at 0.01 SOL; −0.2% at the +3.6% premium | 915 | **yes** (orchestrator's run) |
| **critic's holdout, Oct 14 08:43-13:43 UTC** (frozen 14:46 UTC, pulled after; random multi-wallet launches) | **+3.2% [+1.5, +4.9]**, BH q = 0.005; all 485 clean launches +2.5% [+1.3, +3.8] | 222 | **yes** |

**Independent re-implementation.** The orchestrator re-implemented the policy from scratch (own
curve math, own bundle detection, own exit logic, no shared code) on the fresh holdout tapes and
obtained **+3.67% [+0.8, +6.6], n = 150, hit rate 42%**, with the same exit-reason breakdown. The
agent's arithmetic is right given its model. (`analysis/scripts/strategy/orchestrator_checks.py`)

**The dense window splits the edge by activity.** On the 915 multi-wallet bundled launches of the
6-hour window, launches whose whole 15-minute history fits in one API page (≤100 trades; 614 of
them) give **−0.9% [−1.9, +0.1]**, while the 300 busier launches give **+9.4% [+7.8, +11.0]** with a
65% hit rate. The policy pays only where other snipers follow us in; on quiet launches it is a
small, fast loss. Nothing visible in slot 0 tells the two apart in this data, which is what the
operator-conditioned stage tests next. The busiest launches (full history not reachable in 15
pages) are missing from the window set, which biases its number downward.

**Direction of bias in the fresh holdout.** It is the uniform light sample minus the deep sample.
The deep sample took the random arm and every launch with an all-time-high market cap above the
"interesting" threshold, so the holdout is **depleted of big winners**. For a take-profit policy
that is a conservative bias.

**Robustness.** All 108 neighbouring settings (TP 10-20%, idle 1-2 s, cap slot+3..10, with or
without the bundle-sell and phantom legs) are positive pooled out of sample (+2.3% to +4.2%).
Pre-Oct-9 (268 ms slots) +4.8%, post-Oct-9 (200 ms) +3.6%. Positive on 16 of 17 days.

**Realistic fills (the critic's stress stack, fresh holdout, n = 222).**

| Scenario | EV per trade |
|---|---|
| Base (rank 1, +1.1% median premium, 0.004 SOL, 1-slot exit) | +3.2% [+1.5, +4.9] |
| Rank 1 at the +3.6% premium | +0.4% |
| Rank 1 at gap-2's *mean* rank-1 premium (≈ +3.2%, ≈ +2.4% net of own impact) | +1.8% |
| 0.01 SOL per landed round trip | +2.0% |
| **Rank 2** (one historical sniper ahead of us) | **+0.8%** (earlier OOS launches: +0.0%) |
| Rank 2 with a +2% slippage cap and 0.003 SOL per failed attempt | +0.6% |
| First follower never lands | +0.8% |
| **Combined realistic** (mean premium, buyers keep SOL not tokens, 0.01 SOL, first follower fails 1 in 3) | **−0.4% [−2.0, +1.3]**; all clean launches −1.2% [−2.3, +0.1] |

**Where the money is.** On the 32% of launches where the first third-party buyer after the bundle
lifts the curve by more than 2% ("hot"), rank-1 EV is **+11.7%** (holdout +9.1%); on the other 68%
it is **−0.5%** (holdout +0.3%). Nothing visible in slot 0 before our buy separated the two in the
sweep's filters. The hot launches are where rival bots bid hardest (gap-2: the first slot-0
winner's median bid 0.0034 SOL, p90 0.068, and the earlier winner had the higher bid in 73% of
contested pairs). The policy is therefore a bet on being directly in front of a big sniper.

**Time variation.** On clean sets from the same day the policy ranges from +0.1% (06:12-10:45,
n = 108) to +6.1% (08:43-13:43, n = 114), and by hour from −0.1% to +8.9%. Anything shorter than a
full day is noise.

**What the simulation cannot know.**
- *Our landing rank.* The model assumes rank 1 at a +1.1% premium. gap-2 shows pro snipers win
  slot 0 on about half of bundled launches (62% on CEX-fresh bundled ones). Our rank is unmeasured.
- *Reflexivity.* Other traders' trades are held fixed in token terms. Removing 25% / 50% / 100% of
  the slot-0 snipers at random gives +1.9% / +0.6% / −1.8% on the fresh holdout. A persistent rank-1
  buyer may also provoke the "farming" the guru describes.
- *Failed attempts.* When we lose the race, the slippage-capped buy fails and still pays its
  priority fee (not a Jito tip: a failed bundle is dropped). At 0.003 SOL per failure and a 50%
  landing rate the effective round-trip cost is about 0.007-0.013 SOL (EV ≈ +2.4% at 0.01); at a
  20% landing rate it is 0.016-0.022 SOL and the edge is gone. **The landing rate is the whole
  business case.**
- *One regime.* The holdouts all fall in Oct 12-14. Alpenglow (epoch 1064, about Oct 21) changes
  slot timing and must be re-measured.
- *Sampling.* The tapes are a 1.27% sample of creates; the launch counts are population-scaled
  class sizes, not achievable trade counts.

## 4. Sizing, capacity and what breaks it

| Lever | Finding |
|---|---|
| Size | 0.1 SOL: +0.2% (the fixed cost eats it). 0.25 SOL: +2.7% [+1.0, +4.3]. 0.5 SOL: +3.5%. 1.0 SOL looks better only because the fixed cost per SOL falls; a bigger rank-1 buy knocks out more of the snipers we sell to and invites dumps. **Cap at 0.5 SOL.** |
| Entry premium | break-even ≈ +4.4% at 0.004 SOL/round trip, +3.3% at 0.01, +1.4% at 0.02. Rank 3 (+10.8%) ≈ −6%. **Cap `max_sol_cost` at post-bundle +2%.** |
| Fixed cost | +3.6% at 0.004 SOL, +2.4% at 0.01, +0.4% at 0.02 (base premium). |
| Exit latency | 0 / 1 / 2 / 4 slots: +4.2 / +3.7 / +2.8 / +1.2% on the fresh holdout. A shred feed and a pre-signed exit matter. |
| Fees | 1.25% per side is in the model; any fee change moves EV one for one. |
| Capacity | ≈3,750 multi-wallet bundled launches per day. If we win rank 1 on 10-20% at ≤ +2% premium, that is 375-750 trades/day × 0.5 SOL × 2.6-3.6%: an upper-bound illustration, not a forecast. |

## 5. What else was tried, and lost

| Family | Out-of-sample result | Where |
|---|---|---|
| Survivor entries (enter at slot+5..50 only if the price held and insiders already sold) | −2% to −9%; the launches that "survive" are the ones that dump later | strategy-sweep §1-2 |
| Dip entries after the first insider sell | −6.5%; oracle ceiling only +14-19% | same |
| Buy on dev/insider sell (his "buy on dev sell") | −13% | same |
| End-of-slot+1 entry (his "snipe delay") | −17% | same |
| Fixed TP ≥ +100% with any protective exit | ≤ −1% | same |
| Copy-trading scored pump wallets, one slot late, exit on their sell | −7.0% [−9.5, −4.5]; front-run upper bound −2.4% on 905 fresh launches; 0 of 32,400 configs significant | strategy-copytrade, -critic |
| Copy-trading the operator's own wave wallets | about −20% (they are the operator's tape-painting cost) | same |
| Mayhem coins (any entry/exit; 3,434 configs, 2,431 launches) | −11% to −22% out of sample; pre-registered hold-from-launch −21.7% on 1,688 unseen launches; holding is a fee-free random walk with 87.5% ending below the standard floor | strategy-mayhem |
| Per-operator arming of the lead (creator wallet, empirical-Bayes posterior > 0, ≥ 5 launches) | in-sample +9.0% vs +3.6% unarmed on the operator sets, but on the fresh holdout +2.9% vs +3.2% (lift −0.2 [−2.9, +2.5]); the operator sets were listing-sampled and overstate exactly the armed operators by 4.7 points | strategy-operators, -critic |
| The guru's own arming rule (≥ 3 of the last 10 hit +100%) on top of our exit | +2.1% [−2.0, +6.0], n = 67, not significant; on top of his own +100% hold: −20.5% | same |
| Any "repeat operator" filter without a track record | +4.0% vs a +5.2% control: adds nothing | strategy-operators |
| Top-15 policies ranked on the test set | q = 1.00, negative on train and holdout (the winner's curse made visible) | strategy-sweep §2c |
| **First buyer in slot+1** (same exits as §2, for creates too late in the block to reach in slot 0) | **−3.3% / −1.8% / −4.2%** on test / deep / fresh at the +1.1% premium (orchestrator's own run). The edge is strictly the creation slot. | this doc |
| §2 policy at the +3.6% premium (rank-1 median on CEX-fresh bundled launches) | +0.9% / +0.2% / +1.3%: thin. The +2% slippage cap is part of the policy, not an option. | this doc |

The copy-trade critic found the same thing from the other side: fast early wallets with a median
hold under 5 s keep a small own return (+1.8% to +5.3% after costs across train, test and two
holdouts) that cannot be captured by copying them. The money is in the rank-1 fill plus an exit
within seconds, which is exactly the policy in §2.

**Scope limit.** The 1.27% sample scores only high-frequency wallets. The guru's low-frequency
"pump wallets" (about 8 tokens a day) are untested; testing them needs a full-feed wallet ledger
(every trade on every launch for several days) from the collector, and the priors are unfavourable.

## 6. How it maps to his bot, and to ours

| His feature | Our setting |
|---|---|
| Tracking mode "new token creation", hardcore buy mode | block-0 entry, direct-TPU send with the per-class bid (docs/03 §5), rank-1 or fail |
| Max token age / min-max market cap | not used: the entry is the creation slot and the post-bundle cap is only a +2% slippage bound |
| TP levels | one level, +15% realised |
| Sell on no activity (150 s) | 1 s |
| Phantom SL (10 s, −25%) | 1 s, −15% |
| Auto-sell on big buy | not in the frozen policy (its neighbours with it are also positive) |
| Front-run sell on consolidation | not needed at a 2 s horizon |
| Max loss counter | kept: 5 consecutive losses per operator class suspends; daily loss cap kills (docs/06 §9) |
| Rugger protection / wallet inheritance | kept as the funding watcher: it pre-arms the watchlist, it is not a buy signal (gap-4) |
| Per-rugger selection (his core idea) | **not a trade filter**: it failed the fresh holdout. The creator and bundle-component posteriors are logged on every intent so they can be re-tested on a week of shadow data; the risk latches per operator stay. |

Config (docs/06 §6 names): `exit.default.tp = [[1.15, 1.0]]`, `exit.default.insider_sell = "bundle"`,
`exit.default.no_activity_ms = 1000`, `exit.default.phantom_sl = {after_ms = 1000, drop = 0.15}`,
`exit.default.max_hold_slots = 5`, `entry.max_premium = 0.02`, `entry.filter = "multi_bundle"`,
`risk.max_snipe_lamports = 500_000_000`.

## 7. Operating plan (nothing else moves money)

1. **Shadow mode on the Frankfurt box** (paper, docs/06 §10). On every multi-wallet bundled launch
   send nothing and record: the create's position in the block, the time from the create shred to
   our signed transaction, our would-be landing index against the first follower's real index, the
   post-bundle price and the premium we would have paid, and the simulated exit at that position,
   split hot/quiet. **Go/no-go rule, fixed now:** over at least 1,000 qualifying launches and at
   least one full day (the hourly swings demand it), the shadow EV per attempt at our measured rank
   and premium must have a 90% lower bound above zero, and we must be ahead of the first follower
   on at least one hot launch in three at a premium of +2% or less. Otherwise the answer is no
   edge, and we stop. Failed attempts are charged at their priority fee.
   *Status:* the shadow lane exists in paper mode (`paper.fill_model = "e1"`, `config/paper-lead.example.toml`).
   It reconstructs the creation slot, applies the gap-3 bundle definition, records our would-be
   index, buyers ahead and behind, premium and the exit at that position, and charges failed
   attempts (late-block creates, premium above the cap, slippage). On 50 of 50 closed paper fills
   it agrees with the tape simulator on the exit slot, the post-bundle market cap (1e-11) and the
   return (1e-8). Three 12-20 minute sessions from this container gave −4.0%, +1.4% and +0.7% EV
   per attempt: noise, as the hourly swings predict, and with no latency information because this
   container sees the chain about a second late. The measurement that matters needs the Frankfurt
   box and a shred-level feed.
2. **Probe (capped live).** About 200 trades at 0.25 SOL, `max_sol_cost` +2%, exits as in §2.
   Standard error at 200 trades is 1.4%. **Kill** if the mean is below 0 after 100 trades
   (about 4% false-kill probability if the true EV is +3.5%), if the median realised premium
   exceeds +3%, or if the landing rate falls below 20%. Record realised rank, premium, exit slot and
   the share of snipers behind us.
3. **Live** only after the probe passes and the go/no-go gate of docs/06 §9.5 (≥300 out-of-sample
   snipes, cluster-bootstrap lower bound > 0).
4. **Re-run the sweep weekly** on fresh tapes from the collector, with the same freeze-then-holdout
   protocol. Re-measure everything after Alpenglow (Oct 21).

## 8. Open items

- Running now (`strategy-predictor.md`, `strategy-refinements.md`, `strategy-pnl-and-screens.md`,
  `strategy-angles-critic.md`): a slot-0 predictor of hot launches from every feature visible at
  the decision instant, post-entry exit and size refinements, a daily P&L model tied to landing
  rates and failed-attempt costs, a ledger of every angle tested, three quick screens, and a
  skeptic on a fresh holdout. If the predictor separates hot from quiet, the per-trade number
  roughly triples on the selected launches; if not, the edge stays a pure latency race.
- Resolved: a "first in slot+1" entry loses on every set (§5), so creates that sit in the last
  5-15% of a block are skipped, not chased.
- Failed-attempt costs are modelled here by hand (§3); the hot path must log them so the probe
  measures them.
