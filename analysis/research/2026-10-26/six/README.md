# 6anjqMe as a copy target (docs/18), 2026-10-26

Run from the session scratchpad (`SP/six`), like the rest of the research; they read the month pull (`SP/month`) and the
Triton helper there (`SP/month/tri.py`, its URL in a 0600 file outside the repo).

- `pull_rpc.py WALLET T0 T1`: a wallet's transactions over standard JSON-RPC, as the month pull's compact rows.
- `screen.py`: his round trips on SOL-priced pump coins from the trade events, by day and by slice (tip, size, curve
  level, hold, creator repeat); `trips.json`.
- `tapes.py TRIPS MODE OUT WINDOW`: the curve at the end of each slot after his buy.
- `copy.py`, `copy2.py TAPES TRIPS WINDOW`: the copy on those tapes, exits fixed / after his sell / before his sell.
