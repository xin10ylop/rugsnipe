# 13. Tweet coins: the rule, its backtest on two days, and the tweet check in the bot (2026-10-24)

**Corrected by docs/14.** The trade files behind these numbers left out the pump fee and cut busy coins at 150
transactions; on complete data the rule makes +3.68% a trade (hold 10, all coins), the copy cut of `"exit"` loses, and
`tweet_gate = "log"` is the setting to run.

Data:
- every pump.fun create from 00:00 to 12:00 UTC on Oct 23 and on Oct 24 (18,656 and 17,135), decoded;
- the metadata file of every create with a dev buy of 0.95 SOL or more;
- the first 12 slots of 1,394 launches on the three launch tools: all of his attempts plus random samples of the rest;
- his month of landed round trips (docs/12).

The scripts are in `analysis/research/2026-10-24/` (`rows.py`, `policy*.py`, `final.py`, `signer_key.py`, `lateness.py`,
`late2.py`, `createonly.py`, `rivals_sel.py`, `whofirst.py`).

## 1. Result

**The rule.** Buy a launch only when all of these hold:

1. its metadata is on one of the three launch tools (`metadata.j7tracker.io`, `meta.uxento.io`, `m.rapidlaunch.io`);
2. the dev buy is a template amount: 0.95-1.05, 2.95-4.05, 4.95-5.05, or 7 SOL and up;
3. the post-bundle curve is at most 40 virtual SOL (docs/11's rug cap);
4. it is the **first coin on its tweet**, and the tweet is under an hour old;
5. the deployer is one **he traded in the last 7 days**.

Hold 10 slots after entry.

| | trades | per trade, net of our costs (1.77%) | win rate | net of his costs (0.75%) |
|---|---|---|---|---|
| Oct 23, 00-08 UTC | 107 | +4.0% [+0.4, +8.4] | 50% | +5.1% |
| Oct 23, 08-12 UTC | 31 | +5.1% [+0.6, +9.7] | 57% | +6.2% |
| Oct 24, 00-08 UTC | 169 | +5.7% [+3.2, +8.6] | 59% | +6.8% |
| Oct 24, 08-12 UTC | 35 | +4.8% [+0.6, +10.1] | 44% | +5.8% |
| **both days** | 342 | **+5.0% [+3.2, +7.0]** | 54% | **+6.0% [+4.2, +8.0]** |

- **Volume.** About 20 such coins an hour.
- **By hour.** Positive in 18 of the 21 hours with enough coins to measure (5 or more).
- **What the bot can run.** From Frankfurt the tweet verdict takes 100-400 ms, while most of these races are decided
  in the create slot (section 4). So the bot cannot wait for condition 4 before buying. It buys every coin that meets
  1-3 and 5 at full speed, then sells a copy one slot after entry (`tweet_gate = "exit"`): **+3.15% per trade**, 547
  trades, all four half-days positive. The +5.0% above is the value if the verdict were free.
- **Method.** The value is the realized position of the real first outside buyer after the dev bundle: sales so far
  plus the rest marked at that slot's price (docs/12 section 5). The samples are weighted to the whole universe. The
  brackets are 95% bootstrap intervals.

## 2. What each condition adds

Both days pooled, hold 10 slots, net of our costs:

| | per trade | coins / h |
|---|---|---|
| tool host + template dev buy | +0.2% [-1.1, +1.3] | 99 |
| + post-bundle at most 40 SOL | +0.2% [-1.1, +1.3] | 91 |
| + first coin on a fresh tweet | +2.5% [+0.9, +3.8] | 52 |
| **+ a deployer he traded in the last 7 days** | **+5.0% [+3.2, +7.0]** | **20** |
| his deployer list without the tweet check | +2.8% [+1.3, +4.3] | 32 |
| his deployer list, coin not first on its tweet | -0.9% [-3.3, +1.4] | 12 |

**The tweet check alone is not stable.** Held 8 slots, net of our costs:

| | 00-08 UTC | 08-12 UTC |
|---|---|---|
| Oct 23 | +2.6% | -1.7% |
| Oct 24 | +3.8% | +0.8% |

**His deployer list alone is not stable either.** On Oct 24 08-12 it made -3.2%.

**Together they hold in every half-day.** Roughly 2 of his deployers' coins in 5 are copies of an earlier coin on the
same tweet, or are on stale tweets. Skipping those is the tweet check's value on his list.

**When we win the first spot, we are not left with the worse launches.** Value before costs by who took the first spot
on the rule's coins:

| first spot | Oct 23 | Oct 24 |
|---|---|---|
| a rival | +7.9% (108) | +7.8% (136) |
| him | +6.1% (30) | +6.0% (35) |
| us | none | +7.2% (33) |

**The deployer key.** The bot's table keys on the create's signer (fee payer).
- On tool coins the signer differs from the `creator` field 31% of the time. In those cases the creator is a fresh
  address on almost every coin, so the signer is the deployer's reusable wallet.
- Keyed by signer, with history only from the two decoded half-days, the rule made +3.6% [+2.1, +5.3] (345 trades).
- Keyed by signer or creator it made +4.6% [+2.9, +6.3] (447). The signer-only history here is short, so the bot's
  7-day table should land near that figure.

## 3. What did not hold up

- **The coin's name or ticker in place of the tweet check.** Use "name or ticker not seen in the last 2 minutes":
  - 78% of the coins it picks are tweet-first;
  - on Oct 24 08-12 it made -3.0%;
  - the coins it picks that are not tweet-first lost -13.7%.

  The metadata file must be read.
- **Buy every template coin at once and sell early when the check fails.** On Oct 24 08-12 this made -1.5%, against
  +1.0% for checking first. Coins that fail the check lose even on a 2-slot hold (-0.4% net).
- **Tweet author features** (followers, replies; docs/12 R5) added little next to tweet-first and are left out (they
  need a Twitter lookup).
- **His live attempts in place of the 7-day list.** Using the creators he attempted earlier the same day (his wallet
  watched live, failed buys included) gives:

  | | 00-08 UTC | 08-12 UTC |
  |---|---|---|
  | Oct 23 | +6.1% | +3.6% |
  | Oct 24 | +5.2% | +1.9% |

  That is no better than the table of his landed trades, which the bot already has.

## 4. Timing: why the bot cannot wait for the check

Value of each real outside buyer on tweet-first coins, 8 slots after their entry, before costs:

| entry | value |
|---|---|
| create slot | +3.1% |
| create + 1 | +3.3% |
| create + 2 | +0.2% |
| create + 3 | -1.2% |
| 1st buyer | +5.8% |
| 2nd buyer | +3.2% |
| 3rd buyer | +2.4% |
| 4th buyer | +0.7% |

Where the first outside buy lands:
- in the create slot on 55% of launches, a median of 62 block positions after the bundle;
- in the next slot on 31%;
- nowhere in the first 12 slots on 9%.

So the verdict must come back within about one slot of the create.

**Fetch time from Frankfurt: too slow to wait for.** Public probe machines in Frankfurt (Globalping: Oracle, OVH,
synlinq, PFWeb) timed a fetch of a file the Frankfurt edge did not hold yet. A new coin's file is in that state for
whoever fetches it first. Time to first byte, after the connection was set up:

| tool | Frankfurt | Ashburn (US East) |
|---|---|---|
| uxento (never cached) | 222-388 ms | 68-70 ms |
| j7tracker (a file the edge did not hold) | 256-411 ms | 66-118 ms |
| rapidlaunch | 105-112 ms | 14-132 ms |

A file the Frankfurt edge already holds comes back in 9-25 ms. Most old files are held there, because others fetched
them first.

On his deployer-list coins:
- The first outside buy lands in the create slot on 64% of launches, in the next slot on 33%.
- In the create slot it lands tens of milliseconds after the create.
- Our bot was first on 40 of the 125 of these launches it raced on Oct 24. 30 of the 40 were create-slot wins, a median
  of about 83 block positions after the create.

A buy that waits 100-400 ms for the verdict loses those races. So the bot buys at full speed and lets the verdict
decide the hold (`tweet_gate = "exit"`).

Net of our costs, hold 10, on his deployer-list coins:

| | Oct 23 00-08 | Oct 23 08-12 | Oct 24 00-08 | Oct 24 08-12 | both days |
|---|---|---|---|---|---|
| wait for the verdict, buy only the first coin (`"on"`) | +4.0% | +5.1% | +5.7% | +4.8% | +5.0% (342) |
| buy all at full speed, no tweet check | +2.9% | +1.9% | +3.6% | -0.4% | +2.75% (545) |
| **buy all at full speed, sell a copy one slot after entry (`"exit"`)** | +2.5% | +2.2% | +4.7% | +0.3% | **+3.15%** (547) |

`"on"` has the best per-trade value, but from Frankfurt it gives up most first spots. `"exit"` keeps them.

**What paying could and could not fix:**
- **A faster Twitter feed** (X API Pro, a crypto tweet tracker) does not help. The bot has to tie each new coin to
  its tweet within about 20 ms of the create, and only the coin's metadata file holds that link. The coin's name or
  ticker appears in its tweet's text, author or quoted tweet for 62% of tweet-first coins and for 53% of copies, so
  name matching cannot tell them apart.
- **A server next to the tool servers** (US East) fetches in about 70 ms, then needs about 40 ms more to reach
  Frankfurt, and must see the create on its own feed first. That is still well over 20 ms.
- **The launch tools' own live feeds** of the coins they deploy (mint plus tweet, as the tool sends the create) would
  give the link instantly. No public API was found for j7tracker, uxento or rapidlaunch. If one sells subscriber or
  API access, that is the one paid item that could make `"on"` work.

**The late pass.** A pass that arrives after the create slot may still fire for `entry.tweet_late_slots` (1) more
slots. The buy stays bound at the exact post-bundle state, so it lands only if nobody bought behind the bundle
meanwhile, and otherwise costs a failed attempt (about 15,000 lamports).

## 5. Expected result

- About 32 deployer-list coins an hour in 00-12 UTC (the tool, template and cap filters included). The other 12 hours
  are not measured.
- With `"exit"` at today's first-spot rate on these coins (32%) and 0.5 SOL stakes:
  - about 10 trades an hour at +3.15%;
  - if the rate holds all day, about **+3.8 SOL a day** at our costs, or +5 at his.
- The tip is a fixed cost, so a 1 SOL stake roughly doubles that.

The risks:
- The other 12 hours are unmeasured.
- The deployer table needs a working refresh (section 6).
- The `"exit"` cut relies on a sell landing about one slot after the verdict.

## 6. The bot (this branch)

What the tweet check does:
- It keeps the whole create uri (`TxShape.uri`, up to 160 bytes).
- It fetches the metadata off the hot thread (`crates/rugsnipe/src/tweet.rs`):
  - 4 worker threads share one keep-alive connection pool;
  - a warm thread touches each tool host every 20 s.
- It ranks each coin on its tweet in create order (`TweetBook`).
- With `"exit"` (use this one) the hot loop fires at full speed:
  - A copy, stale or no-tweet verdict that is in before the buy skips it (`skip_tweet_*`).
  - One that comes after the buy cuts the position's hold to the current slot, so it sells at the slot close
    (`cut` in the status line).
  - A first coin keeps its hold, and so does a failed fetch.
- With `"on"` it holds the parked create until the verdict:
  - fires only on `first`;
  - otherwise skips as `skip_tweet_copy`, `skip_tweet_stale`, `skip_tweet_none`, `skip_tweet_error` or
    `skip_tweet_late`, with no nonce or risk taken.
- It journals every verdict: `tweet <mint8> <verdict> rank .. age ..s sid .. fetch ..ms verdict ..ms slot+k <outcome>`.
- The status line gets `tweet <mode> asked .. first .. copy .. stale .. none .. error .. at slot+0/+1/later ../../..
  p50 ..ms p90 ..ms skipped .. late .. fired .. cut ..`.
- `check.sh` shows verdicts by kind with their delay, and today's closed trades by verdict.

Config for the rule (`[entry]` keys; the rest of the operator setup stays):

```toml
[entry]
require_bundle = true
operators_only = true                    # the deployer list: entry.operators_file
operator_holds = false                   # hold 10 for all (below), not the table's per-signer hold
uri_hosts = ["metadata.j7tracker.io", "meta.uxento.io", "m.rapidlaunch.io"]
dev_buy_sol = []
dev_buy_round = false
dev_buy_ranges_sol = [[0.95, 1.05], [2.95, 4.05], [4.95, 5.05], [7.0, 1000.0]]
max_post_bundle_sol = 40.0
min_stake_tier = 0
presend_mode = "log"                     # the rule was measured without the picks model
tweet_gate = "exit"                      # full speed; copies sold one slot after entry (section 4)
tweet_max_age_s = 3600
tweet_late_slots = 1

[exit.default]
hold_to_create_slots = 10
```

**The deployer table.** It must remember 7 days and be refreshed every 4 hours.

The refresh used to read his history only through the Helius Enhanced API. With the Helius key at its usage limit it
fails and the old table stays. `operators_refresh.py --source rpc` reads it over standard JSON-RPC instead
(getSignaturesForAddress plus getTransaction), so it runs on Triton:
- about 140 calls for 2 hours of his history, about 20 s;
- this source skips his failed attempts and the bundle and dump facts. The bot does not use them: it uses
  `hold_slots` and `stake_tier`.

To switch the refresh:

1. Add the Triton URL to `/etc/rugsnipe/keys/api-keys.toml` as a line `TRITON_URL = "https://<endpoint>/<token>"`.
2. Run `sudo systemctl edit rugsnipe-operators.service` and enter:

   ```ini
   [Service]
   ExecStart=
   ExecStart=/usr/bin/python3 -I /opt/rugsnipe/src/analysis/scripts/strategy/operators_refresh.py --source rpc --api-keys /etc/rugsnipe/keys/api-keys.toml --hours 4.5 --decay 1.0 --keep-days 7 --min-n 1 --out /etc/rugsnipe/operators.json --state /var/lib/rugsnipe/state.db --mode 0640 --group rugsnipe
   ```

   - Each run reads the last 4.5 hours and adds them to the table.
   - A deployer stays listed for 7 days after his last trade on it.
   - The `ExecStartPost` clock step still uses Helius. With `operator_holds = false` its clocks are unused, and it
     fails harmlessly.
3. Fill the 7 days once by hand. This takes about 11,000 calls and 25 minutes:

   ```sh
   sudo python3 -I /opt/rugsnipe/src/analysis/scripts/strategy/operators_refresh.py --source rpc --api-keys /etc/rugsnipe/keys/api-keys.toml --hours 168 --decay 1.0 --keep-days 7 --min-n 1 --out /etc/rugsnipe/operators.json --mode 0640 --group rugsnipe --rps 8
   ```

   Then `sudo systemctl try-restart rugsnipe-hot`.

**Order of steps:** start with `"exit"`, which trades at today's speed. In `check.sh` read the verdict delay
(p50 / p90) and today's trades by verdict.

`"on"` is worth trying only if the delay from the box is under about 20 ms, so the verdict lands inside the create
slot. The measurements say it will not be.
