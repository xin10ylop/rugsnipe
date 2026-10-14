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
