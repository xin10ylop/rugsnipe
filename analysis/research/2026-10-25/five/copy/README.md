# Landing behind him, the bound, the exit and the copy (docs/17 section 9), 2026-10-25

Scripts behind docs/17 section 9. Like the rest of `five/`, they run from the session scratchpad: `second.py` to
`bound.py` from `SP/dec/behind`, the rest from `SP/dec/copy`, and they `exec` each other by those paths. They read the
5-day walks and tapes (`SP/dec/five`), the complete 12-slot tapes (`SP/full`, `SP/fp`), the exact replay's output and
operator tables (`SP/ext/replay`) and its +6..+10 curve states (`SP/ext/v9`, `SP/ext/v10`). No RPC, nothing that holds a key.

Our buys and his:
- `second.py`: shared loaders (walks, tapes, the post-bundle state, the P&L model: 0.5 SOL, 1.25% fee each way, 3M
  tip, 0.5M sell tip); our 6042 failures filled at their real positions.
- `decay.py`: a copy's value by how many buys land between him and us; our real position behind him.
- `copytip.py`, `react.py`, `size.py`, `combo.py`: copies by his tip, his size, next-slot reactions, the coins the
  newest bot does not fire (with the one-slot-before-his-sell exit, which the bot cannot know: see `copyval.py`).

The rule's fires (the exact replay, Oct 23 -> now):
- `bound.py` (`python3 bound.py <exit slot>`), `boundday.py`: the fires with the buy bound loosened, by what he did
  and by day.
- `exitcmp.py`: the fires on the complete tapes by exit: the table hold H, H-1, H-2, a flat +9 / +10.
- `bigval.py`: the followed coins above 40 vSOL with a 5% bound.

The copy (a bot that buys when his buy shows):
- `copyval.py`: fills at the end of his slot or a slot later, exits the bot can make (H, H-1, +9) and the oracle.
- `react_pos.py`: the value by the block positions D between his buy and ours.
- `bytip.py`: keyed on his tip (his conviction, in his own transaction).
- `scratch.py`: the copy bot from scratch: D drawn from our own measured trigger-to-landing distances, exit before his
  sell or behind it, 0.5 SOL or his size.
- `exitopt.py`: the copy's exit on his hold logic: H-k on listed signers, flat holds or his last sell on the signer for
  the unlisted, take-profits.
- `listed.py`: the listed-only copy on the coins the newest bot does not fire, by his size, tip and the bound.
- `unlisted.py`: the unlisted signers: his result there, his sell timing, exits (short holds, his hold by launch tool
  or dev-buy size, the oracle) and the entry distance's share of the loss.
- `replayval.py`: the copies the copy replay itself made, valued the same way.
- `move.py`: the price over his fill when a copy lands, and its move to the exit, listed vs unlisted.
- `weakdays.py`, `weakdays2.py`: filters for his weak days, known before our buy: his tip and size, the operator hot for
  him, his recent run, our landing price, a stop-loss; tips crossed with the copy's own bound.
- `stakes.py`: the tip-keyed copy at 0.25 / 0.5 / 1 SOL and at his own size, next to his result on the same coins.
- `drawdown.py`: the size key against the tip key beyond the totals: copies a day, value per copy, the best day's
  share, the worst drawdown inside a day.
- `skips.py`: his coins the rule skips, by the rule's reason (the leader filter, the 40 vSOL cap, not followed), and
  what the tip-keyed copy picks up in each.
- `recent.py`: his current operators counted live (his buys on the signer in the last W hours >= K) against the
  backtest's tables.
- `restart.py`: the same count started empty at each 4-hourly restart of the hot service (the operator refresh).

The replay of the copy trigger itself: `crates/rugsnipe/tests/replay_days.rs` with `entry.copy = "on"` in the replay
dir's `config.toml` (his buys of create + 1..3 then reach the hot loop as live).
