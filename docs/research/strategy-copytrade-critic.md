# C5: adversarial review of the copy-trading study (S5)

**Question.** Does the S5 verdict (`docs/research/strategy-copytrade.md`: no copy-trading policy has a positive out-of-sample expectancy after costs) survive an attempt to break it? We attacked it for look-ahead in wallet scoring, wallets selected on the launches they are scored on, entry fills, our own impact and the rank premium, truncated tapes, swap-api paging gaps, survivorship and multiple testing. Then we ran the top three surviving policies on a fresh holdout of launches created after every tape used for wallet scoring.

*Measured 2026-10-14. Code: `analysis/scripts/strategy/copytrade_critic.py` (stages `holdout_pull`, `hot_pull`, `audit`, `holdout_eval`). Caches: `data/cache/strategy_holdout_copy/`. Results: `data/cache/strategy_holdout_copy/audit.json`, `data/reports/strategy/copytrade_critic_holdout.json`. Fix applied to the study: `analysis/scripts/strategy/copytrade.py` (4 lines), with `sim` and `report` re-run. Nothing was committed.*

## Verdict in brief

HOLDOUT_VERDICT_TBD

## 1. What we checked, and what we found

| Attack | How it was tested | Result | Effect on the verdict |
|---|---|---|---|
| **Look-ahead in wallet scoring** | Read `walk_forward()`. A launch feeds scores only when `bt + horizon < create time of the current launch`. Probe: hold every launch back a further 30 min. | No look-ahead. With the delay, the best realistic policy moves -2.49% → -2.12% and the front-run lead stays at +0.58%. | None. |
| **Later transfers used to exclude signal wallets** | `ins = C ∪ B ∪ funders ∪ xrecv` uses transfers that can happen *after* the signal. Re-ran without `xrecv`. | A latent look-ahead, but it never bites: identical event counts (e.g. 859 `C0`, 693 `Sb_R1`, 247 `Sd_d_1-3`) and identical EVs. | None. |
| **Wallets selected on the launches they are scored on** | Point-in-time `hist` only takes launches that have been flushed. The top-20 tables (§1b, §2) are full-sample, but they are descriptive and feed no policy. Re-derived the §5 persistence table (train terciles → test). | Clean. §5 has **no code in the study's script**, but we reproduced it independently: 26,758 pairs; leader +0.10%, copy -13.41%, front -9.14%, scalp -4.70%; terciles -19.9 / -7.1 / +7.5% on train → -20.8 / -9.7 / -8.3% on test; ρ = 0.23. | None. |
| **Entry at the signal price** | Code: entry = last trade with slot ≤ signal slot + lag, which is the end-of-slot state. Fills checked against `curve.py` integer math on 200 random events. | Max token error 1e-9 (relative), round-trip error 1e-9 SOL. 17-35% of entries carry zero premium, because nobody traded between the signal and our slot, not because of a fill at the signal price. | None. |
| **Entry premium as reported** | Mean and p90 of the premium over the copied fill (test, lag 1). | The study reports medians. Means are larger: `Sd_d_1-3` **+9.7%** (p90 +21.7%), `Sb_R1` +4.4% (p90 +12.1%), `Sb_R3` +3.6%. The EVs already include this; only the table understates it. | None (presentation). |
| **Rank premium and crowding** | Extra premium on our fill on top of the end-of-slot state, for copiers that are not in the tape: +1.1 / +3.6 / +10.8% (gap-2's range). | Best realistic (`Sb_R3` scalp) -2.5% → -3.5 / -5.2 / -13.5%. Front-run lead +0.6% → -0.5 / -3.0 / -9.3%. | Makes it worse. |
| **Later entry** | Lag 2 and lag 3. | `Sb_R3` scalp -3.3 / -3.6%. Front-run lead +0.2 / +0.9% (noise). | None. |
| **Our own impact** | Model: other traders keep their *token* amounts, and our q stays outstanding. Compared with a SOL-amount model on a worked example. | The token model is about 2% of position more generous to us after a +43% move, and equal on a fall. Slightly optimistic for the copier. | Strengthens the null. |
| **Tapes truncated at +300/+900 s** | Exit-reason counts on test. | Truncated holds: 0-1 of about 180 exits for the top policies, 15 of 293 for the `C0` default (300 s holds on 300 s deep tapes). | Immaterial. |
| **swap-api paging gaps** | Reserve continuity per slot: the implied SOL-reserve change must equal the slot's signed SOL. | gap-3: 3 of 673 tapes inconsistent (all late, post-graduation AMM trades). Deep: 0 of 225. **Ops: 17 of 96**, with prices below the standard-curve floor (e.g. 24.2 SOL < 27.96), so non-standard curves, most likely mayhem coins (operators `EhrQmJ…`, `FdSShB…`, `3uXcbK…`). That contradicts the study's "mayhem coins are excluded" for ops tapes. Dropping all 20: `Sb_R3` scalp -2.42%, front lead +0.83%. | Immaterial (ops carry zero population weight). |
| **Survivorship: launches** | Listed every tape on disk that the study did not use. | 39 gap-3 slot-0 graduations (85.005 SOL dev buy, trading on `pump_amm`; no curve, a legitimate exclusion). 6 genuinely hot curve launches were dropped because paging never reached slot 0: 4 gap-3 (all in train) and 2 deep (test, ATH 686 and 1,549 SOL). We pulled the 2 test launches (62 requests). Copying wins big on them (e.g. +36% for the `Sb_R3` scalp, +41% for the `Sb_R1` default), but restored they move test EV by only **+0.05 to +0.25 points** (`Sb_R3` scalp -2.49 → -2.27%; front lead +0.58 → +0.63%). | Immaterial. |
| **Survivorship: wallets** | Wallets that blew up keep every trade in the tapes of the launches they traded; scores use only what was visible. | Clean. But the 1.27% sample can only score wallets with at least ~20 early entries a day: P(≥4 sampled launches) is 0.58 at 20/day and **0.08 at 8/day**, the guru's rate. | A limit on scope (§4). |
| **Multiple testing** | Recomputed BH from the stored p-values with independent code. Also BH on the reverse hypothesis (EV < 0). | Identical q-values. **0 of 32,400** test results have p < 0.05 (about 1,620 would be expected if the tests were independent and the null true). The smallest p is 0.096 and every q is 1.00. **27,549 of 32,400 are significantly *negative*** at q < 0.05. | Confirms. |
| **"All tapes" weighting** | The unit weights ignored gap-1's subsample, which kept 1 in 5 launches with ATH < $5.5k (w = 5). So the all-tapes rows over-weight launches that went on to do well. | **Bug, fixed** (4 lines; `sim` and `report` re-run). The lead's all-tapes **+1.64% → -0.55% [-4.2, +3.6]**. All-tapes configs with n ≥ 20 and EV > 0: **19 → 0**. Population rows unchanged. | Removes the lead's best supporting number. |
| **A guru exit the grid missed: "auto-sell on big buy"** | Exit at the end of slot+1 after the first buy ≥ X SOL by anyone else (X = 0.5/1/2/4), alone or combined with copy and idle3, TP none/+30%, 60/300 s holds: **1,152 configs** over 9 signals and 2 sizes. | Best on train: `Sd_d_lt0.3` big ≥2 + idle3 + TP30, +0.41% → **-2.85%** on test. Best on test: -1.98%. No test EV > 0; minimum BH q = 1.00. | None. |
| **Does the wallet score add anything?** | The best realistic policy, set against the same exit on the control (first independent buyer, `C0`). | Test: `Sb_R3` scalp -2.5% against `C0` scalp -5.2%. Train: -2.5% against -3.1%. At best the score adds about 3 points, which does not close a 2.5-point deficit. | None. |

## 2. Fresh holdout

HOLDOUT_TBD

## 3. Why the null is robust

* **The leaders break even and keep the edge at their own fill.** Early independent wallets earn +0.1% on the launches we copy. The copier pays two more fees (2.5%), the landing cost (0.8% at 0.5 SOL) and the premium of everyone who trades between the leader's slot and ours (mean +3.6% to +9.7%).
* **Fast leaders do have a persistent, small edge.** Take wallets whose point-in-time median hold is under 5 s and whose earlier launches were profitable. Their own ROI is +2.2% per trade on train (t = 2.9, n = 905) and **+1.7% on test (t = 2.1, n = 679)**; SOL-weighted it is +5.7% and +4.2%, before their tips. Copying the same wallets loses about 5% on both splits: their 0-5 s holds end before our sell can land.
* **Slow leaders inherit nothing worth copying.** Wallets whose point-in-time median hold is ≥120 s copy at -17.3%, against -16.9% for the leaders themselves. The copy penalty vanishes, but the leaders lose. Slow wallets with profitable histories (≥30 s holds, prior ROI > 0) earn -4% to -7% themselves going forward, and copying them earns -8% to -11%.
* **Perfect foresight shows where the money is.** Copying only leaders whose own trade ends in profit would earn +19.4% per trade (+75.7% for leaders that end above +50%). No signal visible at entry time separates those trades.

## 4. Limits the study does not state clearly

* **The wallet universe is the bots.** A 1.27% sample over 16 days scores only wallets with at least ~20 early entries a day. The guru's "pump wallets" (~8 tokens a day, ~57 buys a week) have an 8% chance of reaching the 4-launch threshold. The null is therefore established for high-frequency early wallets. It is *not* established for low-frequency wallets. The slow-wallet evidence above (§3) is unfavourable, but it comes from the few slow wallets that do appear in the sample.
* **The train set holds only the bundled class.** All 252 deep launches fall in test, so train-selected policies were chosen on gap-3 and ops launches only. The holdout (§2) removes this issue: it is a random sample of all non-mayhem SOL-quoted creates.
* **Section 5 of the study is not reproducible from its script.** The numbers are right (we reproduced them), but the code was not saved.

## 5. Most promising lead

LEAD_TBD

## Reproduce

```
.venv/bin/python -I analysis/scripts/strategy/copytrade_critic.py audit           # A1-A8 probes (~2 min, no network)
.venv/bin/python -I analysis/scripts/strategy/copytrade_critic.py holdout_pull 1.0 # fresh holdout (RPC + swap-api, <=700 requests)
.venv/bin/python -I analysis/scripts/strategy/copytrade_critic.py hot_pull        # 2 hot test launches dropped by the study
.venv/bin/python -I analysis/scripts/strategy/copytrade_critic.py holdout_eval    # frozen scores -> holdout signals -> policies
.venv/bin/python -I analysis/scripts/strategy/copytrade.py sim && .venv/bin/python -I analysis/scripts/strategy/copytrade.py report   # study re-run with the weight fix
```
