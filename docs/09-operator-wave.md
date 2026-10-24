# 09. The operator wave (2026-10-23)

> **Superseded on the live box by docs/10 (2026-10-24).** The rebuilt hot path (micro-scalp v2) has none of the
> keys below; `rugsnipe-ctl config validate` refuses them. This document stays as the record of the operator wave.

A rule set decoded from the best wallet of 2026-10-23 (`AHwJSnFNUiAU9FfT5ewW2osqMo938WtL7KhfsJyfpaN2`): 430 landed
round trips, +17.8 SOL. Every rule below is a config switch on the LEAD policy's entry (docs/04 section 2); all are
off by default, so a box without the new keys behaves as before.

## The rule set (what the data says)

1. **Entry: first behind the dev's own bundle, at zero premium.** The first trade right after the create transaction
   and the creator set's same-slot buys. The buy asserts the exact post-bundle curve state and fails cheaply (base fee;
   the tip reverts) if anyone traded first: 430 of 430 landed with no outside buyer ahead. Slot window [create, create + 1]
   (278 in the create slot, 152 in the next).
2. **Picks.** A round dev buy (most of the profit from 2-5 SOL); a bundle of exactly 3 wallets outside the creator set
   (69% of trades); above all **repeat operators**: the 33 create signers with 3+ trades carry 289 of the 430 trades and
   13.6 of the 17.8 SOL.
3. **Exit: 100% at create + H**, H a constant per create signer: the sell's slot guard starts at the decision slot L,
   and L - create is 10 (207 trades, 48%), 6 (186, 43%), 3 (27) or 1 (10), never anything else. No take-profit, no stop,
   no insider or idle trigger (each explains at most 28% of the sells); the per-signer hold explains 93% of the sells of
   signers with 2+ trades in sample (74% out of sample in the decode). Unknown signers: 6 or 10 at even odds.
4. **Fees.** Base fee + a flat ~1.5M tip paid only on landing; fan-out to many relays behind one durable nonce.

## How the bot does it

| Rule | Config | In the hot loop |
|---|---|---|
| zero premium | `entry.max_premium = 0.0` | the parked run's last trade is the post-bundle state; `min_tokens_out` is the exact quote there, before our own trade (`setup::min_tokens_out_at`) |
| first behind the bundle | `entry.require_bundle = true` (as shipped) | the park fires behind the run; a trade beyond the run first is `skip_not_first`, and so is a buy inside the run by a wallet outside the creator set and the contiguous bundle (the post-bundle anchor never moves to it: below) |
| repeat operators | `entry.operators_file`, `entry.operators_only` | lookup by the create's fee payer (`operators::create_signer`); unlisted = `skip_not_operator` (0x54) |
| bundle of 3 | `entry.bundle_wallets = [3]` | the park's distinct run buyers outside the creator set; else `skip_bundle_shape` (0x55) |
| round dev buy | `entry.dev_buy_round = true` (1-6 SOL), or the `entry.dev_buy_sol` list | whole SOL +- `dev_buy_tol_sol` in `[dev_buy_round_min, dev_buy_round_max]`; beside the list either passes; else `skip_dev_buy` (0x50) before the park |
| price / bundle size, uri host | `entry.max_post_bundle_sol`, `entry.max_bundle_sol`, `entry.uri_hosts`, `entry.uri_hosts_deny` | the pick template below |
| exit at create + H | `[exit.default] hold_to_create_slots` (unlisted default) + the table's `hold_slots` | `ExitReason::HoldTo` (`hold_to`): sells 100% at dump bounds on the first slot the feed shows at or past create + H |
| stake tier (2026-10-24) | `entry.stake_by_tier = [l0, l1, l2]`, `bid.by_tier = [b0, b1, b2]`, `entry.min_stake_tier` (the floor, `skip_low_tier`) + the table's `stake_tier` (0..2; an unlisted signer is 0) | the wallet's stake is `0.4 SOL x 1.5^a x 2^b`: `a` is jitter (two fair coin flips, no information), `b` its confidence tier, 82% of a signer's attempts at the signer's modal tier. Landed returns by tier +2.3% / +7.3% / +21.3%, bids about 1.5M lamports at tier 0 and 6-24M (about 1% of the stake) above, which is how it wins the contested launches (tonight it took 14 of 16 head-to-head races on bid, 15 in the same slot). The size only narrows the signed ArmSet size and the risk cap (tier 2 = the full size); the bid replaces `bid.lamports` and stays under `risk.max_bid_lamports`. Journal: `operator <mint> hold_to create+H (n, bundle_wallets, tier)` |
| no other exits | `tp = []`, `insider_sell = "off"`, `no_activity_ms = 0`, `phantom_sl = { after_ms = 0, drop = 0.0 }`, `max_hold_slots = 0` | each rule is off at that value; they coexist with `hold_to` when on (first trigger wins) |
| fees | `[bid]`, `[senders]` (as configured on fra1) | unchanged |

### Picks: the template and its knobs

The copied wallet's buys, before the send, look like this: a **round dev buy of 1-6 SOL**, a bundle of **3 wallets**
outside the creator set, **dev + bundle about 7 SOL (p90 17)**, a **post-bundle mcap of 36-65 SOL** (post-bundle
virtual SOL about 37-47: mcap = V^2 / 32.19 on the fresh curve), uri hosts `uxento.io`, `j7tracker.io` and `gg.sc`
favoured and `ipfs.io` disfavoured. The 2026-10-23 loss sat outside it on every axis that matters (dev 4 SOL + a
13 SOL bundle, post-bundle virtual SOL about 58, the operator dumped at create + 2, -58%). Each number is an `[entry]`
key, all off by default and validated at start; every gate records its code and skips before the risk reservation and
the nonce (nothing sent):

| Template | Key (suggested) | Where it runs | Skip |
|---|---|---|---|
| post-bundle price / mcap (the main one) | `max_post_bundle_sol = 47.0` (> 30, the fresh curve) | the fire point (4d), on the reserve the buy is bound at: the parked run's anchor, else V0 (the curve after the dev buy; paper, `require_bundle` off) | `skip_bundle_too_big` (0x57) |
| dev + bundle SOL | `max_bundle_sol = 17.0` | the fire point (4d): the create's own buys + the run's creator-set and bundle buys (without a run, the dev buy) | `skip_bundle_sol` (0x58) |
| round dev buy | `dev_buy_round = true`, `dev_buy_round_min = 1.0`, `dev_buy_round_max = 6.0` | the create (4a), before the park | `skip_dev_buy` (0x50) |
| 3 wallets | `bundle_wallets = [3]` (existing) | the fire point (4c) | `skip_bundle_shape` (0x55) |
| uri hosts | `uri_hosts = ["uxento.io", "j7tracker.io", "gg.sc"]`, `uri_hosts_deny = ["ipfs.io"]` | the create (4a), on the host the wire decoder keeps (`TxShape::uri_host`: `https?://` up to the first `/`, lower case, at most 64 bytes); a suffix on a label boundary, any case (`uxento.io` matches `meta.uxento.io`); a uri with no such host fails a non-empty allow list | `skip_uri_host` (0x59) |

The status line shows ` | template too_big N bundle_sol N dev_buy N uri_host N` while any of them (or
`dev_buy_sol`) is on, and the gate fragment ` wallets@fire 0-5 a/b/c/d/e/f`: the park's bundle-wallet count at each
fire, which is what `bundle_wallets = [3]` is checked against. The fire-point skips also journal
`template <mint> skip_bundle_too_big post-bundle <v> > <cap> SOL`.

**The post-bundle anchor** (2026-10-23, Sj3qBo: the bound was computed after an outside 1.78 SOL buy inside the run,
+11.4% over the post-bundle spot at zero premium, -12.9%). The park re-derives its run from every trade seen, in block
order: the bundle is the creator set's trades (signer, `creator`, fee payer, the create transaction's signers and
buyers) and the buys chained behind the create at most 2 indices apart within create + 5 (rt.json's gap-3 run); a sell
moves the anchor only when it is a bundle wallet's or the creator set's. The anchor is the last bundle trade. Any other
buy ends the run and the fire is `skip_not_first` (decided at the fire, so a bundle trade delivered out of order still
joins first). The limit of an index rule: an outside buy chained to the bundle (gap <= 2) still reads as a bundle wallet,
as it does in rt.json's `n_bw`; `bundle_wallets = [3]` and `max_bundle_sol` then see the extra wallet and SOL.

**Is the count 3 at fire time?** The park fires on the first of: a trade at create + 5, the stream's run-complete
signal, or `bundle_wait_us` after the **first** bundle buy it processed (not the last). Every trade of one polled batch
is counted before the timer is looked at, so a bundle that reaches the box in one burst (one entry on a shred feed, the
gRPC updates of one bundle) counts all of its wallets at 300 us; a bundle transaction arriving more than 300 us after
the first is missed and `[3]` skips the launch as `skip_bundle_shape` (no cost, a missed launch). Read `wallets@fire`
against `feed_bundle.bundle_wallets` of the same launches: if the fire's count is often below the block's, raise
`bundle_wait_us` (1,000-2,000; each microsecond is entry latency) rather than widening `bundle_wallets`.

## What is not copied

* **Their on-chain guard.** Their buy asserts the curve's exact post-bundle state, so any trade ahead (a buy or a sell)
  fails it. Ours is the zero-premium `min_tokens_out`: it fails on any buy ahead (the price moved up) but lets a buy
  through behind a sell (a better price). Same cost on failure (base + priority fee, the tip reverts).
* **Their send path.** We already fan out to five relays behind one durable nonce; their relay set and tip policy
  (flat ~1.5M, landing only) are not reproduced beyond what `[bid]` and `[senders]` already do.
* **Their detection latency** is unknown. They landed first 430 of 430 times; we land in the create slot about 62% of
  the time with a European leader. At zero premium every late buy is a cheap failure, not a bad fill.
* **The bundle count.** rt.json's `n_bw` counts the confirmed block's run (gap <= 2, index <= create + 5) minus the
  creator set; the park counts the same run over what the stream delivered before the fire, saturating at 5. It can
  read lower than the block when a bundle transaction arrives late (see "Is the count 3 at fire time?").

## The operator table

`analysis/scripts/strategy/operators_from_rt.py rt.json --out data/operators/ahw-2026-10-23.json` writes, per
`csig_signer` (the create's fee payer): `n`, `wins` (pnl > 0), `pnl_sol`, mean `dev_sol`, modal `n_bw` as
`bundle_wallets`, and `hold_slots` = the modal `swin[0] - cs` (ties: the latest launch; n < 2: 6). It prints the
L - create check (430/430 in {1, 3, 6, 10}) and the top 40. `--min-n 3` keeps only the repeat operators. Test:
`python3 analysis/scripts/strategy/test_operators_from_rt.py`. Operators rotate wallets: rebuild from a fresh decode
before trusting an old table (daily: next section).

## Keeping the table fresh

A one-day table goes stale in a day: 60-70% of the best wallet's trades each day come from operators absent from the
previous day's table (in the 6 h check below, 64 of 133 round trips), and operators that rug must drop out.
`analysis/scripts/strategy/operators_refresh.py` (stdlib only, `python3 -I`) rebuilds the table from the chain:

1. **The wallet's window** (`--wallet`, default `AHwJSnFN...`; `--hours 24`): landed trades from the Helius Enhanced
   Transactions API (100 per call; SOL from `accountData`, tokens from `tokenTransfers`), failed buy attempts from
   `getTransactionsForAddress` (status failed, 100 per call; mint = the pump buy's account 2, create slot = the buy's
   slot-guard lower bound).
2. **Per launch, one call:** `getTransactionsForAddress` on the bonding curve, ascending from its first transaction
   (the create) to create slot + 3 (the buy's own slot window starts at the create slot), succeeded only -> the create's fee payer (the table key), create slot, creator set,
   the bundle run and `n_bw` exactly as `exec/build.py` decoded rt.json, and any creator-set sell at create + 0..3.
   L is the lower bound of the SELL's slot guard (program `A5c1eN7M...`, args `(min_slot, max_slot)`), read from the
   sell's Enhanced record at no cost; H = L - create.
3. **Per signer:** `n`, `wins` (pnl > 0), `pnl_sol`, mean `dev_sol` (dev buy + bundle run, as rt.json), modal
   `bundle_wallets`, modal H as `hold_slots` (ties: latest launch; n < 2: `--default-hold` 6), and the evidence fields
   `attempts`, `launches`, `dumps`, `last_seen_slot`, `last_seen_unix` (the hot loop ignores unknown fields).
4. **Merge** with the existing table (`--merge`, default `--out`): a signer seen now gets its fresh stats plus
   `--decay` (0.5) x its old `n`/`wins`/`pnl_sol` and keeps its old hold unless it has 2+ fresh round trips; an unseen
   signer is kept, decayed, while last seen within `--keep-days` (3; a table without `last_seen_unix` counts as seen
   at its mtime); then `--min-n` (2). `--drop-dumpers` drops every signer with 2+ launches in the window whose creator
   set sold within 3 slots of the create (including launches the wallet only attempted).
5. **Our own outcomes** (`--state /var/lib/rugsnipe/state.db`, opened read-only): closed positions joined to the
   create signer through `outcomes.signer` (CreateEvent.user), added as `our_n` / `our_pnl_sol`. `positions`,
   `exits`, `intents` and `presend` store no signer; positions without an `outcomes` row are counted and skipped.

The table is checked (32-byte keys, `hold_slots >= 1`), written to a tmp file, fsynced and renamed (`--mode 0640
--group rugsnipe`), with a summary on stderr (added / kept / dropped with reasons, holds, top 20 by n). Fewer than
`--min-trades` (20) round trips aborts with exit 2 and leaves the old table. `--dry-run` prints the JSON to stdout
and writes nothing. Test: `python3 analysis/scripts/strategy/test_operators_refresh.py`.

**On the box:** `rugsnipe-operators.timer` (hot role, installed by `deploy/install.sh`) runs
`rugsnipe-operators.service` every 4 hours at :50 UTC (03:50 .. 23:50; `--min-n 2 --drop-dumpers`), so a new operator the wallet trades twice is in the table within hours, and the 23:50 run starts the new day on a fresh table. In the 2026-10-23 decode, 60% of the wallet's trades were on the previous day's table signers; the next evening only 15% were, so a daily table decays to a quarter of its volume. The service runs as root
(reads the 0700 keys, chowns the table), reads `RPC_URL` from `/etc/rugsnipe/keys/api-keys.toml` at run time, writes
`/etc/rugsnipe/operators.json` (0640 root:rugsnipe), then `systemctl try-restart rugsnipe-hot`: **the hot loop
loads `entry.operators_file` only at start**, so the restart is what applies it (rugsnipe-guard holds the exits
for the 5-20 s; a stopped hot loop is not started). A failed pull restarts nothing. Set
`operators_file = "/etc/rugsnipe/operators.json"` in `[entry]`.

```bash
sudo deploy/install.sh --role hot --no-build          # renders rugsnipe-operators.{service,timer}
sudo python3 -I /opt/rugsnipe/src/analysis/scripts/strategy/operators_refresh.py --dry-run --min-n 2 \
     --drop-dumpers --state /var/lib/rugsnipe/state.db > /tmp/operators.preview.json   # look first
sudo systemctl start rugsnipe-operators.service       # once now: writes the table, try-restarts rugsnipe-hot
journalctl -u rugsnipe-operators -n 60 --no-pager     # the summary; then the hot journal: "operators: N signers"
sudo systemctl enable --now rugsnipe-operators.timer  # daily from tonight
systemctl list-timers rugsnipe-operators.timer
```

### Exit clocks

Each operator sells on its own clock, and it repeats: the slot after the create at which its creator set (the
create's signers, `user`, `creator` and the bundle run's wallets) has sold 25% of the tokens it bought in the run.
DRXPafDu sells at slot 12 / 11.5 on 95% / 91% of its launches on two consecutive days, ENskGQev at 12 / 12,
F5XvCe42 at 8 / 7. The table's hold is the copied wallet's modal hold, and for some signers it rests on two or three
of his round trips: F5XvCe42's hold 1 came from 2. After every table refresh,
`analysis/scripts/strategy/operator_clock.py` (an `ExecStartPost` of `rugsnipe-operators.service`) pulls the
last 48 h of launches of every signer whose `n` is below 5, measures its clock on the bundled ones, and replays a
0.5 SOL buy in the first spot behind the bundle at every hold 1..15. The clock hold (median sell slot - 1, within
2..15; at least 10 bundled launches, a third of them showing the sell) replaces the wallet's hold only when it made
more in each half of those launches on its own. The wallet's hold is kept as `hold_his` and comes back when the
clock no longer qualifies; `clock_slot`, `clock_share` and `clock_launches` record the evidence. A failed run leaves
the wallet's holds.

**Evidence (2026-10-24, 48 h, the first-spot P&L after our costs).** Read on two of three 16 h periods and scored
on the third, three rotations: the wallet's holds +13.78 / +9.07 / +18.18 SOL, the clock with the both-periods check
+16.99 / +13.50 / +19.47. F5XvCe42 (59 launches) made +0.33 / +0.43 at hold 1 and +4.62 / +3.79 at hold 7 on the two
days. What did not work: the clock for every operator without the check (it lost on most of the operators it
moved: 9ncm3Mfx, 8YcbyX92, BADxcCcz, DRXPafDu), each operator's best hold fitted on one day (lost on the other day in
both directions: +6.71 against +9.00, +4.51 against +6.14), selling at the typical peak, and every exit that reacts
during the hold: a sell on the creator set's first sell, on a big outside buy or when no followers come, take-profits
and stop-losses (a rug is one block: the creator set sells together in one slot, so the reaction is late, and every
early exit also cuts the winners).

**Losers leave the table.** The same step drops a measured signer (at least 10 replayed bundled launches) that loses
at every hold 1..15 in both halves of its launches on its own: no exit timing would make it pay. More than 25% of the
table at once reads as a data problem and drops nothing (`--max-drop-share`); `--no-drop-losers` turns it off. The
next refresh can bring a dropped signer back; this step measures it again. Evidence (2026-10-24, 78 operators, 3,177
bundled launches in 48 h, first-spot P&L after our costs): decided on one 24 h window and scored on the other, in both
directions, the flagged operators' later launches summed -1.12 and -1.38 SOL, and the 4 of them we traded lost too.
On the live table it flags CmwFWFQs: 71 launches in bursts (5 within 3 minutes) that nobody follows, -1.22 SOL at
its best hold; our 10 trades on it lost -0.17 SOL, fees and costs only. The 8 operators it flags over the 48 h
include 6 the refresh had already dropped. The looser rule, the wallet's own hold losing in both halves, flags 17
operators whose later launches still lose (-0.56 SOL) but takes 79 winning launches worth +6.9 SOL with them.

```bash
sudo python3 -I /opt/rugsnipe/src/analysis/scripts/strategy/operator_clock.py --dry-run   # the decisions, nothing written
journalctl -u rugsnipe-operators -n 40 --no-pager                                          # after a refresh: "operator_clock: ..."
```

Test: `python3 analysis/scripts/strategy/test_operator_clock.py`.

**Check (2026-10-23, 6 h window, `data/operators/ahw-2026-10-23b.json`):** 132 round trips on 132 mints + 369
attempted-only launches, 501 launches resolved with 0 errors, L - create in {1, 3, 6, 10} on 132/132; on the 28 mints
also in rt.json the signer, create slot, H, `n_bw` and `dev_sol` match 28/28. The table (min-n 2, drop-dumpers) has
15 signers; the 11 also in tonight's table have the same H 11/11 (100%), and 58 of 62 fresh round trips on tonight's
n >= 2 signers sold at that signer's listed H. 4 new signers; 4 dropped as dumpers (2 of them listed tonight,
attempted 10-12 times, landed 0; one new, 77EFxMYZ, was +0.70 SOL on 2 trades: the rule is blunt).
**24 h run (uncached, `--rps 4`, merged with tonight's table):** 444 round trips + 1,192 attempted-only launches,
1,636 resolved, 0 errors, 444/444 offsets in {1, 3, 6, 10}; 2,153 calls (1.32 per launch), 564 s. 47/47 signers with
n >= 2 in both have the same H; table 48 signers (4 added, 44 kept, 109 dropped: 100 below min-n 2, 9 dumpers, among
them xxrr85qs, n 17, +0.16 SOL, 6 dumped launches). The default is now `--rps 5` with a create + 3 window (~7 min).

**Caveats.** (1) The Enhanced API returns landed transactions only (0 errored records in 24 h): failed attempts come
from `getTransactionsForAddress`; a bare `getSignaturesForAddress` would need one `getTransaction` per attempt
(~1,200 a day). (2) Both are Helius methods: a non-Helius `RPC_URL` (no `api-key=`) is refused. `--curve-source
enhanced` swaps the per-launch call for 1-3 Enhanced calls (1.4 per launch; no transaction index, so the bundle run
follows the listing order: signer, create slot and H matched gTFA on 13/13 launches, `n_bw` 12/13, `dev_sol` 10/13). (3) Signer rotation: a new create wallet is a new key with no history; the 24 h window catches it after its
first 2 round trips, the merge cannot. (4) RPC budget: about 1.2 calls per launch (1-3 pages on a busy curve) at `--rps 5` (attempted-only
launches are 2/3 of them; `--no-attempts` skips them and loses the dumper evidence from them). (5) The first refresh
merges tonight's table, whose window overlaps the new one by ~17 h: its counts are halved, not removed.

## Install

1. `deploy/install.sh` copies `data/` to `/var/lib/rugsnipe/data/` (the table lands in
   `/var/lib/rugsnipe/data/operators/`); or `sudo install -m 0644 data/operators/ahw-2026-10-23.json /etc/rugsnipe/operators.json`.
   With the daily refresh (previous section) use `/etc/rugsnipe/operators.json`: the timer rewrites it.
2. In `/etc/rugsnipe/rugsnipe.toml` (see the commented lines in `config/probe-a.example.toml`):
   `[entry]` `operators_file = "/var/lib/rugsnipe/data/operators/ahw-2026-10-23.json"`, `operators_only = true`,
   `bundle_wallets = [3]`, `max_premium = 0.0`, `dev_buy_sol = []`; `[exit.default]` `hold_to_create_slots = 6` with the
   other rules off as in the table above.
3. `rugsnipe-ctl config validate`, restart, and check the journal for `operators: 153 signers from ...`.
4. Watch `rugsnipe_decisions{outcome="skip_not_operator" | "skip_bundle_shape" | "fire"}`,
   `rugsnipe_exits{reason="hold_to"}`, the journal's `operator <mint> hold_to create+H` and `sell ... reason HoldTo`
   lines, and `fills.fail_reason`.

## Paper wide (2026-10-24)

The live bot trades only signers in the operator table (`entry.operators_only`). The copied wallet takes about a
third of its trades on signers it has never traded, so opening the table is the volume lever, but it needs a
measured edge first: projected from the picks model's out-of-sample precision (0.30 at t1, 0.44 at t2 on
unknown signers), the follower-rich share of the added launches comes out at 36-41% against a break-even of
about 52%. `rugsnipe-paper-wide.service` measures it live at no cost.

* **What it runs.** `rugsnipe-hot --mode paper` on the live `/etc/rugsnipe/rugsnipe.toml` with
  `deploy/paper-wide.toml` merged over it (`--overlay`: tables key by key, so every later change of the live
  gates applies to both). The overlay sets `entry.operators_exclude = true` (the mirror of `operators_only`:
  only the signers the live bot skips, so the two never take the same launch), its own kill-latch file, no
  Telegram (the live bot owns the token and its `/kill` polling), no metrics, and keeps only the
  post-execution `feeds.grpc` stream. Paper mode: no keys, no sends, no nonces, its own
  `/var/lib/rugsnipe/paper-wide/state.db` and rslog. Unlisted signers get `[exit.default]
  hold_to_create_slots` and tier 0.
* **Costs.** One extra gRPC stream on the `feeds.grpc` provider, the E1 fill lane at 1 req/s on `RPC_URL`,
  CPUs 6-9 (the live hot is pinned to 2-5).
* **Setup** (once): the paper instance loads the live unit's ArmSet (`--paper-unsigned-armset`: an ArmSet past
  its age still loads in paper):

```sh
A=$(systemctl show -p ExecStart --value rugsnipe-hot | grep -o -- '--armset [^ ;]*' | head -1 | cut -d' ' -f2)
[ -n "$A" ] && echo "ARMSET=$A" | sudo tee /etc/rugsnipe/paper-wide.env && sudo chmod 0640 /etc/rugsnipe/paper-wide.env \
  && sudo chgrp rugsnipe /etc/rugsnipe/paper-wide.env && sudo systemctl daemon-reload && sudo systemctl enable --now rugsnipe-paper-wide.service
```

* **Read it.** `sudo python3 -I /opt/rugsnipe/src/analysis/scripts/strategy/paper_wide_report.py` prints the
  paper trades (closed, per hour, win rate, mean and median return, net SOL, halves) next to the live bot's over
  the same window, and the verdict.
* **The rule.** After 150 or more closed paper trades: OPEN (`entry.operators_only = false` on the live bot)
  only if the paper mean return per trade is at least +3% and both halves are positive; otherwise keep the
  table. The bar is above zero because paper books every launch where we would have been first as landed,
  while live we land about 55% and lose the contested ones to higher bids. The 4-hourly table refresh restarts
  only the live bot; restart the paper instance after a refresh to reload the table it excludes.
* **Stop.** `sudo systemctl disable --now rugsnipe-paper-wide.service`.
