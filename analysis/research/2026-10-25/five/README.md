# The newest bot against him over five days (docs/17), 2026-10-25

Scripts behind docs/17. Like `follow/`, they run from the session scratchpad (`SP/dec/five`, the `big/*` analyses from
there too, with their outputs under `big/`). The RPC helper is `analysis/research/2026-10-24/tri.py`, which reads the
endpoint from a scratchpad file and redacts it from every error. Nothing here holds a key.

Data (Oct 21 00:00 to Oct 25 16:07 UTC):
- `walk5.py`: both wallets' transactions (every buy landed or failed with its error code, every sell, the SOL delta, the
  fill).
- `coin5b.py`: per coin either wallet touched, every transaction of the create slot and the next, plus the curve sampled
  at create + 1..12.
- `coin_extra.py`, `extra2324.py`: the same records for the launches neither wallet touched (the listed signers, Oct 23
  00:00 to Oct 24 11:00).
- `bids.py`: the tip and priority fee in each of our and his buys.
- `tweets5.py`: the tweet each coin names.
- `export.py`: the replay harness's input. It writes the events (creates as base64 transactions, create-slot trades with
  block indexes, his attempts) and the operator table at each 4-hourly refresh. Run the harness with
  `REPLAY_DIR=<SP/dec/five/replay> REPLAY_OUT=<file> cargo test --release -p rugsnipe --test replay_days -- --ignored
  --nocapture` (`crates/rugsnipe/tests/replay_days.rs`). `config.standin.toml` is the stand-in config the run used.

Comparison (docs/17 sections 1-2):
- `an.py`: the replay's decisions against his buys and our live results. It writes `an_rows.json`, which the rest read.
- `exec5.py`: the head-to-head races on the coins we both tried.

The auction and the coins above 40 vSOL (sections 3-4):
- `race.py`: every bid in the create slot and the next.
- `auction.py`: who took the first spot at what bid, and whether bid order explains our races.
- `ev_bid.py`, `ev_bid2.py`: a flat bid's value on the curve, with displacement and tail checks.
- `calib40.py`: the model against our real wins.
- `conv.py`, `ours.py`: his tip against his history, and our above-40 wins coin by coin.
- `tipwhy.py`, `meta.py`, `prebuy.py`, `btip_all.py`: the pre-buy features tested against his tip and the value (tweet
  age, rank, author, metadata, the operator's bundle tip).
- `demand.py`, `behind.py`, `noshow.py`: exits, and buying behind him.
- `fair.py`: entering only the auctions a signer usually clears below our bid.

His skips (sections 5-6):
- `skippred.py`: holder mode and his last decision on the signer, where the universe is complete (Oct 23 00:00 to Oct 24
  11:00), then the 5-day check of the rules (what each removes, his result there, our live fills).
