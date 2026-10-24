# 10. Micro-scalp v2: the dry-run strategy rebuilt on the live plumbing (2026-10-24)

This replaces docs/09 (the operator wave) on the live box. The code is the dry run's own code (0aae91c) plus the
live fixes that do not change the strategy. `deploy/bin/rugsnipe-microscalp-config` converts the box's config, and
`deploy/bin/rugsnipe-check` judges a live day against the dry run.

**Read section 2 before deploying.** v2 fixes every execution gap between the live run and the dry run (section 1).
It does not fix the gap that decides the P&L: the races we win are not the races the dry run counted. Re-scored with
the dry run's own exits, the 188 in-class races we won on Oct 21-23 lose 1.6-1.7% before costs and 2.6% after costs
at 0.5 SOL (95% interval -4.2% to -1.1%). The dry run's +1.1-1.6% per attempt assumed we take rank 1 on every launch,
including the ones a rival takes. v2 on these numbers is a measurement run with a hard daily stop, not +7-9 SOL a day.

## 1. Why live missed the dry run: the execution layers (Oct 21-23, chain truth)

Every transaction of the probe wallet (850) and the full tape of every attempted mint (474), pulled from chain and
replayed with the dry run's exits. The replay matches the dry run's own paper rows: median gap 0.00 points, mean
-0.12, 128 of 148 within one point. All columns except the last two are the mean per landed trade, in % of stake:

| period | attempts | landed | r1 | @fill | curve | real | SOL real | failed copies SOL |
|---|---|---|---|---|---|---|---|---|
| A: probe-A 0.02 SOL, fires on any create | 108 | 75% | -0.5 | -1.6 | -1.9 | -16.7 | -0.426 | 0.041 |
| B: 0.25 SOL, require_bundle, 4 SOL bound | 44 | 45% | -1.1 | -2.1 | -2.9 | -4.9 | -0.298 | 0.054 |
| C: bound fixed, sell bid 50k | 94 | 45% | -0.9 | -1.0 | -2.3 | -4.3 | -0.568 | 0.117 |
| D: sell bid 3.4M, old exit clock | 29 | 34% | +5.7 | +5.7 | +7.3 | +4.0 | +0.051 | 0.049 |
| E: new exit clock, 0.5 SOL | 125 | 46% | -3.1 | -3.6 | -5.2 | -7.9 | -2.363 | 0.310 |
| F: pre-execution fix, max_hold 50 | 69 | 64% | -3.3 | -3.4 | -4.2 | -9.6 | -1.252 | 0.167 |
| total | 469 | | | | | | **-4.856** | 0.738 |

- **r1**: the dry run's exits, entered at the dry run's fill (rank 1 at the post-bundle price).
- **@fill**: the same exits at our real fill.
- **curve**: our real exits on the bonding curve, net of pump fees.
- **real**: the chain's SOL deltas: tips, priority and base fees, failed sells.

Each gap between columns is a layer, and v2 closes each one:

| layer | live cost | cause | v2 |
|---|---|---|---|
| r1 to @fill (entry price) | 0.1-1.5 points | class gate wider than the dry run's; anchor moved by an outside buy | class gate and run derivation as the dry run's (s3) |
| @fill to curve (exits) | 0.3-1.6 points | exits drifted (max_hold 50, hold-to-create); sells landed 3-8 slots late on 50k bids | the dry-run exit spec verbatim; sells at 2M, fanned out to every sender, nonce first |
| curve to real (costs) | 1.4-5.4 points at 0.25-0.5 SOL | 3.4-3.5M priority fees on every copy, including the ones that land and fail | a 2M bid; the Harmonic copy as a Nozomi bundle (paid only when it lands); tip-only plain copies |

With every layer closed, a landed trade scores **r1 minus about 0.9 points** (a 4.5 mSOL round trip at 0.5 SOL). So
v2's result is set by r1, the value of the races we win. That is section 2.

## 2. The gap code cannot close: which races we win

Among the 401 in-class attempts (a bundle wallet outside the creator set), the dry run's exits at rank 1 score +1.17%
[-0.36, +2.69]. That is the dry run's own number (+1.14% over 22 h, +1.56% over 5.5 h). But it splits into two halves
that are far apart:

| re-scored at each stake on the same tapes | n | before costs | after a 4.5 mSOL round trip at 0.5 SOL | per trade at 0.5 SOL |
|---|---|---|---|---|
| races we won (landed) | 188 | **-1.73% [-3.31, -0.15]** | **-2.63% [-4.21, -1.05]** | -13.2 mSOL |
| races we lost | 213 | +3.56% [+1.13, +5.99] | +2.66% [+0.23, +5.09] | +13.3 mSOL |

The before-costs value of the won races is the same at every simulated stake: -1.61% at 0.02 SOL, -1.67% at 0.25 SOL,
-1.73% at 0.5 SOL. So price impact does not explain the gap. A landed trade in the creation slot itself, the dry run's
exact entry, is the worst: -3.03% [-4.73, -1.34] (n=111), against +0.32% one slot later (n=76). Only 7 of the 188 won
races reach the +15% take-profit within 5 slots. Of the lost ones, 40 of 213 do.

**Before the first outside buy, the two halves look the same. They differ after it:**

| | n | bundle SOL (median) | bundle wallets (median) | dev buy (median) | first outside buy (median) | other buyers in the next 2 slots | their SOL |
|---|---|---|---|---|---|---|---|
| we were the first outside buyer | 162 | 7.20 | 3 | 2.96 | 0.25 (ours) | 1.62 | 0.34 |
| a rival was first | 238 | 8.02 | 3 | 2.96 | 0.99 | 3.12 | 1.43 |

The gap also holds at the same bundle size: small bundles -1.86% won vs +2.55% lost, mid -4.96% vs +7.70%, big -1.02%
vs +0.68%. A rival's first buy is followed by twice the buyers and four times the SOL ours is. That leaves two
explanations, and the dry run's count is out of reach under both:

1. **The followers follow the rival.** Copy-traders and momentum bots react to known sniper wallets, and nobody
   follows ours. The dry run's counterfactual removes the rival and keeps its followers, so it credits us with buyers
   who would not come.
2. **The rival knows which launches will be pushed** (a promotion, a caller, an insider), and the launch features
   cannot tell them apart. Taking those races would need beating the rival on every launch. The rivals we lost to
   paid about 25M (68 races) or landed first at near-zero pay (77). Bidding 12M or more is negative in the bid
   curve (section 4), and that curve uses the same optimistic counterfactual.

**But beating an informed rival pays.** The copied wallet of docs/09 (`AHwJ...`) fires on launches it rates. Its
buy carries an exact-state check, so it fails when someone trades first. That tells us, trade by trade, whether we
beat him. These are chain results after the landed transactions' fees and tips:

| | we beat him (his buy failed) | he did not fire |
|---|---|---|
| operator bot, Oct 23 19:18 to Oct 24 03:27 | 40 trades, 21 won, **median +2.0%**, mean -0.3% [-6.4, +5.8] | 35 trades, 8 won, median -3.9%, mean -1.6% |
| the same without the 2 rugs (below -50%) | 38 trades, mean +2.8% [-1.8, +7.4], +0.54 SOL | |
| micro-scalp, Oct 21-23, dry-run exits | 6 trades, +9.1% [+2.8, +15.5] | 156 trades, -3.1% [-4.7, -1.6] |

We beat him by price. Our buy paid about 4M (a 3M priority fee plus a 1M tip). His routine buy is a 1M tip with
almost no priority fee, so on a priority-ordered leader ours went first. When he wants a launch badly he pays about
12M (an 8M tip), and he won all 16 such races. So a race against him marks a launch an informed buyer wants, and
winning it pays. The micro-scalp's universe holds few of those: 6 of its 162 wins. The operator bot raced him on most
launches. It lost on the trades where he did not fire, on 2 rugs, and on 0.24 SOL of failed 3M copies (the cost the
Nozomi route removes).

**What v2 should be expected to make:** about r1 of the races it wins minus costs. On the Oct 21-23 mix that is
-2.6% per landed trade at 0.5 SOL, against the dry run's +1.1-1.6% and docs/04's +3.5%. At docs/04's 1,150 landed
trades a day that is about -15 SOL a day, so the daily loss stop will trip first. v2 bids 2M, below live's 3.4M, so
it should win the same or easier races. The new Harmonic bundle route can change the mix, which is the one thing a
run can measure that the tapes cannot.

Method (scratchpad scripts, not in the repo): `mslive/recon.py` (per-trade reconciliation), `mslive/stakes.py` (the
table above at 0.02-0.5 SOL), `mslive/followers.py` (before and after features, size terciles). The data is every
probe-wallet transaction (Helius `getTransactionsForAddress`) and every attempted mint's full tape, Oct 21-23.

## 3. What v2 is

- **Base:** 0aae91c, the code the dry run ran. The e1 paper lane (the 9 functions behind the dry run's numbers) is
  byte-identical to it.
- **Ported:** all 47 live commits that touch code or config and are not the operator wave. They cover the
  feeds, the sends, nonces, the guard, the risk ledger, credentials and decoding:
  - feeds: Triton Yellowstone gRPC, `grpc_b`, Triton deshred (pre-execution), the shredstream entries feed
    (lookup-table creates, whole-message walks), the slot clock driven by the feed's slot updates
  - sends: keep-alive by connection renewal, per-target dispatch, the bid as a key (`bid.lamports`) bounded by
    `risk.max_bid_lamports`, the Nozomi bundle route (`senders.nozomi_bundle`, paid only when it lands)
  - sells: fan-out to every sender, the sell nonce first so at most one copy executes, a stuck sell handed to the
    guard after 5 s, never waiting on a missing blockhash
  - the risk ledger books every landed fee and tip; the guard books a sale the ledger missed at its real proceeds
  - credentials accept systemd's 0440 files
  - `create_v2` tails decode; one stderr line per decode miss
  - Every source commit is named in the rebuild's history ("ported from ...").
- **Hand-ported** from 5f664a4: the anchor fix only (the pick-template half is the operator wave), plus the sells
  half of that commit.
- **Not ported (the operator wave):**
  - 2a4872a: operator table, hold-to-create exit, bundle shape
  - 0184a71 and f582eeb: stake tiers
  - 89631be: dev-buy template filter
  - bd61d37: dumper skip
  - f0706bd: leader regions
  - c8e545b and 1b0ecf2: pre-send gate
  - 045d1b4: paper-wide instance
  - The binary refuses their config keys (`unknown field`), so no operator-wave rule can run by accident.
- **Parity fixes**, the dry run's rules restored on the real lane:
  - The class gate. Under `entry.filter = "multi_bundle"` a real buy fires only when a bundle wallet is outside the
    creator set. The creator set is the user, the creator argument, the fee payer, the create's signers and its
    in-transaction buyers.
  - The run derivation. The run is the creator set's trades plus buys chained at most 2 apart. An outside buy
    marks the launch NOT_FIRST and never moves the anchor.
  - The insider set (bundle wallets plus the creator set) for `insider_sell`.
  - The exit clock (only a trade's slot advances it).
- **Tests:** 513 pass (`cargo test --workspace --features rugsnipe/real-senders`). The config converter has its own
  suite (`python3 -I deploy/test_microscalp_config.py`).

## 4. The bid

Flat pay-on-success bid per leader class (`ms/bidcurve.py`, 2,885 launches, 0.5 SOL, in time 80%). Expected value
per launch in mSOL, before the sell, for the two halves of the data:

| class | first-winner pay (median) | 0.3M | 1M | 2M | 5M | 12M | 25M |
|---|---|---|---|---|---|---|---|
| Harmonic | 1.50M | +3.2/+2.2 | +3.1/+1.7 | +4.6/+1.0 | +2.7/+0.0 | -2.0/-3.5 | -11.5/-13.1 |
| BAM | 1.38M | +4.7/+3.2 | +4.6/+2.5 | +4.1/+1.5 | +2.9/+1.3 | -0.8/-2.5 | -9.9/-11.4 |
| Jito | 1.10M | +5.5/+3.2 | +5.5/+3.0 | +4.6/+2.3 | +4.6/+1.6 | +0.4/-2.4 | -8.6/-12.5 |
| Firedancer | 0.10M | +9.0/+3.6 | +8.4/+3.9 | +10.8/+3.7 | +8.1/+3.4 | +2.5/-2.1 | -7.6/-10.5 |

v2 bids 2M on the buy (`bid.lamports`), on the sell and on the dump (`exit.default.sell_tip_lamports`,
`dump_tip_lamports`). That round trip of 4M is the dry run's own. From 18M up, every class loses on both halves.
**Caveat:** these values use the same counterfactual as section 2 (the rival replaced by us, its followers kept), so
they overstate what a won race pays. They rank the bids; they do not promise their sign.

## 5. The config: `deploy/bin/rugsnipe-microscalp-config`

`sudo python3 -I deploy/bin/rugsnipe-microscalp-config /etc/rugsnipe/rugsnipe.toml > out.toml` prints the box's
config with these changes and nothing else (stdlib only, no network, no writes):

- **Removed:** the 21 `[entry]` operator-wave keys, `bid.by_tier` and `exit.default.hold_to_create_slots`. This is
  exactly the set of config fields the live code has and the rebuild does not, checked struct by struct.
- **[entry]:** `require_bundle = true`, `filter = "multi_bundle"`, `max_premium = 0.02`, `filter_mode = "log"`.
- **[exit.default]**, the dry run's spec (config/paper-lead.example.toml at 0aae91c):
  - `tp = [[1.15, 1.0]]` on value, fired on a trade
  - `insider_sell = "bundle"`
  - `no_activity_ms = 1_000`
  - `phantom_sl = { after_ms = 1_000, drop = 0.15 }`
  - `max_hold_slots = 5`
  - `tp_slip_bps = 2500`
  - `sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0 }`
  - sell and dump bids of 2M
- **[bid]:** `lamports = 2_000_000`.
- **Pacing caps lifted** to explicit high values; a cap of 0 refuses every buy:
  - `max_buys_per_hour = 600` and `spread_buys_per_hour = false`
  - `max_probes_per_day = 6_000` and `max_buys_per_op_per_day = 6_000`, also in the `[probe]` stage tables (the
    smaller non-zero value binds)
- **Kept as they are:** the stake (the ArmSet's size and `max_probe_lamports`), `max_open_positions`, the daily loss
  stop, the feeds, the senders, the split and its class caps, `bundle_priority`, the keys and the nonce pools.
- Every changed line is tagged `# microscalp v2 (docs/10)`, and running it twice changes nothing.

## 6. Deploy on fra1 (a measurement run)

Order matters. The new binaries refuse the old config, and the old operator timer restarts the hot unit every 4 h. So
stop the timer first and switch the config before anything restarts.

```bash
# 0. the operator timer and the paper-wide unit go first (both are operator-wave only; the new binary has no --overlay)
sudo systemctl disable --now rugsnipe-operators.timer rugsnipe-paper-wide.service
# 1. keep the running binaries and config for a rollback (section 8)
sudo cp -a /opt/rugsnipe/bin /opt/rugsnipe/bin.operator-wave
sudo cp -a /etc/rugsnipe/rugsnipe.toml /etc/rugsnipe/rugsnipe.toml.operator-wave
# 2. build and install (installs, never starts or restarts a unit)
cd ~/rugsnipe && git pull -q && sudo bash deploy/install.sh --role hot 2>&1 | tail -3
# 3. convert the config and read what changed
sudo python3 -I ~/rugsnipe/deploy/bin/rugsnipe-microscalp-config /etc/rugsnipe/rugsnipe.toml | sudo tee /etc/rugsnipe/rugsnipe.microscalp.toml >/dev/null
sudo diff /etc/rugsnipe/rugsnipe.toml /etc/rugsnipe/rugsnipe.microscalp.toml
# 4. validate with the new binary, with the hot unit's own ArmSet
A=$(systemctl cat rugsnipe-hot --no-pager | grep -o -- '--armset [^ ]*' | tail -1 | cut -d' ' -f2); echo "armset $A"
sudo /opt/rugsnipe/bin/rugsnipe-ctl config validate /etc/rugsnipe/rugsnipe.microscalp.toml --creds /etc/rugsnipe/keys \
  --clock-offset-ms 0.05 --post-exec-healthy --armset "$A"
```

Read step 4 like docs/08 section 9 step 4. The good outcomes are `OK`, or `NOT OK` with only the two known false
alarms, `credential "probe-keypair" missing` and `credential "api-keys" missing`. Any `unknown field` means a key the
converter does not know: stop and send the line. Then switch:

```bash
# 5. stop buying, let exits finish, and check that nothing is open
sudo systemctl stop rugsnipe-hot
sudo -u rugsnipe sqlite3 -readonly /var/lib/rugsnipe/state.db "select mint,state from positions where state!='closed'"
# 6. install the new config, restart the guard on it, start hot
sudo install -m 0640 -o root -g rugsnipe /etc/rugsnipe/rugsnipe.microscalp.toml /etc/rugsnipe/rugsnipe.toml
sudo systemctl restart rugsnipe-guard && sudo systemctl start rugsnipe-hot
sleep 20; systemctl is-active rugsnipe-guard rugsnipe-hot; journalctl -u rugsnipe-hot -n 30 --no-pager | grep -i "refus\|error\|rugsnipe-hot"
# 7. the check
cp ~/rugsnipe/deploy/bin/rugsnipe-check ~/check.sh && bash ~/check.sh
```

Step 5 must print no rows. If a position is open, the guard still holds its exit: wait until it closes. If the hot
unit runs into its start limit (5 starts an hour), `sudo systemctl reset-failed rugsnipe-hot` clears it after the
fix. Before every start, docs/08 section 9 step 6a applies: the wallet must hold no stray tokens.

**The daily loss stop is the budget of this run.** At -13 mSOL per landed trade, a stop of X SOL buys about X / 0.013
trades. A verdict needs about 150 (below). That is about 2 SOL of expected loss at 0.5 SOL, or about 1.3 SOL at
0.25 SOL. Set the stop to what you are willing to pay for the answer, in both `[risk]` and the stage table.

## 7. Reading the run: `rugsnipe-check`

- **gates:**
  - `creator_only` counts creates whose bundle was all creator set, skipped as the dry run skips them
  - `not_first` counts launches where an outside buy came first
  - `wallets@fire` gives the bundle wallets per fired launch
  - Both skip counters must grow, and `fired` must stay well below `parked`.
- **process/today:** `per_attempt_pct` is the number to watch. Break-even is 0. The dry run claimed +1.14% to
  +1.56%, and the races we won Oct 21-23 imply about -2.6% at 0.5 SOL.
- **live by rank:**
  - The rank-1 trades are the won races.
  - `mean_pct` is after costs and `curve_pct` before them.
  - The gap between the two should be below 1 point (section 1's last layer). If it is larger, the costs are not
    fixed: look at buy routes.
- **exits by reason:** `tp` should be a real share. It was 7 of 188 won races Oct 21-23, against 40 of 213 lost ones.
- **buy routes:** `nozomi_bundle` has won > 0 and lost always 0. A plain copy that lands and fails pays its
  priority fee.

**Verdict rule.** One trade's result varies by about 15.6%, so a mean over n trades is good to about ±3.0 points at
100, ±2.2 at 200 and ±1.5 at 400. After 150 landed trades:
- **Stop** if the mean result after costs is below 0.
- **Continue** only if it is above +1% (about 2 standard errors above the Oct 21-23 mix).
- **In between:** run to 400 trades and decide on their mean.

A cheap stop at any time: `sudo touch /run/rugsnipe/KILL`. Buys stop, exits continue, and the latch survives a
restart. To clear it: `sudo -u rugsnipe /opt/rugsnipe/bin/rugsnipe-ctl unkill --state /var/lib/rugsnipe/state.db`,
then restart the unit.

## 8. Rollback to the operator wave

The state database is compatible both ways:
- The schema version is the same (5). The rebuild ignores the `dumpers` and `presend` tables.
- Positions are stored column by column, with no serialized policy.
- The hot and guard flags match, except `--overlay`, which only the paper-wide unit uses.

```bash
sudo systemctl stop rugsnipe-hot
sudo -u rugsnipe sqlite3 -readonly /var/lib/rugsnipe/state.db "select mint,state from positions where state!='closed'"   # must be empty
sudo rm -rf /opt/rugsnipe/bin && sudo cp -a /opt/rugsnipe/bin.operator-wave /opt/rugsnipe/bin
sudo install -m 0640 -o root -g rugsnipe /etc/rugsnipe/rugsnipe.toml.operator-wave /etc/rugsnipe/rugsnipe.toml
sudo systemctl restart rugsnipe-guard && sudo systemctl start rugsnipe-hot
sudo systemctl enable --now rugsnipe-operators.timer
```
