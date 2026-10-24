# 12. How the copied wallet trades (AHwJSnFN), decoded from chain data, 2026-10-24

All of this comes from chain data:
- his full month (Sep 24 to Oct 24): 30,499 transactions
- every pump.fun create from 00:00 to 12:00 UTC today (17,135), decoded
- the metadata file of every create with a dev buy of 1 SOL or more
- full tapes of all 570 launches he attempted today
- the first 12 slots of 204 launches he skipped

Wallet: `AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2`. A second wallet, 6anjqMe, is covered in section 8.

## 1. Result

**+151.6 SOL over the month on 19 active days**, with no trading Oct 3-14. The tiny 0.01 SOL stakes on Oct 1-2 and
Oct 15-16 look like test runs.

| days | daily result |
|---|---|
| Oct 17-23 | +4.7, +26.5, +20.1, +10.0, +11.4, +8.2, +26.1 SOL |
| Sep 26 | +21.7 SOL |

- He lands 400-600 trades a day at a 0.6-0.8 SOL stake, wins 47-51% of them, and holds 1-2 s.
- The wallet's balance rose by the same amount (+153.8 SOL). The profit stays in the wallet: no withdrawals, no
  funding wallet.
- **No linked wallets.** His two guard programs (slot window `A5c1eN7M...`, curve state `8jzqTjED...`) are used by
  nobody else; every sampled signer over 39-47 h is him. Nobody co-signs, and his only transfers are 0.25-0.5 SOL
  test sends.

## 2. Execution: read off his transactions

**Entry.** One buy transaction per launch, guarded two ways:
- **Slot window.** The `A5c1eN` guard allows the buy only in the create slot or the next: window width 1 on all 6,304
  landed buys. 66% land in the create slot, 34% in the next.
- **Curve state.** The `8jzqTj` guard checks the curve's real token reserve against one exact value: the reserve right
  after the dev bundle. If anyone trades first, the buy fails with custom error 100 for about 11,000 lamports. Today he
  attempted 570 launches with one transaction each and landed 126.
- **The bid.** His routine buy carries a 1.5M tip and no priority fee. When he wants a launch badly he pays about 12M
  (an 8M tip). He won all 16 such races against us on Oct 23.

**Exit.** Each sell carries the slot-window guard with a start slot of buy + k and no end.
- k is 2, 3, 4, 5, 6, 9 or 10 (most often 5, 6 or 10), and 75% of sells land in exactly that slot.
- So he holds for a planned number of slots and does not react to the flow.
- The hold is chosen per launch: a creator's most common hold covers only about half its trades, and it shifts over
  the month.

**Stake.** 0.6 SOL base, up to 3.6 SOL, set by his own track record with the creator (month, chain truth, test stakes
excluded):

| his P&L with this creator before the trade | trades | per trade |
|---|---|---|
| new creator | 2,446 | +1.34% [+0.61, +2.07] |
| negative | 953 | +1.90% |
| 0 to +0.5 SOL | 920 | +4.32% |
| +0.5 to +2 SOL | 707 | +3.97% |
| above +2 SOL | 590 | **+6.01%** [+3.13, +8.90] |

**Costs.**

| | buy | sell | a failed buy |
|---|---|---|---|
| him | a 1.5M tip | 0.5M priority fee plus a 1M tip | 11,000 lamports |
| us | a 3M tip | 2M priority fee plus a 1M tip | 15,000 lamports since 04:00 (3M before) |

That makes his median round trip 3.3 mSOL (0.75% of stake) against our 9.0 mSOL (1.77%).

## 3. Selection, layer 1: tweet coins

Every launch he attempted in the sampled hour (31 of 31) links in its metadata to a specific tweet. In the 12 hours, 444
of his 469 attempts with readable metadata do.

These coins are deployed on a tweet within seconds, mostly through three launch tools: j7tracker, uxento and
rapidlaunch. The tool is the metadata host, often with images on Axiom's servers. Each coin has a dev buy in the create
and usually 2-3 bundle wallets right behind it.

Over 00:00-12:00 he attempted 555 of the 17,135 creates; 509 had a dev buy of 1 SOL or more. Within the qualifying set
(a tweet coin, a dev buy of at least 0.95 SOL, a tweet under 1 h old) he attempted 424 of 2,055 (20.6%):

| feature | his attempt rate |
|---|---|
| launch tool | uxento 38.8%, j7tracker 22.4%, rapidlaunch 23.0%; other hosts 0-5% |
| tweet-to-coin delay | **2-5 s: 53%**; under 2 s: 22%; 5-10 s: 29%; 10 s or more: 13-18%; never for tweets older than 1 h (0 of 75 in the hour sample) |
| dev buy | exactly 3.0 SOL 32%, 3.05-3.95 SOL 36%, 1.0 SOL 21%, 4.0 SOL 23%, 5.0 SOL 25%, 7 SOL and up 19-26%; **1.05-2.95 SOL only 3-5%** |
| image server | Axiom's old CDN (`axiomtrading.sfo3...`) **81%**, Axiom's current CDN 19% |

## 4. Selection, layer 2: the deployers he follows

| | his attempt rate |
|---|---|
| He attempted 0 / 1 / 2-4 / 5+ of this creator's earlier launches today | 9% / 49% / 61% / **91%** |
| He traded this creator 0 / 1-2 / 3-9 / 10-29 / 30+ times this month | 9% / 22% / 26% / 60% / 64% |
| The creator launched 30+ times earlier today | 77% |

Once a deployer is on his list, he takes almost every launch. New deployers get in at about 9%, mostly through the
template features of section 3.

**A predictor of his choice**, a logistic regression trained on hours 0-7 and tested on 8-11:

| inputs | test AUC | the launches it scores 0.3 or higher |
|---|---|---|
| create-time features plus his live attempts on the creator (the feed can watch his wallet) | **0.86-0.87** | 72-74% are his picks; they cover 54-62% of everything he picks |
| the same features, nothing from him | 0.66-0.74 | 36-43% are his picks |

Dropping the metadata file costs almost nothing in the first version (0.858 against 0.864). So the bot needs only the
create, the creator's history and his live attempts.

## 5. What his choice is worth

The real first outside buyer after the dev bundle, at create + 10 slots: sales so far plus the rest marked at that
slot's price, before tips and fees.

| | launches | mean | median |
|---|---|---|---|
| **launches he attempted** | 555 | **+6.3%** [+4.9, +7.6] | **+3.3%** |
| qualifying launches he skipped | 204 | +2.8% [-0.4, +6.0] | -1.1% |

On his launches, who took the first spot and what they made:

| first buyer | launches | mean | median |
|---|---|---|---|
| a rival | 380 | +8.0% | +4.2% |
| him | 121 | +2.3% | |
| us | 54 | +2.6% | |

The same rivals make +2.8% (median -1.1%) on the launches he skips, so his choice carries real information. The rivals
who take most first spots are snipers across many deployers, not insiders: 9qz7cWBq across 33 deployers, dKMjeGJ4 across
29, CWiNFAEk across 24. They stake 1.8-2.1 SOL (median, up to 7) and some tip 77M-410M. A big tip is a small share of a
big stake.

**Replay values are not usable here.** Replaying the first spot with a rival's buy kept after ours gives +12% on his
launches against his real +1.6%. A rival with an exact-state check would fail behind us and never buy. Only realized
results count.

## 6. What this means for our bot

The operator bot already copies his mechanics: first spot, a bound at the post-bundle state, pay-on-success, a timed
exit, stake tiers. It misses most of his choice:

1. **Our creator table is built from his landed trades over 24 h and refreshed every 4 h.** His real list is his
   attempts, landed or failed, and it changes within the hour. Today 59 of his 104 profitable launches came from
   creators our bot never fired on, while 18 trades where he did not fire cost us -0.40 SOL.
2. **Costs about 1 point above his** per trade (section 2).
3. **Flat 0.5 SOL stakes**, where he sizes up on proven creators (+6% per trade above +2 SOL of history).

**What we actually measured.** When we land first ahead of him (his buy fails behind ours, 51 trades today), we make
+1.6% per trade after costs; at his cost level that would be about +2.6%. The rivals' +8% comes from launches where they
outbid everyone with large stakes. At about 120-170 first spots a day on 0.5 SOL that is roughly +1.2 to +3 SOL a day.
It scales with stake, because the tip is a fixed cost. Realistic: positive and repeatable, but not his +10 to +26 SOL a
day without his volume and capital.

## 7. The implementable rule (for the operator bot)

**Superseded by docs/13.** The rule backtested on two days is his deployer list (the last 7 days) plus the tweet check
(the first coin on a fresh tweet) on the three launch tools: +5.0% per trade net of our costs, positive in all four
half-days. The tweet check alone and the deployer list alone do not hold.

1. **Deployer list from his attempts, live.** Subscribe to his wallet in the gRPC feed, failed transactions included.
   His failed buy shows in our own block, a median 19 transactions behind ours, on 51 of the 57 trades where he fired.
   Then:
   - count his attempts per creator over the last hours;
   - merge in the month table (n, P&L);
   - drop a creator after 2 of its launches pass with no buy from him.
2. **Universe from the create:** a launch-tool URI host (j7tracker, uxento, rapidlaunch), a dev buy of at least 0.95 SOL,
   and not a non-SOL quote. Score with the section 4 predictor and fire at 0.3 or higher.
3. **Execution as now**, at his cost level:
   - a buy tip of 1.5-3M;
   - sells at a 0.5M priority fee plus the 1M sender tip;
   - failed copies already at about 15,000 lamports.
4. **Hold 6-10 slots.** He and the rivals both sell around 6-9.
5. **Stake 0.5 SOL base, 1.0 SOL on creators where his month P&L is above +2 SOL.**
6. **Rugs.** `entry.max_post_bundle_sol = 40` (docs/11): rugs are atomic insider dumps that no exit can beat.

## 8. 6anjqMe (`6anjqMeWigQFo5wpciARAunVQUXTqoat8w8fFWarCkpc`)

- **Result.** Oct 1 to now, 23.5 days: **+92.8 SOL** (+3.9 SOL a day). That is his wallet plus his wrapped-SOL account
  `8UdL67UG...`, net of 20.2 SOL deposited from an exchange and 58.65 SOL withdrawn. The +8.5 SOL a day of an earlier
  10-hour sample does not hold up over the month.
- **Setup.** One wallet with its own private program `satasbJp...` (every sampled signer is him). The 1,020 SOL "sent
  to 8UdL67UG" is him wrapping SOL into his own WSOL account, not profit.
- **SOL-priced pump.fun trades.** 8,008 round trips for +13.4 SOL in total, break-even per trade. 39% win rate, 0.49 SOL
  stake, 3 s median hold, sold in 1-6 pieces.
- **Coins priced in another token.** Bought by swapping WSOL through Raydium into the quote token
  (`TTWofwAg...`, a Token-2022 mint with a permanent delegate), then the coin. 5,419 round trips, about 600 a day on
  Oct 19-22. They must carry most of his result; a sampled P&L measurement is in progress.
