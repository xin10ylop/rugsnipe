# Our failed copies, one by one (Oct 26)

Run from the session scratchpad (`SP/fails`; the 5-day scripts from `SP/dec/copy`, as `five/copy/` does).

Live, since the start (Oct 25 22:08):
- `pull.py`: our wallet's buys (landed or failed, from its own transactions) and every landed trade of the first 40
  slots of each coin we sent a buy on -> `fails/coins.json`.
- `each.py`: per copy, the copied buy, where ours landed, the buys between, and what each way in would have made:
  filled where we landed (no limit), landing right behind the copied buy, re-entering on a pullback.
- `bids.py`, `race.py`: who landed between the copied buy and ours, with their priority fee and tip.

On the 5-day copy of him (tip >= 2M, our measured landing distances, sold one slot before the table hold):
- `prem5.py`: wider limits. `prem5b.py`: which refused copies are worth filling (by his tip, his size, the premium),
  and the value of landing closer behind him.
- `him_second_try.py`: a second buy one to three slots after a refusal. `fastexit.py`: the refused landings filled
  with a short hold.

3yrJE1A8 (the 3-day grid): `../w3/prem.py` (wider limits), `../w3/second_try.py`.
