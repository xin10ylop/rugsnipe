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

## 9. Landing behind him: the bound, the exit and the copy (Oct 25, evening)

Two questions after section 8. Do our buys fail because only one buyer can land behind the bundle? And can a bot that just
tracks him (buys when his buy shows, with the right tip and exit) land in his block or the next and make money? Scripts:
`analysis/research/2026-10-25/five/copy/`.

### Both land: the failures were our own bound

- **We are as fast as he is.** On the 5-day coins, our buys that landed or failed on our bound sit 703 times in the
  create slot (median 92 block positions after the create, p10 40) and 419 times in the next. His landed buys: 1,060 in
  the create slot (median 91, p10 38) and 624 in the next.
- **There is no single spot.** On the coins he landed, 80 of our 83 attempts landed in the block right behind him and
  failed only on our bound. `max_premium = 0` makes `min_tokens_out` the exact post-bundle quote, so any buy ahead of ours
  (his) fails ours with pump error 6042. 618 of our 1,140 buys over Oct 21-25 failed that way.
- **Coverage** (section 8): we fire on 261 of his 871 landed coins. The rest are above 40 vSOL (133, his +12.54 SOL),
  under non-EU leaders (190, +5.34) or not followed (284, +6.73).

### The bound

The replay's 1,816 fires (Oct 23 00:00 to Oct 25 17:58). A fire our live bot really sent is filled where it landed; a 6042
failure is filled at its own position if that is within the bound. The others are filled one buy behind the first buyer
behind the run (our real median position). All are sold at create + 9:

| max_premium | fills | he did not try | he tried, failed | he landed | all fires | Oct 23 / 24 / 25 |
|---|---|---|---|---|---|---|
| 0 (now) | 177 | -0.99 | +1.27 | 0.00 | +0.28 SOL | +0.25 / +0.52 / -0.49 |
| 5% | 585 | -1.29 | +4.43 | +1.04 | **+4.19 SOL** | +3.34 / +1.20 / -0.35 |
| 10% | 1,004 | -4.03 | +4.20 | +0.10 | +0.27 SOL | +3.20 / -2.43 / -0.51 |
| 15% | 1,208 | -6.91 | +4.55 | -0.44 | -2.79 SOL | |
| 25% | 1,453 | -10.29 | +4.56 | -0.78 | -6.51 SOL | |

5% fills the coins he tries and the ones he lands, behind him. Wider bounds fill the coins nobody else wanted. With the 5%
bound, `follow_min_rate` 0.3 stays best (0.6: +2.95, 0.7: +3.17).

### The exit

On the complete 12-slot tapes (Oct 23 00:00 to Oct 24 12:00, 1,066 replay fires):

| exit | max_premium 0 | max_premium 5% |
|---|---|---|
| the operator table's hold H (now) | +1.61 SOL | +4.39 SOL |
| H - 1 | +2.24 | +5.10 |
| H - 2 | +1.80 | +4.61 |
| flat create + 9 | +2.25 | **+5.41** |
| flat create + 10 | +1.60 | +4.81 |

A flat +9 for every rule fire (`operator_holds = false`, `hold_to_create_slots = 9`). It sells before his +10 sells. On
the signers whose table hold is 6 it also beats that hold (+2.78 SOL against +2.19 at 5%).

### Copying his landed buy

His landed buys of 0.7 SOL or more at create + 0/1 over Oct 21-25 (651 coins; his net +49.0 SOL), copied with a 0.5 SOL
buy that lands D block positions after his, bound at his price + 10% and sold one slot before his table hold:

| D | 0 | 1 | 3 | 10 | 20 | 40 | 60 | 90 | 150 |
|---|---|---|---|---|---|---|---|---|---|
| SOL over 5 days | +10.55 | +10.28 | +8.24 | +5.22 | +4.16 | +2.86 | +2.31 | +0.90 | +0.10 |

- **The first positions behind him are taken before his buy shows.** They hold the other bots that fired at the create
  when he did. Their transactions are in flight before his buy is visible anywhere. A copy can only send once his buy is
  visible, so it lands one pipeline later. Our trigger-to-landing distance is 32 to 322 positions (decile mids, median
  about 90). A tip only orders the transactions that reach the leader together with ours.
- **We reach those first positions the way he does: by firing at the create.** On the coins we both fire, we already
  land a median of one buy behind him. That is where the 5% bound pays.
- **The exit has to come before his.** Copies sold behind his own sell (we see it, then sell) lost 23.4 SOL on all his
  buys: his sell is the dump. His hold is known. He sells at a create-relative slot per operator and eats the dumps
  inside it (section 5). The operator table stores that slot.

A copy bot from scratch at our real speed, with D drawn from our own landing distances (5 days, 0.5 SOL):

| copied | listed signers, sold at H - 1 | unlisted signers, best exit tried |
|---|---|---|
| his buys of 0.7 SOL+ | **+6.55 SOL** (338 copies; +1.62 / +0.49 / +5.28 / -0.38 / -0.47 by day) | -2.25 SOL (313, flat +5) |
| his tips of 8M+ | +4.19 (159; +0.37 / +0.66 / +2.89 / +0.39 / -0.11) | -0.47 (92) |
| all his buys | +5.95 (780) | -6.13 (904) |

On listed signers, H - 1 beats H (+2.76 on the 0.7 SOL+ buys), H - 2 (+5.30) and H - 3 (+4.09). Take-profits from +15%
to +60% did not help.

He trades unlisted signers a lot (904 of the 1,684 coins), but makes less there: +13.2 SOL, against +39.7 on the 780
listed coins. Every exit tried on them lost: flat +1 to +10, his last hold on the signer learned live, and his modal hold
by launch tool or by dev-buy size. Even an exit one slot before his actual first sell, which no bot can know, only breaks
even (+0.07 SOL on the 313 copies of his 0.7 SOL+ buys). Entered as close as he is (D = 0) and sold at +5, the same copies
make +2.32.

The copy's speed and entry cost are the same on both kinds of signer. 90 positions after his buy, the price is a mean +3.2%
over his fill on listed coins and +2.9% on unlisted ones (medians +1.5% and +0.5%). The difference is the move after
the entry. To one slot before his hold, listed coins rise a further mean +7.1% (median +0.5%). Unlisted coins rise +1.7%
to create + 5 (median 0.0%), under our round trip's cost of about 5 points. He gets those coins about 3 points cheaper by
being first, and still makes only +0.015 SOL a coin there, against +0.051 on listed coins. `copy_listed_only` stays on. On the coins the newest bot does not fire (Oct 23 on), the listed-only copy of his 0.7 SOL+ buys makes +4.51 SOL
(122 copies: Oct 23 +4.76, Oct 24 +0.07, Oct 25 -0.32). Most of it is Oct 23.

### His weak days

He made money every day (whole wallet: +11.38, +8.24, +26.14, +5.23, +2.82 SOL on Oct 21-25, the 25th to about 18:00).
The copy of his 0.7 SOL+ buys on listed signers did not. It kept 20-28% of his result on those coins on his strong days
and lost on his weak ones: +1.62, +0.49, +5.28, -0.38, -0.47. Each copy starts about 8 points behind him: about 3 points
of price, plus our round trip. Every filter below is known before our buy (`weakdays.py`, `weakdays2.py`), on listed
signers, sold at H - 1:

| copied | bound | total | days positive | Oct 21 / 22 / 23 / 24 / 25 |
|---|---|---|---|---|
| his buy >= 0.7 SOL | 10% | +6.55 SOL | 3 of 5 | +1.62 / +0.49 / +5.28 / -0.38 / -0.47 |
| his tip >= 4M | 10% | +4.77 | 4 of 5 | +0.69 / +0.35 / +3.32 / +0.58 / -0.16 |
| **his tip >= 4M** | **5%** | **+4.71** | **4 of 5** | +0.78 / +0.43 / +3.30 / +0.27 / -0.07 |
| his tip >= 8M | 5% | +4.08 | 4 of 5 | +0.46 / +0.81 / +2.86 / +0.04 / -0.08 |
| his tip >= 4M | 2% | +2.47 | 4 of 5 | +0.64 / -0.06 / +1.29 / +0.60 / +0.01 |
| his buy >= 0.7 SOL, our landing price <= his + 2% | 10% | +4.07 | 4 of 5 | +2.03 / +0.57 / +1.63 / +0.13 / -0.29 |

His tip is his conviction, in his own transaction. Keyed on it instead of his size, the copy holds up on his weak days.
Oct 24 goes from -0.38 to +0.27 SOL and Oct 25 from -0.47 to -0.07, for about 1.8 SOL less on the strong days. These
did not help: the operator hot for him (his last coins on the signer), his own recent run (last 2 or 6 hours), a
stop-loss at -20% or -35%, and his size of 1.0 or 1.5 SOL. High tips on unlisted signers stay flat or negative (tip >=
12M: -0.07 SOL).

### His current operators

The operator table on the box keeps three days of his signers (1,034 on Oct 25). The tables the backtest used were rebuilt
every four hours from his last 24 hours, with the old counts halved each time: 40 to 64 signers, his current operators.
On every signer, the tip-keyed copy makes +2.82 SOL, positive on 3 of 5 days. So the copy counts his landed launch
buys per signer live and copies a signer only after enough of them in a recent window (`recent.py`):

| his buys on the signer before this one | copies | total | days positive |
|---|---|---|---|
| (the backtest's tables) | 238 | +4.71 SOL | 4 of 5 |
| >= 1 in 24 h | 262 | +3.91 | 4 of 5 |
| **>= 2 in 24 h** | **226** | **+4.22** | **4 of 5** |
| >= 3 in 24 h | 200 | +4.30 | 3 of 5 |
| >= 2 in 4 h | 151 | +2.71 | 2 of 5 |
| any signer | 410 | +2.82 | 3 of 5 |

`copy_signer_min_buys = 2`, `copy_signer_window_h = 24`. The table still gives the hold, one slot before his, for the exit.

The count must survive restarts. The operator refresh restarts the hot service every four hours (03, 07, 11, 15, 19 and
23:50 UTC). A count started empty at each of those restarts keeps 99 of the 226 copies, +2.41 of the +4.22 SOL, positive
on 2 of 5 days (`restart.py`). So the cold thread keeps his buys per signer as the hot loop counts them, saves them every
minute and at shutdown (`copy_signers.json` beside `state.db`), and the next start seeds the hot loop with them
(`crates/rugsnipe/src/copybuys.rs`). Only the first start after the file is new counts from empty.

### Built: `entry.copy`

- **Trigger.** His landed buy of at least `copy_min_sol` with a tip of at least `copy_min_tip_lamports`, at create + 0
  or + 1: his executed trade on the gRPC stream, user = `follow_wallet`. The feed reads his tip from the same transaction:
  the SOL his wallet moves by top-level System transfers (`transfers_from`). With `copy_signer_min_buys`, he must have
  bought the signer's launches that many times in the last `copy_signer_window_h` hours (counted live, any size). The launch must have no park, no position and no buy of ours. With `copy_listed_only` (default
  true), its signer must be in the operator table. A park his buy made `skip_not_first` ends and the copy buys. A park
  whose run took his buy in as a bundle buy fires behind him itself.
- **Bound and window.** His post-trade reserve + `copy_max_premium` (10%), sent until create + `copy_late_slots` (2).
- **Gates.** Past the rule's own: template caps, dumper, pre-send, follow, leader and tweet. The stake and bid are tier
  0. The risk caps apply.
- **Exit.** One slot before his table hold, whatever `operator_holds` says. The tweet check's exit cut leaves copies alone.
- **Journal and status line.** Each copy journals `copy <mint> his <SOL> at create+<k> -> buy|log`. The status line adds
  `copy <mode> his small unlisted few seen buy log passed`: his landed launch buys, the ones not copied for their size
  or tip, an unlisted signer or too few of his buys on the signer, then the candidates copied, journaled or passed over.
- **Copy only.** `entry.rule_buys = false`: every create is kept for a copy and the rule decides nothing
  (`skip_rule_off`). Its gates (template, tweet, follow, dumper, leader, pre-send) and the tweet requests are idle, and
  the count caps and the daily stop serve the copies alone. With the rule on, its fires used about 300 attempts a day
  and filled stage B's cap by the evening, which stops the copies too.
- **Replay.** The copy replay ran the exact replay's events with `copy = "on"`, his buys of create + 1..3 also fed to the
  hot loop (`replay_days.rs`). It made 127 copies (72 at create + 0, 55 at create + 1) and lost none of the rule's
  1,816 fires. The 122 with a tape make +4.51 SOL sold at H - 1, against +2.55 at H. The harness now also keeps a fresh
  blockhash, as the cold lane does; without one, a long run paused buys as `skip_no_blockhash`.

### The caps

The replay lifts the count caps: 1,943 fires with the copy in 2.75 days, about 700 a day. Live, the box made about 300
attempts a day (289, 305 and 224 coins on Oct 23, 24 and 25): stage B's `max_probes_per_day = 300` binds, and the buys
stop for the rest of the UTC day once it does. At 5% a replay fire is worth +0.0023 SOL, so the cap keeps less than half of
what the bound adds. Raising it is a risk decision (`[probe] b.max_probes_per_day`, with `max_daily_loss_lamports` as the
stop).

### Changes

- `max_premium = 0.05`.
- `operator_holds = false` and `[exit.default] hold_to_create_slots = 9`: every rule fire sells at create + 9.
- `copy = "on"`, `copy_min_sol = 0.0`, `copy_min_tip_lamports = 4_000_000`, `copy_max_premium = 0.05`,
  `copy_listed_only = true`: his tipped buys on his listed operators, sold one slot before his hold. `"log"` journals the
  copies without buying.
- The bot from scratch, as backtested: `rule_buys = false` (the copies only), with `tweet_gate`, `tweet_feed`, `follow`
  and `dumper_skip` off (they served the rule's buys only).
