# Architecture proposal 2: validation first (quant / purist angle)

Written 2026-10-14 (epoch 1057, slot ~456.78M). This proposal builds on the research in `docs/research/*.md` and on the `analysis/` package.

The design follows one rule. **Every live decision is a lookup into a compiled artifact of out-of-sample statistics.** The hot path never estimates anything. The Python side never touches a key. The backtest is the same arming code replayed over history, so "what we tested" and "what we run" cannot drift apart.

## 0. Stance, and what I re-verified today

The research tells us three things:
- The guru's headline rule loses money (break-even at TP +100% is 43-48% with a natural SL).
- The base rate of 2x from the end of slot 0 is 6.8% (CI 3.5-12.9%).
- No per-operator hit rate has been measured yet.

So the system's first product is a trustworthy *estimate*. Execution comes second. Three consequences shape the architecture:

1. **Validate the policy, not each operator.** At one-sided α=0.05 and power 0.8, separating p=0.40 from a break-even of 0.31 needs n=170 snipes. Separating 0.55 from 0.43 needs n=106. A Bonferroni correction over 50 candidate operators raises that to ~264 (recomputed today; the exact binomial at n=170 gives power 0.76). Operators live about a week at 5-10 launches/day, so per-operator certification is mostly impossible. What *can* be certified is the **selection-and-sizing policy as a whole**, pooled across operators, out of sample. Per-operator data then drives only (a) shrunken posteriors for arming and (b) sequential tests for disarming.
2. **Entry decides EV.** Using `analysis/rugsnipe_analysis/curve.py` (0.25 SOL buy, 1.25% per side, 0.004 SOL buy overhead, TP +100% from our fill, miss exits at the floor), the break-even hit rate is:

   | Post-bundle virtual SOL `V_e` | Break-even hit rate |
   |---|---|
   | 31 | 11% |
   | 37.5 (median slot-0 bundle 7.5 SOL) | 30% |
   | 45.9 | 39% |
   | 70 | 47% |

   The outcome model, the arming rule and the slippage bound must therefore all be **conditional on `V_e`**. In the hot path the slippage bound *is* the EV gate (§5.6).
3. **Public data is perishable.** Public `getBlock` serves only ~1.5 days and publicnode keeps ~2 days. Every day the collector is not running is lost forever. The collector therefore ships before anything else that needs history.

**Checks run today** (beyond the lead's facts):
- **`agave-transaction-view` 7.0.0 does not fit our stack.** It needs `solana-message ^5`, `solana-transaction ^5` and rustc 1.98.1. **6.1.0** needs `solana-message ^4.5` and `solana-transaction ^4.2` (MSRV 1.89) and parses v1. Pin `=6.1.0`. The "7.0.0" listed in `speed-data-feed.md` does not fit our stack.
- **v1 transactions have no ALTs.** The 6.1.0 source says "Don't have ATL in txv1". This settles critique C12: v1 and an ALT are mutually exclusive. Our ~834-byte buy needs no ALT anyway.
- **Rent today:**
  - Token-2022 ATA (170 B): 1,513,840 lamports (0.00151 SOL), not ~0.002.
  - Nonce account (80 B): 1,056,640 lamports.
  - The cost model reads these at startup.
- **Crate versions** (crates.io, today):
  - pump-rust-client 0.4.1, yellowstone-grpc-client 15.0.0 / proto 14.0.1, tonic 0.14.6, quinn 0.11.12, solana-tpu-client-next 4.3.1 (MSRV 1.97.1).
  - rusqlite 0.40.2, tokio-tungstenite 0.30.0, arc-swap 1.9.2, rtrb 0.4.0, hdrhistogram 7.6.0.
- **Python environment:** Python has numpy and sqlite 3.45. pyarrow and duckdb are *not* installed but pip can fetch them. SQLite is therefore the store of record, and Parquet is optional.

## 1. Component diagram

```
                         ANALYTICS PLANE (Python, Ohio box today; no keys)
 ┌──────────────────────────────────────────────────────────────────────────────────────┐
 │ collector ──► store (SQLite WAL: rugsnipe.db  + optional Parquet tapes/)             │
 │  │ PumpPortal newToken (free) / RPC logsSubscribe(mint-authority)                    │
 │  │ getTransaction(create)  swap-api tapes  getSignaturesForAddress(bc) for rivals    │
 │  │ funding.py traces (fresh creators)   labels.py                                    │
 │  ▼                                                                                   │
 │ operators.py (PIT entity graph) ─► features.py (PIT, whitelist) ─► outcome.py (sim)  │
 │                                                       │                              │
 │ prequential.py (walk-forward replay of arming.py) ◄───┘   abtest.py   landing.py     │
 │        │ reports/ (daily OOS report, go/no-go)                ▲            ▲         │
 │        ▼                                                      │            │         │
 │ arming.py ──► armset-<ver>.json (+sha256, valid_until) ───────┼────────────┼──┐      │
 │ alerts.py (ntfy/Telegram/email)    ingest.py ◄── rslog files ─┘            │  │      │
 └────────────────────────────────────────────────────────────────────────────┼──┼──────┘
                                                        rsync/S3 (pull) ▲     │  │ push (signed)
          HOT PLANE (Rust, FRA primary / AMS secondary; one per box)    │     │  ▼
 ┌──────────────────────────────────────────────────────────────────────┼─────┼─────────┐
 │ feeds (pinned core) ─► dedup ─► rs-decode ─► matcher ─► decider ─► builder/signer ─► senders
 │  shreds|deshred|preconf|gRPC|PumpPortal|RPC-ws       ▲ ArmSet (ArcSwap)│     │ Helius,Nozomi,
 │                                                      │                │     │ Astralane,NextBlock,
 │ funding watcher (CEX/mother outflows) ─► watchlist ──┘                │     │ Jito, TPU-QUIC, Paper
 │ exit engine (positions) ◄─ trade/transfer events for held mints       │     │
 │ risk (caps, kill switch, mode) gates decider + exit                   │     │
 │ recorder (SPSC ring ─► rslog files) ──────────────────────────────────┘     │
 │ leader scheduler + nonce pool (per-epoch schedule, primary box per 4-slot window)
 │ metrics (/metrics localhost, hdrhistogram)                                  │
 └─────────────────────────────────────────────────────────────────────────────┘
```

**Two planes, one contract.** The contract is `schema/` (§4): the SQLite DDL, the rslog binary record layout and the ArmSet JSON schema. Both languages run golden tests against the same fixture files.

## 2. Data flows

1. **Discovery (continuous, cold).**
   - Every create is captured: mint, slot, tx index, signers, fee payer, `creator` arg, launcher, quote mint, mayhem flag, tx version, CU and tip fingerprint, metadata host.
   - Every fresh creator gets a funding trace.
   - Tapes (the first 30 min of trades) are pulled for **(a)** every create linked to a candidate operator and **(b)** a seeded 5% random control sample.
   - The tape selection uses point-in-time (PIT) information only, never outcomes. That keeps both arms unbiased and gives us a base rate.
2. **Labelling.** `outcome.py` replays each tape under every policy on a small grid. Per (mint, policy, entry model) it writes the realized net P&L, the exit reason and the hit flags.
3. **Decision compile (every 6 h, and on demand).** `arming.py` reads only data whose outcome horizon has closed. It produces the ArmSet: armed operators, their policies, the watch keys and the funding rules. It is pushed to the hot boxes. The hot path hot-swaps it atomically.
4. **Hot path.**
   - A feed event is decoded. A `create_v2` whose signer, fee payer or `creator` arg is a watched key (or whose funder matched a funding rule) goes to the decider.
   - The decider checks risk, applies the A/B coin and computes the size.
   - The builder patches the template, signs the K variants on one nonce and fans them out.
   - The recorder logs every stage with monotonic timestamps.
5. **Feedback.**
   - rslog files are pulled to analytics every 5 min.
   - `landing.py` joins our sends to on-chain positions (slot, tx index, buyers ahead) and rival bids.
   - `abtest.py` compares traded launches with randomly skipped ones.
   - Disarm events go back into the next ArmSet. The hot side also has local fast-disarm triggers (§6).

## 3. Crates and modules

### 3.1 Rust workspace `crates/`

Settings:
- `edition = "2021"`, `rust-version = "1.97.1"` (rust-toolchain.toml pinned).
- The dependency set is the one in speed-reference-impls §7, plus `agave-transaction-view = "=6.1.0"`.
- No `solana-sdk`. `bincode = "1.3"` (never 3.x).

| Crate | Responsibility | Key types | Deps (beyond std) |
|---|---|---|---|
| `rs-core` | Constants (program IDs, PDAs of global/fee_config/event_authority, buyback recipients); u128 integer curve math mirroring `curve.py`; fee model from FeeConfig; `Lamports`, `VSol` newtypes. **No allocation.** | `CurveState{vsol,vtok}`, `FeeBps{protocol,creator}`, `fn tokens_out()`, `fn sol_out()`, `fn v_after()` | solana-pubkey 4.4 |
| `rs-txview` | Zero-copy view over legacy, v0 and v1 wire txs; static keys, instruction iteration, v1 config (CU, priority lamports, LADS) | `TxView<'a>` (wraps `agave_transaction_view::TransactionView`) | agave-transaction-view =6.1.0 |
| `rs-decode` | Pump instruction decoders: `create_v2` (strings skipped by length, `is_mayhem`, optional tail → quote class), legacy `create`, all buy/sell variants incl. v3; Anchor CPI events (Create/Trade/Complete); System transfer; Token-2022 `Transfer`/`TransferChecked`; PumpSwap buy/sell. Program ID is checked first, because the pump and PumpSwap discriminators collide. | `CreateSeen`, `TradeSeen`, `SolTransferSeen`, `TokenXferSeen` (all `Copy`, fixed size) | borsh 1.8 (tests only) |
| `rs-feed` | `Feed` trait → `FeedEvent{src, recv_mono_ns, slot, sig, payload}`; dedup by signature (open-addressing table of 64k sigs, no alloc); backpressure counters | `PumpPortalFeed`, `RpcWsFeed` (free); `GrpcFeed` (`feature="yellowstone"`), `DeshredFeed` (`"triton-deshred"`), `PreconfFeed` (`"helius-preconf"`), `ShredUdpFeed` (`"shreds"`, served by the separate `shred-ingest` workspace over a Unix socket) | tokio 1.53, tokio-tungstenite 0.30, yellowstone-grpc-client 15.0.0 (opt), tonic 0.14.6 |
| `rs-watch` | ArmSet loader/validator; watch map `FxHashMap<Pubkey, WatchEntry>`; funding-rule engine (amount bands, fresh-only, TTL, remove-if-no-launch, last-transfer, keep-address) | `ArmSet`, `WatchEntry`, `FundingRule`, `Watchlist` (behind `ArcSwap`) | arc-swap 1.9, serde_json 1.0 |
| `rs-exec` | Precomputed tx templates (legacy and v1); patching of mint/bonding_curve/ATAs; ed25519 signing; `Sender` trait with per-provider tip instructions | `BuyTemplate`, `SellTemplate`, `NoncePool`, `Variant`, `Sender` impls: `HeliusSender`, `NozomiSender`, `AstralaneSender`, `NextBlockQuic`, `JitoBe`, `TpuQuic`, `PaperSender` | solana-message 4.6, solana-transaction 4.3, solana-keypair 3.1, reqwest 0.13, quinn 0.11, solana-tpu-client-next 4.3.1 (opt), pump-rust-client =0.4.1 (startup and tests only) |
| `rs-exit` | Position state machines; exit triggers; v3 sell vs PumpSwap routing; retry ladder | `Position`, `ExitPolicy`, `ExitReason` | rs-core, rs-exec |
| `rs-risk` | Mode (`paper`/`probe`/`live`), hard caps, kill switch, local disarm | `RiskGate`, `RiskState` | — |
| `rs-record` | SPSC ring (hot thread → recorder thread) → hourly `*.rslog`; latency histograms | `Rec` (`#[repr(C)]`), `Recorder` | rtrb 0.4, hdrhistogram 7.6 |
| `rs-sched` | Per-epoch leader schedule; identity→metro map; primary box per 4-slot window; heartbeat between boxes | `LeaderMap`, `BoxRole` | solana-rpc-client =4.3.1 (cold) |
| `bin/rugsnipe-hot` | Wires it all up; `--mode paper` is the default | — | core_affinity 0.8 |
| `bin/rugsnipe-replay` | Feeds recorded rslog files or fixture tapes through the **same** pipeline into `PaperSender`. Gives deterministic tests and latency benchmarks. | — | — |
| `bin/rugsnipe-sim` | Builds the buy/sell txs and runs `simulateTransaction` (sigVerify=false, replaceRecentBlockhash=true) with a funded payer from public data, to verify layout, CU and LADS | — | solana-rpc-client |

### 3.2 Python (`analysis/rugsnipe_analysis/`, extending what exists)

Reused as is:
- `rpc.py`, `pumpapi.py`, `cache.py`
- `curve.py` (the reference for `rs-core`)
- `events.py` (the reference for `rs-decode` events)
- `dataset.creation_record`, `firstslot.token_metrics` (its fields seed the outcome tables)
- `funding.trace_funding`/`siblings_from_index`, `labels.get_label`

New modules:

| Module | Responsibility |
|---|---|
| `store.py` | Applies `schema/rugsnipe.sql`; idempotent upserts; `ingest_gaps` bookkeeping |
| `collector.py` | Long-running: newToken stream → creation record → funding (fresh only) → tape scheduler (T+35 min) → bonding-curve signature pass for rivals (incl. failed txs) |
| `operators.py` | PIT entity graph, union-find with typed edges, stable operator IDs, merge log |
| `features.py` | Whitelisted PIT features, each with an `available_at` rule. A banned list (`ath_market_cap`, `volume`, `complete`, anything from `/coins` fetched after `t_create`) is enforced by a test. |
| `outcome.py` | Entry models E0/E1/E2, exit-policy replay, cost model. Mirrors `rs-exit` semantics (conformance fixtures). |
| `prequential.py` | Walk-forward replay of `arming.py`; purge and embargo; block bootstrap; vault |
| `arming.py` | Empirical-Bayes posteriors, EV lower bounds, state machine, sizing, ArmSet emitter. **Live and backtest call the same functions.** |
| `abtest.py` | Analysis of randomized skips; farm detection |
| `landing.py` | Our sends ↔ on-chain position, rivals table, landing model per (box, leader class, metro) |
| `ingest.py` | Decodes rslog (`struct` formats generated from `schema/rslog.toml`) |
| `report.py`, `alerts.py` | Daily markdown report, go/no-go gates, push alerts |

## 4. Data model

### 4.1 SQLite (`data/rugsnipe.db`, WAL)

Columns are abbreviated. Pubkeys are base58 TEXT, and SOL amounts are INTEGER lamports.

```sql
creates(mint PK, slot, tx_index, block_pos REAL, block_time, sig, signer, fee_payer, creator_arg,
        launcher_program, ix_variant, tx_version, quote_mint, is_mayhem, token_program,
        cu_limit, cu_price, priority_lamports, tip_lamports, tip_recipient, lads_limit,
        name, symbol, uri_host, dev_buy_lamports, init_vsol, init_vtok, regime TEXT, src)
trades(mint, slot, tx_index, ev_index, sig, user, is_buy, sol, tokens, vsol_post, vtok_post,
       ix_name, fee_bps, creator_fee_bps, PRIMARY KEY(mint, slot, tx_index, ev_index))
tx_attempts(mint, slot, tx_index, sig, signer, ok, err_code, cu_used, priority_lamports, tip_lamports,
            tip_recipient, path, tx_version)      -- incl. FAILED txs (rivals' slippage losses)
sol_transfers(slot, sig, funder, recipient, lamports, recipient_was_new INT)
token_transfers(mint, slot, tx_index, src_owner, dst_owner, amount)
funding(wallet PK, funder, amount, slot, depth, chain_json, pattern, fresh INT, lag_s)
labels(address PK, entity, category, confidence, verified_at)          -- from labels.py
slot0(mint PK, n_adjacent, adj_sol, n_reactive, reactive_sol, slot0_sol, v_after_bundle,
      v_eos0, v_eos1, shape TEXT, shared_tip_acct INT, any_slot0_sell INT)
operators(op_id PK, created_slot, family TEXT, merged_into, retired_slot)
op_edges(op_id, kind, key, first_slot, last_slot, weight)  -- kind: creator|funder_band|mother|
                                                           -- bundle_wallet|launcher|tip|meta_host|fp
op_assign(mint PK, op_id, as_of_slot, rule, conf)          -- computed with edges < creates.slot only
outcomes(mint, policy_id, entry_model, size_lamports, v_entry, filled INT, exit_reason,
         exit_slot, pnl_lamports, hit_150 INT, hit_200 INT, hit_300 INT, mae_bps, hold_ms,
         PRIMARY KEY(mint, policy_id, entry_model, size_lamports))
policies(policy_id PK, json, grid_version)
runs(run_id PK, code_sha, data_from, data_to, n_configs_tried, params_json, created_at)
folds(run_id, fold, train_from, train_to, test_from, test_to, n, ev_mean, ev_lb, hit, hit_wlb)
arm_decisions(op_id, decided_at, state, reason, n_oos, k_oos, p_mean, p_lb, ev_lb, run_id, armset_ver)
armsets(ver PK, sha256, created_at, valid_until, run_id, n_armed, path)
ab_assign(mint PK, op_id, arm TEXT, u REAL, salt_id)        -- arm: trade|skip
intents(intent_id PK, mint, op_id, box, mode, armset_ver, decided_mono_ns, size, v_max,
        min_tokens_out, nonce_idx, decision TEXT)
sends(intent_id, variant, sender, sig, sent_mono_ns, tip_lamports, priority_lamports)
fills(intent_id, sig, slot, tx_index, buyers_ahead, rival_sol_ahead, v_fill, tokens, err_code)
exits(intent_id, reason, sig, slot, tx_index, sol_out, tries)
latency(box, stage, src, p50_us, p99_us, n, window_start)
risk_events(ts, box, kind, detail)        -- kill, cap_hit, disarm_local, armset_stale
ingest_gaps(source, from_slot, to_slot, reason)
```

### 4.2 Parquet (optional, `data/tapes/date=YYYY-MM-DD/*.parquet`)

The same columns as `trades`. They are used when daily tapes exceed about 2M rows. Requires `pip install pyarrow`.

### 4.3 rslog (hot → cold)

`schema/rslog.toml` declares each record. Rust and Python are both generated or checked from it.

```
Header  { magic u32 = 0x52534C47, kind u16, ver u16, len u32, mono_ns u64, wall_ns u64, box u16 }
FeedSeen      { src u8, slot u64, sig [u8;64] }
CreateSeen    { slot u64, sig [u8;64], mint [32], signer [32], creator [32], flags u16 }
Matched       { intent u64, op_id u32, policy u16, armset_ver u32, via u8 }   -- via: key|funding
Decision      { intent u64, action u8, size u64, v_max u64, min_tokens u64, nonce_idx u16, ab_u f32 }
Signed/Sent   { intent u64, variant u8, sender u8, sig [u8;64] }
PosEvent      { intent u64, kind u8, slot u64, v u64, tokens u64 }
ExitFired     { intent u64, reason u8, sig [u8;64] }
Risk          { kind u8, value i64 }
```

`rugsnipe-replay` and `ingest.py` share `fixtures/rslog/golden.rslog`. CI fails if either decoder disagrees.

### 4.4 ArmSet (cold → hot, JSON, sha256 in its filename; the hot side refuses anything stale)

```json
{"ver": 57, "run_id": "pq-2026-10-21T06", "code_sha": "…", "created_at": 1792540800,
 "valid_until": 1792562400, "regime": "slot200ms/pre-alpenglow",
 "operators": [{"op_id": 1234, "state": "armed", "size_lamports": 250000000,
   "v_max_lamports": 41000000000, "tip_policy": "p75", "ab_skip_p": 0.2,
   "exit": {"tp": [[2.0, 0.6], [3.0, 1.0]], "trail_bps": 3000, "no_activity_ms": 120000,
            "phantom_sl_ms": 10000, "phantom_sl_bps": 5000, "bundle_sell": true,
            "consolidation_pct": 40, "big_buy_sol": [2.8, 3.2], "max_hold_ms": 1800000},
   "stats": {"n": 41, "k": 19, "p_mean": 0.44, "p_lb": 0.33, "ev_lb_sol": 0.012, "p_ev_pos": 0.93}}],
 "watch": [{"key": "H57KAw…", "kind": "creator", "op_id": 1234, "expires_slot": 456790000, "keep": false}],
 "funding_rules": [{"funder": "ET69Tz…", "bands": [[899500000, 900500000]], "fresh_only": true,
   "ttl_s": 1800, "remove_if_no_launch_s": 3600, "last_transfer": true, "op_id": 1234}]}
```

## 5. Validation core (the part that decides whether money moves)

### 5.1 Point-in-time operator index

```
on new create c (time-ordered replay or live):
  E = edges with first_slot < c.slot:
      strong: same creator wallet; funder(c.signer) is a non-CEX "mother" that funded >=2 fresh
              creators with the same amount band (±0.0005 SOL); bundle wallet (adjacent idx +1..+4
              or same tip acct in slot0) seen in a previous op's slot0; Token-2022 transfer
              between wallets of two ops; profit recycling (curve -> fresh hop -> creator)
      CEX-funded: key = (cex_entity, amount_band, dev_buy_band, launcher) — never merge THROUGH a CEX
      weak (features only, never merge): tip recipient, CU/LADS/priority fingerprint, uri_host,
              name pattern, ix_variant, tx_version
  ops = union-find over strong edges touching c's wallets
  if none: op = new_operator(family = (funding_pattern, slot0_shape_bucket, launcher))
  op_assign(c.mint) = (op, as_of_slot = c.slot, rule, conf)      # immutable once written
```

Operator IDs stay stable because a merge keeps the older ID and writes `merged_into`. History is never rewritten: an assignment made at slot s stays as it was, even if a later merge would have changed it. The backtest therefore sees exactly the operator knowledge it would have had live.

### 5.2 Outcome model (`outcome.py`)

Entry models are computed from the tape with in-slot `tx_index`:
- **E0**: end of slot 0, the guru's idealized entry. Reported only.
- **E1** (decision model): after the create, after the adjacent bundle, and after the first `k̂` reactive buyers of slot 0. `k̂` is the landing model's predicted number of buyers ahead for (operator, box, leader class). Before probe data exists, `k̂ = max(1, median reactive)`. When the create's `block_pos > 0.85`, E1 falls back to E2.
- **E2**: end of slot 0+1.
- In all three, our own buy is applied with its curve impact.
- **Unfilled rule:** the entry fills only if `V_entry ≤ v_max`. Otherwise the outcome is "unfilled" and costs base plus priority fee only (the tip is inside the tx and reverts).

Exit replay, for each policy on a discrete grid (24 policies at most, logged in `runs.n_configs_tried`):

```
pos = Position(tokens, v_entry, t_entry)
for slot s in tape after entry:
   for trade/transfer in s (tx_index order):
       update triggers: TP ladder on spot; trailing from running max; no-activity timer;
       bundle-wallet sell (any wallet in slot0.adjacent set or same funder); dev/signer/creator sell;
       consolidation (Σ token_transfers into one owner >= X% supply within 2 slots);
       big buy in [a,b] SOL; phantom SL (armed after phantom_sl_ms); max_hold; completion
   if a trigger fired in slot s:
       fill = state after ALL of slot s's trades plus slot s+1 trades with idx < median idx
              (pessimistic: the dump slot fully precedes us), then apply our sell impact.
       Post-completion fills use PumpSwap reserves and the 1.25%/1.20% canonical fee tiers.
pnl = sol_out - size*(1+fee) - buy_overhead - sell_overhead - ata_rent_unrecovered
      - failed_attempt_rate*(base+priority)
```

Cost inputs come from data, not constants:
- fee_bps from TradeEvents;
- rent from `getMinimumBalanceForRentExemption`;
- buy overhead from the tip policy's quantile of observed winning bids (`tx_attempts`) at the operator's competition level;
- failure rate from the landing model.

**Exclusions:** non-SOL quotes, mayhem coins and legacy SPL-Token creates, until each has its own model.

### 5.3 Prequential walk-forward (`prequential.py`)

```
grid of decision times t = t0, t0+6h, ...
for each t:
   visible = rows with outcome horizon closed: c.time + H_max < t - embargo   (H_max=30min, embargo=1h)
   state_t = arming.compile(visible, as_of=t)          # SAME function the live compiler calls
   for c in creates with t <= c.time < t+6h:
       a = state_t.decide(c, info available at c.slot)    # match, A/B coin, size, v_max
       if a.fire: realized = outcome(c, a.policy, entry=E1, size=a.size); log to folds
report:
   pooled OOS EV/snipe with day-block bootstrap 90% CI; hit rate with Wilson 95% bounds;
   EV by v_entry band; qualifying snipes/day; P&L under E0/E1/E2 and low/base/high costs;
   share of P&L from top-3 operators (concentration); results on the locked VAULT
```

**Vault.** The most recent 3 days at each go/no-go review stay unread until the final evaluation of a frozen `code_sha`. Every new policy grid or feature set increments `n_configs_tried`, and the report states it.

**Go/no-go gate for leaving paper mode.** All of the following must hold:
- ≥300 OOS paper snipes over ≥10 days;
- the pooled EV lower bound (one-sided 95%, block bootstrap) is > 0 under E1 and base costs;
- EV is still ≥ 0 under the high-cost scenario;
- the vault EV is > 0;
- no single operator supplies more than 40% of P&L.

### 5.4 Arming and disarming (`arming.py`)

The hierarchical model works per entry band `b` (four `V_e` bands) and per TP target:
- Family prior: `p ~ Beta(m·μ_{f,b}, m·(1−μ_{f,b}))`, with μ and m fit by empirical Bayes (method of moments) on visible data. Family `f` = (funding pattern, slot0 shape bucket, launcher).
- Operator posterior: `Beta(mμ + k_o, m(1−μ) + n_o − k_o)`.
- Miss loss `L_o,b`: shrunken mean of the realized miss losses.
- EV draws: `EV = p·W(b) − (1−p)·L_o,b − c`, with `W` and `L` taken from the curve at size X.

```
state(op):
  CANDIDATE -> SHADOW      when op has >= 2 launches (always shadow-simulated; costs nothing)
  SHADOW    -> PROBE       when P(EV>0) >= 0.8 and n_o >= 5 and active in last 24h
  PROBE     -> ARMED       when P(EV>0) >= 0.9, E[EV] >= 0.005 SOL, n_o >= 10, probe landing
                           measured >= 5 times (k̂ real), and policy-level gate (5.3) is green
  ARMED     -> SUSPENDED   any of: SPRT(H0: p=p_be(b), H1: p=p_be(b)+0.10, α=.05, β=.2) accepts H0;
                           5 consecutive losses (safety, not inference); farm flag (5.7);
                           landing CUSUM: buyers_ahead drifts > +1 vs model; no launch for 48h
  SUSPENDED -> SHADOW      after 24h cooldown (shadow stats keep accruing)
  any       -> RETIRED     no launch for 7 days
v_max(op) = max V such that E_post[p_b(V)]·W − (1−E_post[p_b(V)])·L(V) − c >= 0, at the 20th
            posterior percentile (interpolated across bands)
```

### 5.5 Sizing

```
X* = argmax over X in {0.02, 0.05, 0.1, 0.25, 0.5, 1.0} SOL of  EV_lb(X)
     EV_lb(X) = p_lb·W(X,V_e) − (1−p_lb)·L(X,V_e) − c_fixed − λ·farm_hazard(X)
subject to X <= cap_snipe, X <= kelly_frac·bankroll·(p_lb·b − q_lb)/b  (b = W/L, kelly_frac=0.25),
           X <= ρ·E[slot0_sol(op)] (ρ=0.15: stay small against the rugger's supply-share trigger)
```

`W` and `L` include our own curve impact on entry and on exit. `farm_hazard(X)` comes from the A/B size arm (§5.7). Until that arm has data, `λ` is set so that X stays ≤ 0.25 SOL.

### 5.6 Hot-path decision as a function of these statistics

The hot path only evaluates `match → state==ARMED → risk ok → ab_u ≥ ab_skip_p → fire`.
- It uses `buy_exact_quote_in_v3(spendable=X, min_tokens_out = tokens_out(X, v_max))`. The slippage bound is exactly the EV boundary: if we land behind more SOL than the model says is profitable, the tx fails cheaply.
- The fee semantics of `spendable` and `partial_fill` are verified by simulation in M5. `partial_fill=false` until then.

### 5.7 Anti-farm randomization

```
u = H(salt_epoch || mint) / 2^64           # decided from the mint alone; the rugger cannot game it
arm = skip if u < ab_skip_p(op) (default 0.2) else trade
size_arm = small (0.05) if 0.2 <= u < 0.35 else policy size        # dose-response for farm_hazard
every 6h: compare traded vs skipped on the SAME shadow simulation of the realized tape
   Δ_dump = P(bundle/creator sell >= 1 SOL within slot0..slot0+2 | traded) − same | skipped
   Δ_hit  = hit(E1) traded − skipped
   farm flag if P(Δ_dump > 0.10) > 0.8 or P(Δ_hit < −0.10) > 0.8 (Beta posteriors, pooled per family
   and per op)
```

Skipped launches are also the unbiased control for "what happens without us", which is what the guru's members cannot measure.

### 5.8 Paper, probe and live measurement of landing position

- **Paper** (free feeds, Ohio or FRA).
  - We record `recv_mono_ns` per feed and estimate the counterfactual arrival: recv + measured build/sign time + RTT to the leader's metro (`stake_geo.json`).
  - That arrival is compared with the rivals' estimated landing times. Those are `slot_start + tx_index/n_tx·slot_ms`, with the slot start taken from our own slot stream.
  - Output: `buyers_ahead_est`, flagged as an estimate (±20-40 ms).
- **Probe** (opt-in, real 0.01-0.02 SOL buys, capped at 20/day).
  - Measures the true `slot`, `tx_index`, `buyers_ahead` and `rival_sol_ahead` per (box, sender, leader class).
  - Round-trip cost ≈ overhead 0.002-0.004 SOL + 2.5% of size; the ATA rent is recovered by closing the account in the sell tx.
  - About 100 probes cost about 0.4 SOL. This is the cheapest way to replace the guessed `k̂` with data.
- **Live.** Same records; `fills.buyers_ahead` feeds the landing CUSUM.

## 6. Hot path

### 6.1 Threads (bare metal; `isolcpus` per speed-geography §6d)

- **Core 2:** feed RX (busy-poll) → decode → match → decide → build → sign → send (first variant).
- **Core 3:** remaining variant signing and sending.
- **Core 4:** exit engine and the post-exec feed for held mints.
- **Core 5:** recorder, metrics and the ArmSet watcher.
- **Tokio runtime** for HTTP/2 and QUIC sender connections: kept warm with `/ping` every 5 s and pre-resolved IPs.

### 6.2 Create handling

```
on FeedEvent(ev):                                    # zero alloc from here to send
  if !dedup.insert(ev.sig): return                   # first source wins; record source rank anyway
  view = TxView::parse(ev.bytes)?                    # legacy | v0 | v1
  for ix in view.instructions() where program == PUMP:
     match disc(ix):
       CREATE_V2 | CREATE => c = decode_create(ix, view)   # mint=#0 (signer), user=#5|#7, creator arg
       _ => continue
     if c.quote != SOL or c.is_mayhem or c.token_program != T22: record(SkipClass); continue
     w = watch.get(c.signer) | watch.get(c.fee_payer) | watch.get(c.creator)   # FxHashMap lookups
     if w.none: continue
     op = armset.op(w.op_id); intent = H(mint, box_id)
     if op.state != ARMED: record(Shadow(intent)); continue
     if !risk.allow(op, op.size): record(SkipRisk); continue
     if ab_u(mint) < op.ab_skip_p: record(SkipAB); continue
     if !sched.should_fire(ev.slot): record(Backup) ; tip = tip_backup else tip = tip_for(op, leader)
     n = nonce_pool.pick(mint)                       # H(mint) mod N, shared across boxes
     for v in senders.variants():                    # fastest path first
        tx = tmpl.patch(c.mint, c.bonding_curve, ata(us, c.mint, T22), n.value, v.tip_ix, ...)
        sign(tx); v.send(tx); record(Sent)
     exits.open(intent, c.mint, op.exit, bundle_watch = slot0 adjacency from this feed)
```

The bonding curve and the associated bonding curve are read from create accounts #2 and #3. The PDA is derived only as a fallback. Top-level program IDs and signers are always static keys, so detection works from raw shreds without ALT resolution.

**Launcher-wrapped creates are the exception.** Some creates reach pump through a CPI from a launcher, for example `J7pour…` (43 of the 282 creator≠signer creates in D1) or `CreateCoinAndBuyBondingCurveV3`. They have no top-level pump instruction, and pre-execution feeds carry no inner instructions. For these, `rs-decode` keeps a per-launcher table (discriminator → account index of mint, curve and user) learned from M0 fixtures. The match key is a watched signer among the static keys. Unknown launchers are recorded and shadow-only. The decode-miss counter (created post-exec but never seen pre-exec) measures how much this costs.

### 6.3 Funding watcher (pre-creation watchlist)

The subscriptions depend on the mode:
- **gRPC:** `account_include` = labelled hot CEX wallets (`labels_verified.json` activity class hot/warm, plus FixedFloat, ChangeNOW, MoonPay) and the active mothers from the ArmSet.
- **Free mode:** RPC `logsSubscribe{mentions:[addr]}` per address plus `getTransaction`. Binance `5tzFki…` runs ~940 tx/h (~0.26/s), which fits the public RPC budget for a handful of sources.

```
on SolTransfer(funder, to, lamports, to_was_new):
  for rule in rules[funder]:
    if rule.fresh_only and !to_was_new: continue
    if lamports in any(rule.bands):
       watch.insert(to, WatchEntry{op: rule.op, kind: Funded, expires: now+rule.ttl_s,
                                   remove_if_no_launch: rule.remove_if_no_launch_s})
on SolTransfer(from=watched w, to, lamports) and rule(w).last_transfer and lamports >= 0.9*bal(w):
    watch.insert(to, inherit(w)); if !w.keep: watch.remove(w)
expiry sweep every slot: drop entries past expires unless keep
```

Today's data supports a 30-minute TTL: fresh, exchange-funded creators launched 24-798 s after funding (median 159 s). The guru's "2.955" constant never occurred, so the bands come from discovered mothers and from per-operator amount clusters. They are never typed in by hand.

### 6.4 Exit engine

- It consumes post-exec TradeEvents and Token-2022 transfers for held mints. Free mode uses `logsSubscribe(mentions: bonding_curve)`; paid mode uses gRPC `account_include: [bonding_curve, mint]`.
- Triggers follow §5.2 exactly. A conformance test replays fixture tapes through `rs-exit` and `outcome.py` and requires identical (reason, slot) decisions.
- Sells:
  - Before completion: `sell_v3` with `min_sol_output` = 0.
  - Emergency exits: `min_sol_output` = (model fill × 0.7).
  - After `CompleteEvent`: PumpSwap `sell_v2` (17 accounts; layout to be confirmed by simulation in M5) on the canonical pool `pool(0, pool_authority(mint), mint, WSOL)`, with effective quote reserves = vault + `virtual_quote_reserves`.
- Retry ladder: every slot for up to 10 slots, rotating senders, with the tip escalating ×2 up to a cap.
- The sell tx closes the ATA, which recovers 0.00151 SOL of rent.

### 6.5 Latency budget (FRA box, FRA/AMS leader, shred or deshred feed)

| Stage | Budget p50 / p99 | Source |
|---|---|---|
| Leader execution → first shred out | 1-5 ms | speed-data-feed (LIKELY) |
| Shred delivery in-metro (DZ Edge / Helius raw) | 0.5-3 ms | UNVERIFIED vendor |
| Deshred + `TxView` + decode create | 30 / 150 µs | target, benchmarked in `rugsnipe-replay` |
| Watch lookup + decide | 1 / 5 µs | target |
| Patch template + ATA PDA (1-3 sha256 tries) | 15 / 40 µs | target |
| Sign first variant (ed25519) | 25 / 40 µs | target |
| **Internal total, feed byte → first send** | **≤ 80 / 250 µs** | gate in CI on replay |
| Network to sender/leader in-metro | 0.2-3 ms (+ ≤ 50 ms Jito tick / 20 ms BAM round) | speed-tx-landing |
| **Observe → at leader** | **≈ 5-15 ms** | consistent with the observed 9-30 ms fastest reactive buys |
| Free mode (PumpPortal, Ohio) | +183 ms median vs slot notification | measured; slot +1 at best, paper only |

Internal stages are recorded in every rslog. External stages are measured by probes and by `deploy/latency_probe.sh`.

## 7. Multi-region: active box per leader window, plus the nonce pool

```
every epoch: schedule = getLeaderSchedule(); metro = stake_geo map; rtt[box][metro] from probes
role(box, window w) = PRIMARY if box == argmin_b rtt[b][metro(leader(w))] else BACKUP
PRIMARY fires with tip_policy(op); BACKUP fires the same intent with tip × 0.5 (still useful when
  the primary's path is congested) — both on the SAME nonce account, so at most one tx can land
nonce pool: N = 32 nonce accounts (authority = hot wallet; 32 × 0.00106 SOL rent), idx = H(mint) mod N
  each box tracks nonce values (gRPC account sub, or getAccountInfo at processed every slot in free mode)
  same-slot collision of two different armed mints on one idx -> second intent uses idx+1 (both boxes
  apply the same deterministic probe sequence)
heartbeat: boxes exchange role/armset_ver/risk_state every 1s; a box with a stale peer acts as PRIMARY
  for all windows (duplicates are still impossible because of the shared nonce)
```

Nozomi warns that builders may deprioritize durable-nonce txs. A blockhash variant can land *in addition to* a nonce variant, so it is enabled only on a single-box setup. Its marginal value is A/B-measured in probe mode before use.

## 8. Config (`config/rugsnipe.example.toml`; the real file is gitignored)

```toml
[mode]
mode = "paper"                 # paper | probe | live ; live also needs CLI flag --i-understand-live
box_id = "fra1"
regime_freeze_until_epoch = 1065   # shadow only until 1 epoch after Alpenglow (epoch 1064)

[wallet]
keypair_path_env = "RUGSNIPE_KEYPAIR"   # path read from env; file must be 0400, outside repo
max_balance_sol = 5.0                   # excess swept to cold wallet daily (alert if exceeded)
cold_wallet = "…"

[risk]
max_snipe_sol = 0.25
max_probe_sol = 0.02
max_probes_per_day = 20
max_open_positions = 4
max_daily_loss_sol = 1.0
max_consecutive_losses_per_op = 5
max_snipes_per_op_per_day = 10
kill_file = "/run/rugsnipe/KILL"
disarm_if_armset_older_s = 21600
disarm_if_decode_miss_rate = 0.01

[armset]
path = "/var/lib/rugsnipe/armset/current.json"
require_sha256 = true

[feeds]
order = ["shreds", "deshred", "preconf", "grpc", "pumpportal", "rpcws"]
[feeds.pumpportal] enabled = true   url = "wss://pumpportal.fun/api/data"
[feeds.rpcws]      enabled = true   url = "wss://api.mainnet-beta.solana.com"
[feeds.grpc]       enabled = false  endpoint = "https://laserstream-mainnet-fra.helius-rpc.com" token_env = "GRPC_TOKEN"
[feeds.deshred]    enabled = false  endpoint = "…triton…" token_env = "TRITON_TOKEN"
[feeds.preconf]    enabled = false  url = "wss://beta.helius-rpc.com" key_env = "HELIUS_KEY"
[feeds.shreds]     enabled = false  socket = "/run/rugsnipe/shreds.sock"

[senders]
[senders.helius]    enabled = false url = "http://fra-sender.helius-rpc.com/fast" swqos_only = true min_tip_lamports = 5000
[senders.nozomi]    enabled = false url = "http://fra2.nozomi.temporal.xyz" key_env = "NOZOMI_KEY" min_tip_lamports = 1000000
[senders.astralane] enabled = false url = "…fr…" key_env = "ASTRALANE_KEY" min_tip_lamports = 1000000
[senders.nextblock] enabled = false quic = "frankfurt.nextblock.io:11100" key_env = "NEXTBLOCK_KEY"
[senders.jito]      enabled = false url = "https://frankfurt.mainnet.block-engine.jito.wtf" bundle_only = true
[senders.tpu]       enabled = false fanout_leaders = 2
[senders.paper]     enabled = true

[tx]
format = "legacy"          # legacy | v1 (v1 after M5 simulation + sender acceptance test)
cu_limit = 75000           # tightened from measured p99 after M5
lads_limit_bytes = 16777216
use_nonce = true
nonce_pool_size = 32

[exit.defaults]
tp = [[2.0, 1.0]]
no_activity_ms = 150000
phantom_sl_ms = 10000
bundle_sell = true
max_hold_ms = 1800000
sell_retry_slots = 10

[record]
dir = "/var/lib/rugsnipe/rslog"
rotate_s = 3600

[sched]
peers = ["ams1.internal:7400"]
stake_geo = "/var/lib/rugsnipe/stake_geo.json"

[alerts]               # delivered by analytics alerts.py; hot box only writes Risk records
ntfy_topic_env = "NTFY_TOPIC"
```

**Secrets.** Only env-var *names* appear in config. `.gitignore` already covers `.env` and `*.pem`; add `config/*.toml` except `*.example.toml`, and `*.json` keypairs. A pre-commit hook rejects any 64-byte base58 or JSON byte-array keypair pattern.

## 9. Safety and operations

- **Paper by default.** The `live` mode needs all of: config `mode="live"`, the CLI flag, a non-zero risk cap, an ArmSet whose run passed the §5.3 gate (the `gate_passed` field is signed into its sha256), and a funded wallet.
- **Kill switch.** Any of these trips it: the kill file exists; `SIGUSR1`; the analytics box posts `kill` over an authenticated localhost-tunnel endpoint; or an automatic trigger fires:
  - daily loss cap reached;
  - balance drop with no matching intent;
  - send error rate > 20% over 5 min;
  - decode-miss rate > 1% (creates seen post-exec but not pre-exec);
  - the FeeConfig or IDL hash changed;
  - clock offset > 50 ms.

  Kill means: stop buying, keep exiting open positions, alert.
- **Metrics.** A `/metrics` endpoint on localhost: stage latencies, feed first-seen share, dedup hits, intents by decision, sends by sender, fills and errors, open positions, PnL. Analytics scrapes it over the SSH tunnel.
- **Alerts.** The user is not watching in real time, so `alerts.py` pushes:
  - kill and risk events (immediately);
  - a stale ArmSet;
  - collector gaps > 10 min;
  - an operator disarmed or flagged as a farm;
  - the daily report.

## 10. Failure modes and mitigations

| Failure | Detection | Mitigation |
|---|---|---|
| Collector down, so public history ages out (~1.5-2 days) | `ingest_gaps`, heartbeat alert | systemd restart; two free feeds (PumpPortal + RPC ws); gaps excluded from folds explicitly |
| Look-ahead leakage (API ATH/volume, later merges) | feature whitelist test; immutable `op_assign` | PIT features only; purge plus embargo; vault |
| Overfitting across policy grid/features | `n_configs_tried` | small grid; EB shrinkage; pooled gate; vault |
| Regime change (Alpenglow epoch 1064, fees, new create variant) | `regime` column; FeeConfig/IDL hash; unknown-discriminator counter | landing model reset per regime; auto-shadow until epoch 1065; disarm on hash change |
| Signer ≠ `creator` (18.9% of creates; launchers such as `J7pour…`) | — | match on signer, fee payer and `creator` arg |
| Launcher-wrapped (CPI) create invisible to pre-exec decode | decode-miss counter per launcher | per-launcher account map from fixtures; unknown launchers shadow-only |
| Landing behind the bundle or rivals | fills.buyers_ahead, CUSUM | `min_tokens_out` from `v_max`; tip inside the tx so it reverts; disarm on drift |
| Farming / supply-share trigger | A/B Δ_dump | randomized skips and size arm; ρ cap on size; disarm |
| Cross-region double buy | — | shared nonce per intent |
| Stale nonce or blockhash (30 s window) | tx error code | per-slot refresh; deterministic idx probing |
| Sell fails during the dump; post-completion gap | exit tries > 3 | sender rotation, tip escalation, PumpSwap path; mayhem and non-SOL excluded |
| Sender rejects v1 or nonce txs | probe A/B per sender | legacy default; per-sender variant flags |
| ArmSet corrupt or stale | sha256, `valid_until` | disarm all, keep shadowing |
| Key leak | balance alarm | hot balance capped; daily sweep; key never in repo or logs |
| Public RPC throttling | 429 counters in `rpc.py` | cache, backoff, tiered tape pulls (candidates plus a 5% control) |

## 11. Build plan (most valuable testable artifact first)

| # | Milestone | Testable artifact (public data only) | Effort |
|---|---|---|---|
| M0 | `schema/` (DDL, rslog.toml, ArmSet JSON schema) + `fixtures/` | ~200 real txs as base64 wire plus RPC JSON: legacy/v0/v1 `create_v2`, legacy `create`, `buy_v3`, `buy_exact_quote_in_v3`, `sell_v3`, legacy `buy` (18 accounts), PumpSwap sell, Token-2022 transfer, Binance transfer; golden rslog | 0.5 d |
| M1 | `store.py` + `collector.py` running on Ohio **now** (data is perishable) | every create with signer, funding (fresh) and fingerprints; tapes for candidates plus 5% control; `ingest_gaps` empty for 24 h | 1 d |
| M2 | `outcome.py` + `prequential.py` on D1/D2 plus the M1 data | **first OOS report:** E0/E1/E2 EV by `V_e` band, Wilson/bootstrap, base rate vs operator-selected; self-tests reproduce the `curve.py` and firstslot numbers | 2 d |
| M3 | `operators.py` + `arming.py` + ArmSet emitter + `report.py`/`alerts.py` | daily ArmSet with provenance; prequential gate status; candidate operator list | 2 d |
| M4 | `rs-core`, `rs-txview`, `rs-decode` | `cargo test`: all M0 fixtures decode; Rust curve == Python curve on 10k random states; v1/v0/legacy parity with `agave-transaction-view` | 2 d |
| M5 | `rs-exec` + `rugsnipe-sim` | `simulateTransaction` against mainnet state for `buy_v3`/`buy_exact_quote_in_v3`/`sell_v3`: account layout, CU p99, tight LADS, `partial_fill` and `spendable` fee semantics, whether SOL-quoted v3 needs wSOL (the decoded competitor tx created one), v1 vs legacy size | 1.5 d |
| M6 | `rs-feed` (free feeds), `rs-watch`, `rs-record`, `rugsnipe-hot --mode paper` on Ohio | 24 h shadow run: decode-miss rate, internal latency histograms, paper intents joined to on-chain outcomes; `rugsnipe-replay` CI latency gate | 2 d |
| M7 | `rs-exit` + PumpSwap sell + conformance | identical exit decisions Rust vs Python on 500 fixture tapes | 2 d |
| M8 | paid-feed/sender features; FRA box; **probe mode** (user funds ≤ 0.5 SOL, opt-in) | true `buyers_ahead` per sender and leader class; real `k̂`; nonce vs blockhash A/B | user action + 2 d |
| M9 | `rs-sched` + nonce pool + AMS box | duplicate-free dual-box probes; per-window role logs | 2 d |
| Gate | live sizing only after the §5.3 gate passes on OOS + vault with probe-measured `k̂` and costs | — | — |

## 12. What cannot be validated without paid infrastructure or live money

1. **True landing position and the slot-0 rate.** Paper mode can only estimate `buyers_ahead` (±20-40 ms of clock and index-to-time error). Real numbers need probe buys, even tiny ones. A 0.01 SOL probe lands the same way a 0.5 SOL buy does, except where rivals react to size.
2. **Shred, deshred and preconf feed latency and coverage** (DZ Edge, Helius raw/preconf, Triton Deshred, SVS): vendor claims only, until a paid trial with geyserbench runs from FRA.
3. **Rival behaviour conditional on our presence (farming, supply-share dumps).** This needs live trades. The A/B design makes it measurable, but only at the cost of real snipes.
4. **Sender acceptance and priority** for v1 txs, durable-nonce txs, `swqos_only` tips, and Harmonic/BAM leaders: needs live sends.
5. **Exit fills inside contested dump slots**, and the completion → `migrate` gap for held positions: the simulator is pessimistic by construction, but the real slippage is unknown until we sell into one.
6. **Microstructure history older than ~1.5-2 days.** Public RPC does not serve it. swap-api tapes give trades with tx index but no failed txs, fees or tips. Until M1 has run for weeks, the landing and cost models rest on short windows.
7. **The post-Alpenglow regime (from Oct 21).** Every landing and latency number measured before it must be re-measured after it. Outcome statistics probably carry over, but that is itself a hypothesis to test.
8. **Capacity at scale.** The qualifying snipes/day ceiling is estimable from M1/M2 data. Whether our own activity shrinks it (more snipers on the same operators, F-Project's 1,731 members) is only visible over live weeks.

Until items 1, 3 and 5 are measured, any EV number this system reports is an upper bound under the stated entry model E1. The report must say so in its header.
