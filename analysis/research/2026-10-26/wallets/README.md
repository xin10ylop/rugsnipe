# More wallets to copy (Oct 26)

Run from the session scratchpad like the rest of the research. Inputs: the replay's create universe
(`SP/ext/replay/events.jsonl`, 26,952 in-class launches Oct 23 00:00 - Oct 25 18:00), the 5-day tapes (`SP/dec/five`),
and complete 13-slot tapes fetched for the candidates' coins (`coin5w.py`, landed trades only -> `SP/wide/coin`).
The 5-day tapes (`five/coin5b.py`) keep one curve state per slot after create + 1, up to his last sell: right for
copying him, not for other wallets (their sells are missing), so the wallet screens read the complete tapes only.

- `discover.py`: every wallet's launch buys (create + 0/1, outside the dev's bundle run) across the universe.
- `screen1.py`: first pass on the 5-day tapes (superseded, see above).
- `screen2.py`: per wallet, own result inside the tape and the copy at our landing distances (bound +5%, exits: learned
  modal hold - 1, +5, +9, a ceiling one slot before their sell), by day.
- `variants.py`: per-wallet filters (size, their own operator repeat) and exits.
- `split.py`: the selection tested on an unseen day (pick on two days, score the third, both directions).
- `diversity.py`: operator concentration, shared coins, the set as one bot (one copy per coin).
- `active.py`: are the candidates still trading.
- `oos_pull.py`, `oos_mints.py`, `oos_screen.py` (with `specs.txt`, each wallet's filter and exit fixed in sample):
  out of sample, Oct 25 18:00 -> Oct 26 11:00: the finalists' own transactions, their coins' tapes, the copy.
- `oos_own.py`: a wallet's chain-true result (balance change of its landed transactions, standalone transfers out).
- `oos_links.py`, `oos_successor.py`, `oos_insider.py`: wallet changes (where its SOL goes, whether that wallet trades),
  and whether it funds the creators of the coins it buys. A pump buy pays the curve by a system transfer, so only
  standalone transfers count as links.
