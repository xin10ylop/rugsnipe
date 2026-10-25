# The exact replay (docs/17 section 8), 2026-10-25

The newest code on every candidate launch from Oct 23 00:00 UTC, with the box's config (`config.box.toml`). Run from the
session scratchpad (`SP/ext`), in this order:

- `creates_ext.py T0`: every create since T0, extending `univ/creates_<hour>.json`.
- `slots.py LIST LO HI DIR`: the trades of create + LO..HI per coin. Used for 0..3 on the new window's candidates
  (`ext/coin/`) and for 1..3 on the Oct 23-24 candidates whose tapes hold only the create slot (`ext/later/`).
- `b64.py`: the create transactions (base64), what the replay's gRPC path decodes.
- `export2.py`: `ext/replay/events.jsonl` and the operator tables at each refresh. The epoch leader schedules
  (`getLeaderSchedule`) go in `ext/replay/leaders/<epoch>.json`, with `data/leader_regions.json` beside them.
- the harness: `REPLAY_DIR=SP/ext/replay REPLAY_OUT=SP/ext/replay/out.jsonl cargo test --release -p rugsnipe --test
  replay_days -- --ignored --nocapture` (about 2 minutes).
- `v10.py`: the curve at create + 6 and + 10 for the fires whose tape stops at + 3.
- `an2.py OUT`: the comparison with his trades and our live bot, the live check, and the projection.

The follow-rate sweep is the same run with `follow_min_rate` changed in `config.toml`.
