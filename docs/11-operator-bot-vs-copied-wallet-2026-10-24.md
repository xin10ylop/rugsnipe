# 11. The operator bot against the copied wallet, 2026-10-24 (00:00 to 12:29 UTC)

Chain truth for both wallets:
- every transaction of ours and of the copied wallet (`AHwJ...`, docs/09), from `getTransactionsForAddress`
- the full tape of the 201 launches either wallet landed, failed transactions included

Every result below is after the landed transactions' fees and tips. Per-trade tables:
`our_trades_2026-10-24.csv` and `his_trades_2026-10-24.csv`, sent with the session's report and not in the repo.

## The day

| | trades | mean | median | trades SOL | failed copies | net |
|---|---|---|---|---|---|---|
| us, all day | 75 | -0.35% | -2.64% | -0.099 | 140, 0.154 SOL | -0.253 |
| him, all day | 126 | +1.59% | +0.95% | +1.341 | 466, 0.005 SOL | +1.336 |
| us, since 04:00 | 42 | -0.05% | -3.46% | -0.013 | 0.001 SOL | -0.014 |
| him, since 04:00 | 64 | +2.89% | +1.77% | +1.358 | 0.003 SOL | +1.355 |

**Costs.** All 51 of our failed copies that cost more than 100k lamports came before 04:00, when plain copies carried a
3M priority fee: 0.153 SOL. Since then a failed copy costs 15,000 lamports, against his 11,000. What remains is the
round trip:

| | buy | sell | median round trip |
|---|---|---|---|
| us | a 3M tip | 2M priority fee plus a 1M tip | 9.0 mSOL, 1.77% of stake |
| him | a 1.5M tip | 0.5M priority fee plus a 1M tip | 3.3 mSOL, 0.75% of stake |

**Where his money is.**
- **Launches our bot never fired on:** 104 trades, +2.45% mean, **+2.36 SOL**. 59 of them came from creators our bot
  never fired on today.
- **Launches our bot also fired on and lost the race:** 22 trades, -2.47% mean, -1.02 SOL. Of these, 9 were next-slot
  entries that lost -13% on average (the rugs below).

## Q1: can the bot tell, in our own slot, that he fired?

**Yes.** In 51 of the 57 trades where he fired, his failed buy (his exact-state check, custom error 100) is in our
block, a median of 19 transactions behind ours (2 to 180). The feed sees it if the gRPC subscription adds his wallet
with failed transactions included, as it already does for ours.

What it is worth:

| our trades (all day) | n | mean | median | SOL |
|---|---|---|---|---|
| he fired behind us in our slot | 51 | +1.60% | -0.26% | +0.418 |
| he fired a slot or more later | 6 | -3.5% | | -0.118 |
| he never fired | 18 | -4.82% | -4.12% | -0.398 |

Since 04:00 it splits 25 trades at +0.47 SOL against 17 at -0.48 SOL. Selling at once when his buy is not in our slot
saves only 0.05 to 0.11 SOL a day. The loss is mostly the round trip, about 2.5% of pump fees plus our tips. The lever
is knowing before we buy, and the creator predicts it: he fired on 37 of 39 CVmkctkS launches we took, but on about
half or fewer of most 3 SOL dev-buy creators.

## Q2: can the rugs be avoided?

The 6 trades below -20% (2 ours, 4 his) are the same event every time. All the insider wallets sell their whole bundle
in one slot, usually 4 transactions back to back, 0 to 9 slots after the entry. No exit rule can sell first. The loss
is the bundle's markup at the entry, gone in one slot.

The pre-entry cap that splits it is `entry.max_post_bundle_sol = 40`, the post-bundle virtual SOL reserve (the curve
starts at 30):

| post-bundle reserve | us | him |
|---|---|---|
| 40 and under | 71 trades, +0.265 SOL | 97 trades, +2.180 SOL |
| above 40 | 4 trades, -0.364 SOL, 1 rug | 29 trades, -0.839 SOL, 2 rugs |

Skipping a creator for 3 h after one of its launches dumps would have saved him 1.54 SOL (the 03:18 8ZN71XTd pair) and
cost us 0.26 SOL. It is not adopted.

## Our day, rebuilt one change at a time (counterfactual exits from the tapes)

| | SOL, 00:00 to 12:29 |
|---|---|
| as traded, with failed copies | -0.253 |
| 1. skip a post-bundle reserve above 40 | +0.110 |
| 2. plus selling at once when his buy is not in our slot | +0.160 |
| 3. plus failed copies at today's post-04:00 cost | +0.312 |
| 4. plus sells at his 1.5M instead of 3M | about +0.42 |

That is about +0.8 SOL a day at 0.5 SOL, against his +2.6 SOL. The rest of the gap is his launches, the 104 we never
fire on. Closing it needs two things: a table that follows the creators he trades now (this one is 24 h, refreshed
every 4 h), and per-creator gating on how often he fires behind us.
