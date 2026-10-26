# The operator rule with each operator's arrival timed (docs/17 section 10), 2026-10-26

Run from the session scratchpad; they extend the copy ablation (`analysis/research/2026-10-25/five/copy/ablate.py`).

- `arrival.py`: his key buys split by what the bot could know at the buy: >= 2 of his buys on the operator in 24 h
  (counted with his month of history, so Oct 21 is not an empty start), traded within 7 days of the last table
  refresh, back after more than 7 days, or brand new; his big day (Oct 23) by class; profit by operator.
- `options.py`: the rule's options with the box's own table lag (listed only after the next 4-hourly refresh, unlisted
  copies sold at the default +9), with and without the one operator that carries most of the profit.
- `nightsig.py`: last night's key coins (Oct 25 22:08 - Oct 26 09:02): each create's signer from the chain and its
  history with him (reads the Triton URL from a 0600 file outside the repo).
