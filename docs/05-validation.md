# Validation: does the edge exist, and under what conditions?

Status: base rates and competition measured on real data (this document); operator-level walk-forward
backtest in progress (section 5 is filled in when it lands). All numbers are from our own on-chain
pulls on 2026-10-12..14 via public RPC and the pump.fun APIs, reproducible with the scripts in
`analysis/`. Nothing here uses the guru's numbers.

## 1. What we measured and how

| Dataset | Size | Source | Purpose |
|---|---|---|---|
| Create index | 101,457 mint-authority signatures, 93,191 successful creates, 48 h | `getSignaturesForAddress` on the pump mint authority | Population frame; creation rate and timing |
| Light sample | 1,500 creates drawn uniformly at random (1,496 decoded, 1,215 SOL-quoted) | `getTransaction` with v1 support, CreateEvent/TradeEvent decoded from CPI logs; `/coins/mints` snapshots | Population facts: dev buys, mayhem share, quote mints, creator ≠ signer |
| Deep sample, random arm | 396 SOL-quoted tokens (instant graduations removed) | `swap-api.pump.fun/v2/coins/{mint}/trades`, first 300 s plus latest trades | Unbiased base rates and P&L |
| Deep sample, "interesting" arm | 91 tokens with ATH ≥ $20k or graduated | same | Outcome-selected: describes winners only, never used for hit rates |
| Competition study | 3,600 random creates over 24 h, 7,073 full blocks (`getBlock`, v1) | Independent agent, separate code path | Who else buys in slot 0 and slot+1, at what price and cost |
| Funding study | 180 creators (100 random + enriched arms), 126 traced | `getSignaturesForAddress` histories, 392 verified address labels | Funding sources, fresh wallets, amounts, delays |

Decoder verification: 256/256 on-chain TradeEvents matched the API trade feed (signature, wallet,
side, amounts; post-trade market cap within 2e-16 relative error); ATH within 0.12% of the API; the
curve math reproduces real dev buys, buys and sells exactly (see `crates/pump-codec` tests).

Definitions: **eos0** = market cap after the last trade of the creation slot (the guru's idealised
entry: after the dev buy and the bundle). **Bundled** = at least one non-dev wallet bought in slot 0
and slot-0 buys total ≥ 1 SOL. **Natural stop** = the curve floor (27.96 SOL market cap) as the exit
when the take-profit is never hit. All market caps in SOL.

## 2. Base rates (random arm, n = 396)

| Metric | All | Bundled (27%) | Not bundled |
|---|---|---|---|
| ATH ≥ 1.5× eos0 | 18.7% | 22.4% | 17.3% |
| **ATH ≥ 2× eos0** | **9.3%** (95% CI 6.5-12.2) | 9.3% | 9.3% |
| ATH ≥ 3× eos0 | 4.5% | 5.6% | 4.2% |
| TP 2× hit **before** a -30% drawdown | 6.8% | 7.5% | 6.6% |
| Median eos0 market cap | 28.4 SOL | **43.7 SOL** | 28.1 SOL |
| Median slot-0 SOL / wallets | 0.24 / 1 | 7.5 / 4 | 0.10 / 1 |
| Slot-0 bag half sold within 60 s | 61% | **81%** (median 8 s) | 52% |
| Retrace ≥ 80% of run-up within 5 min | 30% | 65% | 16% |
| Single-slot dump ≥ 30% of supply | 5.3% | 9.3% | 3.8% |
| Graduated | 2.5% | 1.9% | 2.8% |

Naive strategy (buy 0.3 SOL at eos0, sell at TP, otherwise exit at the last price, 1.25% fees each
way, own price impact, no tips):

| TP | All | Bundled | Not bundled |
|---|---|---|---|
| 1.5× | -14.7% per trade | -20.3% | -12.6% |
| **2×** | **-18.6%** [-23, -14] | **-27.1%** | -15.5% |
| 3× | -19.4% | -26.9% | -16.6% |

Readings:

* **There is no blanket edge.** Buying every launch, or every bundled launch, at the end of slot 0
  loses 15-27% per trade. The guru's rule set applied blindly is negative.
* **A bundle is not a buy signal.** Bundled and unbundled launches hit 2× equally often (9.3%); the
  bundle only raises the entry (43.7 vs 28.1 SOL), which deepens the natural stop from ≈ 0 to -36%.
* **Bundles are the exit liquidity hunters.** The slot-0 wallets have sold half their bag after a
  median 8 s, at 0.87× our entry, after a peak of only 1.08× our entry. On the typical bundled launch
  the rugger dumps on the snipers, exactly the "farming" he warns about.
* **One slot late barely changes the odds** (2× from the end of slot+1: 8.8% vs 9.3%), because only
  22.5% of launches trade at all in slot+1.
* **The "first candle ≤ $15k" filter no longer bites** (99.2% of launches pass it), because bundles
  are smaller than the 4 × 10 SOL he describes.
* **Exclude by construction:** mayhem-mode coins (28% of the random arm, 2B supply, agent-traded, the
  price can fall below the floor, naive P&L -35%), non-SOL quotes (19% of creates), instant
  graduations (~1%).

## 3. Break-even hit rate as a function of the entry

With 1.25% fees per side, TP 2×, and the natural stop, the break-even probability of hitting the TP
is `p* = L / (W + L)` where W ≈ +0.95 and L is the floor loss `1 - (27.96/eos0) × 0.975`:

| eos0 market cap | Floor loss | Break-even hit rate (no tips) | With 0.01 SOL tip on 0.3 SOL |
|---|---|---|---|
| 28 SOL (unbundled, at the floor) | ≈ -2.5% | ≈ 3% | ≈ 6% |
| 36 SOL | -24% | 20% | 23% |
| **44 SOL (median bundled)** | **-38%** | **28%** | **31%** |
| 57 SOL (p75 bundled) | -52% | 35% | 38% |
| 75 SOL (≈ $8k) | -63% | 40% | 43% |
| 140 SOL (≈ $15k) | -80% | 46% | 48% |

Measured hit rate (9.3%) versus break-even at the median bundled entry (28-31%): a factor of three
short. The guru's "3 of 10" rule is roughly break-even at a typical entry and a loss at a large one;
his implicit assumption that entering near the floor makes losses small is true only when nobody is
ahead of you, which is exactly when there is also no bundle to ride.

Being later than first costs more: the competition study measured the fill premium over the
post-bundle price at +1.1% (median) for the first reactive buyer, +10.8% for the third, +15.8% for
the fifth (+3.6% / +15.9% / +32.6% on CEX-funded bundled launches). Each 10 points of premium adds
roughly 4-5 points to the break-even hit rate.

## 4. Competition (independent study, 3,600 creates over 24 h)

| Launch class | Independent slot-0 winner | ≥ 3 winners | Winner by slot+1 | Attempting wallets (mean) |
|---|---|---|---|---|
| All SOL-quoted (n=2,925) | 22.1% | 6.4% | 34.6% | 1.05 |
| Bundled (n=570) | 50.9% | 18.9% | 74.7% | 2.95 |
| **CEX-funded fresh creator + bundled (n=82)** | **62.2%** | 18.3% | **85.4%** | 3.43 |

* The guru's target class is the most contested class on the chain. Professional snipers (78 wallets
  won on ≥ 10 launches each) make 86% of winning slot-0 buys; the fastest land 5-9 transactions after
  the create.
* Winning bids: first winner median 0.0034 SOL (p75 0.0165, p90 0.068); the earlier of two winners
  had the higher bid 73% of the time.
* Of wallets attempting a slot-0 buy, 46% won, 22% failed on slippage (their max cost was set below
  the post-bundle price), 29% were on-chain guard aborts from wrapper programs.
* Creates in the last 5% of a block (6.8% of creates) are unreachable in slot 0; Europe-led slots
  draw 24% reactive buys versus 14% North America and 9% APAC, consistent with where the competition
  sits.

## 5. Operator-level results (walk-forward)

Sources: `docs/research/gap-1.md` (16 days, 848k mint-authority signatures indexed, 9,905 decoded
creates in a 1.27% uniform sample, 7,270 extra coins from 196 expanded wallets), `gap-3.md`
(675 bundled launches with full trade tapes to +900 s), `gap-4.md` (CEX hot wallets and the
fresh-wallet trigger). Code under `analysis/scripts/gap1`, `gap3_exits`, `gap4`. All hit rates
use a realistic entry: after the creator's bundle and the median third-party slot-0 buyer, with
the rank premium from §4.

### 5.1 Who the operators are

| Population (16 days) | Count |
|---|---|
| Creates per day | 49.4k |
| Creator wallets with ≥5 launches | ≈8,900 (they make 68% of all launches) |
| Creator wallets with ≥100 launches | ≈1,390 |
| Creator wallets with ≥1,000 launches | 52 |
| Operator units with ≥15 scored launches | 129 |

Most high-volume wallets are dust-funded spam farms. Funder-linked groups (rotating creators) and
CEX amount-band groups hit +100% at 0-5%.

### 5.2 Hit rate at TP +100% by class (in-sample, Wilson 95%)

| Class | n | hit +100% |
|---|---|---|
| All creates (base rate) | 9,905 | 6-10% |
| ≥1 wallet bundled next to the create | 1,165 | 17.0% [14.9, 19.3] |
| Fresh + bundled + exchange-funded (the guru's archetype) | 171 | 18.1% [13.1, 24.6] |
| Mayhem-mode coins (2B supply, AI agent trades 1B for 24 h, no floor) | sample | 28% |
| Best single operator (in-sample, 6/17) | 17 | 35.3%, Wilson lower bound 17.3% |

### 5.3 Walk-forward (discover on days 1..t, score on t+1..t+7)

| Arming rule | Out-of-sample hit +100% | Launches qualifying per day |
|---|---|---|
| Every operator, no filter | 6-10% | thousands |
| Guru's rule (≥3 of last 10 hit +100%) | 18.6-21.8% (Wilson upper 32.6-35.3%) | 50-90 among expanded wallets |
| Top tercile by past hit rate | 15-16% (bottom tercile ≈1%) | hundreds |

Selection persists (2.2× lift over base) but tops out far below the 43-48% break-even that a
TP +100% strategy needs when a miss rides down to the floor.

### 5.4 What the tapes say about misses and exits (gap-3, n = 675 bundled launches)

| Fact | Value |
|---|---|
| Bundle shape | median 3 wallets × 1.28 SOL; post-bundle mcap 40.3 SOL |
| First bundle-wallet sell | median 35 slots / 9 s (6 s after Oct 9); that slot dumps 67% of the bundle's tokens |
| Creator's first sell | median 23 slots / 6 s |
| Bundle sell in slot 0 / creator sell within 3 slots | 0.9% / 7.1% |
| Peak on a miss | median 1.46× entry |
| Loss on a miss, holding to +900 s | 20-40% for a typical bundle (63-80% only above ~75 SOL post-bundle mcap) |
| Loss on a miss, exit at first insider sell | median ≈0%, but it forfeits 50-80% of the winners |
| Within-operator CV of post-bundle mcap and bundle SOL | 4-15% (a ±10% fingerprint band is a strong match) |

Consequence: with insider-sell exits the break-even hit rate at TP +100% falls to roughly 22-27%
(loss on a miss ≈23%, round trip ≈4% of stake), not 43-48%. The best classes above (17-18%) and
the top tercile of operators (15-16%) still sit below it, so the open question is not speed or
selection alone but the take-profit level and the exit rule. That is what `docs/04-strategy.md`
settles with the policy sweep, the operator-conditioned walk-forward and the copy-trade study.

### 5.5 Strategy search outcome (strategy-*.md, summarised in `docs/04-strategy.md`)

| Family | Best out-of-sample number | Verdict |
|---|---|---|
| Guru's rule (block-0 entry, TP +100%) and his bot's exits | −10.5% / −1% per trade | loses |
| Survivor, dip, buy-on-dev-sell, snipe-delay entries | −2% to −17% | lose |
| Copy-trading pump wallets or wave wallets | −3% to −20% on 905 fresh launches | loses |
| Mayhem coins | −11% to −22% | excluded |
| Per-operator arming (his selection idea) | no lift on the fresh holdout (−0.2 points) | logged only |
| **Rank-1 micro-scalp** (his entry, our 1-3 s exit) | **+2.5% to +3.7%** on four independent sets (n = 915, 427, 222, 150); +9 to +12% on the hot third, −0.5% on the rest; ≈ 0 at rank 2 or under stacked realistic fill costs | the only edge; a latency race |

### 5.6 The funding trigger as a pre-arm (gap-4)

| Rule | Wallets per day | Create within 60 min |
|---|---|---|
| Any SOL from the 7 CEX hot wallets to a never-used wallet | ≈12,550 | 3.3% [1.9, 5.6] |
| Same, amount 0.9-15 SOL | ≈5,000 (≈210/h) | 9.7% [5.5, 16.6] (keeps 11 of 12 creators) |

Of the creates that follow a fresh CEX withdrawal, 50% come within 10 min and 75% within 60 min.
The rule only sizes the watchlist; the buy decision is made at the create from the operator
fingerprint and the slot-0 bundle shape. 2.955 SOL is a number the rugger typed, not a fee artefact
(Binance and Bitget charge 0.001, KuCoin 0.008; OKX and Bybit deliver round amounts).

## 6. What the strategy must therefore be (preview)

1. Never blanket-snipe. Arm only operators whose **out-of-sample** lower-bound hit rate exceeds the
   break-even for *our* expected entry (eos0 plus rank premium plus tips).
2. Prefer exits driven by the bundle's own behaviour (first bundle-wallet sell, big-buy trigger,
   no-activity timeout) over a fixed TP: the median bundled winner peaks at 1.5× before the bag exits
   at 12 s, so a 2× fixed TP misses most of what is there.
3. Condition entry on the observed slot-0 bundle: skip when eos0 is above the operator's historical
   range; cap fills at ≈ +10% over the post-bundle price.
4. Measure, continuously: operator persistence, our landing rank, conditional-on-our-presence dumps.

Full strategy: `docs/04-strategy.md`.
