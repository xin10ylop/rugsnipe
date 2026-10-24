# 15. Big rugs: what predicts them and what reduces them (2026-10-24 night)

Scripts: `analysis/research/2026-10-24/rugs/`. Data: the 1,881 rule coins of docs/14 (complete tapes, Oct 23 and Oct 24
00-12 UTC), under the live `"exit"` policy (a coin that is not first sold at create + 3), net of 1.2%, 0.5 SOL. A big
loss is -30% or worse for the first outside buyer. Every rule is scored on the three 12-hour periods apart.

## 1. Where they are

- **42 big losses** (2.2% of the coins) make **27% of all the money lost**; the mean is +2.97% with them and +3.87%
  without. Holding every coin to create + 10 there are 65: the exit mode already removes a third.
- **Rugs and the big winners are the same coins.** Every big loss had insiders (creator, signer, bundle wallets) holding
  18-27% of the supply after the bundle. Coins under 15% never lost 30% but are worth only +1.5%; at 25% or more they
  are worth +6.9% and lose 30% 7% of the time (hold 10).
- **They are atomic.** The insiders had sold half their tokens by slot +2 in 30 of the 62 big losses at hold 10, before
  any reaction can land.
- **Our own fills:** 2 of the 33 live trades of Oct 24 17:29-18:39 lost 40%. Both were copies (j7, dev 5, post-bundle
  39.3 and 39.9, on tweets by @thegoat195923 and @toly). Our copies made -7.75% there, our first coins +4.38%.

## 2. What does not work

| rule (skip, unless stated) | coins | their value | big losses removed | P&L, 36 h |
|---|---|---|---|---|
| the bot's dumper list (an insider dumped an earlier launch) | 482 | +2.27% | 30 of 42 | -5.46 SOL |
| an insider dumped 4+ launches | 285 | +3.32% | 16 | -4.73 |
| an insider's dump rate >= 50% (2+ known launches) | 40 | +0.12% | 7 | -0.02 |
| the deployer's last coin was a big loss | 56 | +4.83% | 4 | -1.35 |
| the deployer's last 10 coins average below 0 | 412 | +0.50% | 12 | -1.04 |
| the launch template's last coin was a big loss | 57 | +3.51% | 4 | -1.00 |
| the deployer launches mostly copies (60%+ of the last 20) | 220 | +5.14% | 5 | -5.66 |
| ticker or name of a tool coin of the last 60-300 s | 529-824 | +1.8 to +2.2% | 8-17 | -5.3 to -8.7 |
| ticker or name in a tweet an earlier coin used (the bot's tweet book) | 762 | +2.73% | 14 | -10.39 |
| the tweet feed's copy call, every author followed | 530 | +2.61% | 13 | -6.91 |
| sell the dumper-list coins at create + 3 | 482 | | 39 -> 40 | -5.82 |
| sell on the insiders' first sell / when 10, 25, 50% is sold | all | +3.67 to +3.73% (+3.68% now) | 64-65 (65 now) | 0 |
| wait for slot +1, buy only if the insiders have not sold | all | +0.26% (+2.97% now) | 52 (42 now) | -25.5 |

A dumper's next launch, a deployer whose last coin rugged, a template that rugged: each of those coins is worth more
than the average one, not less. The insiders who dump are the same operators whose coins pump. Waiting to see the dump
gives up the slot-0 entry, which is the edge.

**The copy verdict's speed.** Copies sold at create + 1 instead of + 3 would have half the big losses (12 against 24)
at the same value (-0.1% against -0.2%). But the verdict needs the metadata fetch from the launch tool (p50 0.6-0.9 s
live): 3 verdicts in 4 land at slot +2 or later. That latency is on the tool's server.

## 3. The one rule that held, and why it stays in "log"

Insiders on the dumper list **and** the coin launched from `m.rapidlaunch.io`:

| | skipped | their value | big losses removed | P&L |
|---|---|---|---|---|
| Oct 23 00-12 UTC | 35 | -1.47% | 3 of 13 | +0.26 SOL |
| Oct 23 12-24 UTC | 12 | -0.21% | 2 of 12 | +0.01 |
| Oct 24 00-12 UTC | 25 | -1.01% | 3 of 17 | +0.13 |
| all | 72 | -1.10% | 8 of 42 | +0.40 |

It is the only skip with a negative value in all three periods (rapidlaunch coins without a dumper are worth +5.2%).
But it is fragile: the same rule on a list that counts insider sells within 5 slots skips 119 coins worth +0.56%
(-0.33 SOL). The bot's list is built from every bundled launch the feed shows, so it flags more wallets than this
research list does. **Measure it live first.** `entry.dumper_skip_hosts` (new) limits `dumper_skip = "skip"` to the
listed hosts, the journal line now names the host (`dumper <mint8> logged (would skip) host m.rapidlaunch.io`), and
`rugsnipe-check` shows the hits by host and our closed trades by the hit's host, with the big losses counted. After a
day, switch it on if the rapidlaunch hits lose:

```toml
[entry]
dumper_skip = "skip"                      # now "log"
dumper_skip_hosts = ["m.rapidlaunch.io"]  # the skip only there; other hits are journaled and fired
```

## 4. Copies and the tweet feed

Copies are where our own big losses come from: both live -40% trades were copies. On our fills a copy is worth -4.0%
sold at + 3 (backtest) and -7.75% (live), against +1.5% for the rivals' copies (we win the copies they pass on). The
feed can skip a copy before the buy, but only when its author is followed and its ticker or name is in the tweet: 52%
of copies name their coin in the text. The 50 accounts with the most copies on Oct 23 wrote 35% of Oct 24's copies, 19%
of them matchable. **Expect the feed to catch about one copy in five.** A second 5-handle account (a second key,
`entry.tweet_feed_keys = ["TWEETSTREAM_KEY", "TWEETSTREAM_KEY2"]`) adds most with Pojimonkor, elonmusk, toly,
grok12_john and TomTheDogdevs (picked greedily on the simulated feed beside the first five: correct copy calls on the
rule coins 104 -> 147 over the 36 h, big losses among them 2 -> 6; `next5.py`). For the 50-account plan, follow the accounts
that spawn the most copies (Oct 23-24, most first):

devvvooooor, igetsjiggy6767, Thedev1738, elonmusk, ddddddd8a, bwenews, solana, Polymarket, Pojimonkor, gettindevvy,
terrythedevv, esotericpigeon, toly, essencepnl, CoachPatNFT, GoldenCatDev, DanielKuhn77510, texashedge17154, g8uviq,
grok12_john, looshfarmer0000, zumpsol, PetalSoI, ripedevonchain, lilbruhfish, meggarwicky, gentledev3312, wedtm,
Grimezsz, 123greenapple, ihavemotionnn, MarioPrezldente, TomTheDogdevs, FXXdeploys, chillmigratoor, javierthedev,
AndyAyrey, danny8080, thealexmorgan15, HimothyJamesJr_, coinbureau, Trencherbill77, fearonchain, bigEpnl420v2ey,
etquantz, Nino_67x, Jamesbonderr67, gainzy222, Turbodev1x, coinbase.

## 5. What to expect

Rugs cannot be removed without the gains: they come out of the same insider-heavy launches. What reduces them:
the exit mode (65 -> 42, live now), the feed's copy skip once `"on"` (about a fifth of the copies), and the
dumper-on-rapidlaunch skip if the live day confirms it (about 8 of 42). Together that is at most a third of the
remaining big losses. In the backtest 2.2 coins in 100 lose 30% or more (-0.15 to -0.2 SOL each at 0.5 SOL), and the
+3% a trade already counts them; the live 2 in 33 is above that rate, on a small sample.
