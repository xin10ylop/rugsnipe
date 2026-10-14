# Strategy S2: mayhem-mode coins

**Question.** Mayhem-mode coins are about 25% of creates. gap-1 found they reach "+100%" 28% of the time, four times the normal base rate, but it could not measure the loss side because the floor does not hold. Is there a policy on these coins with a real, out-of-sample, positive expectancy after all costs?

*Measured 2026-10-14, 10:50 to 13:30 UTC.*

* Code:
  * `analysis/rugsnipe_analysis/strategy/mayhem.py` (tape reconstruction and simulator)
  * `analysis/scripts/strategy/mayhem_{pull,stats,agent,sim,holdout,wallets}.py`
* Data: `data/cache/mayhem/tapes.jsonl` (2,431 launches).
* Results: `data/reports/strategy/mayhem_*.json|txt`.
* Pre-registration: `data/reports/strategy/mayhem_preregistration.json`.

## Answer

**No. There is no mayhem policy with positive expectancy, in sample or out of sample.**

**VERIFIED on our sample and model.** The protocol's agent turns each mayhem coin into a fee-free random walk.

* The walk is nearly a martingale in price, with a strongly negative median. It ends within about a minute.
* Every exit rule we tried converges to roughly "minus the round-trip cost". The big hit rates are real, but they are paid for by losses that are just as big.

| Test | n | EV per trade (net) | 95% CI | Verdict |
|---|---|---|---|---|
| 3,434 entry × exit configs, time-ordered walk-forward (train = first 60% by create time, test = last 40%) | 2,430 launches | best on train: **-10.7% to -30.7% on test** | all exclude 0 or straddle it | 0 survive BH (FDR 5%), 0 survive Bonferroni |
| Same grid, all launches (in-sample, for reference) | ≥100 per config | best: **-2.0%** | [-12.0, +10.3] | **0 of 3,434 configs have a positive point estimate** |
| 3-fold expanding walk-forward, pooled OOS (policy re-picked on each train fold) | 858 trades | **-17.9%** | [-25.4, -9.6] | negative |
| **Pre-registered H1**: buy at eos0, hold to +900 s (formed on the first 741 launches) | **1,688 unseen launches** | **-21.7%** | **[-29.2, -13.5]** | **rejected** |
| The guru's rule on mayhem: eos0, TP +100%, else hold 900 s | 2,429 | **-5.1%** | [-8.8, -1.6] | negative; TP hit 27.5% (gap-1: 28%) |

**The single most promising lead is a statistically real micro-drift in the agent, which is too small to trade.**

* Each agent price step has E[p_i/p_(i-1)] = **1.0029** [1.0017, 1.0043]. The CI is launch-clustered (141k steps, 2,394 launches), and the drift replicated on the holdout.
* After a third-party sell the next step averages **+1.08%** [+0.65, +1.53].
* A round trip costs about 4.4% (2 × 1.25% fees, a 1.1% fill premium, and 0.004 SOL on 0.5 SOL), so the drift covers about a quarter of the cost.
* Holding through many steps does not compound it. Price at +900 s averages **0.95× eos0** (median 0.34×), because the number of steps is endogenous (§3).
* The least-negative policy family is a pre-bundle entry (eos0 mcap ≤ 28.5) with an exit after 20 agent steps. It returns -2.2% [-7.4, +3.3] on all launches and -6.3% [-13.6, +1.1] on test. It is not a lead.

**Recommendation: drop mayhem coins from the candidate list.** Keep excluding them in the hot path, as the architecture already does.

## 1. Mechanics (VERIFIED on mainnet transactions)

### Sources

* The public docs (`pump-fun/pump-public-docs`, fetched 2026-10-14) have **no mayhem page**. `docs/MAYHEM_README.md` returns 404.
* What they do say:
  * `create_v2(is_mayhem_mode)` passes the mayhem program `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e`, `global_params`, `sol_vault`, `mayhem_state` (seeds `["mayhem-state", mint]`) and `mayhem_token_vault`.
  * Trades on mayhem coins must use one of the 8 **reserved fee recipients**, for example `GesfTA3X…`, `4UQeTP1T…`.
  * Mayhem curves get no synthetic migration (`partial_fill`), cannot be multi-hop routed, and cannot be a quote coin.
  * The pump IDL has `set_mayhem_virtual_params` and `UpdateMayhemVirtualParamsEvent`.
* The rest comes from decoding 7 transactions with `getTransaction`:
  * on `F6NGwrqc…pump`: the create plus dev buy, three agent trades, and one third-party buy;
  * on `BY66gkz7…pump` (2026-10-14): two agent trades;
  * plus `getAccountInfo` on `mayhem_state` and `global_params`.

### What happens on a mayhem coin

1. **Create.** Supply is 2B.
   * 1B goes to the curve, which starts at the standard reserves: vSOL 30, vTok 1.073B, mcap 27.96 SOL.
   * 1B goes to the mayhem token vault, owned by the **sol-vault PDA `BwWK17cbHxwWBKZkUYvzxLcNQ1YVyaFezduWbtm2de6s`**. This is the published constant; the program logs `InitializeMayhemState`.
2. **The agent.**
   * It trades with signer `Gygj9QQby4j2jryqyqBHvLP7ctv2SaANgh4sCb69BUpA`, calling `MAyhSm…` `Buy` / `Sell`, which CPIs pump `Buy` / `Sell`.
   * The `user` field of the trade is the sol-vault PDA. Fee bps are 0 and the creator fee is 0.
   * The instruction carries an explicit token amount (standard buy/sell discriminator plus a u64). **Decisions are made off-chain by the signer. Nothing in the account state lets anyone precompute the next move.**
3. **The agent sets the price.**
   * Each agent trade moves *virtual SOL reserves* by far more than the SOL it pays. For example, a 0.0528 SOL sell took vSOL from 30.15 to 19.44 (-35% price).
   * Virtual token reserves follow the cumulative token flow exactly. Our reconstruction error has a median of 9e-7 over 34k third-party trades.
   * **Real SOL moves in proportion to price on every agent trade**: rsol_after / rsol_before = p_after / p_before. The median absolute error is 6e-5 over 141k agent trades; p95 is 5%.
   * So the real SOL in the curve always backs third-party holdings at spot. A holder's value simply follows the agent's price, and the agent is the counterparty.
4. **Third parties** trade the ordinary constant product on (vSOL, vTok) and pay 1.25% per side. The depth is the same as a normal coin at the same price.
5. **State.** `mayhem_state` (106 bytes) holds:
   * create time and create + 86,400 s (the 24 h window);
   * the mint;
   * the agent's net SOL (i128) and net tokens (i128);
   * the last agent-trade time;
   * one more u64 (unknown).

   `global_params` holds 8000 (bps?), 20 SOL ×2, 50,000 SOL ×2, and authorities. We treat the 20 SOL as a possible per-coin agent budget, as a sensitivity test only (UNVERIFIED semantics).

### Agent fingerprint (n = 2,431 launches, 141,047 agent trades)

| Quantity | Value |
|---|---|
| Launches with an agent trade in 900 s | 98.5% |
| First agent trade after create | median **1 s / 3.5 slots** (p90 6 s / 24 slots) |
| First agent trade is a buy | 49.7% |
| Agent trades per launch (900 s) | median 31 (p10 5, p90 151) |
| Gap between agent trades | median 3 slots (p90 9) |
| Price change per agent trade | median ±18.4% (p10 6.5%, p90 36%) |
| SOL per agent trade | median 0.024 (p90 0.19); scales with real SOL in the curve |
| Buy share per launch | median 50% |
| Last agent trade | median **43 s** after create (p75 109 s, p90 241 s) |
| Agent still active after 1 h / 6 h / 24 h | 1.6% / 0.9% / 0% (n = 320 with checkpoints) |
| Restarts after ≥60 s idle | rare (189 of 141k steps); it follows third-party trading |
| Step drift E[r] | **1.0029** [1.0017, 1.0043]; mean log -0.026, sd 0.247, P(up) 0.502 |
| Agent net SOL per launch | -0.026 [-0.151, +0.085] (median +0.048; 57% of launches positive) |

The "24 h agent" exists on paper: the state window is 24 h. In practice it stops when third-party trading stops, usually within a minute.

## 2. Outcome distribution (entry reference eos0, n = 2,431)

| Quantity | p10 | p25 | p50 | p75 | p90 | Mean |
|---|---|---|---|---|---|---|
| eos0 mcap (SOL) | 28.07 | 28.07 | 28.14 | 28.74 | 29.83 | 29.4 |
| Peak within 900 s / eos0 | 1.00 | 1.11 | **1.47** | 2.25 | 3.87 | 2.35 |
| Time to peak (s) | 0 | 1 | **7** | 23 | 56 | 25 |
| Drawdown after peak (900 s) | -99.6% | -95.9% | **-82%** | -38% | -2% | -66% |
| Minimum / eos0 | 0.007 | 0.067 | 0.24 | 0.66 | 1.00 | 0.37 |
| Price at +900 s / eos0 | 0.009 | 0.088 | **0.34** | 1.15 | 2.02 | **0.95** |
| Price at ATH + 600 s / ATH (n = 320) | 0.007 | 0.066 | 0.29 | 0.77 | 0.98 | 0.41 |

Peak ≥ 1.3× / 1.5× / 2×: **60.8% / 48.7% / 30.9%**. This is the gap-1 "28% at +100%".

**Price after 1 h / 6 h / 24 h / 25 h** (n = 320 with checkpoints): within 1% of the 900 s price in 94% of launches, because the agent has stopped. Median 0.39× eos0. Below the standard floor: **64%**. Below 10% of the floor: **24%**. The price **goes below the standard floor in 87.5% of launches within 900 s**. The floor-based stop does not exist here.

**Price where the coin ends versus launch.** The median coin ends at 34-39% of its launch price and the mean at about 95%. A few launches end at 10-75×; the largest was +75.7× on 2026-10-02.

## 3. Why every exit rule fails

* A third party holding a mayhem coin owns a share of a fee-free random walk run by the agent. The real-SOL invariant (§1) means the payout is honoured; the real-SOL cap never bound on our exits.
* For a (near-)martingale, **no stopping rule changes the expected value**. Take-profit, trailing stops, agent-drop exits and idle timers only reshape the distribution. Each trade still pays about 4.4% in costs.
* The measured EVs line up with that:

| Exit (eos0 entry, base costs, n = 2,429) | Hit / win rate | EV |
|---|---|---|
| TP +30% | TP 54%, win 58% | -5.6% [-8.0, -3.2] |
| TP +100% (guru) | TP 27.5% | -5.1% [-8.8, -1.6] |
| Trailing stop 20% | win 25% | -7.3% [-9.7, -4.6] |
| Sell after the first agent step | win 43% | -7.2% [-8.1, -6.4] |
| Hold to +900 s | win 25%, median -66% | -11.8% [-20.9, -1.0] |

* **Why holding does worse than -4.4% despite E[step] > 1.** The number of agent steps is endogenous: the agent keeps trading while third parties trade.
  * By agent steps in 900 s, the mean / median hold return is +1% / -4% (<10 steps, n = 510), -4% / -44% (10-29), -14% / -85% (30-99) and **-35% / -99%** (≥100, n = 427). The negative log drift compounds.
  * The positive arithmetic drift is realised only on rare runaway launches, and those are too rare to pay for the rest. The mean price at +900 s is 0.95× eos0.
* **What predicts the next agent step** (launch-clustered CIs, `mayhem_agent.json`). Nothing moves it by more than about 1%:
  * after a third-party sell: +1.08%;
  * at step index ≥100: +0.67%;
  * at price < 0.25× eos0: +0.58%;
  * the first step: +0.01% [-0.9, +0.9];
  * after a large agent up-step (>30%): +0.44% [-0.02, +0.87];
  * after a ≥0.5 SOL third-party buy: -0.07%.

  No bucket clears the 2.5% fee round trip, let alone the full 4.4%.

## 4. Policy search

* **Simulator** (`mayhem.py`).
  * **Tape**: the swap-api trade tape from create to +900 s (2,426 of 2,431 complete to 900 s). We reconstruct the state after each trade: vTok from cumulative flow, vSOL = price × vTok, rsol = cumulative SOL.
  * **Our fill**: constant product with 1.25% fees on each side and a 1.1% entry premium (rank-1 median, gap-2). Base costs are 0.004 SOL per round trip on 0.5 SOL.
  * **Real-SOL cap**: exit proceeds are capped at the historical rsol plus our own SOL scaled by the price ratio since entry.
  * **No look-ahead**: signals use only trades up to the signal trade; fills land at the state after slot + 1 (gap-3 "E1"); time triggers fill at the last state before the trigger.
* **Grid.**
  * 17 entries:
    * eos0 and slot+1/10/50;
    * create+15/60 s;
    * the first agent trade (any, buy or sell);
    * dips to 0.8/0.6× by 120 s and deep dips to 0.25/0.1× by 300 s;
    * the first agent up-step ≥20%/35%;
    * eos0 mcap ≤ or > 28.5.
  * 202 exits:
    * TP 30/50/100/150%;
    * trailing 20/30%;
    * agent-drop ≥20%;
    * agent idle 10 s;
    * any-trade idle 3/10 s;
    * after k = 1/2/3/5/10/20 agent steps;
    * TP/SL pairs 10-30% / 10-20%;
    * SL 30%;
    * max hold 60/300/900 s.
  * Total: **3,434 configurations.**
* **Split.** Launches were sorted by create time.
  * Train: 1,458 launches, 2026-09-28 09:04 to 10-07 19:55.
  * Test: 972 launches, 10-07 20:02 to 10-14 02:58.
  * The 3-fold expanding walk-forward uses cuts at 40/60/80%.
* **Multiple testing.** One-sided t p-values per config, Benjamini-Hochberg at 5% and Bonferroni, over all 3,434 configs.

**Best 5 policies selected on TRAIN, evaluated on TEST** (`mayhem_sim.json`):

| Entry | Exit | Train EV (n) | Test EV | Test 95% CI (bootstrap) | Test n |
|---|---|---|---|---|---|
| dip to 0.8× by 120 s | SL 30%, hold ≤900 s | +4.9% (1,145) | **-11.8%** | [-20.8, -0.7] | 805 |
| first agent trade is a buy | hold 60 s | +3.6% (723) | **-10.7%** | [-23.1, +3.1] | 465 |
| eos0, mcap ≤ 28.5 | hold 60 s | +3.2% (1,042) | **-12.6%** | [-23.6, -0.7] | 666 |
| first agent trade is a buy | agent idle 10 s, ≤60 s | +3.2% (723) | **-11.8%** | [-23.9, +1.3] | 465 |
| eos0, mcap ≤ 28.5 | trade idle 3 s, ≤900 s | +1.9% (1,042) | **-22.1%** | [-30.8, -11.9] | 666 |

* Best on TEST, for reference only (picked on test, so biased upward): -0.6% [-7.9, +7.7]. The minimum test p-value is 0.56.
* Walk-forward folds (policy re-picked each fold, then scored on the next 20%):
  * agent-buy-hold: **-31.5%**;
  * dip + SL30: **-12.6%**;
  * agent-buy, 60 s: **-13.1%**.
* Cost sensitivity for the top 10 (test):
  * stress costs (3.6% premium, 0.01 SOL): -14% to -17%;
  * 0.1 SOL size: -12% to -14%;
  * a 20 SOL agent budget cap: -11% to -13%.

**Pre-registered holdout.** After looking at the first 741 launches, we wrote down before seeing any more data (`mayhem_preregistration.json`, 12:36 UTC):

* H1: eos0 then hold 900 s has positive EV.
* H2: E[agent step] > 1.

Both were tested on the 1,688 launches pulled afterwards:

| | Seen (741) | Holdout (1,688) |
|---|---|---|
| H1 mean | +10.8% [-13.7, +41.8] | **-21.7% [-29.2, -13.5]** |
| H1 mean without the top 1% | -20.5% | -34.4% |
| H1 median | -60.5% | -72.3% |
| H1 max | +75.7× | +28.0× |
| H2 E[step] | 1.0047 [1.0023, 1.0071] | 1.0022 [1.0007, 1.0038] |

The positive "seen" mean came from one launch (+75.7×), which contributed 2.9× the total. Entering at slot+3, slot+10 or the first agent trade gives -21% to -23% on the holdout.

**Copy-trading third-party wallets** (`mayhem_wallets.json`):

* Sample: 11,445 closed positions by 3,076 wallets.
* All third parties: mean ROI +0.5%, median -3.5%, **-7.7 SOL in total** after fees. Positions held to zero never close, so this is biased *up*.
* Walk-forward: 266 wallets with ≥3 positions and positive PnL on train, scored on test: **-0.1% [-3.8, +3.9]** on 1,282 positions.
* The two big in-sample winners are creators who make 10-28 SOL dev buys and sell 10-20 s later:
  * `6ATnpbwa…`: 26 launches, mean +16%;
  * `34Kq3jEp…`: 9 launches, +116 SOL.

  Their per-launch results swing from -81% to +317% with no time pattern. This is the same random walk, with luck.

## 5. Capacity

* About **11,500 mayhem creates/day**: 25% of about 46k; gap-1 measured 25.1% of 49.4k. About 97% are SOL-quoted.
* About 98.5% have agent activity, so almost every one is an "opportunity".
* The volume does not matter, because the expectancy is negative under every policy tested.

## 6. Risks (if anyone revisits this)

* **Agent behaviour is off-chain and can change at any time.** The signer decides the amounts, the step size distribution, when to stop, and the 24 h window. It looked stationary across the 16 days: mean |log step| 0.19-0.22 every day.
* **There is no floor.** 87.5% of launches trade below the standard floor within 900 s. 24% end below 10% of it.
* **The tail decides everything.** 1% of launches can carry more than 100% of a sample's PnL, so short evaluations (<1,000 launches) are noise. Our own first 741 launches suggested +11%; the next 1,688 gave -22%.
* **Agent budget.** `global_params` holds 20-SOL values. If they cap the agent's per-coin funding, the runaway tails that are the only source of upside are capped too (the agentcap sensitivity is -11% to -13% on test).
* **Competition.** It is irrelevant to the verdict. The edge is negative even at rank-1 fills with base tips.

## 7. Caveats

* **Horizon is 900 s.** The agent trades after 850 s in only 0.8% of launches. For those, the 900 s exit is a horizon cut, not a stop. The 1 h checkpoint (n = 320) is within 1% of the 900 s price in 94% of launches.
* **Fills** are modelled at the post-slot state with a fixed premium. Same-slot ordering against the agent (it trades every 3 slots) is not modelled; any realistic latency makes entries later, and later entries did worse.
* **Our own buy**: in the real game it adds SOL to the curve. Agent trade sizes scale with real SOL, but the price step (the only thing that matters to our PnL) did not depend on real SOL beyond ±0.3%.
* **Sample**: 2,431 SOL-quoted mayhem creates from the gap-1 uniform sample (16 days to 2026-10-14). Alpenglow (about Oct 21) is not covered.
* **Swap-api pacing**: 6,013 calls, 1,137 of them paused for the shared limit, all retried successfully.

## Reproduce

Run from `data/cache/mayhem/`:

1. `analysis/scripts/strategy/mayhem_pull.py 2431 1.4` (about 2.5 h at the shared swap-api limit; resumable).
2. Then, from the repo root:
   * `mayhem_stats.py`
   * `mayhem_agent.py`
   * `mayhem_wallets.py`
   * `mayhem_sim.py` (5 min, 8 processes)
   * `mayhem_holdout.py`
