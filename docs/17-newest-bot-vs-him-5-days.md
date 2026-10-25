# 17. The newest bot against him over five days, and why we land the coins he does not want (2026-10-25)

The questions: run our newest code on the five days and compare it with his buys. Why does he take coins we skip, and why
do we take coins he skips? Compare our live fills with his. Above 40 vSOL after the bundle, how can we trade like him without
losing? Scripts: `analysis/research/2026-10-25/five/`. Data: every transaction of his and ours from Oct 21 00:00 to Oct 25
16:07 UTC (8,322 and 1,837 signatures). The create slot and the next slot of every coin either of us touched (7,266 coins),
with every transaction in them, landed or failed. Every launch by the listed signers on Oct 23 00:00 to Oct 24 11:00 that
neither wallet touched (2,060). The curve state at create + 1..12.

## What was run

`crates/rugsnipe/tests/replay_days.rs` runs the real `HotLoop`, with the real gates, the real `FollowTracker` and the real
operator tables, on 9,304 creates rebuilt from chain. The tables are the ones `operators_refresh.py` would have written at each
4-hourly refresh. The follow tracker is fed his attempts as they landed. The config is a stand-in, not the box's. It is the
probe-a example with the live settings we know: tool hosts, the four dev ranges, `operators_only`, `max_post_bundle_sol` 40,
`follow` on with rate 0.3, `tweet_gate` exit, `tweet_feed` log, `dumper` log, `hidden_dev` grpc. Run the box's own config
through the same harness before trusting a decision that depends on a setting not listed here.

"v10" below is the curve price at create + 10 over the price right after the bundle, minus 1. It is the gross value of the
first spot behind the bundle at his usual hold, before fees and price impact. Our round trip costs about 5 points: fees of
1.25% each way, about 1-1.5% price impact each way for 0.5 SOL on a 35-47 vSOL curve, and the bid.

## 1. Coin by coin

The newest bot fires on 3,847 coins in the five days. He attempted 3,273 of them (85%) and landed 694 (18%).

| the coins HE bought, by the newest bot's decision | coins | his net | share of his net | v10 |
|---|---|---|---|---|
| skip_bundle_too_big (post-bundle > 40 vSOL) | 299 | +25.57 SOL | 48% | +15.7% |
| fire | 694 | +21.72 SOL | 41% | +11.6% |
| skip_not_operator | 113 | +2.52 SOL | 5% | +10.5% |
| skip_uri_host | 145 | +1.64 SOL | 3% | +7.4% |
| skip_class / skip_dev_buy / skip_not_followed | 56 / 354 / 19 | +0.60 / +0.58 / +0.37 SOL | 3% | +7..+10% |

| the coins THE NEWEST BOT fires on, by what he did | fires | v10 | our live fills there | our live net |
|---|---|---|---|---|
| he landed | 694 | +11.6% | 2 | +0.065 SOL |
| he tried and failed (someone else was first) | 2,579 | +17.8% | 97 | +1.354 SOL |
| he did not try | 574 | +5.8% (8.4% dump below -25%) | 136 | -1.884 SOL |

The newest code is already close to him: what it fires on is what he goes for, 85% of the time. Below 40 vSOL nothing we skip
holds a material share of his profit. The one large block is above 40 (section 4).

## 2. Live: our fills against his

Our live result over the five days, under the configs of the time: -6.582 SOL on 1,138 coins sent (517 filled).

| our live sends, by the newest bot's decision | coins | filled | live net |
|---|---|---|---|
| coins the newest bot skips | 492 | 282 | -5.598 SOL |
| fires he tried | 362 | 99 | +1.079 SOL |
| fires he did not try | 284 | 136 | -2.063 SOL |

The newest gates remove 85% of the loss: -5.60 of -6.58 SOL. What is left is the fires he did not try, -2.06 SOL. The fires
he tried made +1.08 SOL. This includes the cost of failed attempts. The remaining problem is not which coins we pick. It is
which of our picks we actually land.

## 3. The mechanism: the first spot is an auction, and we only win it when nobody else wants the coin

Every transaction of the create slot and the next was decoded on 4,249 coins: the 402 followed coins above 40 vSOL and the
3,847 newest-bot fires below it. 3,869 of them have a matched first spot. For each one we read the tip transfers and the
priority fee: who bid what for the first spot behind the bundle, and who got it.

- **The higher bid wins.** It explains 52 of our 60 races above 40 and 452 of 565 below. Of the 113 below that it does not
  explain, 109 were lost in the same slot to a lower bid that landed earlier (median 34 positions ahead of us). That is
  timing, or a tip paid in a separate transaction we do not see.
- **The price sorts the coins.** The market (him and the rivals) bids up for coins worth more:

  | winning bid, above 40 vSOL | coins | v10 mean | v10 median |
  |---|---|---|---|
  | <= 1.5M lamports | 100 | +8.2% | +3.1% |
  | 1.5-4.5M | 53 | +10.7% | +4.5% |
  | 4.5-8M | 44 | +13.9% | +4.9% |
  | 8-16M | 69 | +15.8% | +10.1% |
  | 16-32M | 52 | +30.1% | +13.2% |
  | > 32M | 18 | +26.4% | +22.5% |

- **Our bid is fixed. His is not.** We bid 1.5-7M per attempt depending on the route, the same on every coin (median 3.99M
  on the races we won). He bids 0-1M on the coins he rates low and 8-48M on the coins he rates high. So we beat his 1M bids and lose to his high ones. We land the coins he rated lowest, and the rivals take
  most of the rest:

  | first spot won by | above 40: coins | v10 | below 40: coins | v10 |
  |---|---|---|---|---|
  | him | 153 (median bid 8.0M) | +21.3% | 694 (2.4M) | +11.6% |
  | a rival | 158 (4.0M) | +12.4% | 2,617 (6.6M) | +17.8% |
  | us | 25 (4.0M) | -4.2% | 222 (4.0M) | +5.9% |

- **His low bids lose for him too.** Above 40, on the coins where we both bid about 1M, he won 8 (v10 -11%) and we won 10
  (v10 -11.4%, 3 dumps). On the coins he won against us with 8-32M (13 coins), v10 averaged +30.7%. His edge on these coins is
  his bid, that is, his information about the coin, not his selection.

This is the winner's curse of a common-value auction. Our wins are the coins nobody else would pay for. That is why the coins
we land go 0 to +3% while the coins he lands go +11 to +21%, with the same rule and the same hold. Examples:
4Gtgb5jp and dptWPf7m we won at 4M against his 1M (-56%, -57%); 5ogU3AgL and 8vXjciim he won at 15M and 24M against our 4M
(+62%, +94%).

## 4. Above 40 vSOL: why the cap stays

The followed coins above 40 vSOL made him +22.3 SOL on 153 landed (5 days). The newest bot without the cap would fire on 402 of them.

- **At our bid we land his rejects.** Live: 25 wins, -1.149 SOL (-9.4% a trade), against his 153 at +21.3%.
- **A higher flat bid does not fix it.** A curve model of a flat bid B (we win when B beats every other bid; his guarded buy
  drops out when we take his spot; our buy and sell on the curve) gives, at 0.49 SOL:

  | flat bid | wins | net (5 days) | net without the top 10 coins |
  |---|---|---|---|
  | 4M | 153 | +2.8 SOL | -1.0 SOL |
  | 16M | 261 | +5.3 SOL | -0.4 SOL |
  | 24M | 294 | +6.0 SOL | -0.8 SOL |

  Every row is a bet on the few coins that go +100%. From 4M up, the median win loses at every bid. The model is also optimistic. On our
  real wins it overstates by 0.009 SOL a win above 40 and 0.013 below, and only 67% of the wins it predicts below 40 were real
  wins. Our own live data says the same. Our wins below 40 with bids above 6.5M lost -1.007 SOL on 37 (v10 +0.8%). With bids
  of 6.5M or less they made +0.55 SOL on 185. Paying more did not buy better coins.
- **Nothing we see before we fire reproduces his bid.** Tested on these coins against his tip and against v10:
  - the tweet's age at the create;
  - the coin's rank among coins for the same tweet;
  - his earlier tries on that tweet;
  - the tweet author (343 handles; texashedge17154 got a high tip on 13 of 52);
  - the metadata: image host (Axiom vs j7tracker), description, website;
  - his history on the signer (net, last decision, last 5 coins' v10);
  - follow-on demand behind the first spot.

  None separates his 8-48M coins from his 1M coins. One thing does mark bad coins above 40: the operator's own bundle tip
  above 12M (51 coins, v10 +4.0%, negative at every bid in the model). It does not carry below 40, where 2,616 coins with
  bundle tips above 12M are fine (+12.9% to +16.2%), so it is not built.
- **Exits do not fix it.** The dumps come from the insiders selling everything at once at create + 2..+8 (the dev and the 3
  bundle wallets in one slot). On our three big dumps, other snipers had bought behind us, so "nobody followed us" is not the
  signal. A coin he did not show up for has 8.4% dumps against 0.4%. But its path is flat (v2 +5.2%, v10 +5.8%), so selling
  at +2 saves nothing on average. Its value is simply below our cost.

To trade above 40 like him we would need his information: which coins he will pay 8-48M for. We cannot see that before we
fire, because his bid lands in the same slot as ours. Until something we can see predicts it, the cap is the right setting.
It is not a skipped opportunity: at our bid, the coins we would land there lose, for him as much as for us.

## 5. His logic, as far as the chain shows it

- **Universe:** the operators he follows: bundle wallet x fee routing, attempted on at least 30% of their runs (docs/16).
- **Entry guard:** a slot window (create slot or the next) and an exact-state guard: a buy that is not first behind the
  bundle fails, at 11,001 lamports.
- **Bid by conviction:** 0-1M on the coins he rates low (with failed spray attempts), 1.5-48M on the coins he rates high.
  His landed tips: 1.0, 1.5, 1.8, 2.4, 3.6, 4.8, 7.2, 12, 18, 24M and up. On high-conviction coins the tip is 1-2% of the
  stake, and the stake also rises (0.6-1.8 SOL). We have not found what he keys the conviction on. It is not in the create,
  the bundle, the metadata or his history on the signer.
- **Sticky on/off per operator, and holder mode:** after he skips an operator's coin he skips its next one 48% of the time
  (11-15% otherwise). On holder-reward coins after such a skip, 78% of the time.
- **Hold:** the operator table's modal hold (mostly create + 10 on these signers). He eats the insider dumps when they come
  inside the hold.

## 6. Changes

Keep:
- `max_post_bundle_sol = 40` (section 4).
- The newest gates. They remove -5.60 of the -6.58 SOL live loss and keep the fires he goes for.

Worth adding (small, it copies what he does):
- **Skip a holder-reward coin when he did not attempt the signer's previous coin.** 268 of 3,847 fires. He skipped 78% of
  them, and his net on the rest is -0.05 SOL. Our live fills there: 48, -0.362 SOL. The bot already sees his attempts on the
  copied-wallet stream. It needs a per-signer record of whether he attempted the last coin by create + 1.

Do not add (tested, they cut his profit or do not hold):
- "his last decision on the signer was a skip" alone: removes 686 fires on which he made +5.78 SOL.
- holder mode alone: removes 1,122 fires on which he made +1.87 SOL.
- the operator bundle tip filter, a tweet/author/metadata filter, an early exit on "nobody followed" or "he did not show up"
  (section 4).
- a higher flat bid (section 4).

## 7. Files

- `crates/rugsnipe/tests/replay_days.rs`: the replay harness (ignored test).
  `REPLAY_DIR=<dir> REPLAY_OUT=<file> cargo test --release -p rugsnipe --test replay_days -- --ignored --nocapture`.
- `analysis/research/2026-10-25/five/`: the data pull, the export for the harness, the comparison, and the auction study
  (README there).

## 8. The exact replay, Oct 23 00:00 to Oct 25 17:58 UTC

The replay of sections 1-6 used a stand-in config and only the coins either wallet touched. This one runs the newest code on
**every launch the box would have parked**, with the box's own config. Scripts: `analysis/research/2026-10-25/five/exact/`.

**Inputs.** Every pump.fun create since Oct 23 00:00: 118,367, of which 73% landed in EU-led slots. The candidates are the
26,952 creates in class (`create_v2`, not mayhem) with a dev buy of at least 0.25 SOL: what `entry.follow` parks, and the
rule's coins are a subset. For each candidate the replay feeds:
- the create transaction itself;
- every trade of the create slot with its block index;
- every trade of create + 1..3, which the dump tracker reads;
- his attempts;
- the operator table at each refresh;
- the leader schedule of epochs 1065-1067.

**Config.** The box's: `mode = "probe"`, stage b, `follow = "on"` (rate 0.3), `leader_regions = ["eu"]`,
`dumper_skip = "skip"` on m.rapidlaunch.io, `hidden_dev_buy = "grpc"`, `max_post_bundle_sol = 40`, `max_premium = 0`,
`tweet_gate = "exit"`, `presend_mode = "off"`, `filter_mode = "skip"`. The entry filter only acts on the paper (E1) lane, so it
never changes a real fire. With `follow = "on"`, a launch fires exactly when its bundle wallets are followed:
`follow_verdict` gates the rule's coins and adds the parked ones. So the operator table only names the skips.

**The harness** (`crates/rugsnipe/tests/replay_days.rs`) now also builds what the cold lane builds:
- the leader map with the regions of `data/leader_regions.json`, so the hot loop's `skip_far_leader` runs;
- the real `DumpTracker`, fed each launch's run and its trades of create + 1..3;
- the follow tracker, starting empty at Oct 23 00:00. The box's own seed was built from Oct 23 too, but partly from
  operator coins only, so the replay is the newest code as if it had run since then.

**Live check.** While the newest config ran live (14:51-15:36 today), our bot sent on 17 launches. The replay fired on 14,
12 of them the same. The 5 the live bot bought and the replay skipped were follow adds whose wallets the box's seeded
tracker followed and the replay's had not yet.

### Results

| day | candidates | replay fires | he tried | he landed | fires he tried | fires he landed | his landings we skip |
|---|---|---|---|---|---|---|---|
| Oct 23 | 11,307 | 723 | 1,590 | 418 | 511 | 135 | 283 |
| Oct 24 | 9,857 | 696 | 1,209 | 293 | 467 | 84 | 209 |
| Oct 25 (to 17:58) | 5,788 | 397 | 683 | 160 | 227 | 42 | 118 |

Where his +33.5 SOL on these launches goes, by the newest bot's decision:

| decision | his landed coins | his net | share |
|---|---|---|---|
| skip_bundle_too_big (post-bundle > 40 vSOL) | 133 | +12.54 SOL | 37% |
| fire | 261 | +8.87 SOL | 26% |
| skip_far_leader (leader outside Europe) | 190 | +5.34 SOL | 16% |
| not followed (skip_dev_buy / not_followed / not_operator / uri_host) | 284 | +6.73 SOL | 20% |

| the replay's fires, by what he did | fires | value at +10 | our live landing rate | our live result per fill |
|---|---|---|---|---|
| he did not try | 611 | +6.0% | 42% (78 of 184) | -0.0145 SOL |
| he tried and failed (a rival was first) | 944 | +17.3% | 34% (92 of 267) | +0.0148 SOL |
| he landed | 261 | +7.6% | 0% (0 of 47) | - |

At our live landing rates and per-fill results, the newest bot comes out at **+1.1 SOL over the 2.7 days, against his
+33.5 SOL** on the same launches. Its money comes only from the coins he tries and loses. The coins he lands, we cannot
land: he is first, and our exact-state bound fails behind him. The coins he skips lose for us.

### The setting that moves it: `follow_min_rate`

The follow rate is his own attempt share on an operator's launches. Raising it drops the fires he would not try. Replayed
at each value, with the economics of the 0.3 run:

| follow_min_rate | fires | he did not try | he tried, failed | he landed | projected | Oct 23 / 24 / 25 |
|---|---|---|---|---|---|---|
| 0.3 (now) | 1,816 | 611 | 944 | 261 | +1.03 SOL | +0.60 / +0.53 / -0.10 |
| 0.4 | 1,701 | 543 | 908 | 250 | +1.26 SOL | +0.70 / +0.62 / -0.06 |
| 0.5 | 1,483 | 418 | 830 | 235 | +1.63 SOL | +0.74 / +0.80 / +0.09 |
| 0.6 | 1,153 | 256 | 699 | 198 | +1.96 SOL | +0.80 / +0.86 / +0.30 |
| 0.7 | 856 | 135 | 572 | 149 | +2.06 SOL | +0.88 / +0.84 / +0.33 |

0.6 is better than 0.3 on every day. It keeps 74% of the fires he tries and cuts 58% of the ones he skips. 0.7 adds
little and cuts volume further. The rest of the gap to him is structural: the cap above 40 (section 4), the non-EU slots
(the Frankfurt box lands poorly there), and the coins he lands first.
