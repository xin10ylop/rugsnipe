# 16. Who he follows: operators by their bundle wallets (2026-10-25)

The question: how does the copied wallet pick the coins outside our rule? (Our rule: a tool host, a template dev buy,
post-bundle <= 40 SOL, a deployer he traded in the last 7 days.) Scripts: `analysis/research/2026-10-25/follow/`.
The data covers every create of Oct 23 and Oct 24 00-12 UTC (60,000). We pulled the create slot of every coin with a dev buy of
0.25 SOL or more outside the rule (16,862) and of every tool-host template coin (4,885). Full first-12-slot tapes for every coin any
setting below would buy. His attempts are his transactions on each mint, landed or failed: his failed buys land too, rejected by his
own guard, and they are 74% of his buy transactions this month. The model is fit on Oct 23 00-12 and scored on Oct 23 12-24 and
Oct 24 00-12.

## 1. What his outside trades are

- **713 attempts in 36 h**, 34% of his 2,103. He fires them at the same slots as his rule coins (51% at create + 0, 46% at
  + 1) and with the same stake (0.59 SOL median). They are create-time decisions.
- **They are a small part of his profit.** His 201 closed outside trades made +3.5 SOL; his 366 rule trades made +23.9 SOL.
  ipfs.io coins made +3.7% a trade, other hosts +1.1%, and tool hosts off the template -0.7%.
- **His visible ceiling is about 75%.** On the rule coins of the deployers he trades most (10+ trades in 7 days) we see him
  attempt 76%. A rule that reaches 70-75% is "he always takes it".

## 2. What it is not

Each of these was fit on Oct 23 00-12 and scored on the later periods (precision = his attempt rate on the selected coins,
recall = the share of his outside picks selected):

| signal at the create | precision | recall |
|---|---|---|
| exact dev buy and host, n >= 3, rate >= 50% on the fit period | 80% / 92% | 10% / 8% |
| the same, broader (n >= 5, rate >= 20%) | 21% / 11% | 28% / 17% |
| tool settings (dev buy, Jito tip, compute-unit price) | 39% / 24% | 22% / 19% |
| the creator traded by him before (his month) | 10% / 9% | 36% / 37% |
| the creator attempted by him earlier today | 22% / 20% | 41% / 22% |
| a first fresh tweet (dev >= 0.95 SOL) | 9.4% against 4.0% without | - |
| the deployer's funder funded an earlier pick | 25-44%, partial data | - |

Fingerprints change from day to day. 64% of his outside picks come from creators he never traded. No public feature of the
coin explains his picks.

## 3. What it is: the bundle wallets

An operator launches from a fresh deployer each time but buys its bundle with the same few wallets. Example: dev at index 54,
then three bundle buys of ~1 SOL each at 55-57 (`5sWHrtKx`, `2rXT8S98`, `C57Ax2Bc`), and all three sell together at slot +12.
Grouped by those wallets:

| operator (a bundle wallet) | launches | he attempted |
|---|---|---|
| `5sWHrtKx` (+2) | 236 rule coins | 98% |
| `5Eb5rURZ` (+2) | 87 rule coins | 92% |
| `DnkLKQtY` (+5) | 58 rule coins | 93% |
| `HeYpA9MA` | 28 outside coins | 93% |
| `89jFbt1W` | 27 outside coins | 93% |
| `2T3U2FzV` | 14 outside coins | 100% |

**The ex-ante rule.** For each bundle wallet, count before each coin the bundled launches it was in and how many he
attempted. A coin is **followed** when one of its bundle wallets has 2 or more attempts at a rate of 50% or more. Out of sample:

| period | coins | followed | his rate followed | his rate not followed | his picks covered |
|---|---|---|---|---|---|
| Oct 23 12-24 | rule host + template | 613 | **74%** | 11.7% | 72% |
| Oct 24 00-12 | rule host + template | 440 | **74%** | 8.6% | 79% |
| Oct 23 12-24 | outside, dev >= 0.25 | 119 | 67% | 3.4% | 24% |
| Oct 24 00-12 | outside, dev >= 0.25 | 34 | 74% | 2.5% | 19% |

74% is his visible ceiling: on a followed coin he attempts as often as on his own top deployers. Adding the creator and the
create signer as identities lifts the outside coverage to 30-32% at 64-67%. Exact dev-buy fingerprints add nothing more.

**His outside picks by what his history knew** (Oct 23 12-24 and Oct 24 00-12, 466 picks):

| the coin's wallets | coins | his picks | his rate |
|---|---|---|---|
| followed | 213 | 30% | 66% |
| seen, he took one before (below the threshold) | 1,002 | 31% | 14.5% |
| seen, he never took one | 7,206 | 13% | 0.9% |
| never seen | 3,346 | 25% | 3.5% |

So 30% of his outside picks are decoded at his ceiling. The rest are low-rate picks no data feature separates. They are a small
part of his profit (section 1), and nothing tested predicts them.

**Speed matters.** Operators rotate bundle wallets within hours. With his attempts known only 1 h late, coverage of his rule
picks falls from 72-79% to 54-69%; 6 h late, to 22-50%. The bot must see his attempts as they land.

## 4. The value for us

These are the four `entry.follow` modes. The same coins are scored under the live exit policy: a tool-host coin that is not a first fresh tweet is
sold at create + 3, the rest at create + 10. Values are for the first outside buyer, net of 1.2%. "Sum" adds the per-trade %, so
divide by 100 for SOL per 1 SOL staked per trade. Out of sample (Oct 23 12-24 + Oct 24 00-12), with the coins the rule skips
limited to post-bundle <= 40 SOL as the bot's cap does:

| mode | coins | value | sum |
|---|---|---|---|
| current rule | 1,357 | +2.45% [+1.75, +3.18] | +3,322 |
| `gate`: the rule's followed coins | 808 | +3.63% [+2.67, +4.60] | +2,936 |
| (the rule's coins `gate` drops) | 549 | +0.70% [-0.37, +1.81] | +386 |
| (the followed coins the rule skips, which `add` adds) | 188 | **+8.36% [+6.18, +10.32]** | +1,572 |
| `add`: the rule + those | 1,545 | +3.17% [+2.47, +3.88] | **+4,894** |
| `on`: gate + add | 996 | +4.53% [+3.68, +5.48] | +4,508 |

The added coins are positive in every period: +5.5% (fit), +7.5% and +10.1% (tests). This holds at every threshold tried
(K 1-3, R 0.3-0.7): +3.6% to +12.8% a period. With R = 0.7 they are +9.25% (138 coins) and `on` is +5.76% a trade.
`add` makes about 47% more than the current rule; `on` is the best per trade. The coins `gate` drops are about break-even
after costs, so `gate` only pays when capital or position slots are the limit. About 5% of the rule's coins have no complete
tape and are left out of every row alike.

## 5. Built: `entry.follow`

- **Feed:** the gRPC subscription gets a `copied` filter on `follow_wallet`, failed transactions included. Each of its
  transactions that names a mint created in the last 4 slots is one attempt, handed to the cold thread.
- **Cold thread:** `rugsnipe::follow::FollowTracker` counts, per bundle wallet, the bundled launches it was in (every run the
  feed assembles) and the attempts. It publishes the followed set (>= `follow_min_attempts` at >= `follow_min_rate`) and
  saves `follow.json` beside `state.db` every minute, so a restart keeps what the day taught. A wallet unseen for about 3 days
  is forgotten.
- **Hot loop, at the fire:** `follow <mint8> yes|no rule|<skip> host <h> dev <SOL> -> fire|skip|log` in the journal.
  - `log`: no change to what is bought; the creates the rule skips are parked to get their verdict too.
  - `gate`: an unfollowed rule coin is `skip_not_followed` (0x61).
  - `add`: a followed coin the rule skips (dev buy, uri host, operator table, tier) is bought at stake tier 0, through every
    other check (post-bundle cap, bundle shape, first spot, tweet cut).
  - `on`: both.
- `rugsnipe-check` has a follow section: verdicts by kind, and our closed trades by verdict.

## 6. Last night, replayed (Oct 25 00:00-09:13 UTC)

We lost 0.10 SOL on 72 trades while he made +1.07 SOL on 86. The sets barely overlapped: 62 of our trades were coins he
never attempted, and we tried none of his 86. Replayed with only what the bot would have known (`night*.py`: his attempts
from Oct 24 12:00, each bundle wallet's real launches, the distinct coins it bought):

| | followed (K 2, R 0.5) | not followed |
|---|---|---|
| our real trades | 12, +0.19 SOL (+3.2% a trade) | 60, **-0.28 SOL** (-0.9%) |
| his real trades | 16, +0.51 SOL (+4.1%) | 70, +0.57 SOL (+1.0%) |

At R 0.3 his followed trades carry +0.95 of his +1.07 SOL (27 trades). The night under each mode, counting only coins
with a European slot leader (our `leader_regions`) and landing the added coins at our real rate (38%):

| | actual | gate | add | on |
|---|---|---|---|---|
| R 0.5 | -0.10 SOL | +0.18 | +0.13 | +0.40 |
| R 0.3 | -0.10 SOL | +0.09 | +0.40 | **+0.59** |

His night: +0.95 SOL. In the backtest at R 0.3 the coins `gate` drops are worth +0.17% (nothing). `on` makes +4.30% a
trade on 1,134 coins (total +4,872, as much as `add`'s +4,949 on 1,577), and the added coins are worth +7.40% (220).
**Use `on` with `follow_min_rate = 0.3`.** The rest of the gap to him is landing (38%) and the slots the leader filter skips.

## 7. Switching it on

The tracker starts empty and learns from his live attempts; the tests above had 12 h of warm-up. `log` learns exactly like
`on`, so start in `log`. When `journalctl -u rugsnipe-hot | grep '^follow:'` shows followed wallets and the `follow ... ->
log` lines look right (an hour or two), switch to `on`:

```toml
[entry]
follow = "on"                  # first "log"
follow_wallet = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
follow_min_attempts = 2
follow_min_rate = 0.3
```

Not built: the creator and signer as identities (+6-8 points of outside coverage, section 3), and seeding the records from
history.
