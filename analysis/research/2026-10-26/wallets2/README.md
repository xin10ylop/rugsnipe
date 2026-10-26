# The second copy-target search (Oct 26, evening)

Run from `SP/oos` (the session scratchpad), on the complete 13-slot tapes in `SP/oos/coin` and the wallets' own
transactions (`SP/oos/<wallet8>.jsonl`, `wallets/oos_pull.py`).

- `discover2.py`: every wallet's launch buys on the tapes, copied as the bot copies (our landing distances, +5%, 0.5 SOL,
  sold at create + h whatever they do), by 6-hour block. SOL-quoted coins only: a coin priced in another token reports
  0 SOL reserves (bought through a swap), is outside the bot's class, and was valued on a rebuilt reserve before.
- `run2.sh` / `mints2.py`: stage 2 for 8 candidates (their own trades since Oct 25 18:00, then the tapes of their
  launch-buy coins: buys at a fresh curve). `process2.py`: the process 3yrJE1A8 went through: the exit picked on the
  first half and scored on the second, their sell timing, their size bands, the 5% limit, the overlap with our wallets.
  `insider2.py`: do they fund the creators of the coins they buy.
- `discover3.py`: the widened search, picked on the first half only and scored on the second. `run4.sh` / `mints3.py`:
  stage 2 for the next five.
