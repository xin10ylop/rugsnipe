# 16. Who he follows: operators by their bundle wallets and fee routing (2026-10-25)

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
- **Cold thread:** `rugsnipe::follow::FollowTracker` counts, per record (bundle wallet, fee routing: section 8), the bundled
  launches it was in (every run the feed assembles; the feed tags each run's fee routing at the create) and the attempts. It
  publishes the followed set (>= `follow_min_attempts` at >= `follow_min_rate`) and saves `follow.json` beside `state.db`
  every minute, so a restart keeps what the day taught. A record unseen for about 3 days is forgotten.
- **Hot loop, at the fire:** a coin is followed when one of its bundle wallets is, with the coin's own fee routing.
  `follow <mint8> yes|no rule|<skip> host <h> dev <SOL> fees dev|third -> fire|skip|log` in the journal.
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

The tracker learns from his live attempts; the tests above had 12 h of warm-up. Start it from `data/follow_seed.json`
(section 8) so it knows the operators of Oct 23-25 at once. `log` learns exactly like `on`. When `journalctl -u rugsnipe-hot
| grep '^follow:'` shows followed records and the `follow ... -> log` lines look right, switch to `on`:

```toml
[entry]
follow = "on"                  # first "log"
follow_wallet = "AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2"
follow_min_attempts = 2
follow_min_rate = 0.3
```

Not built: the creator and signer as identities (+6-8 points of outside coverage, section 3; for coins with no bundle,
section 8).

## 8. The key is the bundle wallet and the fee routing (dissection, 2026-10-25)

Scripts: `evkey.py`, `cell_*.py`, `authkey.py`, `gaps.py`, `skipval.py`, `onb.py`, `memory.py`, `nobundle.py`, `leader.py`,
`exit.py`, `seed.py`.

**He follows an operator per fee routing.** A create names a `creator` that gets the creator fees: the dev itself (the
create's user or the fee payer) or a third wallet (fresh for each coin, no earlier transactions). An operator uses both, with
the same bundle wallets, and he treats them as two operators. Last night (Oct 24 12:00 - Oct 25 09:13), his attempts:

| bundle wallets | dev keeps the fees | a third wallet gets them |
|---|---|---|
| `J4o3bqXK` + 2 | 21 of 36 | 0 of 44 |
| `Gg3ska7S` + 1 | 14 of 24 | 0 of 39 |
| `G3fk9Nyk` + 2 | 14 of 18 | 7 of 68 |
| `J5dUdMdH` + 2 (`5Eb5rURZ`) | 25 of 26 | 12 of 27 |
| `C57Ax2Bc` + 2 (`5sWHrtKx`) | 50 of 64 | 41 of 69 |

Out of sample (Oct 23 12-24 and Oct 24 00-12, the coins of section 4), keyed on (bundle wallet, fee routing):

| K 2 | followed coins he attempted | his picks covered | value a trade (pb <= 40) | sum |
|---|---|---|---|---|
| wallet alone, R 0.5 | 73% / 74% | 65% / 74% | +4.53% [+3.68, +5.48] (996) | +4,508 |
| (wallet, fee routing), R 0.5 | **82% / 83%** | 66% / 77% | +4.87% [+4.03, +5.76] (909) | +4,426 |
| wallet alone, R 0.3 | 68% / 69% | 73% / 82% | +4.30% [+3.53, +5.09] (1,134) | +4,872 |
| (wallet, fee routing), R 0.3 | **79% / 80%** | 70% / 80% | +4.74% [+3.82, +5.61] (981) | +4,653 |

The key matches his picks 10 points better and each trade is worth more; it buys 13% fewer coins, so the backtest sum is 4-5%
lower. On last night's replay at R 0.3 `on` makes +1.10 SOL with the key, +0.59 without (his night: +0.95). The bot keys its
records this way.

**What it is not** (each tested inside his followed operators): the launch tool (j7tracker 53%, rapidlaunch 59%, uxento
17%, ipfs 8% of the coins taken, but adding the host to the key leaves as many half-taken cells); the bundle wallets
rotating (each operator's set is fixed); his own result on the operator's last coin (next taken 66% after a win, 65% after a
loss); the tweet author (many are the operator's own X personas, `devvvooooor`, `thedev1738`, `terrythedevv`, but as a key
it reaches only 55-68% on rule coins and adds nothing to the wallet key); the linked tweet's age or rank (78-94% taken at
every age). He sends one transaction per coin and his failed buys land, so a coin without his transaction is a skip or a
send that never landed.

**Inside a followed cell he skips 18%, and those coins are worth having.** Followed rule coins he skipped: +2.81% a trade
[+0.64, +5.20] (174); the ones he took: +4.62% [+3.70, +5.54] (840). Buying every followed coin, as the bot does, is positive.

**His profit by what the keyed follow knew** (Oct 23 12-24 + Oct 24 00-12, 12 h of warm-up before, +20.97 SOL):

| at the create | his trades | his P&L |
|---|---|---|
| followed | 252 | +16.19 SOL (77%) |
| known, he took 1+ (below K / R) | 27 | +1.87 (9%) |
| no bundle wallets | 58 | +1.21 (6%) |
| known, he never took | 27 | +0.56 (3%) |
| new wallets | 48 | +0.49 (2%) |
| the wallet known with the other fee routing | 8 | +0.48 (2%) |
| no bundle-slot tape | 13 | +0.18 (1%) |

- **Long memory explains part of the rest.** Of his 162 rule picks the follow missed, 33% had a bundle wallet he attempted 2+
  times before Oct 23 (24%: 5+), against 18% (9%) of a control of rule coins he skipped. He has months of history; the bot
  learns from his live attempts. `data/follow_seed.json` (`seed.py`: 10,051 launches of Oct 23 - Oct 25 09:13, 9,146
  records, 523 followed at K 2, R 0.3) starts the bot where these tests start.
- **Coins with no bundle:** a reused dev wallet (creator and signer attempted 2+ times at 30%+) carries 63 of his 177 such
  outside attempts at 46%. Not built.
- **The remainder** (about 8% of his profit) are low-rate picks among thousands of coins with no identity his history knew
  (0.8-10% taken). Wallets that buy in the same create slot show up 5-25x more often on them than on his skips (`Anubis51`,
  `TAB16RxX`, `FiFawHqx`: other snipers with the same operators), but they land with him, so they are not his signal.
  Nothing on chain tested so far predicts these picks.

**His exit, copied per cell, is not a proven gain.** Selling each followed coin at the median create-relative sell slot of his
last 3 trades in its (wallet, fee routing) cell (`exit.py`): +4.04% a trade against +3.47% under the live policy on the
same 895 coins; paired, +0.57 points [-0.39, +1.73], better on 214, worse on 231. Not built.

Slots now take about 0.21 s (970,976 slots in 205,941 s, Oct 23 - Oct 25), so a record is forgotten after 1,250,000 slots
unseen (about 3 days).

## 9. Today live, the full pre-buy state, the full history (2026-10-25 afternoon)

**Today, live, out of sample** (`today.py`: Oct 25 09:13-13:07 UTC, the seed then each coin in slot order, as the bot
learns): of the 67 coins (post-bundle <= 40 SOL) a followed record bundled, he attempted 49 (73%) and made +0.584 SOL on
them; his whole window made +0.332 SOL. Of the other coins seen (his picks and the followed wallets' other launches, 80)
he attempted 14 (18%), at -0.001 SOL.

**The full pre-buy state** (`prebuy.py`, `prebuy_cmp.py`, `structure.py`, `within.py`, `bfund.py`: every bundled launch of last night's
operators, 1,806, with its create transaction, bundle transactions, the dev wallet's history and funding, the bundle
wallets' top-ups, the slot leader and his own recent activity). Inside his half-taken (operator, fee routing) cells, each
feature's mapping learned on the cell's first half and scored on its second:

- **A bundle inside the create transaction is a hard skip:** 3 of 409 launches (0.7%), 0 of 59 in the cells he takes 30%+
  (those coins: +2.6% at +3 behind the bundle, against +10.6%). The bot's bundle gate already needs a separate buy behind
  the create.
- **A create with no dev buy:** 7 of 97 (only operators he always takes); a single bundle transaction: 3 of 55.
- **A bundle smaller than the operator's usual:** 12% taken (57) against 49% (same) and 57% (bigger). Those coins are not
  worth less (+10.9%), so it is his preference, not value.
- **Nothing else separates:** compute price and limit, tip and route, programs, lookup tables, instruction count, uri host,
  name and symbol, dev wallet age and reuse (most operators reuse the dev wallet), dev and bundle funding source, amount and
  timing (the same treasuries fund his takes and his skips), the slot leader and slot in its window, his own recent P&L
  (41% after losses, 50% after gains) and how busy he is. The best single feature (bundle size against usual) scores 65.7%
  against the cells' majority 57.2%.

**What moves him inside an operator is its recent coins' value** (vC at +10 behind the bundle of its last 3 coins, taken or
not): after a skip he takes the next 9.8% when they made -5% or worse, 26% in between, 39.6% at +10% or better; after a
take 53% and 69%. Out of sample (`evval.py`, K 2, R 0.3):

| followed by | coins | his rate | value a trade (pb <= 40) | sum |
|---|---|---|---|---|
| his attempts (built) | 1,204 | 79% | +4.74% [+3.82, +5.61] | +4,653 |
| attempts and the cell's last 3 coins >= +10% | 321 | 83% | **+7.72% [+5.90, +9.69]** | +2,038 |
| attempts, recent value below | 883 | 78% | +3.65% [+2.65, +4.59] | +2,615 |
| recent value, not his attempts | 201 | 23% | +3.26% [-0.46, +7.47] | +274 |

**And it is his sizing:** on the coins he bought, his stake is 0.909 SOL median (1.106 mean) when the cell's last coins made
+10% or better, 0.604 (0.747) otherwise (61 and 191 trades); 0.810 against 0.604 on coins outside the follow (9 and 159). He buys a hot operator's coin at
1.5x, and those coins are worth twice the rest. Not built (the bot needs each run's price at create + 10 in the cold thread).

**The full history** (`hist_depth.py`, `hist_older.py`, `memory2.py`): his wallet starts sniping in Aug 2026 (12,554
signatures before Sep 24, 6,442 buy attempts; 36 in Jan 2025, then nothing), so no earlier memory of his own trades exists
on this wallet. With all of it (Aug 1 - Oct 23) against the bundle wallets' whole histories (533 of 549 complete): of his
162 rule picks the follow missed, 62% had 2+ earlier attempts on a bundle wallet (50%: 5+), against 48% (39%) of a control
of rule coins he skipped. History raises both; as a count it separates them only weakly. Using it fully needs the
operators' launches since August to rate them, not only his attempts.
