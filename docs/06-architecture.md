# 06 Architecture (final synthesis)

Date 2026-10-14, epoch 1057, slot ≈456.79M. This is the build specification for `rugsnipe`. It merges three proposals and two judge reviews (`docs/research/arch-*.md`) and fixes every defect they found.

Tags:
- **[V]** I re-checked it today.
- **[J]** A judge checked it today.
- **[L]** Likely.
- **[U]** Unverified; test before relying on it.

## 0. Decisions

**Shape.** Judge 1 picked P3, judge 2 picked P2; both prescribe the same graft. **P3's process skeleton** (chain as truth, guard, keeper, upgrade gate, recovery before READY), **P1's hot-path internals** (pinned run-to-completion loop, complete PatchTable, per-class bids, advertised-TPU routing) and **P2's validation core**, which is normative and blocking (point-in-time operators, the same `arming` code live and in backtest, vault, `n_configs_tried`, randomized skips, `min_tokens_out` from `v_max`). On conflict: P2 wins on statistics, P3 on operations, P1 on packet → wire.

**Prior.** The evidence leans negative. The guru's headline rule loses ~27% of stake per snipe (skeptic §2), and the 16-day uniform study, in progress when this was designed, showed **0 hits in ~30 out-of-sample rows** for its first rule (preliminary; the final verdict is docs/04) [V, `data/reports/gap1_operators.json`]. The system is built to reach a **cheap, honest "no"** fast and stays paper-only unless the gate (§9.5) passes.

| Verified today | Result |
|---|---|
| Rent: T22 ATA / nonce | 1,513,840 / 1,056,640 lamports [V] |
| pump redeploy | programdata `B5MvUw…` slot **456,494,698**, authority `7gZufw…` [V] |
| Public history | mainnet-beta `getFirstAvailableBlock` = 0; `getBlock(445,000,000)` (Sep 7) served [V]. Rate-limited, **not perishable** |
| v3 buy fixture | signer `8Z2tbj…`; tokens to **owner `5aR7Xt…`**; quote ATAs at 0 lamports (no wSOL); **67,116 CU**; no ComputeBudget ix [V] |
| Create `Mz4aKh…` | **3 signers**: create_v2 + dev legacy buy + v3 buy by `GGd2xV…` [V] |
| Crates | agave-transaction-view 7.0.0 needs message ^5 / rustc 1.98.1, so **6.1.0**. litesvm 0.18.0, rusqlite 0.40.2, toml 1.1.8, solana-nonce 3.4.0 exist [V] |
| Code | `pump-codec` **53 tests pass** on 20 real fixtures; Python selftest 15 pass [V] |
| Sample size, α .05 one-sided, power .8 | 0.31 vs 0.40: 170 (**419** with Bonferroni over 50); 0.43 vs 0.55: 106 (264) [V] |

## 1. Contradictions and defects, resolved

| # | Issue | Ruling |
|---|---|---|
| 1 | "Public data is perishable" | False [V]. **Backfill** 21-30 days now (§9.1); the collector stays for post-Alpenglow data. |
| 2 | `idx+1` nonce probing on local state allows a cross-box double buy | **No probing**; fire only with the pre-create nonce value (§7.3). Collisions only skip. |
| 3 | Entry gate: observed bundle (P1), p75 (P3), on-chain bound (P2) | **All in-tx buys** plus on-chain `min_tokens_out(v_max)`: one information set for backtest and live (§7.1). |
| 4 | Free feeds have no wire bytes | `CreateHit` from `logsSubscribe` CreateEvent + TradeEvents (§7.1); wire decoder covered by fixtures and a `getTransaction` lane. |
| 5 | `creator_arg` spoofing | Live fire only on a **signer/fee-payer** match; `creator_arg`-only is shadow. Shared fee payers and launchers are never keys. |
| 6, 7 | Bundle set keyed on signers; single "dev buy" | Sum **every** pump buy in the create tx. The bundle set adds every in-tx buyer and the **owners of credited token accounts** [V]. |
| 8 | UpdateParent on Oct 21 | No: header/footer markers and the **8-byte abort batch**. UpdateParent waits for FLH (≥ Nov 9); poll `FLHoAW…` each epoch. |
| 9 | Stale nonce variant lands after a dump | Burn on the **first dump signal** or at 3 slots; `maxRetries=0` senders; `late_fill` class. |
| 10, 29 | Sell bounds (P1 tight, P2 inverted, P3 zero); uncapped escalation | TP sells: trigger quote × (1 − 15%). Dump sells: **floor quote** × 0.98 (never fails on price). Per-position sell-spend cap, ≤ 15 tries. |
| 11 | Partial-sell double fill; P2's blockhash+nonce buy variant | Final sell = blockhash + `CloseAccount`; partial = sell-nonce sub-pool. P2's buy variant **dropped**. |
| 12 | Arming at n ≥ 30 (P3); binary hit model (P2) | Empirical Bayes on **realized P&L**; hit SPRT only disarms. |
| 13 | iid Wilson; level metric; repeated looks | **Operator×day cluster bootstrap**; Δ vs a V_e-matched control; pre-registration; k̂ grid; family-pooled farm flag (§9.4). |
| 14 | `tx_index` → time landing model | Invalid on BAM/Harmonic batches (~63% of stake). Paper reports E2; landing comes from lock probes [U] and probe buys. |
| 15, 16, 17 | PatchTable byte test; missing `aqbc`; wSOL | SDK sorts keys: test **semantic** equality. Five mint keys include `aqbc` = ATA(bc, SPL, WSOL) [V IDL]. No wSOL [V]. |
| 18 | Metro from `stake_geo.json` | **Advertised `tpuQuic`** metro + Jito region, refreshed on gossip change. |
| 19, 20 | 13 crates; shred parser or 4.2.2 sidecar now | **Reuse `pump-codec`**. Shred parser only once a feed is bought, with markers and abort (sol-shred-sdk drops them). |
| 21 | Polling fallback 18.6 req/s vs a 4 req/s cap | Real positions require a **paid post-exec feed**; fallback polls every 2 slots within a per-IP **RPC budget**. |
| 22-24 | Two PumpPortal sockets; remote guard with a key copy; A/B skips in paper | One PumpPortal connection (collector). No remote guard (balance cap bounds the loss). A/B off in paper. |
| 25 | D2 survivorship | Uniform samples only. **Also excluded** (new finding): gap-1 `s6_expand` rows (wallets picked on full-window counts) and `s7` multi-wallet components (union-find over future rows) [V]. |
| 26 | Alerts via Ohio | Critical alerts **direct from the hot box** + healthchecks.io dead-man. |
| 27, 28, 30 | Bonferroni 264; P1's loose defaults; no create-age gate | 419 [V]. `max_entry_vsol` 40 SOL (floor loss 1 − (30/40)² = 44%); sizes 0.05-0.25 SOL. `max_age_slots = 2`. |
| 31 | Live token names one ArmSet sha (P3), but ArmSets regenerate every 6 h (new finding) | The token names the **gate run** (`code_sha` + `prereg_sha`), valid 7 days; every ArmSet from that run inherits it. |

## 2. System overview

```
 public RPC, PumpPortal(x1),  ┌──────── OHIO ohio1 (AWS us-east-2, exists; analytics + control, no hot path) ────────┐
 pump.fun API ───────────────►│ collector.py ──► analytics.db (SQLite WAL) ◄── backfill.py (shared RPC budget)        │
                              │ discover.timer 6h: op_assign(PIT) → outcome(E0/E1/E2) → prequential → arming          │
                              │                   → armset-<ver>-<sha>.json + ed25519 .sig (Ohio-only signing key)    │
                              │ report.py (go/no-go)   alertd.py (/status /kill)   backup → S3                       │
                              └───────▲ rslog + state.db snapshots (wg0, 5 min) ──────┬ ArmSet push (wg0) ───────────┘
                                      │                                               ▼
 paid shreds/gRPC (features) ┌────────┴────────── FRANKFURT fra1 (to rent: TeraSwitch bare metal) ────────────────────┐
 logsSubscribe (free) ──────►│ rugsnipe-hot   feed core : dedup → decode → match → gates → nonce → patch → sign → send│
                             │                sender cores: variants → Helius|Nozomi|Astralane|Jito|NextBlock|TPU|Paper│
                             │                exit core : positions ◄ TradeEvents/transfers of held mints ► sells      │
                             │                cold rt   : ledger → state.db, recorder → rslog, metrics, ArmSet watcher │
                             │ rugsnipe-guard  exit lease; sells when hot's exit loop stops renewing                  │
                             │ rugsnipe-keeper recovery/reconcile, nonce pool + burns, leader table, upgrade gate, sweep│
                             └────────────────────────────────────────────────────────────────────────────────────────┘
```

## 3. Rust workspace

The root `Cargo.toml` stays: edition 2021, `rust-version = "1.97.1"`, the speed-reference-impls §7 set, no `solana-sdk`, `bincode = "1.3"`.

Add `agave-transaction-view =6.1.0`, `solana-nonce 3.4`, `rusqlite 0.40` (bundled), `rtrb 0.4`, `arc-swap 1.9`, `xxhash-rust 0.8` (xxh3), `hdrhistogram 7.6`, `prometheus-client 0.25`, `sd-notify 0.5`, `tokio-tungstenite 0.30`, `zeroize 1.9`, `signal-hook 0.4`, `nix 0.31`, `core_affinity 0.8`, `toml 1.1`. Dev: `litesvm 0.18`, `proptest 1.12`, `insta 1.49`. `pump-rust-client =0.4.1` (no default features) is used at startup and in tests only.

| Crate | Responsibility | Key items |
|---|---|---|
| `pump-codec` (exists) | constants, PDAs, curve, events, ix parse/build, zero-copy `TxView`. **Add:** `create_hit` (all in-tx buys, owners, aqbc hint), `from_logs`, `launchers`, `transfers`, `alpenglow` (entry batch / marker / 8-byte abort) | `TxView`, `CurveState`, `SolV3Keys`, `CreateHit`, `BlockComponent` |
| `rugsnipe-core` | config + validation, ArmSet (sha256 + ed25519), `Watchlist` (ArcSwap snapshot + dynamic), `RiskGate` (atomics), rslog `Rec`, `IntentId = xxh3(mint‖wallet)`; no I/O | `Config`, `ArmSet`, `WatchEntry`, `Mode` |
| `rugsnipe-feed` | `trait Feed { fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus }`; `LogsWs` (free), `CollectorRelay`, `Grpc` (feature; raw codec + protobuf walker, prost oracle), `Shreds` (later); dedup, `FiredSet`, `FeedHealth` | `FeedEvent{src, recv_ns, slot, bank_id, payload: Wire\|Hit}` |
| `rugsnipe-tx` | templates + `PatchTable`, `NoncePool` (packed `AtomicU64`), signer (`zeroize`), `trait Sender` (`Paper`, `RpcSend`; feature-gated Helius, Nozomi, Astralane, Jito, NextBlock, TPU-QUIC), fixed-buffer HTTP/1.1, `Breaker` | `PatchTable`, `NonceSlot`, `Variant` |
| `rugsnipe-exit` | `Position` machine, triggers (§8), venue routing, bounds | `ExitPolicy`, `ExitReason` |
| `rugsnipe-state` | SQLite `StateStore` (one writer thread), `Recovery`, `Reconcile`, `Lease`, `ChainRead`/`TxSubmit` + `RpcChain` (solana-rpc-client =4.3.1, cold), `RpcBudget` | `Ledger`, `Lease` |
| `rugsnipe-sched` | leader schedule, tpuQuic metro, client class, Jito region, roles, heartbeat (phase 2) | `LeaderMap`, `Role` |
| `rugsnipe` (bins) | `rugsnipe-hot`, `-guard`, `-keeper`, `-ctl`, `-replay`, `-sim` | — |
| `rugsnipe-it` (tests) | LiteSVM runs of our real transactions against dumped mainnet programs | `SvmChain` |

`CreateHit` (`Copy`, no heap): `slot, bank_id, tx_idx, recv_ns, src, version, mint, bc, abc, aqbc_hint, user, fee_payer, creator_arg, signers[4], quote, token_prog, mayhem, cashback, intx_sol_in, intx_tokens_out, intx_buyers[4], launcher`.

## 4. Python package (`analysis/rugsnipe_analysis/`)

**Reused:** `rpc`, `pumpapi`, `cache`, `curve` (oracle for Rust math), `events` (independent decoder), `dataset.creation_record`, `firstslot`, `funding`, `labels`, `selftest`.

gap-1 stages s1-s4 become library functions. s5-s7 are superseded. `numpy` is added to `pyproject.toml`.

| New module | Purpose |
|---|---|
| `store.py`, `rpcbudget.py` | schema, upserts, `ingest_gaps`; per-IP, per-method token bucket (`[rpc_budget]`) |
| `collector.py` | the single PumpPortal connection + `logsSubscribe(TSLvdd…)` → creates, funding, tapes at T+35 min, rivals via `getSignaturesForAddress(bc)` (failed txs included); relays frames on `/run/rugsnipe/pp.sock` |
| `backfill.py` | signature index over 21-30 days → seeded uniform 5-10% + candidate-operator creates → `getTransaction` for slots s..s+3; regime tags |
| `fixtures.py` | ~200 fixtures + a manifest from the Python decoder |
| `operators.py`, `features.py` | point-in-time graph, immutable `op_assign`; feature whitelist with a banned-list test (`ath_market_cap`, volume, `complete`, any field fetched after `t_create`) |
| `outcome.py`, `bidmodel.py` | E0/E1/E2, exit replay (mirrors Rust), costs; point-in-time bid quantiles per leader class |
| `prequential.py`, `arming.py` | walk-forward of `arming.compile`, vault, cluster bootstrap, matched control; posteriors, states, sizing, ArmSet emit + sign (**same code live and in backtest**) |
| `abtest`, `landing`, `ingest`, `report`, `alertd` | farm analysis, landing, rslog decode, report, alerts |

## 5. Data model

### 5.1 Hot box `state.db` (SQLite WAL; integer lamports and raw units)

```sql
intents(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT, slot INT,
  bank_id INT, via TEXT CHECK(via IN('signer','fee_payer','funding')), feed_class TEXT, spend INT, v_max INT,
  min_tokens_out INT, nonce_idx INT, nonce_value TEXT, decided_ns INT,
  state TEXT CHECK(state IN('paper','sent','landed','failed','burned','late_fill')), UNIQUE(mint, mode));
sends(sig TEXT PRIMARY KEY, intent_id INT, variant INT, sender TEXT, prio INT, tip INT, sent_ns INT, result TEXT);
fills(sig TEXT PRIMARY KEY, intent_id INT, mint TEXT, slot INT, tx_index INT, tokens INT, sol_delta INT, err TEXT,
  buyers_ahead INT, rival_sol_ahead INT, late INT);
positions(mint TEXT PRIMARY KEY, intent_id INT, opened_slot INT, tokens INT, cost INT, v_fill INT, peak_v INT,
  bundle_json TEXT, policy_json TEXT, origin TEXT CHECK(origin IN('intent','orphan')),
  state TEXT CHECK(state IN('open','exiting','closed')), sell_spend INT, closed_slot INT);
exits(sig TEXT PRIMARY KEY, mint TEXT, reason TEXT, venue TEXT, tokens INT, sol_delta INT, slot INT, owner TEXT, try_no INT);
nonces(pubkey TEXT PRIMARY KEY, pool TEXT CHECK(pool IN('buy','sell')), idx INT, value TEXT, last_change_slot INT,
  state TEXT CHECK(state IN('free','inflight','burning','quarantined')), intent_id INT, since_slot INT);
watch_dyn(key TEXT PRIMARY KEY, kind TEXT, op_id INT, rule_id INT, added_slot INT, expires_unix INT,
  remove_if_no_launch_unix INT, keep INT, parent TEXT, state TEXT);
risk(day TEXT PRIMARY KEY, start_balance INT, deposits INT, sweeps INT, realized INT, buys INT, killed INT, kill_reason TEXT);
op_risk(op_id INT PRIMARY KEY, consec_losses INT, buys_today INT, disarmed_until INT);
lease(name TEXT PRIMARY KEY, owner TEXT, hb_unix_ms INT, epoch INT);
kv(k TEXT PRIMARY KEY, v TEXT);   -- armset_ver, last_wallet_sig, programdata slots, regime
-- schema v2 (shadow mode, docs/04 section 7): intents.via also 'class' (ArmSet class arm),
-- fills.fail_reason TEXT ('premium','slippage','not_in_slot0','class') + fills.fail_cost INT, and
shadow(intent_id PK, mint, model, outcome, create_slot, create_index, block_txs, bundle_trades, bundle_wallets,
  our_index, tp_ahead, tp_behind, s0_failed, premium_bps, post_bundle_v, post_bundle_t, post_bundle_mcap, cost);
-- schema v4 (probe, real sends): sends.bid / sent_unix_ms / kind ('buy','sell','burn') / err, and
-- fills.fee (meta.fee) / variant / sender / tip of the landed transaction; shadow.model = 'probe' for real landings
```

A version-1 file is upgraded in place (`schema::migrate`: columns added, `intents` copied to the new CHECK).

### 5.2 Ohio `analytics.db`

P2 §4.1 tables (trades, tx_attempts, transfers, funding, labels, slot0, operators, op_edges, arm_decisions, armsets, ab_assign, ingest_gaps, ingested intents/fills), changed and extended as follows:

```sql
creates(mint PK, slot, tx_index, sig, signer, fee_payer, creator_arg, launcher, tx_version, quote_mint, is_mayhem,
  is_cashback, priority_lamports, tip_lamports, intx_sol_in, intx_tokens_out, v_after_intx, regime, sample_arm)
create_buys(mint, sig, signer, token_owner, ix_name, sol_in, tokens_out)     -- every pump buy inside the create tx
trades(..., user, token_owner, ...)   slot0(..., bundle_owners_json)          -- owners, not just signers
op_assign(mint PK, op_id, as_of_slot, rule, conf)            -- written once, edges with first_slot < creates.slot
outcomes(mint, policy_id, entry_model, k_hat, cost_level, size, v_entry, filled, exit_reason, pnl, hold_ms)
prereg(sha PK, committed_at, grid_json, gate_json, vault_from)   runs(run_id PK, code_sha, prereg_sha, n_configs_tried)
folds(run_id, fold, test_from, test_to, n, ev_sel, ev_ctrl, diff_lb_cluster, hit, hit_wilson_lb)
landing(intent_id, box, sender, leader_class, slot_offset, buyers_ahead, rival_sol_ahead)   box_events(ts, box, kind, detail)
```

### 5.3 rslog and ArmSet

**rslog:**
- Header: `magic 0x52534C47, kind u16, ver u16, len u32, mono_ns u64, wall_ns u64, box u16`.
- Records: `FeedSeen`, `CreateHit`, `Matched`, `Decision`, `Sent`, `PosEvent`, `ExitFired`, `Risk`, `FeedHealth`, `Lease`, `Upgrade`, `NonceEvt`, and for shadow mode `Shadow` (13: create index, block size, bundle trades/wallets, our would-be index, third-party buyers ahead/behind, premium, post-bundle state) and `FailedBuy` (14: reason and cost of a buy that would have failed).
- `schema/rslog.toml` generates both decoders. A golden file is checked in CI.

**ArmSet:** P2's JSON plus `regime`, `min_hot_version`, `gate_passed`, `probe_allowed` (default false), `probe_stage` (optional `"a"` / `"b"`, must equal `[probe] stage`; required for stage `"b"`), `prereg_sha` and `shared_fee_payers`. File `armset-<ver>-<sha256>.json` with a detached ed25519 `.sig`. If it is unsigned, stale or from the wrong regime, the hot box runs exits only. `probe_allowed` unlocks probe buys, `gate_passed` unlocks live buys (§6).

## 6. Configuration (`config/rugsnipe.example.toml` in git; real file `/etc/rugsnipe/rugsnipe.toml`, 0640, gitignored)

```toml
[box]
id = "fra1"
role = "hot"
singleton = true
[mode]
mode = "paper"                                     # paper | probe | live
regime_freeze_slots = [[459_640_000, 459_869_000]] # Alpenglow: buys forced to paper
max_age_slots = 2
[secrets]                                          # systemd credential NAMES, never key material
hot_keypair = "hot-keypair"
probe_keypair = "probe-keypair"
api_keys = "api-keys"
armset_pubkey = "<base58>"
cold_wallet = "<base58>"
[risk]
max_snipe_lamports = 250_000_000
max_probe_lamports = 250_000_000                   # ceiling; the [probe] stage narrows it (a: 0.02 SOL, b: 0.25 SOL)
max_probes_per_day = 300                           # ceiling, attempts (a: 50, b: 300)
max_open_positions = 4
max_daily_loss_lamports = 1_000_000_000            # chain-derived, marked to the sell quote
max_consec_losses_per_op = 5
max_buys_per_op_per_day = 10
max_buys_per_hour = 20
max_bid_lamports = 25_000_000
max_sell_spend_lamports = 20_000_000
max_entry_vsol_lamports = 40_000_000_000
max_hot_balance_lamports = 5_000_000_000
min_hot_balance_lamports = 1_000_000_000
kill_file = "/run/rugsnipe/KILL"
armset_max_age_s = 21_600
decode_miss_kill_ratio = 0.01
send_error_kill_ratio = 0.20
max_clock_offset_ms = 5
[feeds]
logsws = { enabled = true, url = "wss://api.mainnet-beta.solana.com", stale_ms = 10_000 }
relay = { enabled = false, socket = "/run/rugsnipe/pp.sock" }    # collector's PumpPortal frames
grpc = { enabled = false, endpoint = "", token = "GRPC_TOKEN", stale_ms = 2_000 }
shreds = { enabled = false, bind = "0.0.0.0:7733", sources = ["dz_edge", "helius"] }
probe_live_require = "post_exec"
[senders]
paper = { enabled = true }
rpcsend = { enabled = true, url = "https://api.mainnet-beta.solana.com", sells_only = true }
helius = { enabled = false, url = "http://fra-sender.helius-rpc.com/fast", swqos_only = true, min_tip = 5_000 }
nozomi = { enabled = false, url = "http://fra2.nozomi.temporal.xyz", key = "NOZOMI_KEY", min_tip = 1_000_000 }
astralane = { enabled = false, key = "ASTRALANE_KEY", min_tip = 1_000_000 }
jito = { enabled = false, region = "auto", max_rps = 1 }
nextblock = { enabled = false, key = "NEXTBLOCK_KEY" }
tpu = { enabled = false, leaders_ahead = 2 }
[rpc_budget]                                       # roles sum <= 32 req/s per egress IP
hot = 4
guard = 4
keeper = 8
collector = 12
backfill = 4
per_method = 4                                     # Python buckets: per (host, method); methods = { getBlock = 2 } overrides
per_host = 10                                      # Python buckets: per host
[tx]
format = "legacy"
cu_limit = 0                                       # FLOOR: the hot path patches 65,000 + 1,500*S per mint (gap-5), clamped to [cu_limit, 1.4M]; 88_000 recommended; 0 = probe/live refused until `ctl sim` confirms the fit
lads_bytes = 0
nonce_pool = 32
sell_nonce_pool = 8
burn_after_slots = 3
partial_fill = false
[bid]                                              # docs/03 section 6.1 (pay on success)
quantile = 0.75
split = { jito_agave = [0.0, 1.0], bam = [1.0, 0.0], harmonic = [1.0, 0.0], other = [1.0, 0.0] }   # [priority, tip]; BAM priority-ordered (measured)
min_priority_lamports = 10_000                     # header priority fee floor of every variant
failed_attempt_lamports_max = 1_000_000            # base + priority of a failed attempt; priority clamped to it
[probe]                                            # docs/04 section 7; read in mode = "probe" only
stage = "a"                                        # a | b; an ArmSet naming probe_stage must agree (b: must name b)
a = { max_probe_lamports = 20_000_000, max_probes_per_day = 50, max_open_positions = 1, max_daily_loss_lamports = 200_000_000 }
b = { max_probe_lamports = 250_000_000, max_probes_per_day = 300, max_open_positions = 2, max_daily_loss_lamports = 1_000_000_000 }
kill = { min_landed = 100, min_attempts = 400, max_median_premium = 0.03, landing_attempts = 200 }
[exit.default]
tp = [[2.0, 0.5], [3.0, 1.0]]
tp_slip_bps = 1500
trail = { activate = 1.5, retrace_bps = 2500 }
phantom_sl = { after_ms = 10_000, bps = 4000 }
no_activity_ms = 150_000
max_hold_ms = 1_800_000
bundle_sell_min_lamports = 100_000_000
consolidation_pct = 20
big_buy_lamports = [0, 0]
sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0 }
[guard]
takeover_after_ms = 5_000
drop_from_peak_bps = 3000
grace_s = 60
[watch]
default_ttl_s = 1800
remove_if_no_launch_s = 3600
last_transfer_min_frac = 0.9
[obs]
metrics_listen = ["127.0.0.1:9464", "10.77.0.2:9464"]
rslog_dir = "/var/lib/rugsnipe/rslog"
alert = { telegram = "TG_TOKEN", deadman = "HC_URL" }   # credential names
[ab]                                               # probe/live only
skip_p = 0.2
small_arm = [0.2, 0.35]
small_lamports = 50_000_000
size_jitter = 0.10
[entry]                                            # docs/04 section 6
max_premium = 0.02                                 # fill cap over the post-bundle price
filter = "any"                                     # any | multi_bundle (>= 1 bundle wallet outside the creator set)
filter_model = ""                                  # frozen P(take-profit) trees (data/reports/strategy/entry_filter_v1.json); "" = off
filter_mode = "log"                                # log: score + tertile on every intent | skip: also drop score < t1 (no send, no cost)
[paper]                                            # shadow fill model (docs/04 section 7)
fill_model = "e1"                                  # e1: creation slot behind the bundle | e2: end of slot+1
entry_premium = 0.011                              # gap-2 rank-1 median
failed_attempt_lamports = 0                        # 0 = modelled per leader class from [bid]; > 0 = fixed override
leader_mix = { jito_agave = 0.272, bam = 0.379, harmonic = 0.251, other = 0.098 }   # weights while the leader is unknown
round_trip_lamports = 4_000_000                    # fixed cost per landed round trip (summary net P&L)
exit_latency_slots = 1                             # a paper sell fills at the end of trigger slot + 1
slot_ms = 217                                      # slot clock of E1 replays
replay_slots = 12
delay_ms = 8_000
late_block_frac = 0.10                             # create in the last 10% of its block: not_in_slot0
max_tx = 100
min_replay_slots = 7                               # tape must cover s+7 (exits by +5, fill +1), else unresolved
[collector]                                        # Python collector overrides (all optional; typed and validated in Rust too)
```

**Bid and the cost of a failed attempt** (docs/03 §6.1, `rugsnipe-core::bid`). A lost race still
lands and fails (or drops). The base fee (5,000 lamports per signature) and the priority fee are
charged on failure; the tip is a transfer **after** the buy in the same transaction and reverts with
it; a failed bundle is not committed. So the bid of a tip-ordered leader class (Jito-Agave) goes
into the tip with the priority fee at `min_priority_lamports`, and the bid of a priority-ordered class
(BAM, Harmonic, other) goes into the priority fee, clamped so that base fee + priority never exceeds
`failed_attempt_lamports_max`. BAM is priority-ordered by default because our own measurement
(docs/03 §6) found CU-price order inside BAM rounds 86-89% of the time; the probe measures which
variant lands per leader class before that changes. Sells take the same split
(`BidPolicy::plan_sell`): floor priority + the sell bid as a tip on a tip-ordered class, the sell bid
as the priority fee elsewhere (and for an unknown leader), never both; the buy cap does not clamp a
sell (`risk.max_sell_spend_lamports` bounds it). Validation refuses a cap below `2 × 5,000 + min_priority_lamports`
in every mode and warns when a tip class puts part of the bid into the priority fee. While the
leader's class is unknown the primary variant is the pay-on-success one and the secondary the
clamped priority one, on one durable nonce. **Every failed attempt is recorded with its realised
cost** in `fills.fail_cost` (and `paper.failed_cost_lamports` / `failed_cost_lamports_mean` in the
session summary): in paper the cost modelled per class (`paper.failed_attempt_lamports = 0`: base +
the matching variant's priority fee, weighted by `paper.leader_mix`, ≈ 0.00073 SOL at the defaults; a
value > 0 overrides it, e.g. 3,000,000 for docs/04's 0.003 SOL case); in probe/live the `meta.fee` of
our own failed transaction (`e1::failed_tx_fee`; the real fill lane, §16, must call it). Sensitivity
(docs/04 §4, `pnl_model.py`): 0.003 SOL per failed attempt puts the 0.5 SOL LEAD's break-even landing
rate at 12.1%; the modelled range 0.00036 / 0.00073 (default) / 0.001 SOL puts it at 1.6% / 3.3% / 4.4%
(0.25 SOL: 4.1% / 8.1% / 10.7%). The session summary also carries the probe kill rule's inputs
(`attempts_scored`, `ev_per_attempt_se`, `ev_per_attempt_90ci`, `median_premium_bps_fills`,
`failed_cost_lamports_mean`, `landing_rate`; docs/04 §7).

**Lead-policy exit knobs** (all off by default; `config/paper-lead.example.toml` sets docs/04 section 6):
`tp_basis = "value"` (take-profit on the realised value of the stake after both fees and our own impact),
`fire_on = "trade"` (normal triggers also on the first trade that shows them), `insider_sell = "bundle" |
"bundle_only" | "creator" | "off"`, `phantom_sl = { after_ms, drop }` (measured from the post-bundle price, the
curve marked without our own tokens; `{ after_ms, bps }` keeps the fill reference) and `max_hold_slots` (hard exit
at landing slot + N). An ArmSet may carry `match_all = <op_id>`: a class arm that every create no key matches maps
to (`via = class`); the lead policy's universe is a class of launches, not a list of operators.

One file serves both sides: the Rust parser refuses unknown keys, so the Python-only knobs (`[rpc_budget]` `per_method`/`per_host`/`methods`, `[collector]`) are typed there as well.

**Per-mint CU.** `S = Σ (255 − bump)` over `bonding_curve`, the curve's base ATA, the curve's wSOL ATA and our base ATA, recovered from keys the hot path already holds by hashing alone (no curve checks; `S + 4` SHA-256 rounds). The patch writes `65,000 + 1,500·S` (expected cost `63,500 + 1,500·S`: gap-5's `62,310 + 1,500·S` plus 1,185 for `buy_exact_quote_in_v3`) into the v1 header or the legacy `SetComputeUnitLimit`, and re-derives the legacy CU price from the same lamport total. The formula reproduces all 361 gap-5 simulations within −263/+38 CU (`rugsnipe-tx/tests/cu_gap5.rs`); `ctl sim` re-measures it on live mints. **Wallet term:** pump also re-derives the buyer's `user_volume_accumulator`, so S includes `W = 255 − UVA bump` of the hot wallet (computed once at startup; gap-5's wallet has W = 0). Live simulations on 2026-10-14 (F3): a wallet with UVA bump 254 used 1,500 CU more on every mint and one with 252 used 4,500 more, so without W the legacy snipe failed `ComputationalBudgetExceeded` at the per-mint limit; with W all runs fit with 1,050 (legacy) / 1,500 (v1) CU to spare. The wallet's wSOL-ATA bump has no effect. Grinding the hot key to UVA bump 255 (one key in two) makes W = 0. With the recommended 88,000 floor the limit only rises above the floor for S ≥ 16; a lower floor (e.g. 65,000) lets it tighten.

Validation **refuses probe or live** if any of these holds:
- a cap is 0 or missing, or `cu_limit = 0`;
- a key credential is group- or world-readable;
- clock offset > 5 ms;
- no healthy post-exec feed;
- the ArmSet is unsigned, stale or from the wrong regime;
- **probe:** the ArmSet does not carry `probe_allowed = true`, or it names a `probe_stage` other than `[probe] stage` (stage b: it must name `"b"`), or a cap of the selected stage is 0. Probe does **not** need `gate_passed`: the probe exists to measure landing, premium and k̂, which the gate needs (§9.5, §16). In probe mode each cap of the selected stage narrows the `[risk]` cap of the same name (`Config::effective_risk`; a tighter `[risk]` cap is warned);
- **live:** the ArmSet does not carry `gate_passed = true` (plus the `--live` flag and the arm-live token, §10);
- the RPC budget role sum is > 32.

## 7. Hot path

### 7.1 Sequence (feed core, run to completion, zero heap allocation after warm-up)

```
1  ev = feed.poll()                      # Wire(&[u8]) from gRPC/shreds, or Hit(CreateHit) from LogsWs/relay
2  if !dedup.insert(ev.sig) or !fired.first(ev.mint, ev.class): rec(Dup{src, delay}); return   # free feeds never claim a live slot
3  hit = ev.hit or create_hit(TxView::parse(ev.bytes)?)    # legacy|v0|v1, launchers, all in-tx buys
4  if hit.quote != SOL or hit.mayhem or hit.token_prog != T22 or hit.cashback_untested: rec(SkipClass); return
5  w = watch.get(signers ∪ fee_payer − shared_fee_payers); if !w { if watch.get(creator_arg): rec(Shadow); return }; op = armset.op(w.op_id)
6  if slot_now − hit.slot > max_age_slots or aborted(hit.slot, hit.bank_id): rec(Stale); return
7  d = risk.decide(op, feed_class, regime(hit.slot), mode); if d == Skip: return   # latch, caps, freeze
8  V0 = curve after all in-tx buys; if V0 ≥ op.v_max: rec(TooHigh); return
9  if mode ∈ {probe, live} and H(salt‖mint) < op.ab_skip_p: rec(SkipAB); return
10 n = nonces.buy[xxh3(mint) % N]; if n.state != Free or n.last_change_slot ≥ hit.slot: rec(NoNonce); return  # simulated in paper
11 X = op.size·(1 ± jitter); min_out = tokens_out(X, CurveState{v: op.v_max, t: k/op.v_max}, fees)
12 tmpl.patch(mint, bc, abc, aqbc_hint or ATA(bc,SPL,WSOL), ATA(us,mint,T22), X, min_out, n.value, bid(class(leader)))
13 sign → write primary socket; push FireJob to sender cores (other variants, each with its own tip ix)
14 n.state = InFlight{intent, slot}; rec(Decision, Sent); ledger.push(Intent)
```

**Free mode.** `logsSubscribe(mentions: TSLvdd…)` yields the CreateEvent (mint, curve, user, which is a signer, creator, token program, mayhem, quote mint [V, `events.rs`]) and the same transaction's TradeEvents (the in-tx buys). Fee payer and token owners are unknown, so free mode matches on `user` and is **paper only**. A cold lane fetches confirmed `getTransaction` bytes to fill the owners and exercise the wire decoder.

**Template** (legacy): `AdvanceNonceAccount`, `SetComputeUnitLimit`/`Price`, `CreateIdempotent` (T22 ATA), `buy_exact_quote_in_v3` (17 accounts; `spendable` gross [J2]), tip transfer. v1 (config bits for priority, CU, LADS) waits for simulation and sender acceptance. Patch offsets sit in the writable non-signer class; `user_volume_accumulator` and nonces are pre-created. A seed account instead of the ATA [U] is IDL-legal (the competitor used 54,877 CU); simulate first.

### 7.2 Latency budget (fra1, FRA/AMS leader)

| Stage | Budget | Basis |
|---|---|---|
| Leader execute → first shred | 1-5 ms | [L] |
| Shred delivery in-metro (DZ Edge / Helius raw) | 0.1-3 ms | vendor [U] |
| `TxView` + classify | 0.04-0.13 µs | P1 measured |
| ATA PDA (+ aqbc if no hint) | ~10.5 µs each | J1 measured |
| ed25519 sign | 22-32 µs VM / ~10 µs bare metal | P1/J1 measured |
| Patch, base64, `write()` | 4-11 µs | target |
| **Feed byte → primary on wire** | **p99 ≤ 100 µs VM, ≤ 60 µs bare metal** | CI replay gate |
| Box → sender / proxy TPU, same building | 0.1-0.3 ms | [U] |
| Sequencer wait: Jito-Agave / Harmonic FBA / BAM | 0 / ≤ 50 / 20-50 ms | critique-speed |
| **Total: Jito-Agave leader / batched (~63% of stake)** | **≈ 2-9 ms / 25-60 ms** | |
| Free mode | +≈183 ms median → **slot +1 at best** | measured |

Our code is 1-3% of the budget. Effort goes to placement, then feed, then bidding.

**Bids.** `B = min(Q_q(point-in-time winning bids[class]) × 1.1, op.ev_cap, max_bid)`, split by leader class (Harmonic ranks on priority; the BAM split is learned in probe). Routing uses the advertised tpuQuic metro and the leader's Jito region.

### 7.3 Nonce pool and the no-double-buy rule

**Pool.** 32 buy + 8 sell nonces. Authority = hot wallet. `ctl nonce init` on the singleton box. Rent 0.042 SOL.

**State per slot.** Packed `AtomicU64` plus `last_change_slot` from the account stream.

**Transitions:**
- `Free → InFlight` (fire).
- `InFlight → Free` when the value changes.
- `InFlight → Burning` at `since + 3 slots` **or on the first dump signal** (keeper sends an AdvanceNonce-only tx).
- `Burning → Free`; if a buy variant beat the burn, it is recorded as `late_fill` and opens a position.
- `Burning → Quarantined` after 3 failed burns (alert).

**Rule.** Fire a mint only on `xxh3(mint) % N`, with **no probing**, and only with a value whose `last_change_slot < create_slot`.

**Why it holds:**
- Every box maps the mint to one account.
- All variants and boxes hold the same pre-create value, so at most one executes.
- Any landing or burn changes the value at a slot ≥ create_slot, which disqualifies every later attempt from any box or after any restart.
- A collision between two mints costs one snipe, never a double buy.

I3 is chaos-tested with two boxes on divergent feeds. Partial sells use the same rule with the trigger slot in place of the create slot, keyed on (mint, ladder step).

## 8. Exit engine (`rugsnipe-exit`; Python mirror, conformance on 500 tapes)

**Opening.** A position opens on **our own** TradeEvent or a non-zero token balance, never on a send.

**Inputs:** TradeEvents and `CompleteEvent` for the mint, timers, and Token-2022 transfers out of the bundle wallets' ATAs. Plain `Transfer` omits the mint, so consolidation is a paid-feed-only trigger.

**Bundle set:** in-tx buyers and their token owners, slot-0 adjacent and shared-tip signers, creator/signer/fee payer, and funding-linked wallets from the ArmSet.

**Price.** `mult = (v / v_fill)²`, since price ∝ V².

| # | Trigger | Default rule (per-operator overrides come from the backtest) | Kind |
|---|---|---|---|
| 1 | BundleSell | any bundle wallet sells ≥ `bundle_sell_min` → 100% | dump |
| 2 | Consolidation | Σ transfers into one owner within 2 slots ≥ `consolidation_pct` of supply → 100% | dump |
| 3 | BigBuy | one buy within `[lo, hi]` → 100% | dump |
| 4 | TakeProfit | ≤ 5 `(mult, fraction)` levels | normal |
| 5 | Trailing | after `activate`, price ≤ peak × (1 − retrace) | normal |
| 6 | PhantomSL | after `after_ms`, `mult ≤ 1 − bps` | normal |
| 7 | NoActivity / MaxHold | 150 s without a trade / 30 min held | normal |
| 8 | Completion | venue → PumpSwap `sell` on the canonical pool (pump-codec `AmmTrade`; fixtures exist; simulate) | — |

**Sells.** Bounds: normal `min_sol_output = quote_sell(tokens, V_trigger) × (1 − tp_slip)`; dump `quote_sell(tokens, V = 30 SOL) × 0.98`. A final sell uses a blockhash + `CloseAccount` (recovers 1,513,840 lamports); a partial sell uses a sell nonce. Send to all healthy senders + `rpcsend`; retry every 2 slots with tip ×2 until `max_tries` or `max_sell_spend`, then a critical alert. Size always comes from the chain balance.

**Not in v1** (each can be backtested later with the same outcome engine): copy-trading, buy-on-dev-sell, buy-the-dip, multi-wallet buys.

**Guard.** Hot's exit loop renews `lease('exits')` each iteration. If the lease is older than 5 s with a position open, guard takes over by compare-and-set (`epoch + 1`) and sells on a ≥ 30% drop from peak, on MaxHold, or after `grace_s`. It reads the curve through its own gRPC stream, or `getMultipleAccounts` within its RPC budget.

## 9. Discovery, scoring and arming

### 9.1 Data

| Set | What it is |
|---|---|
| **D1** | `tokens_light.jsonl`, 1,496 random creates in 48 h |
| **G1** | gap-1 uniform prefix of a 16-day, 848k-signature index |
| **B** | backfill, 21-30 days: 5-10% uniform + candidate creates (~0.5-1M calls, ≈ 2-3 days in budget [estimate]) |
| **F** | the forward collector |

- **Excluded:** D2, s6 rows, mayhem (22.3% of D1; the curve breaks the floor [J2]), non-SOL quote, legacy SPL creates.
- **Regimes:** before Sep 15 (no v1); before Oct 9 (about 268 ms mean slot time, not the nominal 400 ms; see research/gap-6: E2 only, never landing); after Oct 21 (re-measure landing).

### 9.2 Point-in-time operators

For a create c, use only edges with `first_slot < c.slot`.

**Strong edges** (merge): same creator wallet; a non-CEX mother funding ≥ 2 fresh creators in one amount band (±0.0005 SOL); a slot-0 bundle wallet or token owner seen in an earlier operator; a T22 transfer between two operators' wallets; profit recycling.

**CEX-funded creators** are never merged through the CEX. Their key is (exchange, amount band, dev-buy band, launcher).

**Weak signals** (tip account, CU fingerprint, `uri_host`) are features only.

`op_assign` is immutable. A merge keeps the older id and records `merged_into`.

### 9.3 Outcomes

**Entry models:** E0 = end of slot 0 (reported only); **E1** = after all in-tx buys, the adjacent bundle and k̂ reactive buyers (the decision model); E2 = end of slot 0+1 (free paper is always E2). If `V_entry > v_max` the entry is unfilled and costs base fee plus priority. The exit fill is pessimistic: the whole trigger slot, plus slot s+1 up to the median index, plus our impact.

**Costs**, all point-in-time: fees from TradeEvents/FeeConfig tiers, rent from RPC, `bidmodel` bids, failed-attempt and burn fees.

**Grids.** Every report covers k̂ ∈ {0, 1, 2, 3, 5} × skeptic §2's {low, base, high} costs.

### 9.4 Prequential evaluation and arming

**Loop.** Every 6 h, `state_t = arming.compile(visible, as_of=t)` (the live function) runs on data whose 30 min outcome horizon closed before a 1 h embargo, and decides creates in [t, t+6 h) point-in-time. The last 3 days are a **vault**, unread until a frozen `code_sha` is evaluated. Grid changes increment `n_configs_tried`. `prereg/<date>.toml` (≤ 24 policies, thresholds, costs, vault boundary) is committed before the first OOS read, and `report.py` refuses to run without it.

**State machine:**

```
CANDIDATE→SHADOW   ≥ 2 launches
SHADOW→PROBE       P(EV>0) ≥ .8, n ≥ 5, active in the last 24 h
PROBE→ARMED        P(EV>0) ≥ .9, E[EV] ≥ 0.005 SOL, n ≥ 10, ≥ 5 measured probe landings, pooled gate green
ARMED→SUSPENDED    SPRT(hit vs band break-even, +0.10; α .05, β .2) accepts H0 | landing or P&L CUSUM
                   | 5 consecutive losses | family farm flag | 48 h without a launch
SUSPENDED→SHADOW   after 24 h;   any→RETIRED after 7 days without a launch
```

**Statistics:**
- **Posterior.** Normal-normal empirical Bayes on realized P&L per unit stake; prior per family (funding pattern, slot-0 shape, launcher) × V_e band, by method of moments.
- `v_max` = the largest V whose 20th-percentile posterior EV is ≥ 0, capped at 40 SOL.
- **Size.** `argmax over {0.05, 0.1, 0.25} of EV_lb`, subject to quarter-Kelly on the lower bound, `max_snipe`, and `≤ 0.15 × E[slot-0 SOL]`.
- **Anti-farm.** Skip if our buy pushes the bundle's share below its p25. Randomized skips + small-size arm. Consecutive-loss disarm. The farm flag is pooled per family with alpha-spending over 4 looks a day, because per operator 300 snipes detect a 10-point Δ at only z ≈ 1.5 [J2]. Snipe delay (re-enter at slot +N) is a paper-evaluated policy.

### 9.5 Go/no-go gate (signed into the ArmSet as `gate_passed`)

The gate unlocks **live** only, for the class-arm LEAD of docs/04 (a class of launches, not a list of operators). The **probe** comes before it in two stages (docs/04 §7, `[probe]`): probe-A, 0.02 SOL buys, ≤ 50 attempts a day, 1 open position, 0.2 SOL daily loss cap, until 300 attempts or 3 days; probe-B, 0.25 SOL, ≤ 300 attempts a day, 2 open positions, 1 SOL daily loss cap, about 200 landed trades, entered only if probe-A's landing rate beats the break-even for its measured failed-attempt cost and its median premium is ≤ +2%. Probe needs a signed, fresh ArmSet with `probe_allowed = true` plus every risk cap and credential check of §6, not `gate_passed`, because the landing, premium and failed-attempt cost it measures are inputs to this gate.

All must hold for `gate_passed`:
- **(a) shadow:** ≥ 1,000 qualifying launches over at least one full day, and the shadow EV per attempt at the **measured** rank, premium and failed-attempt cost has a 90% lower bound above 0, bootstrapped by hour clusters (`shadow_gate.py`);
- **(b) probe-B complete:** ≥ 200 landed trades; the EV per attempt, failed attempts included, has a 90% lower bound above 0; no single hour carries more than 40% of the P&L; we land ahead of the first follower on at least one hot launch in three at a premium of +2% or less (`shadow_gate.py --probe`);
- **(c)** every cost is measured, not modelled: the failed-attempt cost, the landed round trip, the premium and the exit fills come from our own transactions;
- **(d)** all of it is re-verified after Alpenglow (epoch 1064, Oct 21 2026) before any live start: pre-cutover data does not count.

The operator-count, matched-control and vault-EV criteria of the earlier gate are dropped: the LEAD is a class arm, and per-operator arming failed its fresh holdout (docs/04 §5). The per-operator risk latches stay (`max_consec_losses_per_op`, `max_buys_per_op_per_day`, the §9.4 suspensions).

Every report is headed: *"EV is an upper bound until landing, farming and exit fills are measured by probes."*

### 9.6 Funding watcher (pre-creation watchlist)

An ArmSet funder sending a fresh recipient (pre-balance 0) an amount in ≤ 5 **discovered** bands (2.955 never occurred) upserts `Funded`: TTL 30 min (lag 24-798 s, median 159 s), remove-if-no-launch 60 min, `keep`. A watched wallet moving ≥ 90% of its balance creates a `Successor` that inherits the preset. Precision per rule goes back to Python. Free mode uses per-address `logsSubscribe` (Binance `5tzFki…` ≈ 0.26 tx/s fits the budget); paid mode uses gRPC `account_include`.

## 10. Modes and landing measurement

| Mode | Where | What it proves |
|---|---|---|
| `paper` | here / Ohio, free feeds | decode coverage, decisions, internal latency; E1 shadow entries reconstructed from confirmed history (where we would have landed, premium, failed attempts) and their exits, or E2 P&L; never our real landing rank |
| `shadow` | fra1 + SVS gRPC ($150/mo) | detection lead vs rivals' landing slots; not landing position |
| lock probe [U] | fra1 | 1-lamport transfer to the curve (takes its write lock) on our nonce, sender and bid; `tx_index` vs rival buys = landing position, no tokens held |
| `probe` | fra1 + paid post-exec feed | `probe_allowed` ArmSet (no gate needed), two stages (§9.5, `[probe] stage`): **A** 0.02 SOL buys, ≤ 50 attempts/day, 1 position, 0.2 SOL/day loss cap (landing rank, premium, failed-attempt cost per class and the variant that lands; EV negative by design); **B** 0.25 SOL, ≤ 300/day, 2 positions, 1 SOL/day, ~200 landed trades; kill rule `shadow_gate.py --probe` |
| `live` | fra1 | `gate_passed` ArmSet + TOML + CLI flag + local `ctl arm-live` token for the gate run |

**Shadow fill model E1** (`paper.fill_model = "e1"`, `rugsnipe::e1`). The free feed sees a create ~1 s late, so the cold lane rebuilds the creation slot from confirmed history inside the RPC budget (`getSignaturesForAddress` on the curve; `getBlock` with `transactionDetails = "signatures"` for the transaction indices; `getTransaction` for the slot-0 transactions), applies the gap-3 bundle definition (create-tx buys, creator-set buys, the contiguous run of slot-0 buys at index ≤ create + 5 with gaps ≤ 2; mirrored from `strategy/tapes.py`), and returns a verdict: **fill** at the post-bundle state with `max(entry_premium, measured)` (a wallet seen sniping another launch's slot 0 this session, found inside the run, is ahead of us and its price impact is the measured premium); **premium** (above `entry.max_premium`), **not_in_slot0** (create in the last `late_block_frac` of its block) and, in the hot loop, **slippage** (the operator's `v_max` bound) are failed attempts that pay `failed_attempt_lamports`; **class** (not multi-bundle under `entry.filter`) is a skip at no cost. A fill replays the next `replay_slots` slots through the exit engine on a slot clock with a tick per slot, on the counterfactual curve that keeps our tokens outstanding (the tape simulator's model), and the sell fills at the end of trigger slot + `exit_latency_slots`. The session summary's `cold.paper` block carries the totals (outcomes, landing rate, mean premium, exits by reason, realized and net P&L, EV per attempt including failures). `analysis/scripts/paper_vs_sim.py` re-derives every closed fill from swap-api tapes with `strategy/sim.py`.

**Paper recorder.** Every `CreateHit` (hit or miss) logs per-feed `recv_ns` (the `FiredSet` race gives feed lead for free), the decision and the would-send bytes. `landing.py` joins these to the rivals that landed in slots 0-1 (`tx_index`, SOL), the earliest slot we could reach (send time + RTT to the leader's metro), and an estimated `buyers_ahead` for Jito-Agave leaders only.


**Status (2026-10-16, task #12 first half).**
- *Built (probe):* probe key loading from `$CREDENTIALS_DIRECTORY` only (`probe-keypair`, `api-keys`; missing, malformed, group/world-readable or systemd `ABSENT` credentials and sender key names missing from `api-keys` are refused; `rugsnipe::keys`).
- *Built (probe):* the real senders in `rugsnipe-hot` behind `--features real-senders`: Helius `/fast?api-key=` plus the `swqos_only` leg, Jito `frankfurt` + `amsterdam` `bundleOnly` (a 1 req/s token bucket per region, the pay-on-success primary only), Nozomi batch send, Astralane `irisb`, BlockRazor `sendTransaction` mode `fast`, NextBlock. Each sender gets its own variant of every attempt: its tip account (rotated, unique inside the intent), `tip = max(bid, its minimum)`, the class plan's priority fee, all on the intent's one durable nonce (`rugsnipe::sender::plan_variants`). Every send is a `sends` row (sender, variant, bid, tip, priority, wall-clock time, error).
- *Built (probe):* the real fill lane (`rugsnipe::real`). A position opens only on our own TradeEvent (the feed's or the landed transaction's) or a token balance. A landed buy is ranked in its slot: block index, third-party buyers ahead and behind, premium over the post-bundle price. A landed failure costs `meta.fee` (`e1::failed_tx_fee`; 6002/6042 = `slippage`). Nothing landed means the keeper's AdvanceNonce burn, then `dropped` at no cost; a variant that beats the burn is a late fill.
- *Built (probe):* real sells on a recent blockhash through the same senders (`BidPolicy::plan_sell`) and the guard's `--real-sell`; a position closes on our own sell TradeEvent or a zero balance.
- *Built (probe):* nonce seeding from chain at recovery (foreign-authority accounts quarantined); recovered balances adopted with an exit owner; `--probe-dry` (sign with the real key, record, simulate, send nothing).
- *Built (probe):* the per-sender and per-variant landing stats in the summary (`cold.probe`) and `kv.probe_report`. The `fills` / `shadow` rows carry exactly what `shadow_gate.py --probe` reads; paper intents forced to paper inside a probe run are written with `intents.mode = 'paper'`.
- *Missing (second half):* the gRPC post-execution feed, so real buys cannot fire yet (probe start is refused with "no healthy post-execution feed"; `--probe-dry` works); kill-latch persistence in `state.db`; the chain-derived daily loss (failed attempts' `meta.fee` and closed positions count today); the remaining kill triggers (send-error ratio, unexplained drop, upgrade gate, disk, clock, drift); the leader schedule (per-class bids and stats); Telegram alerts; the TPU sender; the sell nonce pool (partial sells use a blockhash); PumpSwap real sells.
- *Live:* refused in this build.
## 11. Multi-region scheduler

**Phase 1** is fra1 alone until the gate passes. **Phase 2** adds a second box; docs/03 §4 picks Tokyo (TeraSwitch TYO2, +7.8 points of same-slot coverage) before Amsterdam (AMS3), so the phase-2 box is tyo1, with ams1 as the third. The owner of each 4-slot window (still 4 slots under Alpenglow) is `argmin(rtt[box][tpuQuic metro])`. The backup fires at half tip only if the owner's heartbeat (wg0, 50 ms) is older than 150 ms. The nonce rule makes overlap harmless; roles exist for Jito's 1 req/s/IP/region limit and fee accounting. The heartbeat carries `{armset_ver, role_epoch, killed, mode}`, so a kill propagates, and each box computes the chain-derived loss itself, which makes the cap global.

**Leader table:** `getLeaderSchedule`, `getClusterNodes`, DZ `validators-metadata`, Jito region; refreshed on gossip change (every 60 s, Oct 18-28).

## 12. Deployment and failover

| Box | Runs | Notes |
|---|---|---|
| **This container** | tests, collector, backfill, paper bot, reports | Ephemeral; no SSH to the user's boxes |
| **ohio1** (exists) | collector, backfill, timers, S3 backup; paper bot until fra1 exists | Only the ArmSet signing key. Never live (0% of stake, ~105 ms to FRA). User runs `deploy/install.sh --role analytics` |
| **fra1** (to rent) | hot, guard, keeper; chrony; isolcpus | TeraSwitch FRA bare metal (Latitude/Cherry; AWS eu-central-1 fallback). SVS gRPC ($150) first; after the gate, DZ Edge ($1,500) + Helius shreds ($800-1,000) |
| **tyo1** (then ams1) | phase 2 | Only after the gate passes; order per docs/03 §4 |

**systemd:** `Type=notify`, `WatchdogSec=3` fed only while the feed and exit loops advance, `LoadCredential=`; chain recovery before `READY=1`.

**Failover:**

| Failure | Response |
|---|---|
| hot crash | restart + recovery; guard takeover after 5 s |
| Ohio down | last ArmSet until `armset_max_age`, then exits only |
| fra1 dead (phase 1) | positions stranded, bounded by `max_open_positions × max_snipe`; dead-man alert |
| paid feeds down | entries go to paper; exits poll within the RPC budget |

## 13. Safety

**Kill latch.** Buys stop, exits continue, unkill is local only.

**Triggers:** kill file, `SIGUSR1` or Telegram `/kill`; the daily-loss cap (wallet SOL + Σ position sell quotes + recoverable ATA rent, against start + deposits − sweeps); an unexplained drop > 0.05 SOL; send errors > 20% over 5 min; decode-miss > 1%; a program upgrade or FeeConfig change; a stale ArmSet; disk > 90%; clock offset > 50 ms; reconcile drift > 60 s.

**Metrics and alerts.** `prometheus-client` on loopback and wg0: feed lag and first-seen share, decode misses, stage latency, intents by decision, sends and breakers, landed slot offset, positions, exits by reason, free nonces, wallet, daily P&L, mode, ArmSet age, lease owner, drift. Critical alerts: killed; a position whose exit feed is down or owned by guard; a sell failing 5 tries; hot down 30 s; drift; upgrade; low balance. A daily digest gives per-operator stats.

**Upgrade gate.** Keeper watches the programdata slots of pump (redeployed 17 h ago [V]), PumpSwap and pump-fees, plus FeeConfig `8Wf5Ti…`. On a change, buys go to paper while `rugsnipe-sim` simulates `buy_exact_quote_in_v3`, `sell_v3` and PumpSwap against a live curve and decodes the last 100 pump transactions; buys resume only if everything passes.

**Alpenglow:** freeze window in config; events keyed on `(slot, bank_id)`; an abort batch cancels that slot's pending actions; tick-based timing retired; marker fixtures captured 24 h after cutover; unfreeze needs decode-miss < 1% for 6 h.

**Secrets:** keys 0400 via `LoadCredential`, in a `zeroize` buffer under `mlockall`. `.gitignore` gains `keys/`, `*.keypair.json`, `config/*.toml` (except `*.example.toml`) and `secrets/` **now** (none are blocked today [V]); a pre-commit hook rejects keypair patterns. Separate probe and hot wallets; daily sweeps to the user's cold wallet.

## 14. Testing strategy

1. **Fixtures.** Today: 20 tx, 4 accounts, 5 API files. `fixtures.py` extends them to ~200 (every create and trade variant, launcher creates, the 3-signer create, the third-party v3 buy, PumpSwap, T22 transfers, a Binance withdrawal, the competitor nonce snipe, a mayhem create for class tests only). Rust must match the Python decoder manifest field for field.
2. **Golden math.** proptest over 10k `(V, T, X)` states: Rust equals `curve.py` to the lamport.
3. **Alpenglow vectors.** Marker, header, footer and **8-byte abort**, from `solana-entry 4.3.1/src/block_component.rs` (in the cargo registry).
4. **PatchTable** semantic equality with pump-rust-client over 1,000 random mints.
5. **Counting `#[global_allocator]`.** Zero allocations on the feed thread in `rugsnipe-replay`; p99 ≤ 100 µs on this VM.
6. **`rugsnipe-sim`** (`simulateTransaction`, `sigVerify=false`): template legality, CU and loaded bytes on live curves; **impersonate an existing nonce account's authority** to test legacy+nonce and v1+nonce without funds [L]; seed-account and lock-probe checks.
7. **LiteSVM** (`litesvm` 0.18) on dumped programs: create → nonce buy → sell + close; two variants on one nonce execute exactly once; the burn invalidates the pending variant; an oversell fails; v1 acceptance.
8. **Conformance.** Rust exits equal `outcome.py` on 500 tapes. rslog golden file, banned-feature test, Python selftests.
9. **Nightly chaos:** SIGKILL anywhere, feed loss, sender 5xx, 429 storms, disk full, clock jumps, bad ArmSet, upgrade, abort batch.

   Invariants: **I1** no real send in paper or killed; **I2** every balance has an exit owner within 2 s (hot) or 6 s (guard); **I3** ≤ 1 landed buy per mint with **two boxes**; **I4** ledger P&L = chain deltas; **I5** no nonce InFlight beyond the window + 40 slots; **I6** positions after recovery = non-zero token accounts; **I7** no fire past `max_age_slots`.

## 15. Build plan for this session

| M | Deliverable | Acceptance |
|---|---|---|
| **M0** | `.gitignore` + hook; `prereg/`; `store`, `rpcbudget`, `collector --local`, `backfill` running here; `deploy/install.sh` | Dummy keypair refused; collector 1 h with no gaps; backfill progressing in budget. |
| **M1** | `pump-codec`: `create_hit`, `from_logs`, `launchers`, `transfers`, `alpenglow` | `cargo test` green; 3-signer fixture sums both buys and lists `GGd2xV…`; v3 fixture yields owner `5aR7Xt…`; abort/marker vectors pass; logs path == wire path. |
| **M2** | `operators`, `features`, `outcome`, `bidmodel`, `prequential`, `arming`, `report` | **First OOS report** (D1 + G1 + backfill): E0/E1/E2 × k̂ × cost, cluster-bootstrap Δ vs control, vault untouched. |
| **M3** | `rugsnipe-core`, `rugsnipe-tx` (PatchTable, NoncePool + rule, Paper/RpcSend), `rugsnipe-sim` | Semantic equality on 1,000 mints; template sim succeeds on a live curve; two-box nonce tests pass; unsafe configs refused. |
| **M4** | `rugsnipe-feed::LogsWs`, `rugsnipe-exit`, `rugsnipe-hot --mode paper`, `rugsnipe-replay` | 1 h paper run here; decode-miss reported; zero allocations, p99 ≤ 100 µs; exit conformance on tapes. |
| **M5** | `rugsnipe-state` (recovery, reconcile, lease), guard, keeper, `rugsnipe-it`, chaos | SIGKILL recovery; LiteSVM nonce exclusivity and burn; upgrade gate on a faked slot; I1-I7 green. |
| **M6** | Feature-gated `Grpc`, HTTP/QUIC senders, `rugsnipe-sched` skeleton | `--all-features` builds; mock-sender tests; walker == prost on synthetic frames. |
| M7 (user) | Rent fra1; SVS shadow; lock probes → probe-A (0.02 SOL) → probe-B (0.25 SOL) under the kill rule; re-measure after Alpenglow | Measured k̂, landing and costs feed the gate. |

M0-M4 are the session minimum. M2's report is the go/no-go artifact and can answer "no" before any money is spent.

## 16. Needs paid infrastructure or live money (cannot be verified here)

1. **Pre-execution feed latency and coverage** (DZ Edge covers 53% of slots). Until measured, block-0 numbers are models.
2. **True landing position, slot-0 rate and k̂** (lock probes [U] or probe buys from fra1).
3. **Sender behaviour:** v1/nonce acceptance, `maxRetries=0`, BAM/Harmonic admission and bid split.
4. **Farming caused by us**; exit fills in contested dump slots and across completion.
5. **Post-Alpenglow behaviour** (Oct 21; FLH ≥ Nov 9); every landing number is re-measured.
6. **FRA in-building RTT and bare-metal tuning.**
7. **Untested ideas:** slot-expiry assertion, seed-account CU savings, lock probe (simulation, then one live send each).

Until items 1-4 are measured, every reported EV is an upper bound under E1, and no ArmSet can carry `gate_passed = true`, so live stays refused. Measuring items 2-4 is what **probe** is for: capped probe buys need a signed ArmSet with `probe_allowed = true` plus every risk cap and credential check (§6), not the gate.
