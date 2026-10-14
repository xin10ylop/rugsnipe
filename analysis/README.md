# rugsnipe analysis

Validation tooling for the pump.fun "rugger sniping" hypothesis: pull an unbiased
sample of recent pump.fun tokens, decode what happened in their creation slot, and
measure whether "buy at the end of the creation slot, take profit at +100 %" has an edge.

## Setup

```bash
python3 -m venv .venv
.venv/bin/pip install -e analysis          # installs `requests` + this package
.venv/bin/python -I -m rugsnipe_analysis.selftest
```

## Scripts

| script | what it does | runtime |
|---|---|---|
| `analysis/scripts/pull_dataset.py` | index -> light -> deep -> stats (see below) | 47 min cold (swap-api allows ~1000 req / 5 min), seconds warm |
| `analysis/scripts/verify_decoding.py` | cross-checks swap-api trades vs on-chain TradeEvents (last run: 256/256 trades matched on 12 tokens) | ~2-3 min |

```bash
.venv/bin/python -I analysis/scripts/pull_dataset.py                 # everything
.venv/bin/python -I analysis/scripts/pull_dataset.py --stages stats  # recompute stats only
.venv/bin/python -I analysis/scripts/verify_decoding.py
```

All network responses are cached under `data/cache/` (gitignored), so every script is
resumable and re-runs are cheap. `--refresh-index` re-anchors the population window.
For a faster cold run (< 25 min) use e.g. `--n-interesting 60 --early-pages 8 --latest-pages 3`.
The graduated-coin list used to top up the interesting arm is snapshotted to
`data/cache/graduated_window.json` so the sample stays reproducible.

### Pipeline

1. **index** - every create in the last 48 h from `getSignaturesForAddress` on the
   pump.fun mint-authority PDA `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM` (it signs
   every `create`/`create_v2`). ~100k sigs / 48 h, ~100 RPC calls. This is the
   population frame; the coins API cannot be used for that because its `offset` is
   capped at ~1000 rows (~50 min of launches).
2. **light** - seeded random sample (default 1500) of creates older than 2 h ->
   `getTransaction` -> decoded `CreateEvent` (+ dev-buy `TradeEvent`s, launcher program,
   mayhem flag, quote mint) -> coin snapshot via `POST /coins/mints` (batch of 100).
3. **deep** - random arm (first 400 SOL-quoted tokens of the light sample) + interesting
   arm (light tokens with ATH >= $20k, topped up with graduated coins created in the
   window) -> swap-api trades: everything in the first 300 s after creation plus the
   latest 600 trades -> per-token metrics (`firstslot.token_metrics`).
4. **stats** - aggregates with 95 % CIs (`firstslot.aggregate`) -> JSON, markdown tables
   (`data/reports/firstslot_tables.md`) + console. Tokens whose curve completed inside
   the creation slot ("instant graduation") are excluded from the groups.

Results and definitions: `docs/research/onchain-firstslot-stats.md`.

## Modules

| module | purpose |
|---|---|
| `rpc.py` | round-robin JSON-RPC over mainnet-beta + publicnode, 8 req/s, backoff, disk cache; **v1 transactions** need `maxSupportedTransactionVersion: 1` |
| `pumpapi.py` | coins (frontend-api-v3) and trades (swap-api v2, timestamp cursor) |
| `events.py` | base58, Anchor event decoding (layouts documented in the module) |
| `curve.py` | bonding-curve math, fees (95 + 30 bps per side), round-trip P&L |
| `dataset.py` | sampling, creation-tx decoding, trade pulling |
| `firstslot.py` | per-token metrics + aggregate statistics |
| `verify.py` | RPC vs swap-api trade cross-check, block tx-index check |
| `selftest.py` | plain-assert tests incl. real mainnet event fixtures |

## Data sources (state on 2026-10-14)

* `frontend-api-v3.pump.fun`: `GET /coins` (list; `complete=true` filter works),
  `GET /coins-v2/{mint}`, `POST /coins/mints`. `market_cap` is SOL;
  `usd_market_cap` and **`ath_market_cap` are USD**. Dead: `/coins/{mint}`,
  `/trades/all/{mint}` (400, wants an undocumented chainId), `/candlesticks/*`,
  `/coins/user-created-coins/*`.
* `swap-api.pump.fun/v2/coins/{mint}/trades?limit<=100&cursor=<slotIndexId>-<ts_ms>`:
  newest first; `slotIndexId` = slot(12) + block tx index(6) + sub index(4) - the tx
  index was verified against `getBlock`. A crafted cursor `9999999999999999999999-<ts_ms>`
  returns the trades just before `ts_ms`, which is how the first minutes are pulled
  without paging through the entire history.
* Public RPC `getBlock` only serves roughly the last ~1.5 days of blocks.

Outputs: `data/tokens_light.jsonl`, `data/tokens_sample.jsonl`,
`data/reports/firstslot_stats.json`, `data/reports/decoder_verification.json`,
`docs/research/onchain-firstslot-stats.md`.

## Funding tracer and address labels (Task B)

| piece | what it does |
|---|---|
| `rugsnipe_analysis/labels.py` | ~390 curated Solana addresses: CEX hot/cold wallets, instant exchanges (FixedFloat, ChangeNOW, Changelly, ChangeHero), on-ramps (MoonPay), casinos, bridges (Wormhole, deBridge, Mayan, Relay, Allbridge, CCTP...), DEX programs, pump.fun programs and fee recipients, Jito tip accounts, trading-bot fee wallets (Axiom, GMGN, Photon, BullX, Trojan, Banana Gun, Maestro). Every entry lists its public sources (`SOURCES`) and a Solscan URL. |
| `rugsnipe_analysis/labels_verified.json` | on-chain check of every label (owner program, balance, tx rate from the last 1000/200 signatures, last activity) and a verdict: `confirmed`, `inconclusive` or `rejected`. `labels.get_label()` merges it into a final confidence (`high`/`medium`/`low`/`rejected`). Rejected labels are never matched. |
| `rugsnipe_analysis/funding.py` | `first_funding()` (first and largest incoming SOL before a cutoff time), `trace_funding()` (walks up to 3 hops and stops at a labeled exchange/bridge/DEX or a wallet with more than 2000 txs), `summarize_chain()` (sorts chains into the guru's exchange, mother and simple schemes), `siblings_scan()` (the funder's other outgoing transfers of the same amount, filtered to *fresh* recipients) and `siblings_from_index()` (the same lookup against our own creator index). |
| `scripts/verify_labels.py` | re-verify labels (`--only-new` for added ones), ~5 min |
| `scripts/trace_funding.py` | sample study over recent / high-ATH / bundled creators. Writes `data/funding_creators.jsonl`, `data/reports/funding_sample.json` and the siblings scans. Takes ~20 min cold. `--from-rows` re-traces a previous sample. |
| `rugsnipe_analysis/selftest_funding.py` | offline tests (also run by `selftest`) |

```bash
.venv/bin/python -I analysis/scripts/verify_labels.py
.venv/bin/python -I analysis/scripts/trace_funding.py --n-recent 100 --n-notable 40 --n-bundled 40
```

RPC facts we found while building this (also documented in `rpc.py`):
* `solana-rpc.publicnode.com` keeps only about 2 days of ledger. Its
  `getSignaturesForAddress` silently returns **truncated** lists for old or busy
  wallets: 3 signatures instead of 1000 for a Binance wallet, and 0 for an old
  wallet. Signature history is therefore pinned to `api.mainnet-beta.solana.com`.
* publicnode rejects `getMultipleAccounts` with more than 10 keys ("Request blocked").
* `frontend-api-v3 /coins?creator=<wallet>` works (newest 50 coins of a creator).

Results: `docs/research/onchain-funding-patterns.md`.

## Rugger discovery and backtest (Task C)

| piece | what it does |
|---|---|
| `rugsnipe_analysis/window.py` | dense-window acquisition: decode **every** create in a contiguous window (Task A's 1.6 % sample almost never shows an operator twice), coin snapshots, a cheap-first swap-api pull (`staged_trades`: one page of the 15-min window, else the first 60 s to read slot 0, and the full path only for bundled launches), and `creator_funding` (fresh test + first funder/amount, 1 sigs page + <= 3 txs). `could_be_bundled` skips launches whose ATH proves slot 0 held < 1 SOL of buys. |
| `rugsnipe_analysis/discovery.py` | `slot0_structure` (dev / bundle / third-party roles in the creation slot, from slot-0 trades only), typed edges for the four signals (a: same funder + amount +/- 0.02 SOL + fresh creator, split into mother vs exchange; b: identical bundle signature; c: shared bundle wallets minus sniper bots; d: same signer, launch services keyed by `creator`), union-find clusters over strong edges, cluster score, chance test for signal (a). |
| `rugsnipe_analysis/expand.py` | grows top clusters across 72 h: d via `/coins?creator=`, a via the mother wallet's own transfers -> fresh recipients -> creates in the 48 h create index, c via the bundle wallet's signatures in create slots. |
| `rugsnipe_analysis/backtest.py` | trade-path simulator: constant-product overlay of our buy (impact, 1.25 % fee per side, fixed cost per tx), entries eos0/eos1, exits TP / SL / idle 150 s / trailing / big-buy / oracle max / oracle pre-dump, exit latency in slots, entry premium for competition, filters (bundle size, mayhem, entry mcap); `summarize` (hit rate + Wilson CI, avg win/loss, expectancy + CI, guru's 33 % and 3:1 tests); `check_no_lookahead`. |
| `rugsnipe_analysis/report.py` | renders `docs/research/onchain-backtest.md` (hand-written verdict in `docs/research/onchain-backtest.narrative.json`). |
| `scripts/discover_ruggers.py` | stages decode, coins, trades, funding, cluster, expand -> `data/window_tokens.jsonl`, `data/rugger_clusters.json`, `data/reports/discovery_summary.json`. `--max-minutes` stops network stages; re-run to resume. |
| `scripts/backtest.py` | variant grid over the window, cluster members (+ walk-forward "follow the good rugger") and Task A's random arm -> `data/reports/backtest_results.json`, `data/backtest_trades.csv`, `docs/research/onchain-backtest.md`. Reads trades from the cache only unless `--allow-fetch`. |

```bash
.venv/bin/python -I analysis/scripts/discover_ruggers.py --hours 6 --stages decode,coins
.venv/bin/python -I analysis/scripts/discover_ruggers.py --stages trades &      # swap-api bound
.venv/bin/python -I analysis/scripts/discover_ruggers.py --stages funding; wait  # RPC bound
.venv/bin/python -I analysis/scripts/discover_ruggers.py --stages cluster,expand
.venv/bin/python -I analysis/scripts/backtest.py            # seconds, cache only
```

## M0 data plane: analytics.db, RPC budget, collector, backfill

Spec: `docs/06-architecture.md` sections 4, 5.2, 9.1 and 15 (M0).  Everything below runs
with `.venv/bin/python -I` (deps: `requests`, `websockets`); the selftests are offline.

| piece | what it does |
|---|---|
| `rugsnipe_analysis/store.py` | `analytics.db` (SQLite WAL): the section 5.2 tables (`creates`, `create_buys`, `trades`, `slot0`, `funding`, `labels`, `operators`/`op_edges`/`op_assign`, `outcomes`, `prereg`/`runs`/`folds`, `landing`, `box_events`, `ingest_gaps`, `armsets`, `ab_assign`, plus `feed_seen`, `tx_attempts`, `tapes`, `samples`, `jobs`, ingested `intents`/`fills`). Integer lamports / raw token units. `upsert()` is idempotent and never lets a NULL erase a known value, so partial sources merge in any order; `op_assign` is insert-once. `schema_version` + `migrate()`. `regime_for(slot, block_time)` tags `pre_v1` (< Sep 15), `slot400` (< epoch 1053 = Oct 9), `slot200`, `alpenglow` (>= slot 459,648,000 [U], move when the cutover is observed). JSONL loaders for the pre-M0 data. |
| `rugsnipe_analysis/rpcbudget.py` | per-process token buckets: role total from `[rpc_budget]` (hot 4, guard 4, keeper 8, collector 12, backfill 4 req/s), per endpoint host (`per_host`, default 10: mainnet-beta's 100 req / 10 s per IP) and per (host, method) (`per_method`, default 4; overrides in `[rpc_budget.methods]`). `validate()` refuses a role sum > 32. `RpcClient(budget=...)` (new optional argument in `rpc.py`) uses it instead of its global limiter. Buckets are per process: two processes on one IP must split the per-IP limits in their configs. |
| `rugsnipe_analysis/createtx.py` | create tx -> `creates` + `create_buys`: reuses `dataset.creation_record`, adds every signer, fee payer, tx version, `transactionIndex`, priority fee (`fee - 5000 x signatures`, valid for v1), MEV tips (labels category `mev`), CU, `create_v2` args (`is_cashback`) and the **owner of the token account credited by each in-tx buy**. Also decodes a `logsSubscribe` notification (free mode; truncated logs leave the in-tx totals NULL). |
| `rugsnipe_analysis/collector.py`, `scripts/collector.py` | the forward collector (below) |
| `rugsnipe_analysis/backfill.py`, `scripts/backfill.py` | the historical backfill (below) |
| `scripts/ingest_existing.py` | loads `tokens_light.jsonl` (D1), `tokens_sample.jsonl` (D2 + firstslot metrics, slot0), `funding_creators.jsonl` and the labels into `analytics.db`; idempotent (1,754 creates, 1,253 create buys, 515 slot0 rows, 180 funding rows, 360 labels) |
| `rugsnipe_analysis/selftest_m0.py` | offline tests: schema/upserts/loaders/regimes, token buckets, the create decoder on a real v1 tx, collector parsing and handlers on recorded live frames (`testdata/collector_frames.json`), the job scheduler, tape slot0, backfill sampling determinism, resumable index, block parsing |

`creates.sample_arm` holds the last arm written (D1 is loaded last); the authoritative
membership of each design is the `samples` table.  `data/analytics.db*` is gitignored.

### Collector

```bash
.venv/bin/python -I analysis/scripts/collector.py --local --duration 1800 [--relay /tmp/pp.sock]
python3 -I analysis/scripts/collector.py --config /etc/rugsnipe/rugsnipe.toml     # Ohio, runs forever
```

* ONE PumpPortal connection (`subscribeNewToken`) and ONE `logsSubscribe(mentions TSLvdd…, processed)`
  on mainnet-beta.  Every create signature gets a `feed_seen` row per source with the local receive
  time (ns); the logs path already yields mint, curve, signer (`user`), creator argument, mayhem,
  quote and the in-tx buys.
* Persistent jobs (`jobs` table; a restart resumes them): `decode` (T+8 s, `getTransaction`),
  `funding` (T+45 s: one `getSignaturesForAddress(signer, before=create)` page; signers with
  <= 10 prior txs are traced at once with `funding.trace_funding` (1 hop by default), others go to
  the idle-only `funding_deep` queue, >= 1000 prior txs = `serial`), `rivals` (T+90 s:
  `getSignaturesForAddress(bonding curve)`, failed txs included, slots s and s+1 -> `tx_attempts`),
  `tape` (T+35 min: swap-api trades of the first 300 s + the latest page -> `trades`, `tapes`,
  `slot0`), `repair` (after a logs outage: the missed creates from `getSignaturesForAddress(TSLvdd,
  before, until)`).
* `--relay [PATH]` (or `[feeds.relay]` in the TOML) serves the raw PumpPortal frames on a Unix
  socket, one line each: `<recv_unix_ns>\t<frame>\n`.
* A source silent for 30 s (logs) / 60 s (PumpPortal) is force-reconnected; every outage is an
  `ingest_gaps` row (time and slot range); reconnects back off 1-60 s with jitter; a restart writes a
  `collector_down` gap.  The run summary is printed and stored as a `box_events` row.
