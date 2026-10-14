# Gap 6: which source gives 30+ days of pump.fun microstructure history?

**Question.** Which data source can deliver at least 30 days of history, kept up to date, with:
- every pump.fun create;
- every trade in slots s..s+3, with in-slot tx index, signer, fee payer, compute-budget and tip fields;
- the SOL funding edges into creator wallets?

For each candidate: what does it cost, how fresh is it, and does it decode `create_v2`, `buy_v3` and v1 transactions?

**Answer.**
- **Primary backfill: Old Faithful (Triton's public CAR archive).** It is free and complete. I verified it byte-for-byte against RPC, v1 included. It runs one epoch (12-38 h) behind. Read only the slots we need through its 12-byte slot-ranges index.
- **Recent tail and ongoing data:** our own live collector, plus Helius `getBlock` at 1 credit per block (about $49-85/month).
- **Funding edges:** Helius `getTransactionsForAddress` per creator, or a full Old Faithful stream.
- **Dune** is useful for cross-checks only. Its pump.fun spell does not map `create_v2` mints to their bonding curves.
- **BigQuery** is most likely broken for v1 blocks.
- **Allium** lacked v1 fee rows until Oct 13.
- **Flipside** docs no longer resolve.

Run on 2026-10-14 at about 11:00 UTC. The current slot was about 456.8M (epoch 1057).

---

## 1. Volume we must cover (measured)

| Quantity | Value | Source |
|---|---|---|
| Successful creates in the last 24 h | **44,355** in 40,379 distinct slots | `data/cache/create_index.json` (mint-authority sig index, anchor 1791965133) |
| Union of slots {s..s+3} over those creates | **134,725 slots/day**, 34% of the 395,787 slots in 24 h | same file, computed here |
| Average Old Faithful slot size, epoch 1055 | 1.72 MB (64 random slots); 641 GB / 431,653 blocks ≈ 1.49 MB | range reads, CAR-REPORT |
| Public RPC `getBlock` full (base64), one slot | 1.0-1.6 MB gzip, about 4.8 MB uncompressed | measured on api.mainnet-beta |
| Slots per day, 30-day window | about 11.9M (27-28 epochs of 432k) | `getBlockTime` at epoch starts, below |

So a targeted 30-day backfill is about **4.0M blocks ≈ 7 TB**. A full-chain 30-day backfill is about **19-21 TB**.

## 2. Regime boundaries (tag every record)

| Regime | Slot range | Start (UTC) | Evidence | Status |
|---|---|---|---|---|
| R0, no v1 | < 447,120,000 | — | — | — |
| R1, v1 live, ~268 ms slots | 447,120,000 (epoch 1035) to 454,895,999 | 2026-09-15 01:04:23 | SIMD-0385 activated at epoch 1035 ([solanacompass](https://solanacompass.com/news/transaction-v1-simd-0385-is-live-on-solana-mainnet-at-epoch-1035)); `getBlockTime(447120000)` = 1789434263 | LIKELY |
| R2, ~200 ms slots | 454,896,000 (epoch 1053) to 459,647,999 | 2026-10-09 14:40:56 | `getBlockTime`: 454.89M→454.896M = 269 ms/slot; 454.896M→454.902M = 219 ms/slot | VERIFIED |
| R3, Alpenglow | ≥ 459,648,000 (epoch 1064) | about Oct 21 | 1064 × 432,000 (arch-judge-2 V13) | LIKELY |

**Correction to the brief.** Before Oct 9 the mean slot time was about **268 ms, not 400 ms**. It was measured from epoch start times:

| Epochs | Mean slot time |
|---|---|
| 1040→1045 | 267 ms |
| 1045→1048 | 268 ms |
| 1030→1040 | 301 ms |
| 1020→1030 | 336 ms |

"400 ms" is the nominal figure, not what the chain did (VERIFIED, `getBlockTime` at 1020/1030/1040/1045/1048/1050/1052-1057 × 432,000).

**Consequence.** A "last 30 days" window today holds about 1 day of R0, 24 days of R1 and 5 days of R2.
- Single-regime walk-forward with 30 days is possible only in **R1** (about 1M creates).
- R2 tops out at about 12 days before Alpenglow.
- 30 days of R3 will exist about **Nov 20** at the earliest.
- Plan R3 validation as a forward-collected dataset, and keep the backfill for R1/R2 only.

## 3. Candidates

### 3.1 Old Faithful (Triton public archive), RECOMMENDED for the backfill

- **Access.** Per-epoch CAR at `https://files.old-faithful.net/{E}/epoch-{E}.car`. There is also `epoch-{E}-slot-ranges.raw`: 432,000 × 12-byte LE records of (u64 offset, u32 length) into the CAR (format from [jetstreamer `index.rs`](https://raw.githubusercontent.com/anza-xyz/jetstreamer/main/jetstreamer-firehose/src/index.rs)). HTTP range requests are supported. Hosted in Amsterdam ([docs](https://docs.old-faithful.net/llms-full.txt)). VERIFIED.
- **Cost.** "Currently completely free to use" (same docs; "subject to change"). Our cost is bandwidth and compute only. VERIFIED (docs) / LIKELY (stays free).
- **Freshness.** [CAR-REPORT](https://raw.githubusercontent.com/rpcpool/yellowstone-faithful/gha-report/docs/CAR-REPORT.md) shows 1055 complete, 1056 missing, 1057 ongoing ("1 behind"). Epoch 1055 ended 2026-10-12 21:18Z. Its tx-metadata check finished at 05:05Z and its PoH check at 08:58Z on Oct 13, so the archive was ready about 12 h after the epoch closed. Data is therefore **12-38 h stale**, because an epoch now lasts about 26 h. The documented target is "within 4 epochs". VERIFIED.
- **Size.** Epochs 1040-1055 are 584-808 GB each, plus 31-38 GB of indexes. The gsfa (address→sigs) index is **not** hosted for recent epochs (`epoch-1055-gsfa.index.tar.bz2` returns 404), so per-address lookups are not possible from the archive. VERIFIED.
- **v1, `create_v2`, `buy_v3`: VERIFIED by my own decode.** I range-fetched slot 456,191,999 (last slot of epoch 1055; 1,250,125 bytes; 1.36 s) and parsed the DAG-CBOR nodes and zstd protobuf metadata:
  - 945 transactions: 715 legacy, 114 v0, **116 v1**.
  - Signature order is **identical to RPC `getBlock`**, so the tx index is exact.
  - The v1 `create_v2` tx `2qQDVo…` sits at index 455.
  - Its metadata gives fee 35,000, CU consumed 299,090 and cost units 309,949, matching `getTransaction`. It also carries 133 log lines (Anchor `Program data` events) and inner instructions.
  - The protobuf fields present are 2, 3, 4, 5, 6, 7, 8, 15, 16, 17: fee, pre/post balances, inner instructions, logs, token balances, CU and cost units.
  - Raw wire bytes are stored, so our own `pump-codec` decodes `create_v2`, `buy_v3`, the v1 config (CU limit and priority lamports) and tip transfers, exactly as it does on the hot path. Scripts: `scratchpad/ofparse.py`, `ofcheck.py`, `ofmeta.py`.
- **Throughput measured here.** 64 random slots with 32 threads, including per-slot index lookups, took 7.3 s for 110 MB: **15 MB/s, 8.8 slots/s**. This sandbox sits behind a proxy, so treat it as a floor.
  - At this rate the targeted 30-day pull (4.0M slots) takes 5.3 days.
  - On a 1 Gbps EU box with the index cached (5.2 MB per epoch) it takes about 16 h.
  - The daily increment is about 232 GB.
- **Tooling.** [Jetstreamer](https://crates.io/crates/jetstreamer) is 0.8.1, released 2026-10-09. It depends on `solana-transaction`/`solana-message` 4.x, the line with the V1 variant (critique-speed). It reads the slot-ranges index and claims 2.7M TPS at 30 Gbps+. LIKELY (not run). Upstream fixed v1 signature extraction in [yellowstone-faithful PR #398](https://github.com/rpcpool/yellowstone-faithful/pull/398). LIKELY.
- **Gaps.**
  - No account-state snapshots, so curve state must come from `TradeEvent` reserves.
  - Not live.
  - Alpenglow's block markers may delay CAR production for epochs ≥ 1064. UNVERIFIED risk.

### 3.2 Our own `getBlock` crawler (baseline in `analysis/rugsnipe_analysis/rpc.py`)

- **Current design is N+1.** `get_block_signatures` fetches signatures only, then `getTransaction` per signature, which means about 945 calls per slot. Replace it with **one `getBlock` per slot**: `encoding: base64`, `transactionDetails: full`, `rewards: false`, `maxSupportedTransactionVersion: 1`. Decode locally with pump-codec.
- **v1 pitfall.** `getBlock` with `maxSupportedTransactionVersion: 0` and full details returns **-32015 "Transaction version (1) is not supported"** for slot 456,191,999. Signatures-only mode still works. VERIFIED.
- **Calls per day** (steady state, at the 44k creates/day level):

| Item | Calls/day |
|---|---|
| Create index: `getSignaturesForAddress(TSLvdd…, 1000)` | about 45 |
| Blocks s..s+3 | **134,725** |
| Funding, 1-hop (sigs + oldest tx per creator) | about 88k |
| Funding, 2-hop (×2) | about 176k |
| **Total** | **about 225-310k** |

  The 30-day backfill is about 4.0M `getBlock` calls plus about 2.6-5.3M funding calls.
- **Public RPC limits (measured).** I sent 240 full `getBlock` calls with 8 threads.
  - The first 429 came after 4.5 s (`retry-after: 10`).
  - 121 of 240 succeeded in 15.7 s.
  - The sustainable ceiling therefore looks like the documented ~40 req/10 s per method per IP, i.e. **about 4 getBlock/s/IP**.
  - At that rate the daily increment costs 9.4 h of one IP's budget, and the 30-day backfill takes about 11.7 days per IP.
  - publicnode serves only about 2 days: first available block is 456,418,837, and slot 456,191,999 is "cleaned up". VERIFIED.
- **Helius.**
  - `getBlock`, `getTransaction` and `getSignaturesForAddress` (archival) cost **1 credit**.
  - `getTransactionsForAddress` costs 10 credits per 100 full txs returned, or 10 flat for signatures only.
  - Plans: Developer $49 (10M credits, 50 rps); Business $499 (100M, 200 rps). Extra credits cost **$5/M**.
  - Helius says it runs its own archive that "fully supports versioned transactions" ([credits](https://www.helius.dev/docs/billing/credits.md), [plans](https://www.helius.dev/docs/billing/plans.md), [additional credits](https://www.helius.dev/docs/billing/additional-credits.md), [historical data](https://www.helius.dev/docs/rpc/historical-data.md)). VERIFIED (docs); v1 not tested by me.
  - Backfill: 4.0M blocks at 50 rps take about 22 h and cost about 4M credits.
  - Ongoing: about 4.0M credits/month for blocks plus about 13M for funding (44k creators × 10 credits × 30). That is **about 17M credits/month ≈ $49 + $35 = $84/month**.
  - Download volume is about 7 TB/month gzip. Helius may throttle it: UNVERIFIED.
- **Triton RPC.**
  - Hydrant archive: **$10 per million queries**, $10 minimum ([docs](https://docs.triton.one/chains/solana/old-faithful-historical-archive-1.md)).
  - Archival gRPC: "$0.12/GB bandwidth + $10/million calls" (triton.one/pricing page data). At 7 TB that is about $840 in bandwidth, so the free HTTP archive (3.1) is strictly cheaper. LIKELY (base plan price not read).

### 3.3 Dune

- **Raw tables.**
  - `solana.transactions` has `index` (in-block), `fee`, `compute_units_consumed`, `signer` (fee payer), `signers[]`, `instructions` with inner instructions, `log_messages` and pre/post balances ([schema](https://docs.dune.com/data-catalog/solana/transactions.md)).
  - A `transaction_config` struct holds the v1 CU limit, heap, LADS and **priority fee**. It was added in the week of **Sep 14, 2026** ([data changelog](https://docs.dune.com/docs/data-changelog.md)).
  - `solana.instruction_calls` has `tx_index`, `tx_signer` and inner/outer indexes.
  - `solana.account_activity` has per-address SOL `balance_change`, which can serve as funding edges.
  - VERIFIED (docs).
- **pump.fun spell.** `pumpdotfun_solana.base_trades` ([source](https://raw.githubusercontent.com/duneanalytics/spellbook/main/dbt_subprojects/solana/models/_sector/dex/pumpdotfun/solana/pumpdotfun_solana_base_trades.sql)):
  - It decodes trades from the self-CPI **TradeEvent** (`e445a52e51cb9a1d` + `bddb7fd34ee661ee`), which is version-agnostic and covers buy, `buy_v3`, `buy_exact_quote_in_v3` and sells.
  - It parses only the first event fields (mint, sol, token, is_buy, user, ts, virtual reserves). It keeps `tx_index` and `block_slot`.
  - **Bug for us:** the `bonding_curves` CTE matches only the legacy `create` discriminator `0x181ec828051c0777`. `create_v2` is `d6904cec5f8b31b4` (sha256 "global:create_v2"; computed here). About 99.3% of current creates are `create_v2` (critique-claims C4: 11/1,496 legacy). So `pool_id` and the vaults are **NULL for nearly every 2026 mint**.
  - Creates must be pulled from raw `instruction_calls` with our own discriminator.
  - VERIFIED (code read); effect on output UNVERIFIED (no query run).
- **Cost.**
  - Free is view-only. Analyst is $75/month (4k credits); Plus is $399/month (25k credits).
  - Extra credits cost $1.596 per 100 on Plus. Exports cost 2 credits/MB on Plus ([credits](https://docs.dune.com/resources/credits-billing/how-credits-work.md), [billing](https://docs.dune.com/api-reference/overview/billing.md)).
  - Compute credits for 30-day scans of `solana.instruction_calls` are not published. UNVERIFIED, and probably the dominant cost.
- **Freshness.** Datashare is "24 hours for most tables, some key tables every few minutes" ([FAQ](https://docs.dune.com/datashare/datashare-faqs.md)). In-app latency for Solana raw tables: UNVERIFIED.
- **Verdict.** Good for an independent cross-check of our counts. Not the primary store: there is no tip or funding model, `create_v2` needs custom SQL, and costs are opaque.

### 3.4 BigQuery `bigquery-public-data.crypto_solana_mainnet_us`

- **Status.** A "Community"-maintained dataset with no schema link ([Google](https://docs.cloud.google.com/blockchain-analytics/docs/supported-datasets)). The listing says freshness ranges from minutes to hours; the blockchain-etl README says 2-5 min.
- **History.** It stalled on 2025-03-31 ([forum](https://discuss.google.dev/t/public-solana-bigquery-dataset-crypto-solana-mainnet-us-stopped-updating-on-march-31-2025/185629)). LIKELY.
- **v1 risk.** The public pipeline [solana-etl](https://raw.githubusercontent.com/blockchain-etl/solana-etl/main/src/solana_config/constants.rs) pins `TRANSACTION_VERSION: u8 = 0`. That value is passed as `max_supported_transaction_version` in its `bigtable.rs`. Full blocks containing any v1 tx fail with -32015 (VERIFIED above). So if Google runs that code unpatched, **every block since Sep 15 would fail or be missing**. LIKELY broken; not confirmed.
- **Not checked here.** There are no GCP credentials in this sandbox (`bq show` returns "Invalid Credentials"). Check with this query (dry-run first; about $6.25/TB on-demand per third-party sources):
  `SELECT MAX(block_timestamp) FROM bigquery-public-data.crypto_solana_mainnet_us.Transactions WHERE block_timestamp > TIMESTAMP_SUB(CURRENT_TIMESTAMP(), INTERVAL 2 DAY)`
- **Verdict.** Do not depend on it.

### 3.5 Allium

- **Tables.**
  - `solana.raw.transactions` has `txn_index`, `signer` (fee payer), `compute_units_consumed`, logs and balances ([schema](https://docs.allium.so/historical-data/supported-blockchains/solana/solana/raw/transactions.md)).
  - `solana.dex.trades` lists project **pumpfun** and **pumpswap**.
  - VERIFIED (docs).
- **v1 lag.** In its own [Oct 2026 notice](https://docs.allium.so/changelog/solana-data-corrections-2026.md), Allium says Transaction V1 rows were added to `solana.raw.fees` only **for new data from 13 Oct 2026**. The history backfill and the transfer changes have no date yet. So v1 priority fees between Sep 15 and Oct 13 are currently missing or wrong. VERIFIED (vendor statement).
- **Cost.** Enterprise pricing is not public. The x402 Explorer `run-sql` costs $0.01 per submit plus "dynamic" result cost ([pricing](https://docs.allium.so/ai/machine-payments/endpoints-pricing.md)).
- **Verdict.** Usable later as a cross-check. Not suitable for R1 compute-budget features today.

### 3.6 Flipside

`docs.flipsidecrypto.xyz` now 301-redirects to `edisyl.com`, a marketing page. I found no current pricing, schema or pump.fun model. UNVERIFIED. Excluded.

### 3.7 pump.fun HTTP APIs (for completeness)

They are already known to be inadequate (gap brief):
- `/coins` stops at about 1,000 rows.
- `/trades` works per mint at about 60 requests/minute.
- No tx index, compute-budget or tip fields.

Use them only for post-hoc labels (ATH, `complete`).

## 4. Comparison

| Source | 30 d history | Fresh | Tx index / signer / payer | CU limit and priority (v1) | Tips | Funding edges | `create_v2` / `buy_v3` / v1 | Cost for 30 d + 1 month ongoing |
|---|---|---|---|---|---|---|---|---|
| **Old Faithful** (targeted) | yes | 12-38 h | exact (= RPC order) | raw bytes → our decoder | yes (system transfers) | only with a full scan (~650 GB/day) | **yes, verified** | $0 data; about 7 TB + 232 GB/day of transfer |
| Old Faithful full stream (Jetstreamer) | yes | 12-38 h | exact | yes | yes | **yes, all SOL transfers** | yes (LIKELY) | $0 data; about 20 TB + about 650 GB/day; 10 Gbps+ box advised |
| Own crawler, public RPC | yes (api.mainnet-beta is full archive) | real time | exact | yes | yes | sigs + tx per creator | yes with `maxSupported=1` | $0; about 4 getBlock/s/IP, so the backfill takes about 12 days per IP |
| Own crawler, Helius | yes | real time | exact | yes | yes | `getTransactionsForAddress` | yes (docs) | about $84/month + about $85 one-off backfill |
| Dune | yes | minutes-24 h (UNVERIFIED) | yes | `transaction_config` (since Sep 14) | derivable | `account_activity` | trades yes; spell misses `create_v2` curves | $399/month + compute (UNVERIFIED) |
| BigQuery | ? | ? | yes (schema) | ? | ? | ? | **LIKELY broken** for v1 | unknown |
| Allium | yes | ? | yes | v1 fees only from Oct 13 | ? | transfers table | pumpfun in dex.trades | enterprise |

## 5. Recommended pipeline

1. **Create index (VERIFIED approach, `dataset.build_create_index`).**
   - Call `getSignaturesForAddress(TSLvdd…)` on api.mainnet-beta. The mint authority signs every create, CPI-launcher creates included.
   - 30 days is about 1.33M signatures, about 1,330 pages, or minutes of work.
   - Tag each record by regime from its slot (§2).
2. **Blocks s..s+3 for epochs ≤ the last published epoch: Old Faithful.**
   - Download `slot-ranges.raw` once per epoch.
   - Coalesce adjacent slots into one range request.
   - Decode with pump-codec in Rust. The Python prototype in the scratchpad is for verification only.
   - Store per tx: slot, index, sig, fee payer, signers, version, CU limit/price (v1 config or ComputeBudget ix), fee, CU consumed, tip transfers (Jito/sender tip accounts), and pump events.
   - Run it from an EU box (FRA/AMS). The archive is in Amsterdam, and AWS ingress is free if we stay on AWS (`eu-central-1`).
3. **Tail (slots after the last Old Faithful epoch) and live data:**
   - the live collector already planned (Yellowstone/LaserStream) as the source of truth for R3;
   - plus Helius `getBlock` to fill holes, about 135k credits/day.
   - Reconcile tail rows against Old Faithful once the epoch is published, using the signature set per slot.
4. **Funding edges.**
   - For each creator, call Helius `getTransactionsForAddress` (ascending, first N, full) at 10 credits each. Recurse one hop for non-CEX funders, using the label set in `labels.py`.
   - Free fallback: `funding.py` on api.mainnet-beta, 2 calls per hop, about 3 h/day of one IP's budget.
   - Graph-wide funding (mother wallets, CEX amount bands): run a monthly Jetstreamer full scan that keeps only System `transfer` and `createAccount` with lamports < 100 SOL to wallets with no prior tx. UNVERIFIED volume.
5. **Cross-check.** Once per week, count creates and slot-0 buys per day on Dune using our own `create_v2` SQL. Alert if the counts differ by more than 1%.

## 6. Open items

- BigQuery freshness and v1 handling: needs one dry-run query with GCP credentials.
- Dune compute credits for a 30-day `instruction_calls` scan: needs one trial query (2,500 trial credits).
- Helius bandwidth or throttling at 50 rps of full `getBlock` (about 80 MB/s gzip): untested.
- Old Faithful CAR production after Alpenglow (epochs ≥ 1064): watch CAR-REPORT around Oct 22-23.
- Jetstreamer 0.8.1 v1 decode: not run here. The raw-bytes path was verified with my own parser.
