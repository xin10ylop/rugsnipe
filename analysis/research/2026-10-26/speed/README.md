# Can the copy land closer to him with the boxes we have? (docs/17 section 10), 2026-10-26

Run from the session scratchpad like the rest of the research; they read the 5-day dataset (`SP/dec/five`), the copy
backtest (`SP/dec/copy`, the same scripts as `analysis/research/2026-10-25/five/copy/`), the create universe (`SP/univ`)
and the leader schedule of epochs 1065-1067 with its regions (`SP/ext/replay_copy`).

- `region.py`: where our at-create fires and his buys land after the create, by the create-slot leader's region.
- `cover.py`: the creates of his hot listed operators (Oct 23 00:00 - Oct 25 18:00), how many he attempted, tape coverage.
- `prefire.py`: firing at the create on those operators instead of reacting to his buy, his own transaction deciding
  hold or sell; against the live copy on the same window.
- `failcopy.py`: copying his failed key attempts too.
- `copyregion.py`: the live copy by the leader's region, landing in his block or the next slot.
