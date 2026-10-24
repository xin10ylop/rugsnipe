# Research scripts, 2026-10-24 (docs/11, docs/12)

These are the scripts behind docs/11 (the operator bot against the copied wallet) and docs/12 (how the copied wallet
trades). They are kept as written in the session, so their paths point at the session's scratchpad. To rerun one, change
`SP` and supply the RPC helpers:
- `common.py` (Helius, `getTransactionsForAddress`) reads its key from a local file outside the repo
- `tri.py` (Triton) reads its URL from `month/.triton_url`, also outside the repo

Neither key nor URL is in this directory.

Order:

| stage | scripts |
|---|---|
| Wallet months | `pull_month.py WALLET DAYS`, then `profile.py`, `guards.py`, `creators.py`, `wsol.py`, `q6anj.py` |
| Linked wallets | `guard_users.py`, `funders.py` |
| Today's trades | `pull_wallets.py`, `pull_tapes.py`, `pull_his.py`, `trades.py`, then `q1.py`, `him.py`, `rugs.py`, `q2.py`, `final.py`, `csv.py` |
| The tweet-coin universe | `pipeline.py H0 H1` (creates, metadata, early trades), `skipped_early.py N`, `dataset.py`, `rates.py`, `hisvalue.py`, `scoreboard.py`, `fb10.py H`, `predict.py`, `predict2.py` |
