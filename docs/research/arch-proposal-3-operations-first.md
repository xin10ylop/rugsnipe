# Architecture proposal 3: operations first (robustness / unattended-running angle)

Written 2026-10-14 (epoch 1057, slot ~456.78M). Builds on `docs/research/*.md`, on `analysis/`, and on proposal 2 (validation first). Proposal 2's analytics schema and statistics are reused. This document adds what keeps the system correct while nobody is watching.

**Design rule.** The chain is the source of truth. Local state is a cache that can always be rebuilt from it.

Consequences of the rule:
- Every money-moving action is **idempotent by construction**: a durable nonce per intent, one row per signature, and positions derived from token balances.
- Every component can be killed with `SIGKILL` at any instruction and restarted without a human.

## 0. Facts I re-verified today (beyond the lead's list)

| Fact | How I checked it | Consequence |
|---|---|---|
| rustc **1.97.1** is now installed | `rustc --version` | The Agave 4.3.1 MSRV problem is gone. Pin `rust-toolchain.toml` to 1.97.1. |
| `agave-transaction-view` **7.0.0** needs solana-message/transaction ^5 and rustc 1.98.1. **6.1.0** needs message ^4.5, transaction ^4.2, keypair ^3.1.2 (MSRV 1.89). | crates.io sparse index | Pin `=6.1.0`, which matches proposal 2. |
| `yellowstone-grpc-proto` **14.0.1** (the crate, not just master) already has `bank_id`, `SubscribeUpdateEntryUpdateParent{cleared_bank_id}`, `SubscribeUpdateDeshredUpdateParent`, `BlockFooter`, `include_update_parent`, `SLOT_DEAD` | Downloaded the `.crate` and grepped `proto/geyser.proto` | We can be Alpenglow-ready on the crate set we already chose. |
| The **pump program was upgraded 2026-10-13 15:40 UTC** (programdata `B5MvUw…` deploy slot 456,494,698). PumpSwap was upgraded 2026-10-08 16:20 UTC. Upgrade authority is `7gZufw…` for both. | `getAccountInfo` jsonParsed + `getBlockTime` | Program upgrades happen weekly or more often. Program-upgrade detection is a first-class kill trigger (§9). |
| Rent: nonce account (80 B) 1,056,640 lamports; Token-2022 ATA (170 B) 1,513,840 lamports | `getMinimumBalanceForRentExemption` | Pool of 32 nonces = 0.0338 SOL. |
| Epoch 1057, slotIndex 158,414 / 432,000; 215-220 ms per slot | `getEpochInfo`, `getRecentPerformanceSamples` | Epoch 1064 starts at slot **459,648,000**. Alpenglow consensus starts at **459,653,000** (+5,000), matching critique-speed. |
| SIMD-0385 v1: `LifetimeSpecifier` "is the same as the Recent Blockhash field". The spec says nothing about nonces. Max 64 addresses, no duplicates, no ALT. Unset CU limit = **0**. | Fetched the proposal text | Durable nonce in v1 is LIKELY to work, but it must be proven by simulation. We default to **legacy** format, as the competitor's nonce snipe already lands that way. |
| `litesvm` **0.18.0** sits on the Agave 4.3 line (transaction ^4.1.5, message ^4.2.4, keypair ^3.1.2, MSRV 1.97.1) | sparse index | In-process execution tests of our real txs against dumped mainnet programs, with no money and no network. |
| Crate versions: rusqlite 0.40.2, prometheus-client 0.25.1, sd-notify 0.5.0, tracing 0.1.44 / tracing-subscriber 0.3.23, rtrb 0.4.0, hdrhistogram 7.6.0, arc-swap 1.9.2, toml 1.1.8, signal-hook 0.4.5, zeroize 1.9.1, proptest 1.12.0, insta 1.49.0, solana-nonce 3.4.0 | crates.io API | Used below |
| `analysis` selftest | `.venv/bin/python -I -m rugsnipe_analysis.selftest` | 15 tests pass, so the Python reference implementation is usable as an oracle. |

## 1. Process and box layout

```
                 OHIO  ohio1 (AWS us-east-2, exists today)  -- analytics + control plane, NO hot path
 ┌────────────────────────────────────────────────────────────────────────────────────────────┐
 │ rs-collector.service (Python, always on) ──► analytics.db (SQLite WAL) ◄── rs-ingest.timer   │
 │ rs-discover.timer (6 h): operators → outcome → walkforward → armset → sign ──► push (wg0)   │
 │ prometheus + alertmanager ──► Telegram / Discord      rs-alertd (Telegram /status /kill)   │
 │ rs-backup.timer (sqlite .backup → S3)    [optional] rugsnipe-guard --remote (exit-only)     │
 └──────────────▲──────────────────────────────▲───────────────────────────┬─────────────────┘
                │ rslog + state.db snapshots   │ scrape /metrics            │ armset-<ver>.json + .sig
                │ (rsync over WireGuard)       │ (wg0, 15 s)                ▼
 FRANKFURT fra1 (TeraSwitch bare metal; AWS eu-central-1 fallback)    AMSTERDAM ams1 (phase 2, same units)
 ┌────────────────────────────────────────────────────────────────────────────────────────────┐
 │ rugsnipe-hot.service   (one process, pinned threads, Type=notify, WatchdogSec=3)             │
 │   feeds ─► dedup ─► decode ─► match ─► decide ─► build ─► [sender threads sign+send]        │
 │   funding watcher ─► watchlist        exit engine ◄─ post-exec feed (held mints, own wallet)│
 │   risk gate / kill        ledger thread ─► state.db (WAL)      recorder ─► rslog/           │
 │ rugsnipe-guard.service  (independent; takes over exits when hot's heartbeat is stale)       │
 │ rugsnipe-keeper.service (cold: reconcile, nonce pool, leader/TPU table, sweep, upgrades)   │
 │ [feature] shred-ingest.service (separate Cargo workspace; Unix socket to hot)               │
 │ node_exporter, chrony (Jito NTP), wireguard                                                 │
 └────────────────────────────────────────────────────────────────────────────────────────────┘
```

**Before the FRA box exists,** `fra1` units run on `ohio1` with free feeds in **paper mode**. That is the configuration for the whole validation phase. Ohio is never used for live buys (speed-geography: P_same 0.51, race score 0.01).

**Why three binaries on the hot box, not one:**
- `hot` is fast and complex.
- `guard` is slow, simple and separately supervised. Its only job is to make sure open positions get sold.
- `keeper` holds everything that does blocking RPC.

A crash-loop in `hot` therefore never strands a position. A slow RPC in `keeper` never touches the hot cores.

## 2. Crates and modules

Rust workspace `crates/`: `edition = "2021"`, `rust-version = "1.97.1"`, dependency set from speed-reference-impls §7, plus `agave-transaction-view = "=6.1.0"`. No `solana-sdk`. `bincode = "1.3"`.

| Crate | Responsibility | Key types | Notable deps |
|---|---|---|---|
| `rs-types` | Program IDs, PDAs, 8 buyback recipients, u128 curve math (golden-tested against `curve.py`), FeeConfig model, newtypes. No alloc. | `Lamports`, `CurveState`, `FeeBps`, `Mint`, `IntentId(u64)` | solana-pubkey 4.4 |
| `rs-wire` | Zero-copy view of legacy/v0/v1 txs. Alpenglow `BlockComponent` marker parsing for raw-shred batches. | `TxView<'a>`, `Marker::{Header,UpdateParent,Footer}` | agave-transaction-view =6.1.0 |
| `rs-decode` | `create_v2`, legacy `create`, launcher table (`J7pour…`, `rapidV…`), all buy/sell variants incl. v3, Anchor CPI events, System and Token-2022 transfers, PumpSwap buy/sell. Program ID is checked before the discriminator. | `CreateSeen`, `TradeSeen`, `SolXfer`, `TokXfer` (all `Copy`) | rs-wire |
| `rs-feed` | `Feed` trait; adapters `PumpPortalWs`, `RpcWs`, `Grpc` (feature `yellowstone`), `Deshred` (`triton-deshred`), `Preconf` (`helius-preconf`), `ShredSock` (`shreds`). Signature dedup ring. Per-feed health state machine. | `FeedEvent{src,recv_ns,slot,bank_id,bytes}`, `FeedHealth` | tokio 1.53, tokio-tungstenite 0.30, yellowstone-grpc-client 15.0.0 (opt) |
| `rs-watch` | Watchlist lifecycle and funding rules (§8). ArmSet verification: ed25519 signature, sha256, `valid_until`, regime. | `Watchlist`, `WatchEntry`, `FundingRule`, `ArmSet` | arc-swap 1.9.2, serde_json |
| `rs-tx` | Legacy and v1 templates, patching, signing, nonce pool, `Sender` trait plus Helius, Nozomi, Astralane, NextBlock (QUIC), Jito BE, TPU-QUIC, RpcSend and Paper. Circuit breakers. | `BuyTemplate`, `SellTemplate`, `NoncePool`, `Variant`, `Breaker` | solana-message 4.6, solana-transaction 4.3, solana-keypair 3.1, solana-nonce 3.4, reqwest 0.13, quinn 0.11; pump-rust-client =0.4.1 at startup and in tests only |
| `rs-exit` | Position state machine and triggers (§7); routes the sell to `sell_v3` or PumpSwap | `Position`, `ExitPolicy`, `ExitReason` | rs-tx |
| `rs-risk` | Modes, caps, kill latch, chain-derived daily loss | `RiskGate`, `Mode` | — |
| `rs-state` | `StateStore` trait. SQLite impl (one writer thread, WAL). Recovery and reconcile algorithm (§10). Exit lease. | `Ledger`, `Recovery`, `Lease` | rusqlite 0.40.2 (`bundled`) |
| `rs-chain` | `ChainRead` / `TxSubmit` traits: RPC impl for production, LiteSVM impl for tests | `ChainRead`, `RpcChain` | solana-rpc-client =4.3.1 |
| `rs-obs` | Cache-line-padded atomic counters (hot side). Prometheus exposition on a cold thread. JSON tracing. rslog recorder (SPSC ring to file). Direct panic-alert pusher. | `Metrics`, `Rec`, `Recorder` | prometheus-client 0.25.1, rtrb 0.4.0, hdrhistogram 7.6.0, tracing-subscriber 0.3.23, sd-notify 0.5.0 |
| `rs-sched` | Leader schedule, advertised `tpuQuic` table, client class, Jito region, box role per leader window, peer heartbeat | `LeaderMap`, `Role` | rs-chain |
| `bin/rugsnipe-hot`, `bin/rugsnipe-guard`, `bin/rugsnipe-keeper`, `bin/rugsnipe-ctl` | Processes and the operator CLI | — | signal-hook 0.4.5, nix 0.31 (mlockall) |
| `bin/rugsnipe-replay`, `bin/rugsnipe-sim`, `bin/rugsnipe-chaos`, `bin/chaosproxy` | Replay harness, `simulateTransaction` checker, fault-injection runner, ws/HTTP fault proxy | — | — |
| `crates/rs-it` (tests only) | End-to-end execution of our real txs in LiteSVM against dumped mainnet program and account state | `SvmChain: ChainRead + TxSubmit` | litesvm 0.18.0 (dev-dep only) |

**Reuse of work already in progress.** A root `Cargo.toml` workspace (`members = ["crates/*"]`, rust-version 1.97.1, the §7 dependency set) and a `crates/pump-codec` crate already exist. `pump-codec` is a pure-byte codec covering constants, PDAs, curve math, events, instruction parse/build and a zero-copy legacy/v0/v1 view. It comes with tx, account and API fixtures under `crates/pump-codec/fixtures/`.

`pump-codec` **is** `rs-types` + `rs-wire` + `rs-decode` in the table above. Do not create those three crates. Instead, extend `pump-codec` with three things:
- Alpenglow marker parsing;
- the launcher table;
- `SolXfer`/`TokXfer` decoding.

Every other crate depends on it.

Python (`analysis/rugsnipe_analysis/`) reuses `rpc.py`, `pumpapi.py`, `cache.py`, `curve.py`, `events.py`, `dataset.creation_record`, `firstslot.token_metrics`, `funding.*` and `labels.*` unchanged. New modules:
- `store.py`, `collector.py`, `operators.py`, `outcome.py`, `walkforward.py`, `armset.py`, `landing.py`, `ingest.py`. Schema and semantics follow proposal 2 §4-5.
- `fixtures.py`, which writes `fixtures/` for Rust.
- `alertd.py`.

## 3. Data flows

1. **Collect (Ohio, continuous).** PumpPortal `subscribeNewToken` plus RPC `logsSubscribe(mentions: TSLvdd…)` (two independent free sources) → `creation_record()` → `analytics.db.creates`. Funding is traced for fresh creators. Tapes are pulled at T+35 min for candidate operators plus a seeded 5% control. Public `getBlock` keeps only ~1.5 days, so every gap is recorded in `ingest_gaps` and alerted.
2. **Discover and arm (Ohio, every 6 h).** Operators are point-in-time → outcomes under exit policies → walk-forward → ArmSet. The ArmSet is signed with the Ohio `armset` key (ed25519, pubkey pinned in the hot config) and pushed by rsync over WireGuard to `fra1:/var/lib/rugsnipe/armset/incoming/`.
3. **Pre-creation trigger (hot box).** A funding-source transfer → watchlist entry (with TTL). Last-transfer successors are inherited.
4. **Create → buy (hot box).** feed → decode → watch hit → risk → build → K variants on one nonce → senders. Intent and send records go to the ledger ring.
5. **Fill → position → exit.** The own-wallet post-exec stream (or keeper polling) confirms the fill by signature → position → exit triggers → sell → close the ATA → realized P&L from tx meta.
6. **Feedback.** rslog files and `state.db` snapshots are rsynced to Ohio every 5 min → `ingest.py` → `landing.py` (our position vs rivals) → the next ArmSet.
7. **Health.** `/metrics` is scraped every 15 s → Alertmanager → Telegram/Discord. Kill events are also pushed straight from the hot box, so they still arrive when Ohio is down.

## 4. Data model

### 4.1 Hot-box `state.db`

Path `/var/lib/rugsnipe/state.db`. SQLite, `journal_mode=WAL`, `synchronous=NORMAL`, `busy_timeout=2000`. The only writers are the hot ledger thread, guard and keeper. All amounts are INTEGER lamports or raw token units.

```sql
CREATE TABLE intents (intent_id INTEGER PRIMARY KEY,         -- xxh3(mint || wallet), deterministic
  mint TEXT NOT NULL, op_id INTEGER, box TEXT, mode TEXT, armset_ver INTEGER, slot INTEGER, bank_id INTEGER,
  decided_ns INTEGER, spend INTEGER, min_tokens_out INTEGER, v_max INTEGER, nonce_pubkey TEXT,
  nonce_value TEXT, state TEXT NOT NULL CHECK(state IN ('sent','landed','failed','burned','paper')),
  UNIQUE(mint, mode));                                         -- at most one buy intent per mint and mode
CREATE TABLE sends (sig TEXT PRIMARY KEY, intent_id INTEGER NOT NULL, variant INTEGER, sender TEXT,
  tip INTEGER, prio INTEGER, sent_ns INTEGER, result TEXT);
CREATE TABLE fills (sig TEXT PRIMARY KEY, intent_id INTEGER, mint TEXT, slot INTEGER, tx_index INTEGER,
  tokens INTEGER, sol_delta INTEGER, err TEXT, buyers_ahead INTEGER);        -- sol_delta from meta
CREATE TABLE positions (mint TEXT PRIMARY KEY, intent_id INTEGER, opened_slot INTEGER, tokens INTEGER,
  cost INTEGER, peak_v INTEGER, policy_json TEXT, origin TEXT CHECK(origin IN ('intent','orphan')),
  state TEXT CHECK(state IN ('open','exiting','closed')), closed_slot INTEGER);
CREATE TABLE exits (sig TEXT PRIMARY KEY, mint TEXT, reason TEXT, venue TEXT, tokens INTEGER,
  sol_delta INTEGER, slot INTEGER, owner TEXT, try_no INTEGER);
CREATE TABLE nonces (pubkey TEXT PRIMARY KEY, idx INTEGER UNIQUE, value TEXT, state TEXT
  CHECK(state IN ('free','inflight','burning')), intent_id INTEGER, since_slot INTEGER);
CREATE TABLE watch_dyn (key TEXT PRIMARY KEY, kind TEXT, op_id INTEGER, origin TEXT, added_slot INTEGER,
  expires_unix INTEGER, remove_if_no_launch_unix INTEGER, keep INTEGER, parent TEXT, state TEXT);
CREATE TABLE risk (day TEXT PRIMARY KEY, start_balance INTEGER, realized INTEGER, buys INTEGER,
  killed INTEGER, kill_reason TEXT);
CREATE TABLE op_risk (op_id INTEGER PRIMARY KEY, consec_losses INTEGER, buys_today INTEGER, disarmed_until INTEGER);
CREATE TABLE lease (name TEXT PRIMARY KEY, owner TEXT, hb_unix_ms INTEGER, epoch INTEGER);  -- 'exits'
CREATE TABLE kv (k TEXT PRIMARY KEY, v TEXT);   -- armset_ver, last_wallet_sig, programdata_slot, regime
```

What the uniqueness keys buy:
- `sends.sig`, `fills.sig` and `exits.sig` make P&L ingestion exactly-once.
- `intents UNIQUE(mint, mode)` prevents a second buy of the same mint, even after a restart.

`analytics.db` on Ohio uses proposal 2's schema (`creates`, `trades`, `tx_attempts`, `funding`, `operators`, `op_assign`, `outcomes`, `folds`, `armsets`, `ab_assign`, `landing`). This proposal adds two tables: `box_events(ts, box, kind, detail)` for kills, takeovers, upgrades and regime changes, and `ingest_gaps`.

### 4.2 rslog and ArmSet

- **rslog** uses proposal 2's record layout (header magic `0x52534C47`). This proposal adds three records:
  - `FeedHealth{feed, state, lag_slots}`
  - `Lease{owner, epoch}`
  - `Upgrade{program, slot}`
- **ArmSet** uses proposal 2's JSON. Its file name is `armset-<ver>-<sha256>.json`. It carries a detached `.sig`, and three extra fields: `regime`, `min_hot_version` and `gate_passed`.

## 5. Config (`/etc/rugsnipe/rugsnipe.toml`, 0640 root:rugsnipe; `config/rugsnipe.example.toml` in git)

```toml
[box]
id = "fra1"                       # fra1 | ams1 | ohio1
role = "hot"                      # hot | analytics
singleton_tasks = true            # nonce creation, sweeps: exactly one box
cores = { feed = 2, senders = [3, 4], exit = 5, cold = [0, 1] }

[mode]
mode = "paper"                    # paper | probe | live
live_requires_flag = "--i-understand-live"   # and ctl confirmation, see §11
regime_freeze = [[459640000, 459869000]]     # slot ranges: buys forced to paper (Alpenglow ±~12 h)

[secrets]                         # names of systemd credentials, never values
hot_keypair = "hot-keypair"       # /run/credentials/rugsnipe-hot.service/hot-keypair
armset_pubkey = "8xA…"            # verify-only public key
cold_wallet = "Cold…"             # sweep destination (public key only)
api_keys = "api-keys"             # one credential, KEY=VALUE lines

[risk]
max_snipe_lamports = 250_000_000
max_probe_lamports = 20_000_000
max_open_positions = 4
max_daily_loss_lamports = 1_000_000_000      # chain-derived, §11
max_consec_losses_per_op = 5
max_buys_per_op_per_day = 10
max_hot_balance_lamports = 5_000_000_000     # keeper sweeps the excess
min_hot_balance_lamports = 1_000_000_000     # alert below this
kill_file = "/run/rugsnipe/KILL"
armset_max_age_s = 21600
decode_miss_kill_ratio = 0.01

[feeds]                           # all enabled feeds run concurrently; first arrival wins
pumpportal = { enabled = true, url = "wss://pumpportal.fun/api/data", stale_ms = 30000 }
rpcws      = { enabled = true, url = "wss://api.mainnet-beta.solana.com", stale_ms = 10000 }
grpc       = { enabled = false, endpoint = "https://laserstream-mainnet-fra.helius-rpc.com", key = "GRPC_TOKEN", stale_ms = 2000 }
deshred    = { enabled = false, endpoint = "", key = "TRITON_TOKEN", stale_ms = 2000 }
preconf    = { enabled = false, url = "wss://beta.helius-rpc.com", key = "HELIUS_KEY" }
shreds     = { enabled = false, socket = "/run/rugsnipe/shreds.sock", stale_ms = 1000 }
degraded_entry = "paper"          # when only post-exec or free feeds are healthy: paper | skip | allow

[senders]                         # breaker: open after 5 errors in 30 s, half-open after 20 s
helius    = { enabled = false, url = "http://fra-sender.helius-rpc.com/fast", swqos_only = true, min_tip = 5000 }
nozomi    = { enabled = false, url = "http://fra2.nozomi.temporal.xyz", key = "NOZOMI_KEY", min_tip = 1000000 }
astralane = { enabled = false, url = "", key = "ASTRALANE_KEY", min_tip = 1000000 }
nextblock = { enabled = false, quic = "fra.nextblock.io:11100", key = "NEXTBLOCK_KEY" }
jito      = { enabled = false, url = "https://frankfurt.mainnet.block-engine.jito.wtf", bundle_only = true, max_rps = 1 }
tpu       = { enabled = false, leaders_ahead = 2 }
rpcsend   = { enabled = true, url = "https://api.mainnet-beta.solana.com", sells_only = true }
paper     = { enabled = true }

[tx]
format = "legacy"                 # v1 only after M4 simulation + per-sender acceptance
cu_limit = 75000                  # tightened from simulated p99 + 5 %
lads_bytes = 16777216
nonce_pool = 32                   # buys (32 × 1,056,640 lamports rent)
sell_nonce_pool = 8               # partial ladder sells only
nonce_burn_after_slots = 3
buy_ix = "buy_exact_quote_in_v3"  # spend X, require min_tokens_out(V_max)

[exit.default]
tp = [[2.0, 0.5], [3.0, 1.0]]     # (price multiple from fill, fraction of the remaining position)
trail_from_peak_bps = 3000
no_activity_ms = 150000
phantom_sl = { after_ms = 10000, bps = 5000 }
bundle_wallet_sell = true
consolidation_pct = 40
big_buy_lamports = [2_800_000_000, 3_200_000_000]
max_hold_ms = 1_800_000
sell_retry = { every_slots = 2, max_tries = 15, tip_mult = 2.0, tip_cap = 10_000_000 }

[guard]
takeover_after_ms = 5000
policy = "protect"                # protect | sell_now
grace_s = 60

[watch]
default_ttl_s = 1800
default_remove_if_no_launch_s = 3600
last_transfer_min_frac = 0.9
max_entries = 50000

[obs]
metrics_listen = ["127.0.0.1:9464", "10.77.0.2:9464"]   # loopback + WireGuard only
rslog_dir = "/var/lib/rugsnipe/rslog"
panic_alert = { telegram_chat = "-100…", token = "TG_TOKEN" }

[sched]
peers = ["10.77.0.3:7400"]        # ams1 over wg0
tpu_refresh_s = 300               # 60 between Oct 18 and Oct 28 (BAM maintenance, Alpenglow)
```

**Secrets.** Neither git nor the TOML ever holds a secret.

- **Keypairs.**
  - Stored as `/etc/rugsnipe/keys/*.json`, mode 0400, owner root.
  - Handed to the process through systemd `LoadCredential=`, which gives a per-unit ramfs that other units cannot read.
  - Optionally `LoadCredentialEncrypted=`, created with `systemd-creds encrypt` (host key or TPM).
  - On the AWS fallback, the keeper can decrypt with KMS at boot instead.
  - The hot process copies the key into a `zeroize`-on-drop buffer and calls `mlockall(MCL_CURRENT|MCL_FUTURE)`, so the key never reaches swap.
- **Repository guards.** `.gitignore` gains `config/*.toml` (with `!*.example.toml`), `keys/`, `*.keypair.json` and `secrets/`. `deploy/hooks/pre-commit` rejects any 64-element JSON byte array and any 87-88-char base58 string.
- **Cold wallet.** It is user-held and never on a server. The config holds only its public key, as the sweep destination.

## 6. Hot path

### 6.1 Threads (all pinned; cores isolated with `isolcpus`/`nohz_full` per speed-geography §6d)

| Core | Threads |
|---|---|
| `feed` | Busy-polls feed sockets (or the shred Unix socket), then dedup → decode → match → decide → build the unsigned message. Hands off to sender threads through one `rtrb` SPSC ring each. |
| `senders[]` | Each patches its own tip ix, signs and sends (HTTP/2 or QUIC kept warm with `/ping` every 5 s and pinned IPs). Variants are signed in parallel, not serially. |
| `exit` | Exit engine, post-exec stream for held mints and the own wallet, sell building. |
| `cold` | Tokio runtime for: ledger writer, recorder, metrics, ArmSet watcher, peer heartbeat, watchdog notify, panic alerts. |

"Allocation-free" means: no heap allocation from packet bytes to signed message bytes. The checks:
- A `#[global_allocator]` counting wrapper is enabled in tests.
- `rugsnipe-replay` asserts **zero allocations on the feed thread** after warm-up.

The HTTP and QUIC client stacks allocate on the sender threads. That is accepted and measured.

### 6.2 Create → buy

```
on FeedEvent ev (feed thread):
  if dedup.seen(ev.sig): metrics.dup[ev.src]++ ; return          # first arrival wins, rank recorded
  tx = TxView::parse(ev.bytes)?                                   # legacy | v0 | v1
  for c in decode_creates(tx):                                    # top-level pump ix, or launcher table
    if c.quote != WSOL or c.mayhem or c.token_prog != T22: rec(Skip(class)); continue
    w = watch.hit(c.signer) or watch.hit(c.fee_payer) or watch.hit(c.creator_arg)
    if !w: continue
    op = armset.op(w.op_id)
    d = risk.decide(op, ev.src, regime(ev.slot))    # paper, probe or live; kill latch; caps; feed class
    if d == Skip: rec(Skip(reason)); continue
    v_exp = v_after(c.dev_buy_args) + op.bundle_sol_p75 + op.rival_sol_ahead_p50
    if v_exp > op.v_max: rec(Skip(TooHigh)); continue             # entry EV gate
    if ab_skip(mint, op.ab_skip_p): rec(SkipAB); continue          # randomized control
    spend = op.size ; min_out = tokens_out(spend_net, at V = op.v_max)   # never fill above V_max
    n = nonces.acquire(xxh3(mint) % N)              # deterministic across boxes; probe idx+1.. if busy
    if !n: rec(Skip(NoNonce)); continue
    msg = tmpl.patch(c.mint, c.bonding_curve, c.assoc_bc, ata(us,c.mint,T22), n.value, spend, min_out)
    intent = IntentId(xxh3(mint || wallet))
    for s in senders.healthy_for(leader_class(ev.slot)): s.ring.push((intent, msg, tip_for(s, op)))
    ledger.push(Intent{..., nonce: n, state: sent}); exits.expect(intent, c, op.exit, c.slot)
```

**Why a nonce, not a blockhash.** The template carries `AdvanceNonceAccount(nonce, RecentBlockhashes sysvar, wallet)` as its first instruction, and `recent_blockhash = nonce value`. Every variant (one per sender, each with its own tip ix and signature) advances the same nonce, so at most one variant can ever land, across all senders and all boxes. A nonce can advance only once per blockhash. A nonce therefore serves at most one intent per slot, so the pool needs to be larger than the peak number of same-slot intents.

### 6.3 Nonce lifecycle (the ops-critical part)

**Durable-nonce transactions never expire.** If none of the variants lands, any one of them could still land much later (for example through a sender's auto-retry, which Nozomi does), long after the price moved. So every nonce that is not consumed within a short window is **burned**:

```
state Free(value) --acquire--> InFlight(intent, since_slot)
InFlight:
  observe nonce account change (own-wallet stream / keeper poll):
     if tx with our intent landed      -> fills.upsert(sig); Free(new_value)
     else (someone else? impossible: we are the authority) -> alert + Free(new_value)
  slot > since_slot + nonce_burn_after_slots and no landing seen:
     send AdvanceNonce-only tx (all senders + rpcsend)   -> Burning
Burning:
  nonce changed -> if a buy variant landed instead of the burn: it counts as a fill (fills.upsert);
                   else intent.state = burned; Free(new_value)
  slot > since + 40 and unchanged -> re-send burn; after 3 tries -> alert, nonce quarantined
```

Cost of a burn: 5,000 lamports plus a tiny priority fee, paid only on misses.

**Nonce authority.** The authority is the hot wallet: one signature, no extra key. The keeper creates the pool once (`rugsnipe-ctl nonce init --n 32`), on the singleton box only.

### 6.4 Latency budget (FRA box, FRA/AMS leader)

| Stage | Target p50 / p99 | Status |
|---|---|---|
| Leader execution → first shred out | 1-5 ms | LIKELY |
| Shred delivery in-metro (DZ Edge / Helius raw) | 0.5-3 ms | vendor claim |
| `TxView` + decode + watch lookup + decide | 20 / 80 µs | gated in CI by `rugsnipe-replay` |
| Patch template + ATA PDA + handoff | 10 / 30 µs | gated |
| Sign (one per sender thread, in parallel) | 25 / 40 µs | gated |
| **Feed byte → first variant on the wire** | **≤ 70 / 200 µs** | gated |
| Network to sender or leader in-metro | 0.2-3 ms | + up to 50 ms on Harmonic FBA, BAM rounds or Jito ticks (critique-speed) |
| Observe → at leader | ≈ 5-15 ms (≈ 30-60 ms on batched leaders) | LIKELY |
| gRPC `processed` feed instead of shreds | + 10-80 ms | vendor |
| Free mode (PumpPortal, Ohio) | + 183 ms median vs the slot notification | measured; slot +1 at best, paper only |

## 7. Exit engine and guard

```
Position(mint, tokens, cost, v_fill, peak_v, opened_ns, bundle_wallets, policy, ladder_idx)
inputs per held mint: TradeEvent (v, user, is_buy, sol), TokXfer (src, dst, amount), CompleteEvent,
                      own-wallet balance changes, timer ticks
on event:
  v = curve.vsol_post ; peak_v = max(peak_v, v) ; mult = (v/v_fill)^2        # price ∝ vSOL²
  reason =
    BundleSell     if policy.bundle_wallet_sell and ev.is_sell and ev.user in bundle_wallets ∪ {signer, creator}
    Consolidation  if Σ TokXfer into one owner within 2 slots ≥ consolidation_pct of supply
    BigBuy         if ev.is_buy and ev.sol in policy.big_buy_lamports
    TakeProfit(i)  if mult ≥ tp[ladder_idx].mult
    Trailing       if (v/peak_v)^2 ≤ 1 - trail_bps and peak above fill
    PhantomSL      if age ≥ phantom_sl.after_ms and mult ≤ 1 - phantom_sl.bps
    NoActivity     if now - last_trade_ns ≥ no_activity_ms
    MaxHold        if age ≥ max_hold_ms
  if reason: fire_sell(fraction(reason), reason)
fire_sell:
  venue = PumpSwap sell (canonical pool(0, pool_authority(mint), mint, WSOL)) if completed else sell_v3
  min_out = 0 for dump-type reasons, else model_fill × (1 - 0.3)
  if final: append CloseAccount(ATA) (recovers 1,513,840 lamports)
  lifetime = blockhash if final (100 %) else sell-nonce (small separate pool, burned like buys)
  send via ALL healthy senders + rpcsend; retry every 2 slots, tip ×2 up to cap, max 15 tries
  exits.upsert(sig) on landing; position.tokens from on-chain balance, never from arithmetic
```

**Sell lifetimes.** A **final** (100%) sell uses a blockhash, not a nonce. A duplicate cannot over-sell, because the program rejects a sell larger than the remaining balance, so a re-signed retry with a higher tip costs at most a base fee.

A **partial** ladder sell is different: two re-signed retries could both land and sell twice the intended fraction. Partial sells therefore use a nonce from a separate 8-account sell pool, with the same burn rule as buys.

**Guard.** `rugsnipe-guard` is a separate binary with a separate unit, about 1,500 lines, depending only on rs-types, rs-decode, rs-tx and rs-state.
- Every 500 ms it reads `lease('exits')`. The hot exit thread renews the lease as part of its loop, so the timestamp proves that the loop is alive, not just the process.
- **Takeover.** If the lease is older than `takeover_after_ms` and an open position exists (in `positions`, or a non-zero Token-2022 balance from `getTokenAccountsByOwner`), guard writes `owner='guard', epoch+1` with a compare-and-set.
- **`protect` policy.** Guard polls the bonding curve with `getAccountInfo` once a slot. It sells at once on a vSOL drop of ≥ 30% from peak, on MaxHold, or when `grace_s` passes without hot reacquiring the lease.
- **Handback.** When hot restarts, it bumps the lease epoch after guard releases. Guard checks the epoch before each send, so the two never fight.

`rugsnipe-guard --remote` can run on Ohio. It watches fra1's heartbeat over wg0. This covers whole-box death, but it requires a copy of the hot key on Ohio. It is off by default: the user decides, and the hot balance cap bounds the exposure.

## 8. Watchlist lifecycle

`WatchEntry{key, kind: Creator|Funded|Mother|BundleWallet|Successor, op_id, origin: ArmSet|Dynamic, state, added, expires, remove_if_no_launch, keep, parent}`

```
Pending ──create seen──► Launched ──(kind=Funded and !keep)──► Retired
   │ ttl or remove_if_no_launch elapsed (unless keep)        ▲
   └──────────────────────────────────────────► Expired       │
Mother/Creator(keep) never expire; Launched Creator returns to Pending (serial wallets)

on SolXfer(funder=f, to, lamports, to_was_new):                       # funding-graph trigger
  for r in rules[f]: if (!r.fresh_only or to_was_new) and lamports ∈ r.bands:
     upsert(to, Funded, r.op_id, expires=now+r.ttl_s, rinl=now+r.remove_if_no_launch_s, keep=r.keep)
on SolXfer(from=w ∈ watch, to, lamports) with rule(w).last_transfer
   and lamports ≥ last_transfer_min_frac × balance_before(w):         # "last transfer" mode
  upsert(to, Successor, inherit(w), parent=w) ; if !w.keep: retire(w)
sweep every slot (cold thread): expire past-ttl; persist changes to watch_dyn (ledger ring)
```

Notes:
- The ArmSet entries replace the ArmSet-origin set atomically through `ArcSwap`. Dynamic entries survive ArmSet swaps and restarts, because they live in `watch_dyn`.
- Default TTL is 30 min: fresh exchange-funded creators launched 24-798 s after funding.
- Amount bands are never typed by hand. They come from discovered mothers and operator clusters (`onchain-funding-patterns.md`: 2.955 never occurred, ET69's 0.9 SOL did).

## 9. Feed and sender failover

**Feeds** are not primary/backup. Every enabled feed runs at once and the dedup ring takes the first arrival. Failover is a per-feed **health state**: `Up → Lagging → Stale → Reconnecting → Up`.
- **Lag.** A feed is lagging when its `slot` trails the maximum slot seen across all feeds by more than 2 slots.
- **Stale.** A feed is stale when it has been silent for `stale_ms`.
- **Reconnect** with jittered exponential backoff, 100 ms to 10 s. gRPC resubscribes with `from_slot = last_slot - 32`, so the exit engine sees no gaps.

The **entry class** comes from the best healthy feed:
- `pre-exec` (shreds, deshred, preconf) or `post-exec` (gRPC): normal entry.
- `free-only`: `degraded_entry` applies, which defaults to paper.

The **exit engine** needs at least one post-exec source for held mints. If none is healthy for more than 2 s, it falls back to `getAccountInfo(bonding_curve)` polling every slot over public RPC. That stays within public-RPC limits for ≤ 4 positions.

**Senders** have a circuit breaker each:
- `Closed → Open` after 5 errors or timeouts within 30 s.
- `Open → HalfOpen` after 20 s. One probe send decides whether it returns to `Closed`.
- A sender is also marked **degraded** when its landed share over the last 50 intents falls below 25% of the best sender's share.

Routing rules:
- Buys go to every healthy sender in `healthy_for(leader_class)`.
- Sells always add `rpcsend`, so there is at least one free path even if every paid sender is down.
- If every paid sender is Open, new buys are skipped. Exits continue.

**Program upgrades and IDL drift.** Keeper subscribes to the programdata accounts of pump, PumpSwap and pump-fees (`B5MvUw…`, `6naEzK…`, `75Uu23…`). When their `slot` field changes:
1. Raise the `upgrade` kill: buys go to paper, exits continue.
2. Run `rugsnipe-sim` against a live curve: `buy_v3`, `buy_exact_quote_in_v3` and `sell_v3` simulations, plus a decode of the last 100 pump txs.
3. Resume buys automatically only if every simulation passes and the decode-error rate is 0. Otherwise stay in paper and alert.

FeeConfig changes (account `8Wf5Ti…`) are handled the same way.

## 10. Persistence, idempotency, crash recovery

**What survives where:**

| Fact | Lives in |
|---|---|
| Holdings | Token-2022 balances on chain |
| Nonce values | Chain |
| P&L | Tx meta, keyed by signature |
| Intents and decisions | `state.db` (cache, plus audit) |
| Dynamic watchlist | `state.db` |
| ArmSet | Signed files |

**Write ordering.** The hot thread **does not wait** for the write-ahead log before sending: an fsync on the critical path would cost 0.05-5 ms. Safety comes instead from:
- (a) deterministic intent IDs plus `UNIQUE(mint, mode)`;
- (b) the nonce, which makes duplicate landings impossible;
- (c) recovery from the chain.

The ledger thread commits every 5 ms or 64 records, whichever comes first.

**Recovery algorithm** (`rs-state::Recovery`), run by hot, guard and keeper at startup:

```
1. load config, verify the newest valid ArmSet (else disarmed: exits only)
2. open state.db (WAL replay is SQLite's job); if corrupt -> move aside, start empty, alert
3. chain snapshot:
     bal  = getBalance(wallet); toks = getTokenAccountsByOwner(wallet, programId=Token-2022)
     sigs = getSignaturesForAddress(wallet, until = kv.last_wallet_sig)   # paginate
     for s in sigs: meta = getTransaction(s) -> classify buy/sell/burn/sweep -> upsert fills/exits by sig
     nonces = getMultipleAccounts(pool) (≤10 keys per call on publicnode)
4. positions := { mint : toks[mint] > 0 }
     known intent -> restore policy from positions.policy_json
     unknown      -> origin='orphan', policy=exit.default with max_hold from now, alert
     positions with 0 tokens -> closed
5. nonces: value differs from db -> Free(new); InFlight older than burn window -> burn now
6. dedup watermark: any mint with an intent row is never bought again (UNIQUE)
7. reacquire lease('exits') (CAS, epoch+1), resubscribe held mints, then enable entries
```

Recovery completes before `sd_notify(READY=1)`. The systemd start timeout is 60 s. A public-RPC recovery takes about 1-5 s.

**Reconcile loop.** Keeper runs steps 3-5 every 10 s, and the result is exported as metrics. Any drift between `positions.tokens` and the chain, or between ledger P&L and the wallet-balance delta, raises `rugsnipe_reconcile_drift` and an alert. The ledger is then corrected from the chain, never the other way.

**Upgrades with open positions.** `rugsnipe-ctl drain` sets `entries=off`, waits for positions to close or for `--max-wait`, and then the release switches. Restarting without draining is still safe by design. Drain just avoids a guard takeover.

## 11. Risk caps, modes and kill switch

**Modes.**

| Mode | Behaviour |
|---|---|
| `paper` (default) | Only the `PaperSender`. It records what would be sent, and `landing.py` estimates the position after the fact. |
| `probe` | `max_probe_lamports` and `max_probes_per_day`. Measures real landing. |
| `live` | Requires all of: the TOML mode set to live, the CLI flag, `rugsnipe-ctl arm-live` typed locally on the box (it writes a 24 h token to `kv`), and an ArmSet with `gate_passed=true` whose sha256 the token names. Any one missing means paper. |

**Caps, checked in `RiskGate::decide`.** All are O(1) atomics on the hot thread, with state persisted by the ledger.
- Per snipe, open positions, buys per operator per day, consecutive losses per operator.
- **Daily loss.** The local estimate is realized plus mark-to-market. The keeper computes the authoritative figure every 10 s from the chain: `wallet SOL + Σ position value at current curve − (start_of_day_balance + deposits − sweeps)`.

  Both boxes share the same wallet and compute the same figure independently, so the loss cap is global without coordination.

**Kill latch.** Once set it stays set (`risk.killed`) until `rugsnipe-ctl unkill` runs locally. Its effect: **buys stop, exits continue**. Triggers:
- the kill file exists, or `SIGUSR1`;
- `/kill` from an authorized Telegram chat (the only remote command besides `/status`; there is no remote unkill and no remote live);
- daily loss cap; consecutive-loss cap (per operator, it disarms that operator only);
- an unexplained wallet-balance drop of more than 0.05 SOL with no matching signature;
- send error rate above 20% over 5 min;
- decode-miss rate above 1% (creates seen post-exec but not pre-exec);
- program upgrade or FeeConfig change;
- ArmSet stale; disk above 90%; clock offset above 50 ms;
- reconcile drift that persists for 60 s.

## 12. Multi-region: active box per leader window

`rs-sched` builds a table once per epoch, and every `tpu_refresh_s`. Per leader it records:
- the advertised `tpuQuic` IP and its metro;
- the client class, from DZ `validators-metadata` (`Jito-Agave`, `BAM`, `Harmonic`, …);
- the Jito region (`GetConnectedLeadersRegioned`).

The role of each box per 4-slot window (or per maximal same-leader run, after Alpenglow) is `Primary` if `rtt[box][metro] − margin` is the smallest across live boxes, else `Backup`. A backup sends the same intent, on the same nonce (index `xxh3(mint) % N`, same probe order on every box), with half the tip.

Peers exchange heartbeats over wg0 every 200 ms, carrying `{armset_ver, role_epoch, killed, mode}`.
- A box whose peer has been silent for more than 1 s acts as Primary for all windows. Split-brain only costs duplicate send attempts; it cannot cause a duplicate fill.
- A kill on any box propagates through the heartbeat. Each box also computes the chain-derived loss itself.

Singleton duties (nonce init, sweeps) run only where `singleton_tasks = true`.

## 13. Observability

**Logs.**
- The hot thread never formats text. It writes binary `Rec`s into the ring. The recorder writes hourly `rslog` files and keeps them for 14 days locally; Ohio keeps them forever.
- Cold threads and the other binaries log JSON through `tracing-subscriber` to journald (`SyslogIdentifier=rugsnipe-*`), with intent ID, mint, op_id and sig as fields.

**Metrics** (`prometheus-client`, served on loopback plus wg0 only):

```
rugsnipe_feed_up{feed} rugsnipe_feed_lag_slots{feed} rugsnipe_feed_first_seen_total{feed}
rugsnipe_feed_reconnects_total{feed} rugsnipe_decode_errors_total{kind} rugsnipe_decode_miss_total{launcher}
rugsnipe_watch_entries{kind,state} rugsnipe_watch_hits_total{via} rugsnipe_intents_total{decision}
rugsnipe_stage_latency_us{stage,q}  rugsnipe_sends_total{sender,result} rugsnipe_sender_breaker{sender}
rugsnipe_landed_total{sender,slot_offset} rugsnipe_positions_open rugsnipe_position_age_max_s
rugsnipe_exit_total{reason} rugsnipe_sell_retries_total rugsnipe_nonce_free rugsnipe_nonce_burns_total
rugsnipe_wallet_lamports rugsnipe_daily_pnl_lamports rugsnipe_mode rugsnipe_killed{reason}
rugsnipe_armset_age_s rugsnipe_lease_owner{owner} rugsnipe_reconcile_drift rugsnipe_program_upgrade
rugsnipe_clock_offset_ms rugsnipe_ring_drops_total{ring} rugsnipe_peer_age_ms{peer}
```

**Alerts.** Prometheus and Alertmanager run on Ohio. Alertmanager's native `telegram_configs` and `discord_configs` receivers are LIKELY to work (present since 0.24/0.25).

| Severity | Alerts |
|---|---|
| Critical (immediate) | `killed`; `positions_open>0 and (exit feed down > 10 s or lease_owner=guard)`; sell failing more than 5 tries; hot down 30 s; crash-loop (`OnFailure=`); reconcile drift; program upgrade; wallet below the minimum |
| Warning | ArmSet stale; collector gap > 10 min; breaker open > 10 min; disk > 80%; first-seen share shift > 20 pts |
| Daily digest | Per-operator stats and P&L |

**Dead-man.** An always-firing `Watchdog` alert route goes to a healthchecks.io ping, and the hot box pings it too, so silence from Ohio itself becomes an email.

**Direct path.** Kill and takeover events are also sent straight from the hot box to Telegram, rate-limited, so they arrive even when Ohio is down.

## 14. Deployment (bash, no Ansible)

```
deploy/
  bootstrap.sh     # once per box: user rugsnipe, dirs, sysctl, chrony->ntp.frankfurt.jito.wtf,
                   # grub isolcpus/nohz_full/rcu_nocbs, governor performance, irqbalance off,
                   # NIC irq pinning + ethtool rings/coalesce, THP madvise, swappiness 1, wireguard
  install.sh       # build on box: cargo build --release --locked; install to /opt/rugsnipe/releases/<sha>,
                   # flip /opt/rugsnipe/current symlink; systemctl daemon-reload; ctl drain; restart
  rollback.sh      # flip symlink to previous release, restart
  units/*.service, units/*.timer, sysctl.d/90-rugsnipe.conf, wg0.conf.example
  latency_probe.sh, stake_geo.py, infra_endpoints.py   # existing
```

`units/rugsnipe-hot.service` (essentials):

```ini
[Service]
Type=notify
NotifyAccess=main
WatchdogSec=3
ExecStart=/opt/rugsnipe/current/bin/rugsnipe-hot --config /etc/rugsnipe/rugsnipe.toml
Restart=always
RestartSec=1
StartLimitIntervalSec=120
StartLimitBurst=10
OnFailure=rugsnipe-alert@%n.service
User=rugsnipe
LoadCredential=hot-keypair:/etc/rugsnipe/keys/hot.json
LoadCredential=api-keys:/etc/rugsnipe/secrets/api-keys.env
LimitMEMLOCK=infinity
OOMScoreAdjust=-900
CPUAffinity=0-5
ProtectSystem=strict
ReadWritePaths=/var/lib/rugsnipe /run/rugsnipe
PrivateTmp=yes
NoNewPrivileges=yes
ProtectHome=yes
RuntimeDirectory=rugsnipe
RuntimeDirectoryPreserve=yes
```

The watchdog is fed by a cold thread, and **only if** the feed-thread and exit-thread progress counters both advanced in the last second. A wedged hot loop is therefore restarted, not just a dead process.

Other units:
- `rugsnipe-guard.service`: same hardening, `WatchdogSec=10`, `OOMScoreAdjust=-950`.
- `rugsnipe-keeper.service`: same hardening.
- On Ohio: `rs-collector.service`, plus timers `rs-discover.timer` (6 h), `rs-ingest.timer` (5 min), `rs-backup.timer` (hourly `sqlite3 .backup` → `aws s3 cp` in us-east-2) and `rs-fixtures.timer` (daily fixture refresh).

OS tuning follows speed-geography §6d verbatim. `bootstrap.sh` applies it idempotently and prints a checklist (`cat /sys/devices/system/cpu/isolated`, `chronyc tracking`, `ethtool -c`).

## 15. Alpenglow upgrade path (epoch 1064, Wed Oct 21 ~15:00 UTC)

| When | Action |
|---|---|
| Now (M1) | `rs-wire` parses `BlockComponent` markers. `rs-feed` carries `(slot, bank_id)` on every event. The gRPC adapter sets `include_update_parent=true` and handles `SLOT_DEAD` and `EntryUpdateParent` (verified present in proto 14.0.1). |
| Oct 18-28 | `tpu_refresh_s=60`. Re-read `getClusterNodes`, because BAM goes into maintenance and about 15.7% of stake (the Firedancer family) migrates to Agave builds. |
| Slots 459,640,000 → +~12 h after 459,653,000 | `regime_freeze` forces buys to paper. Exits are unaffected. |
| Cutover | On `UpdateParent(cleared_bank_id)`: cancel not-yet-sent intents from that bank. Sent ones are handled by the nonce burn. Positions count only after `confirmed`, which equals finalized at ~150 ms. |
| +24 h | Capture post-Alpenglow fixtures (marker-bearing blocks), add them to CI, re-run the decode-miss audit, re-measure leader-run length and landing, and tag the landing model `regime='alpenglow-1'`. |
| Re-enable gate | 6 h with decode-miss below 1%, zero marker parse errors, sim smoke test passing. Then `ctl unfreeze`. |

## 16. Discovery and validation (enough to build; statistics follow proposal 2)

**Operator keys,** point-in-time, computed only from data available before each create:
- creator/signer;
- funder plus amount band (±0.0005 SOL);
- mother (≤ 3 hops, `funding.trace_funding`);
- bundle wallets: adjacent tx index or a shared Jito tip in slot 0;
- launcher program;
- metadata host;
- fee fingerprint (CU limit, tip size, ix variant).

Operators are formed by union-find over typed edges with minimum weights. An operator's ID never changes retroactively.

**Entry models.**
- **E1** (default): fill at `V = V_after(dev buy + operator bundle) + the median third-party slot-0 rival SOL`.
- **E2** (pessimistic): end-of-slot-0 V.

The backtest applies the same `rs-exit` rules (the Python mirror is conformance-tested on fixture tapes), and the full cost model:
- 1.25% per side;
- tip and priority from the observed winner p50/p75;
- failed-attempt fees, burn fees and ATA rent;
- sell slippage against the realized tape.

**Scoring.** Walk-forward: discover on days 1..t, test on t+1..t+7, with purge and embargo. Report the Wilson 95% lower bound of the hit rate and a bootstrap lower bound of EV per snipe. An operator is armed only if:
- EV_lb > 0, and
- n_oos ≥ 30 for **probe** sizing, and the **pooled policy** passes on ≥ 170 OOS snipes for **live**.

These numbers come from skeptic §3.11 and proposal 2 §0.

**Anti-farm controls:**
- per-operator `v_max` from its bundle p90;
- `ab_skip_p` randomized skips to measure our own effect;
- size jitter of ±15%;
- consecutive-loss disarm;
- a farm flag when slot-0/1 sells after our fill exceed the matched-skip rate (proposal 2 `abtest.py`).

## 17. Testing strategy

1. **Fixtures (`fixtures/`, M0).** `analysis/rugsnipe_analysis/fixtures.py` uses `rpc.py` to pull about 200 txs, each as base64 wire bytes plus the RPC JSON, with a manifest giving the sha256 and the expected decoded fields from the **independent Python decoder** (`events.py`, `dataset.creation_record`). Coverage:
   - legacy, v0 and v1 `create_v2`;
   - legacy SPL `create` (0.7% of creates);
   - launcher creates;
   - `buy_v3`, `buy_exact_quote_in_v3`, `sell_v3`, legacy 18-account `buy`, `buy_v2`;
   - PumpSwap buy/sell;
   - Token-2022 `TransferChecked`;
   - a Binance `5tzFki…` withdrawal;
   - the decoded competitor nonce snipe.

   Rust tests must agree field for field with the manifest, so two implementations check each other.
2. **Golden math.** proptest runs 10k random `(V, T, X)` states. Rust must equal vectors generated by `curve.py` exactly, to the lamport.
3. **Execution tests (`rs-it`, LiteSVM 0.18).**
   - Dump the pump, PumpSwap and pump-fees program data and Global/FeeConfig/curve accounts at a pinned slot. pump programdata is 10 MiB (space).
   - Execute our real `create_v2` (for a synthetic mint) → `buy_v3` (nonce variant) → `sell_v3` + CloseAccount → burn.
   - Assert CU, LADS, balances, the nonce advance, the exclusivity of two variants on one nonce, and that a sell larger than the balance fails.
   - Whether LiteSVM accepts v1 is itself tested: LIKELY, since it is on the 4.3 line.
4. **`rugsnipe-sim`.** `simulateTransaction` against live mainnet state with `sigVerify=false` and `replaceRecentBlockhash=true` (public RPC). This is the M4 gate for v1, `partial_fill`, the wSOL question, and the CU p99.
5. **Replay harness.** `rugsnipe-replay` drives the **same pipeline** from rslog captures or fixture-built feed tapes with a virtual clock, into `PaperSender` and `SvmChain`. Decision outputs are snapshot-tested with `insta`. It enforces the latency and zero-alloc gates.
6. **Chaos (`rugsnipe-chaos`, nightly).** hot, guard and keeper run as subprocesses against `SvmChain`, with `chaosproxy` in front of the ws/HTTP feeds and senders.

   **Faults injected:**
   - SIGKILL at random instructions, including mid-ledger-commit and between send and record;
   - feed disconnects, stalls and duplicate or reordered events;
   - sender 5xx, timeouts and black-holing;
   - RPC 429 storms;
   - `StateStore` write failures and disk full;
   - a clock jump of ±2 s;
   - a corrupt or stale ArmSet;
   - an `UpdateParent` that clears the bank of a create we already bought;
   - a program-upgrade event.

   **Invariants checked after every run:**
   - **I1:** no non-paper send while in paper mode or killed.
   - **I2:** every non-zero token balance has a live exit owner within 2 s (hot) or `takeover_after_ms + 1 s` (guard).
   - **I3:** at most one landed buy per intent.
   - **I4:** ledger P&L equals the Σ SOL delta by signature on the SvmChain (drift = 0).
   - **I5:** no nonce stays InFlight past the burn window plus 40 slots.
   - **I6:** after recovery, positions equal the set of non-zero token accounts.

## 18. Failure modes and mitigations

| Failure | Detection | Mitigation |
|---|---|---|
| hot crashes with positions open | systemd, lease age | restart in ~1 s plus recovery; guard takeover after 5 s; alert |
| hot loop wedged (alive but stuck) | watchdog progress counters | WatchdogSec=3 restart |
| whole FRA box dies | peer heartbeat, Prometheus `up` | ams1 takes all windows; optional remote guard on Ohio sells; alert |
| A late landing of an old nonce tx | nonce state machine | burn after 3 slots |
| Double buy (two boxes, restart) | `UNIQUE(mint, mode)`, nonce | impossible by construction; I3 tested |
| Ledger lost or corrupt | SQLite integrity check at start | rebuild from chain (§10), alert |
| Feed silently lagging | cross-feed slot lag | Lagging state; entry class downgraded |
| All pre-exec feeds down | health | `degraded_entry=paper`; exits poll RPC |
| Sender dead or rejecting v1/nonce | breaker, landed share | route around it; legacy default; per-sender A/B in probe |
| Pump program upgrade or IDL change | programdata slot watcher | buys to paper, sim smoke test, auto-resume only if it passes |
| Alpenglow bank cleared | `UpdateParent`, `bank_id` | cancel or burn; regime freeze |
| BAM maintenance or TPU changes | `tpu_refresh_s=60` | routing table re-read |
| Wallet drained (key leak) | unexplained balance drop | kill; hot balance cap; daily sweep to cold |
| Clock drift | chrony offset metric | kill above 50 ms (it breaks cross-box logs) |
| Public RPC throttling (recovery, free mode) | 429 counters | rotate mainnet-beta and publicnode; cache; ≤ 4 positions in free mode |
| Collector gap (data is perishable) | `ingest_gaps`, alert after 10 min | two free sources; systemd restart; gaps excluded from folds |
| Disk full | node_exporter | rslog rotation; kill above 90% (ledger cannot persist) |
| Operator farms us | slot-0/1 sells after our fill vs skip arm | per-operator disarm; randomized skips |

## 19. Build plan (most valuable testable artifact first)

| # | Milestone | Testable artifact (public data only) | Effort |
|---|---|---|---|
| M0 | `collector.py` + `store.py` running on Ohio **today**, plus `fixtures.py` | `analysis.db` filling with every create and fresh-creator funding; `ingest_gaps` empty for 24 h; `fixtures/` with ~200 manifests | 1 d |
| M1 | finish `pump-codec` (= rs-types/rs-wire/rs-decode): Alpenglow markers, launcher table, transfers | `cargo test` passes on all fixtures; curve proptest equals Python; marker unit tests built from the Agave 4.3 `block_component.rs` layout | 2 d |
| M2 | `operators.py`, `outcome.py`, `walkforward.py` on D1, D2 and M0 data | the first out-of-sample report (EV by `V_e` band, Wilson/bootstrap bounds); the go/no-go document | 2 d |
| M3 | `rs-state` (state.db, recovery), `rs-obs`, `rs-feed` (free feeds), `rs-watch`, `rugsnipe-hot --mode paper` and `rugsnipe-replay` on Ohio | a 24 h shadow run; decode-miss rate; latency and zero-alloc gates; recovery tests with SIGKILL | 3 d |
| M4 | `rs-tx` (legacy + nonce + v1), `rugsnipe-sim`, `rs-it` (LiteSVM) | simulated CU/LADS p99; v1 and nonce legality; nonce exclusivity and burn in LiteSVM; the wSOL and `partial_fill` answers | 2 d |
| M5 | `rs-exit`, `rugsnipe-guard`, `rugsnipe-keeper`, `rugsnipe-chaos` | Rust exit decisions equal Python on 500 tapes; chaos invariants I1-I6 green nightly | 3 d |
| M6 | `deploy/` (bootstrap, install, rollback, units), Prometheus/Alertmanager/alertd | a fresh VM goes from bootstrap to paper-running in one command; a test alert reaches Telegram | 1.5 d |
| M7 | Alpenglow readiness, **before Oct 21** | the freeze window configured; the gRPC UpdateParent path tested with synthetic events; post-cutover fixture capture scripted | 1 d (parallel to M3) |
| M8 | FRA box (user action), paid feed and sender trials behind features, **probe mode** (≤ 0.5 SOL total, opt-in) | true landing position per sender and leader class; real costs; geyserbench results | user + 2 d |
| M9 | ams1, `rs-sched` roles, shared nonce pool | dual-box probes with zero duplicate fills; role logs | 2 d |
| Gate | Live sizing only after the pooled walk-forward gate passes using probe-measured costs and landing | — | — |

## 20. What cannot be validated without paid infrastructure or live money

1. **Pre-execution feed latency and coverage** (DZ Edge, Helius raw or preconf, Triton Deshred, SVS). DZ publishing covers only ~53% of stake (critique-speed). Paid trials plus geyserbench from FRA are needed. Without them, all "block 0" numbers are models.
2. **Real landing position, the slot-0 rate and rival response** to our presence (farming). These need probe buys. Paper mode estimates position only to within ±20-40 ms.
3. **Sender behaviour:** acceptance of v1 and durable-nonce txs, nonce deprioritization (Nozomi's warning), the `swqos_only` path, and Harmonic FBA or BAM proxies for unstaked senders. This needs live sends. Simulation proves legality only.
4. **Exit fills inside contested dump slots,** and the completion → migrate gap for a held position.
5. **Post-Alpenglow semantics:** cleared-bank re-execution, and leader-run length under Votor. This can only be observed after Oct 21. Every pre-cutover landing number must be re-measured.
6. **Bare-metal tuning effects** (isolcpus, busy-poll, in-building RTT < 0.5 ms to `64.130.50.88`/`.91`, `198.13.140.105`/`.227`). This needs the FRA box. Nothing measured from Ohio or this container transfers.
7. **Long-horizon operator persistence** older than ~1.5-2 days. Public RPC does not serve that history. It accumulates only as long as M0 keeps running, which is why M0 ships first.

Until 1-3 are measured, every EV figure the system reports is an **upper bound** under entry model E1. The daily report and every ArmSet must carry that label.
