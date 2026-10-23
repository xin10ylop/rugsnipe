# 09. The operator wave (2026-10-23)

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
| first behind the bundle | `entry.require_bundle = true` (as shipped) | the park fires behind the run; a trade beyond the run first is `skip_not_first` |
| repeat operators | `entry.operators_file`, `entry.operators_only` | lookup by the create's fee payer (`operators::create_signer`); unlisted = `skip_not_operator` (0x54) |
| bundle of 3 | `entry.bundle_wallets = [3]` | the park's distinct run buyers outside the creator set; else `skip_bundle_shape` (0x55) |
| round dev buy | `entry.dev_buy_sol` (existing) | unchanged; leave `[]` to let the operator table pick |
| exit at create + H | `[exit.default] hold_to_create_slots` (unlisted default) + the table's `hold_slots` | `ExitReason::HoldTo` (`hold_to`): sells 100% at dump bounds on the first slot the feed shows at or past create + H |
| no other exits | `tp = []`, `insider_sell = "off"`, `no_activity_ms = 0`, `phantom_sl = { after_ms = 0, drop = 0.0 }`, `max_hold_slots = 0` | each rule is off at that value; they coexist with `hold_to` when on (first trigger wins) |
| fees | `[bid]`, `[senders]` (as configured on fra1) | unchanged |

## What is not copied

* **Their on-chain guard.** Their buy asserts the curve's exact post-bundle state, so any trade ahead (a buy or a sell)
  fails it. Ours is the zero-premium `min_tokens_out`: it fails on any buy ahead (the price moved up) but lets a buy
  through behind a sell (a better price). Same cost on failure (base + priority fee, the tip reverts).
* **Their send path.** We already fan out to five relays behind one durable nonce; their relay set and tip policy
  (flat ~1.5M, landing only) are not reproduced beyond what `[bid]` and `[senders]` already do.
* **Their detection latency** is unknown. They landed first 430 of 430 times; we land in the create slot about 62% of
  the time with a European leader. At zero premium every late buy is a cheap failure, not a bad fill.
* **The bundle count.** rt.json's `n_bw` counts the confirmed block's run (gap <= 2, index <= create + 5) minus the
  creator set; the park counts what the stream delivered before the fire (any buy at create + 1..5 by a wallet outside
  the creator set), saturating at 5. It can read lower than the block when a bundle transaction arrives late.

## The operator table

`analysis/scripts/strategy/operators_from_rt.py rt.json --out data/operators/ahw-2026-10-23.json` writes, per
`csig_signer` (the create's fee payer): `n`, `wins` (pnl > 0), `pnl_sol`, mean `dev_sol`, modal `n_bw` as
`bundle_wallets`, and `hold_slots` = the modal `swin[0] - cs` (ties: the latest launch; n < 2: 6). It prints the
L - create check (430/430 in {1, 3, 6, 10}) and the top 40. `--min-n 3` keeps only the repeat operators. Test:
`python3 analysis/scripts/strategy/test_operators_from_rt.py`. Operators rotate wallets: rebuild from a fresh decode
before trusting an old table.

## Install

1. `deploy/install.sh` copies `data/` to `/var/lib/rugsnipe/data/` (the table lands in
   `/var/lib/rugsnipe/data/operators/`); or `sudo install -m 0644 data/operators/ahw-2026-10-23.json /etc/rugsnipe/operators.json`.
2. In `/etc/rugsnipe/rugsnipe.toml` (see the commented lines in `config/probe-a.example.toml`):
   `[entry]` `operators_file = "/var/lib/rugsnipe/data/operators/ahw-2026-10-23.json"`, `operators_only = true`,
   `bundle_wallets = [3]`, `max_premium = 0.0`, `dev_buy_sol = []`; `[exit.default]` `hold_to_create_slots = 6` with the
   other rules off as in the table above.
3. `rugsnipe-ctl config validate`, restart, and check the journal for `operators: 153 signers from ...`.
4. Watch `rugsnipe_decisions{outcome="skip_not_operator" | "skip_bundle_shape" | "fire"}`,
   `rugsnipe_exits{reason="hold_to"}`, the journal's `operator <mint> hold_to create+H` and `sell ... reason HoldTo`
   lines, and `fills.fail_reason`.
