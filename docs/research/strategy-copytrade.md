# S5: copy-trading pump wallets and operator wave wallets

**Question.** Does copying wallets that buy into fresh pump.fun launches have positive expectancy out of sample, after all costs? This covers the guru's method 2 ("pump wallets"), with his bot's track-buy range, buy-once, cooldown, wait-for-buy, front-run sell and auto-sell options. Two kinds of wallet are tested:

* (a) the operator's own **wave wallets**: linked wallets that buy after slot 0;
* (b) **independent pump wallets**: unlinked wallets that keep entering early and make money.

*Measured 2026-10-14. Code: `analysis/rugsnipe_analysis/strategy/copytrade.py` (library) and `analysis/scripts/strategy/copytrade.py` (stages `discover`, `pull`, `hist`, `sim`, `report`). Caches: `data/cache/strategy_copy/`. Results: `data/reports/strategy/copytrade_{wallets,sweep,summary,wallet_hist}.json`.*

## Verdict in brief

1. **No copy policy clears break-even out of sample. VERIFIED on our tapes.**
   * We tried **32,400 configurations**: 15 signal variants × 2 entry lags × 3 sizes × 360 exit rules.
   * On the test set (last 40% of launches by create time), **no configuration has a BH q-value below 1.00**. The smallest one-sided p-value among the configs with n ≥ 5 is 0.096; a sweep this size produces that by chance.
   * Among realistic configs (no front-running) with n ≥ 20, **none has a positive test EV**. The best is **-2.5% per trade [90% CI -6.3, +1.4]** (`Sb_R3`, lag 1, 0.5 SOL, idle-3 s or TP +30%, max 60 s, n=171).
   * The honest walk-forward pick is the best config on train. It scores +0.2% on train and **-3.2% [-6.1, -0.4]** on test.
2. **Copying the guru's way loses 6-9% per trade.** The default policy (enter at the end of slot+1, sell 1 slot after the tracked wallet's first sell, max 300 s, 0.25 SOL) loses on every signal family:
   * scored pump wallet (`Sb_R1`): **-7.0% [-9.5, -4.5]** (n=234);
   * two scored wallets within 5 s (`Sc`): **-6.4%**;
   * copy the first independent early buyer (control `C0`): **-7.5%**;
   * operator wave wallets (`Sa`): **-20% to -23%** (n=17-20).
3. **The "pump wallets" are real, but there is nothing in them to copy.**
   * 1,012 unlinked wallets entered ≥4 launches within 30 s. Mostly they are sniper bots (vanity prefixes `EDGE…`, `nya666…`, `9999…`, `CCCC…`, `8888…`, `sssss…`).
   * Pooled, they made **-4.1%** on their own money. Only 372 had positive PnL.
   * 28 had a per-launch t-stat above 2, where pure noise would give about 23.
   * The leaders on our copy events made **+0.1%** themselves. Copying the same trades makes **-13.4%** (exit after their sell) or **-9.1%** (front-running their sell).
4. **Wallet skill persists only on the downside.**
   * We took 242 wallets with ≥4 train and ≥3 test copy events and ranked them by train copy return.
   * The top tercile goes from **+7.4% on train to -8.2% on test**. The bottom tercile stays at **-20%**.
   * Scoring tells us whom to avoid, not whom to follow. The correlation of copy return from train to test is 0.23.
5. **Operator wave wallets exist and are the worst thing to copy.**
   * 940 wallets recur after slot 0 in ≥2 launches of one operator. One example is a set of 0.1-SOL wallets that buy every `61xHM4s8…` launch at slots 1-12.
   * Together they lose **-12.3%** of their own money. They are paint-the-tape wallets, and they pay the operator's costs.
   * Copying them pays a median **+4-5% entry premium** and loses about 20%.
6. **The ceiling is not the problem; the exit is.**
   * Oracle: sell at the best state after a slot+1 entry. That gives **+24% to +33%** per trade (random sample) and +42-50% (all tapes).
   * The best realistic exits capture none of it. The front-run upper bound (sell in the same slot as the tracked wallet, 0 lag) tops out at **+0.6% [-3.1, +4.3]** (`Sd` 1-3 SOL buys, n=89, q = 1.00).
   * **The single most promising lead** is `Sd_d_1-3` + front-run sell + idle 3 s. It needs a 0-slot reaction to a wallet's sell, which is not achievable, and even then it is +0.6% ± 3.6 points. Section 6 explains.

## Data and method

**Tapes.** 1,023 launches with full trade tapes from disk (no new trade pulls):

| Source | Launches | Horizon | Population | Weight |
|---|---|---|---|---|
| gap-3 | 675 | create → +900 s | bundled, or dev buy ≥1 SOL, from the 16-day gap-1 random sample | w (5 for the 1-in-5 low-ATH subsample) × 74.4 creates/launch, per day of window |
| ops | 96 | +900 s | 7 operators that reuse their creator wallet (gap-3 §2) | 0 in the population EV; 1 in "all tapes" |
| deep | 252 | +300 s | random arm of the 2026-10-12..14 deep sample: non-mayhem, SOL-quoted, all launch types | 186.6 creates/launch over 2 days; halved for bundled-class launches, where gap-3 also covers the window |

* **Mayhem coins are excluded.** They have a different supply and the agent trades them.
* **Population EV.** "Pop" weights each trade by the population trades per day it represents, scaled to 46k creates/day. "All tapes" uses unit weights and includes ops.
* **Split by create time.**
  * Train is the first 60%: 614 launches before 2026-10-10 20:48 UTC (gap-3 and ops only).
  * Test is the last 40%: 409 launches (138 gap-3, 19 ops, 252 deep).
  * All deep launches are therefore test-only.

**Per-wallet ledger** (`ledger()`; 63,265 wallet-launch rows, 35,432 wallets). For each wallet in each launch we record:

* first-buy slot offset and seconds;
* SOL in (fee included), SOL out (fee deducted), realized PnL on the sold part, and PnL marked to market at the tape horizon;
* hold time;
* role: **C** (creator / signer / fee payer), **B** (gap-3 bundle wallet), **F** (in the creator's funder chain per `gap1/funding.jsonl`, or received this mint from C/B by transfer per `gap3/xfers.jsonl`), or **O** (independent).

**Point-in-time scoring** (`walk_forward()`):

* Launches are processed in create order.
* A prior launch updates wallet scores, operator links and wave sets only once its whole tape horizon (create + 300/900 s) has passed.
* Operators are tracked with an incremental union-find over creator, bundle wallets and creator funders. A launch's operator is "known" at signal time only if its creator or a slot-0 bundle wallet was already in the graph. That held for 29% of launches.

**Signals** (buy-once per coin: the first qualifying event per launch and signal variant):

| Signal | Definition |
|---|---|
| `Sa_N{10,50,150}` | A wave wallet of the point-in-time operator buys after slot 0, within N slots. A wave wallet appeared after slot 0 in ≥2 earlier launches of that operator, and is either linked (bundle wallet or F role there) or ≥50% exclusive to that operator. |
| `Sb_R1` | Task definition: independent wallet, ≥4 earlier launches with first buy <30 s, cumulative PnL > 0. Signal = its first buy within 30 s of create. |
| `Sb_R2` | ≥4 earlier launches and ROI ≥ +10%. |
| `Sb_R3` | "Copyability": ≥4 earlier copy events, and the mean return of *copying* the wallet in those events (default policy) is > 0. |
| `Sb_R4` | ≥8 earlier launches, PnL > 0, win rate ≥ 50%. |
| `Sc_2in5s` | ≥2 distinct `Sb_R1` wallets buy within 5 s. The copy exit fires on either wallet's first sell. |
| `Sd_*` | The guru's track-buy range applied to `Sb_R1`: buy size <0.3, 0.3-1, 1-3, ≥3 SOL, or `band` (within ×0.5-×2 of the wallet's earlier median buy). |
| `C0`, `C1` | Controls: copy the first independent buyer after slot 0 within 30 s (`C0`); any wallet with ≥4 earlier early launches, regardless of PnL (`C1`). |

**Entry and exits.**

* **Entry.** The END of slot (signal slot + lag), lag 1 or 2, from the history state. The fill is the exact constant-product buy with a 1.25% fee. We checked it against `curve.py` integer math: entry value is 0.9753 × size, and exit values match to 1e-5 SOL.
* **Our tokens stay outstanding.** A sell of q tokens at history state h returns k/(vt_h − q) − vs_h, minus 1.25%, as in gap-3.
* **Reactive exits** land at the end of (trigger slot + 1):
  * `copy`: the tracked wallet's first sell;
  * `insider`: the first sell by C, B or a transfer recipient;
  * `idle3`: no trade for 3 s, slot-based;
  * `trail20`: value −20% from its running max;
  * TP +20/30/50/100% on position value.
* **Holds and horizon.** Max hold is 60/300/900 s, and a hold always applies. Deep tapes are cut at 300 s, so 900 s holds are truncated there.
* **Front-run variant** (`FRONT:`): exit at the state just before the tracked wallet's sell tx, 0 lag. This is an upper bound and cannot be achieved.
* **Exit grid.** 16 subsets of {copy, insider, idle3, trail20} × 5 TP levels × 3 holds, plus 120 front-run variants: 360 exits.
* **Costs.** 0.004 SOL per landed round trip (columns marked "comp" use 0.01).
* **Not simulated.** Cooldown per wallet cannot be simulated faithfully at our 1.27% launch sampling (consecutive signals from one wallet are about 80 launches apart in real time), so it is left out. Buy-once is enforced.

**Statistics.**

* EV per trade = (proceeds − size − cost) / size, as a weighted mean.
* p-values are one-sided (H0: EV ≤ 0), from the weighted-mean standard error. Benjamini-Hochberg runs across all 32,400 test results.
* Reported rows carry a 2,000-draw bootstrap 90% CI clustered by launch. Buy-once means each row is a distinct launch.

## 1. Wallet discovery

### 1a. Operator wave wallets (top 20 by number of operator launches)

940 wallets qualify (full sample, descriptive). Only 6 are linked through the bundle or funding graph; the rest qualify on exclusivity. 229 of the 940 have positive PnL, and their pooled PnL is **-340 SOL on 2,770 SOL in (-12.3%)**.

| Wallet | Operator (launches) | Op. launches bought | All launches | Exclusivity | Linked | Median slot | Median SOL | PnL (SOL) | ROI | Median hold (s) |
|---|---|---|---|---|---|---|---|---|---|---|
| `MFQKMm2N` | `61xHM4s8` (15) | 15 | 19 | 0.79 | no | 6 | 0.20 | -0.55 | -5.9% | 14 |
| `9jbqS7sC` | `61xHM4s8` (15) | 15 | 20 | 0.75 | no | 10 | 0.10 | -0.33 | -21.9% | 20 |
| `3wj6H6tc` | `61xHM4s8` (15) | 14 | 18 | 0.78 | no | 1 | 0.10 | -0.32 | -23.1% | 56 |
| `62LwMV62` | `61xHM4s8` (15) | 14 | 20 | 0.70 | no | 9 | 0.10 | -0.33 | -23.6% | 55 |
| `5WYnwka6` | `2sXYUSSK` (15) | 13 | 13 | 1.00 | no | 2 | 0.00 | -0.90 | -8.2% | 15 |
| `QqQ8Dkkn` | `8RvtT818` (39) | 13 | 17 | 0.76 | no | 10 | 0.99 | +0.49 | +2.1% | 4 |
| `5ykz5UvP` | `61xHM4s8` (15) | 13 | 21 | 0.62 | no | 1 | 0.10 | -0.29 | -22.6% | 23 |
| `3B8T6sDF` | `2sXYUSSK` (15) | 12 | 12 | 1.00 | no | 2 | 0.98 | -0.77 | -5.5% | 16 |
| `2vPYViLt` | `61xHM4s8` (15) | 12 | 16 | 0.75 | no | 12 | 0.23 | -1.37 | -14.6% | 13 |
| `5JTvwLFy` | `61xHM4s8` (15) | 11 | 15 | 0.73 | no | 12 | 0.57 | -0.68 | -2.4% | 6 |
| `Fr1crPdE` | `2sXYUSSK` (15) | 10 | 10 | 1.00 | no | 4 | 1.05 | -1.65 | -13.5% | 14 |
| `A73sQP8E` | `288q6Cai` (17) | 10 | 10 | 1.00 | no | 2 | 0.99 | -2.66 | -22.6% | 31 |
| `CUwhyA6r` | `288q6Cai` (17) | 10 | 10 | 1.00 | no | 4 | 2.27 | -5.46 | -25.5% | 29 |
| `GC9gKkJj` | `8RvtT818` (39) | 10 | 13 | 0.77 | yes | 220 | 1.47 | +5.86 | +27.3% | 100 |
| `ELmYS1zv` | `61xHM4s8` (15) | 10 | 18 | 0.56 | no | 9 | 0.03 | -0.25 | -41.0% | 8 |
| `9aMZN5sh` | `61xHM4s8` (15) | 10 | 19 | 0.53 | no | 3 | 0.01 | -0.01 | -24.9% | 5 |
| `HT6yg7rz` | `2sXYUSSK` (15) | 9 | 9 | 1.00 | no | 6 | 0.86 | -1.52 | -16.1% | 30 |
| `83XWjY5R` | `288q6Cai` (17) | 9 | 10 | 0.90 | no | 2 | 1.19 | -1.87 | -16.8% | 26 |
| `4mwKAjYj` | `61xHM4s8` (15) | 9 | 13 | 0.69 | no | 16 | 0.09 | -0.90 | -17.0% | 17 |
| `2bd29mNJ` | `61xHM4s8` (15) | 9 | 16 | 0.56 | no | 9 | 0.04 | -0.14 | -38.9% | 3 |

* **Wave wallets cluster on the ops tapes.** Wave detection needs ≥2 launches of one operator. At 1.27% sampling only the 7 pulled operators (15 launches each) and a few reused-bundle components get there, so wave signals are rare: 53 train and 33 test events.
* **These are the guru's "pump wallets".** They buy 0.01-2 SOL at slots 1-16 of every launch of one operator and hold 5-60 s. Most lose money, which is what painting candles costs.
* **The exception.** `GC9gKkJj` (`8RvtT818`, linked, buys at slot ~220 with 1.5 SOL and holds 100 s) made +27% across 10 launches. It is a single wallet on one operator; its walk-forward copy events are inside the `Sa` rows below.

### 1b. Independent pump wallets (top 20 qualified, ranked by per-launch t-stat; full sample, descriptive)

| Wallet | Early launches | All | Median entry (s) | Median slot | Median SOL | PnL (SOL) | ROI | Win | t | Median hold (s) |
|---|---|---|---|---|---|---|---|---|---|---|
| `97zvZgkJ` | 4 | 6 | 4.1 | 14 | 0.367 | +0.88 | +59.4% | 100% | 3.99 | 10 |
| `2pcL7kT6` | 12 | 12 | 0.0 | 0 | 0.591 | +2.22 | +29.2% | 92% | 3.98 | 2 |
| `2Fb17Uz6` | 4 | 7 | 17.1 | 79 | 0.010 | +0.01 | +25.9% | 100% | 3.48 | 11 |
| `DjcoMKQT` | 11 | 20 | 14.8 | 68 | 0.077 | +0.29 | +21.9% | 91% | 3.31 | 33 |
| `DL11CHBX` | 13 | 13 | 0.0 | 0 | 1.185 | +2.93 | +13.9% | 69% | 2.97 | 3 |
| `HXHoJwF5` | 5 | 5 | 5.6 | 25 | 1.975 | +0.62 | +7.5% | 80% | 2.96 | 0 |
| `2mqrindM` | 13 | 13 | 0.0 | 0 | 2.926 | +2.79 | +8.4% | 77% | 2.88 | 0 |
| `2H6DwrqA` | 4 | 7 | 2.3 | 6 | 9.383 | +7.95 | +20.4% | 100% | 2.84 | 3 |
| `9qL4hMBN` | 12 | 12 | 0.4 | 1 | 0.010 | +0.05 | +41.9% | 67% | 2.78 | 16 |
| `4VfKyL8J` | 20 | 46 | 8.7 | 32 | 0.114 | +0.29 | +11.1% | 70% | 2.62 | 32 |
| `SKmksffB` | 8 | 8 | 1.4 | 4 | 0.099 | +0.10 | +11.9% | 75% | 2.57 | 4 |
| `zAnr3Gph` | 5 | 6 | 0.4 | 1 | 0.198 | +0.41 | +22.2% | 80% | 2.55 | 7 |
| `BXB7FF4m` | 19 | 19 | 0.0 | 0 | 1.481 | +1.68 | +7.0% | 68% | 2.48 | 1 |
| `dKMjeGJ4` | 23 | 23 | 0.0 | 0 | 1.778 | +6.87 | +11.6% | 65% | 2.48 | 0 |
| `9Dmsi7mZ` | 16 | 34 | 8.3 | 22 | 0.094 | +0.23 | +10.5% | 75% | 2.44 | 9 |
| `9jQ55cAg` | 7 | 7 | 0.0 | 0 | 0.741 | +0.76 | +8.5% | 86% | 2.38 | 1 |
| `8ptvshQR` | 13 | 13 | 0.0 | 0 | 0.822 | +2.23 | +16.1% | 69% | 2.38 | 3 |
| `haqqiUUX` | 154 | 175 | 5.3 | 18 | 0.741 | +7.61 | +5.4% | 42% | 2.31 | 6 |
| `7iURkMGA` | 6 | 6 | 5.8 | 14 | 1.877 | +1.37 | +9.3% | 67% | 2.30 | 1 |
| `6sfsvDJM` | 8 | 8 | 0.0 | 0 | 0.897 | +0.93 | +10.1% | 62% | 2.28 | 1 |

* **The profitable ones are mostly slot-0 snipers that sell within 0-3 s**, among them `2pcL7kT6`, `DL11CHBX`, `2mqrindM`, `BXB7FF4m` and `dKMjeGJ4`. Their edge is the rank-1 fill, and a copier cannot inherit it: by the end of slot+1 they are often already out.
* **The rest are slower 0.01-0.1 SOL scalpers** (`DjcoMKQT`, `4VfKyL8J`, `9Dmsi7mZ`) with small absolute PnL.
* **The table is consistent with selection noise.** Across all 1,012 candidates, 28 have t > 2 (about 23 expected by chance) and 236 have t < -2. The top 20 is the right tail of a distribution centred below zero.
* `haqqiUUX` is the most active wallet in our data: 154 early entries, +5.4%.

## 2. The top 20 pump wallets on-chain (1,000 most recent signatures each)

We pulled `getSignaturesForAddress` (1,000 signatures) for each wallet, then 150 evenly spaced successful `getTransaction`s, and decoded the pump TradeEvents whose `user` is the wallet (`pull`, `hist` stages; cached in `data/cache/strategy_copy/wallet_hist.jsonl` and `data/cache/rpc/`). Wallets are ranked as in §1b.

| Wallet | Window (days) | Txs/day | Pump trades sampled | Mints | Buy share | Median buy (SOL) | Median entry mcap (SOL) | Our sample, train: launches / PnL | Our sample, test: launches / PnL |
|---|---|---|---|---|---|---|---|---|---|
| `97zvZgkJ` | 3.10 | 323 | 133 | 126 | 63% | 0.342 | 55.1 | 3 / +0.62 | 1 / +0.26 |
| `2pcL7kT6` | 5.76 | 174 | 143 | 143 | 44% | 0.752 | 39.4 | 12 / +2.22 | 0 / +0.00 |
| `2Fb17Uz6` | 0.06 | 16,457 | 98 | 35 | 36% | 0.010 | 43.9 | 1 / +0.00 | 3 / +0.01 |
| `DjcoMKQT` | 0.88 | 1,132 | 122 | 115 | 48% | 0.110 | 42.5 | 2 / +0.07 | 9 / +0.22 |
| `DL11CHBX` | 3.49 | 286 | 88 | 88 | 38% | 2.370 | 43.6 | 10 / +2.59 | 3 / +0.34 |
| `HXHoJwF5` | 14.25 | 70 | 120 | 120 | 52% | 1.778 | 71.0 | 2 / +0.27 | 3 / +0.35 |
| `2mqrindM` | 0.28 | 3,572 | 124 | 57 | 28% | 1.837 | 39.4 | 10 / +2.75 | 3 / +0.04 |
| `2H6DwrqA` | 1.06 | 941 | 120 | 47 | 12% | 6.914 | 59.3 | 2 / +3.68 | 2 / +4.27 |
| `9qL4hMBN` | 5.76 | 174 | 149 | 149 | 38% | 0.010 | 50.4 | 11 / +0.05 | 1 / -0.00 |
| `4VfKyL8J` | 1.02 | 982 | 121 | 118 | 50% | 0.088 | 40.7 | 11 / +0.18 | 9 / +0.11 |
| `SKmksffB` | 1.98 | 504 | 148 | 148 | 16% | 0.099 | 44.5 | 7 / +0.08 | 1 / +0.02 |
| `zAnr3Gph` | 2.80 | 357 | 54 | 54 | 39% | 0.494 | 54.2 | 3 / +0.30 | 2 / +0.11 |
| `BXB7FF4m` | 1.20 | 834 | 146 | 146 | 24% | 1.481 | 56.9 | 10 / +1.80 | 9 / -0.12 |
| `dKMjeGJ4` | 0.39 | 2,553 | 115 | 109 | 25% | 1.778 | 46.7 | 12 / +2.43 | 11 / +4.44 |
| `9Dmsi7mZ` | 0.79 | 1,262 | 114 | 112 | 49% | 0.127 | 42.4 | 8 / +0.02 | 8 / +0.21 |
| `9jQ55cAg` | 2.85 | 351 | 105 | 105 | 53% | 2.469 | 49.7 | 4 / +0.22 | 3 / +0.54 |
| `8ptvshQR` | 1.40 | 716 | 144 | 144 | 44% | 1.076 | 40.0 | 10 / +1.67 | 3 / +0.56 |
| `haqqiUUX` | 0.12 | 8,072 | 119 | 100 | 47% | 0.395 | 47.6 | 91 / +5.40 | 63 / +2.21 |
| `7iURkMGA` | 0.04 | 28,365 | 0 | 0 | 0% | - | - | 6 / +1.37 | 0 / +0.00 |
| `6sfsvDJM` | 0.40 | 2,474 | 149 | 127 | 32% | 1.067 | 34.5 | 8 / +0.93 | 0 / +0.00 |

* **These are high-frequency bots, not the guru's ~8 tokens/day rugger wallets.** The median rate is 887 txs/day (range 70-28,365), so 1,000 signatures cover 1 hour to 14 days. The guru's qualification ("~57 buys in 7 days") would reject all but `HXHoJwF5`.
* **They enter at a median market cap of 44 SOL** (about $4.5-5k), inside the guru's "2k-9k" band. Entry market cap does not separate good wallets from bad ones.
* **No out-of-sample PnL from the histories.**
  * Sampling every k-th tx breaks buy/sell pairs per mint. Most windows also open with inventory: buy shares are 12-63%, and several wallets sell far more than they buy in the window, probably because tokens arrive by transfer or through venues we do not decode.
  * So the Horvitz-Thompson net-flow estimates in `copytrade_wallet_hist.json` (e.g. `BXB7FF4m` +769 SOL, `97zvZgkJ` -53 SOL in the window) are **not** PnL. We do not use them.
  * The histories confirm activity and style only.
* **Our own test-period launches** (last column) total about +13.6 SOL over 120 launches for these 20 wallets. The top 20 was ranked on the full sample, test included, so this is not out of sample. The point-in-time copy results for the same kind of wallet are in §4 (`Sb_*`), and they are negative.
* `7iURkMGA` sends about 28k txs/day, almost all of them failed (a spray bot). Only 3 of its 1,000 signatures were successful.

## 3. Signal frequency (test window, scaled to 46k creates/day)

| Signal | Test events | ≈ trades/day (gap-3 bundled class + deep unbundled) | Bundled class per day, estimated from deep |
|---|---|---|---|
| `C0` first independent buyer | 293 | 8,600 | 10,400 |
| `Sb_R1` scored pump wallet | 249 | 6,200 | 9,100 |
| `Sb_R3` copyability score | 184 | 4,400 | 6,400 |
| `Sc_2in5s` | 183 | 3,900 | 6,900 |
| `Sd_d_1-3` (1-3 SOL buys) | 96 | 2,100 | 3,300 |
| `Sa_N150` wave wallet | 33 | 460 | 560 |

* **Scored pump wallets concentrate on bundled launches.** Only 17 of the 117 unbundled deep launches carried an `Sb_R1` signal, against 97 of 135 bundled ones.
* **Capacity is not the constraint**; signals fire on most bundled launches. The two frequency estimates differ by about 1.5× because the gap-3 test window (3.2 days) and the deep window (2 days) differ in activity.

## 4. Policy results (test set)

### 4a. Default copy policy (lag 1, 0.25 SOL, sell 1 slot after the tracked wallet's first sell, max 300 s)

| Signal | n test | EV/trade [90% CI] | EV at 0.01 SOL cost | Hit | Oracle ceiling | Median entry premium vs signal price | Train EV |
|---|---|---|---|---|---|---|---|
| `C0` first indep. buyer (control) | 274 | -7.5% [-10.4, -4.6] | -9.9% | 19% | +23.5% | +0.2% | -8.5% |
| `C1` active ≥4 (control) | 251 | -8.4% [-11.2, -5.8] | -10.8% | 21% | +26.8% | +0.5% | -6.8% |
| `Sb_R1` (task definition) | 234 | **-7.0% [-9.5, -4.5]** | -9.4% | 23% | +27.1% | +0.6% | -7.0% |
| `Sb_R2` ROI ≥10% | 135 | -9.4% [-13.9, -5.0] | -11.8% | 25% | +28.1% | +0.6% | -5.6% |
| `Sb_R3` copyability | 171 | -9.4% [-13.3, -5.4] | -11.8% | 26% | +31.8% | +0.5% | -6.7% |
| `Sb_R4` ≥8, win ≥50% | 151 | -8.8% [-12.4, -5.5] | -11.2% | 28% | +28.9% | +0.9% | -6.2% |
| `Sc_2in5s` | 170 | -6.4% [-9.4, -3.7] | -8.8% | 20% | +30.3% | +0.4% | -4.2% |
| `Sd_band` (wallet's own range) | 220 | -7.7% [-10.4, -5.1] | -10.1% | 24% | +27.7% | +0.7% | -6.9% |
| `Sd` <0.3 SOL | 191 | -6.3% [-8.9, -3.6] | -8.7% | 26% | +29.7% | +0.5% | -3.7% |
| `Sd` 0.3-1 SOL | 174 | -7.4% [-9.6, -5.3] | -9.8% | 20% | +27.6% | +0.7% | -7.4% |
| `Sd` 1-3 SOL | 89 | -12.3% [-17.7, -7.4] | -14.7% | 21% | +24.6% | +1.7% | -8.6% |
| `Sd` ≥3 SOL | 12 | -15.6% [-39.7, +3.2] | -18.0% | 44% | +22.6% | +6.7% | -16.7% (n=6) |
| `Sa_N10` wave (all tapes) | 20 | -22.6% [-32.5, -11.3] | -25.0% | 10% | +67.4% | +5.2% | -15.7% |
| `Sa_N50` wave | 17 | -20.1% [-29.8, -10.2] | -22.5% | 9% | +35.4% | +5.1% | -32.6% (n=15) |
| `Sa_N150` wave | 18 | -20.1% [-29.4, -10.8] | -22.5% | 9% | +34.4% | +4.4% | -31.1% (n=16) |

Lag 2 changes these by -0.5 to +1.2 points. Most signals fire at slot 0-1, and the curve rarely moves much in the extra slot.

### 4b. Top 15 realistic configs on the test set (population weights, n ≥ 20, duplicates collapsed)

*Selected on the test set itself, so these are optimistic. Compare §4d.*

| # | Config (signal / lag / size / exits / TP / max hold) | n | EV [90% CI] | EV, 0.01 cost | Hit | BH q | Train EV |
|---|---|---|---|---|---|---|---|
| 1 | `Sb_R3` / 1 / 0.5 / idle3 / +30% / 60 s | 171 | -2.5% [-6.3, +1.4] | -3.7% | 31% | 1.00 | -2.5% |
| 2 | `Sb_R3` / 1 / 0.5 / copy+idle3+trail20 / +20% / 60 s | 171 | -2.6% [-5.0, -0.2] | -3.8% | 31% | 1.00 | -3.2% |
| 3 | `Sb_R3` / 1 / 0.5 / idle3 / +20% / 60 s | 171 | -2.6% [-5.7, +0.5] | -3.8% | 35% | 1.00 | -2.0% |
| 4 | `Sb_R4` / 1 / 0.5 / idle3 / +30% / 300 s | 151 | -2.7% [-6.6, +1.6] | -3.9% | 29% | 1.00 | -2.4% |
| 5 | `Sb_R3` / 1 / 0.5 / copy+insider+idle3 / +20% / 60 s | 171 | -2.7% [-5.0, -0.4] | -3.9% | 29% | 1.00 | -3.2% |
| 6 | `Sb_R3` / 1 / 0.5 / copy+insider+idle3+trail20 / +20% / 60 s | 171 | -2.7% [-5.0, -0.5] | -3.9% | 28% | 1.00 | -3.4% |
| 7 | `Sb_R4` / 1 / 0.5 / idle3 / +30% / 60 s | 151 | -2.8% [-6.7, +1.6] | -4.0% | 29% | 1.00 | -2.4% |
| 8 | `Sb_R3` / 1 / 0.5 / insider+idle3+trail20 / +30% / 60 s | 171 | -2.8% [-5.8, +0.3] | -4.0% | 27% | 1.00 | -3.4% |
| 9 | `Sb_R3` / 1 / 0.5 / copy+insider+idle3 / +30% / 60 s | 171 | -2.8% [-5.3, -0.4] | -4.0% | 28% | 1.00 | -3.5% |
| 10 | `Sb_R4` / 1 / 0.5 / idle3+trail20 / +30% / 60 s | 151 | -2.8% [-6.1, +0.8] | -4.0% | 28% | 1.00 | -2.4% |
| 11 | `Sb_R3` / 1 / 0.5 / copy+insider+idle3+trail20 / +30% / 60 s | 171 | -2.8% [-5.3, -0.5] | -4.0% | 27% | 1.00 | -3.7% |
| 12 | `Sb_R3` / 1 / 0.5 / insider+idle3+trail20 / +20% / 60 s | 171 | -2.9% [-5.4, -0.4] | -4.1% | 30% | 1.00 | -3.2% |
| 13 | `Sb_R3` / 1 / 0.5 / copy+insider+trail20 / +30% / 300 s | 171 | -2.9% [-5.7, -0.1] | -4.1% | 30% | 1.00 | -4.2% |
| 14 | `Sb_R3` / 1 / 0.5 / idle3 / +30% / 300 s | 171 | -2.9% [-6.8, +1.1] | -4.1% | 31% | 1.00 | -2.7% |
| 15 | `Sb_R3` / 2 / 0.5 / copy+insider+idle3 / +30% / 60 s | 171 | -2.9% [-5.3, -0.6] | -4.1% | 24% | 1.00 | -4.7% |

* **What the best realistic policies have in common:**
  * the largest size (0.5 SOL), because the fixed 0.004 SOL cost is 0.8% at 0.5 SOL against 4% at 0.1 SOL;
  * a small TP (+20/30%);
  * the 3-second idle exit;
  * a 60 s max hold.
* Their hit rates are about 30%.
* The "copy the wallet's sell" exit is not what saves them. Fast scalping exits on the curve do, as in the gap-3 exit table.
* **Across all tapes** (unit weights, including ops), the best realistic row is `Sa_N10` / TP +30% / 900 s at **-1.0% [-20.5, +18.3]** (n=20). It is pure noise, and its train EV is -19.5%.

### 4c. Front-run sell upper bound (0-lag exit on the tracked wallet's sell; cannot be achieved)

| Config | n | EV [90% CI] | EV, 0.01 cost | BH q | Train EV |
|---|---|---|---|---|---|
| `Sd_d_1-3` / 1 / 0.5 / FRONT:copy+idle3 / +100% / 60 s | 89 | **+0.6% [-3.1, +4.3]** | -0.6% | 1.00 | -1.0% |
| `Sd_d_1-3` / 1 / 0.5 / FRONT:copy+idle3 / none / 60 s | 89 | +0.4% [-3.2, +4.1] | -0.8% | 1.00 | -1.2% |
| `Sd_d_1-3` / 2 / 0.5 / FRONT:copy+idle3 / +100% / 60 s | 89 | +0.2% [-2.9, +3.3] | -1.0% | 1.00 | -1.8% |
| same, all tapes (n=96) | 96 | +1.6% [-2.3, +6.2] | +0.4% | 1.00 | +0.2% |
| `Sc_2in5s` / 1 / 0.5 / FRONT:copy+idle3+trail20 / +100% / 60 s, all tapes | 183 | +0.1% [-1.9, +2.5] | -1.1% | 1.00 | -1.0% |

* Only **5 of 23,760** population configs with n ≥ 20 have a positive test EV. All 5 are front-run variants of one signal (`Sd_d_1-3`), and their train EVs are negative.

### 4d. Walk-forward selection (best config on train → its test result)

| Family | Train-selected config | Train EV (n) | Test EV [90% CI] (n) |
|---|---|---|---|
| Overall best on train | `Sd_d_lt0.3` / 1 / 0.5 / idle3 / +20% / 60 s | +0.2% (303) | **-3.2% [-6.1, -0.4]** (191) |
| `Sb_R1` | idle3+trail20 / +20% / 60 s | -2.3% (384) | -3.7% [-5.9, -1.5] (234) |
| `Sb_R3` | idle3 / +20% / 300 s | -1.9% (267) | -3.1% [-6.4, +0.2] (171) |
| `Sb_R4` | idle3 / +30% / 300 s | -2.4% (222) | -2.7% [-6.6, +1.6] (151) |
| `Sc_2in5s` | lag 2, idle3 / +20% / 60 s | -2.0% (264) | -5.0% [-8.1, -1.5] (170) |
| `Sb_R2` | idle3+trail20 / +50% / 60 s | -0.7% (227) | -8.0% [-11.5, -4.5] (135) |
| `C0` control | idle3 / +50% / 60 s | -2.8% (497) | -6.1% [-8.6, -3.5] (274) |
| `Sa` (all tapes) | lag 2, idle3+trail20 / 60 s | -1.0% to -2.7% (40-53) | -11.9% to -14.3% (20-33) |

## 5. Why copying loses

On 26,758 (wallet, launch) pairs of early independent wallets, unweighted, all tapes:

| Quantity | Mean per trade |
|---|---|
| Leader's own ROI on the same launches | **+0.1%** |
| Copy, exit 1 slot after the leader's sell, 300 s cap | **-13.4%** |
| Copy, front-running the leader's sell (0 lag) | -9.1% |
| Copy, best simple scalp (idle 3 s / trail 20% / TP +20%, 60 s) | -4.7% |
| Median entry premium over the leader's fill slot | +0.44% |

* **The leaders themselves are break-even.** They earn the rank-1 fill and pay one fee each way. The copier pays two more fees (2.5% of size), the landing cost (0.8-4% of size), and the impact of every trade between the leader's slot and ours.
* **We sell after the leader has dumped on us.** The median leader holds **14 s**, and 27% are out within 5 s. When the copied wallet sells, we are typically the last holder still in.
* **Persistence of copy return** (242 wallets with ≥4 train and ≥3 test events):

  | Train tercile by copy return | Train | Test copy return | Test own ROI |
  |---|---|---|---|
  | Bottom | -19.9% | -20.8% | -13.3% |
  | Middle | -7.2% | -9.8% | -2.2% |
  | Top | +7.4% | **-8.2%** | -2.7% |

  Ranking by the wallet's *own* ROI instead: the top tercile goes from +13.5% to -0.6% own ROI, and -8.9% copy return. Losers stay losers, while winners regress to the population mean. The pattern matches gap-1's operator persistence: skill exists, but at the wrong level.
* **Wave wallets are the operator's cost centre.** They buy 0.01-2 SOL into the operator's own launch to paint candles. Following them buys into a curve the operator is about to sell into. Median entry premium +4-5%, test EV about -20%.

## 6. Honest reading and the most promising lead

* **There is no copy-trading edge in this data.** We checked 15 signal definitions, including the guru's track-buy range, his two-wallet confirmation, and a direct "copyability" score. None beats the dumbest control (copy the first independent buyer) by more than the noise, and every realistic configuration is negative on test. After BH correction over 32,400 configurations nothing is significant, positive or negative. The negative default results are individually far from zero (CIs exclude 0).
* **The ceiling is large** (+24-33% per trade with a perfect exit after a slot+1 entry). It sits in short, early run-ups that no observable trigger we tested (wallet sells, insider sells, idle, trailing, TP) captures before the reversal.
* **Most promising lead: `Sd_d_1-3` with a front-run of the tracked wallet's sell plus idle 3 s.** Copy scored wallets whose buy is 1-3 SOL, and exit in the same slot as their sell.
  * Test EV **+0.6% [-3.1, +4.3]** (n=89; +1.6% [-2.3, +6.2] on all tapes); train EV is -1.0%.
  * It is still not significant (q = 1.00), and it assumes a sell landing *before* the leader's own sell in the same slot.
  * That requires predicting the sell, for example from the leader's pending tx on a shred feed or a fixed hold-time habit.
  * Testing it needs per-wallet hold-time predictability (§7). Without that, the same signal at a 1-slot lag is -3.9% at best.
* **Second lead: use the score negatively.** The bottom tercile of copy return persists out of sample (-20% → -21%). As an *anti-signal*, "skip launches where a known loser-copy wallet is buying first" could be a filter for the other strategies (S1-S4). It is not a strategy on its own.

## 7. What a live system would need (if pursued)

* **Wallet-score refresh.**
  * Scores have to be rebuilt from a full trade feed, not a 1.27% sample. At 46k creates/day the active-wallet universe is tens of thousands, and the top 1,000 sniper wallets touch most bundled launches.
  * With a full feed, a 4-launch history accrues in hours, not days. Refresh **hourly**, with a horizon lag (score only launches whose 15-minute window has closed), as done here.
  * Drop wallets after a few consecutive copy losses. That is the guru's max-loss counter, and §5 shows losers persist.
* **Signal latency.**
  * On the **free feed** (RPC `logsSubscribe`/`blockSubscribe` at confirmed), a copied buy is visible about 1 slot after it lands. Our entry then lands at the end of slot+1 to slot+2, which is what we simulated (lag 1-2). Lag 2 costs only about 0.5 points more than lag 1 here.
  * A **shred feed** (ShredStream / Yellowstone at processed) sees the leader's tx in its own slot, so a same-slot or slot+1 entry is realistic. It does not make the front-run sell possible: you see the leader's *sell* only when it is already in the block you are trying to beat.
  * The front-run bound would need the leader's sell to be predictable (hold-time regularity, or watching its pending transactions in shreds). It is worth at most a few points, and even the upper bound is not significant.
* **Costs.**
  * Size at 0.5 SOL or more, so the 0.004 SOL landing cost is under 1%.
  * At 0.01 SOL (competitive landing) every row loses another 1.2 points at 0.5 SOL, or 2.4 at 0.25 SOL.

## Caveats

* **Sampling.** Our tapes are about 1.3% of launches, so a wallet's 4-launch history spans days here, against hours on a full feed. Wave-wallet detection is limited to operators with ≥2 sampled launches; ops tapes make up most `Sa` events, and n is small (20-33 test events).
* **Horizons.** Deep tapes are cut at 300 s, so 900 s holds there are truncated; this affects only the slow-exit rows. Gap-3 tapes are 900 s.
* **Regimes.** Train is mostly pre-Oct-9 (400 ms slots); test is mostly 200 ms slots. Alpenglow (~Oct 21) is not covered.
* **Population weights are approximate.** Gap-3 and deep sampling fractions come from their frames (781k creates / 9,905 sampled; 93k / 1,496 light / 400 random-arm SOL-quoted). Unweighted "all tapes" results tell the same story.
* **Modelling simplifications.** No reaction from others to our own trades. Same-slot sells by other holders are not re-ordered. Our sell lands at the end of the slot after the trigger. All of these are optimistic for the copier.
* **Leader PnL** is marked to market at the tape horizon for open positions.

## Reproduce

```
.venv/bin/python -I analysis/scripts/strategy/copytrade.py discover   # ledger + wallet tables (~3 s)
.venv/bin/python -I analysis/scripts/strategy/copytrade.py pull       # top-20 histories (RPC, cached)
.venv/bin/python -I analysis/scripts/strategy/copytrade.py hist
.venv/bin/python -I analysis/scripts/strategy/copytrade.py sim        # walk-forward + 32,400 configs (~3 min)
.venv/bin/python -I analysis/scripts/strategy/copytrade.py report     # bootstrap CIs, summary json
```
