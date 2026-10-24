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

## tweets/ (docs/13: the tweet-coin rule, backtested on Oct 23 and Oct 24)

| stage | scripts |
|---|---|
| A second day | `RS_DAY=23 RS_STOP_AFTER_B=1 pipeline_day.py 0 12` (creates, metadata), `RS_DAY=23 dataset_day.py` |
| Outcomes | `RS_DAY=23 RS_SKIP_ERR=1 early_fast.py tt N` (his attempts plus N others, first 12 slots), `RS_DAY=24 early_fast.py others N` |
| Per-coin rows | `RS_DAY=D rows.py` (first outside buyer, its entry slot, its value at entry + 1..10 slots, post-bundle SOL) |
| Rules | `policy.py` (check first vs buy-then-exit vs none), `policy2.py` (tweet check x his deployers), `policy3.py` (the deployer lists the bot can know), `createonly.py` / `symnew.py` (name and ticker proxies) |
| Timing | `lateness.py`, `late2.py` (value by entry slot and order), `rivals_sel.py` (who is first, and how far behind the bundle) |
| The result | `final.py 10` (each condition's step, hour by hour), `halves.py`, `whofirst.py`, `signer_key.py` |
| Tweets (docs/12) | `tweets.py`, `tw_analysis.py`, `report.py` |
