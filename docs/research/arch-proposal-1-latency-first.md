# Architecture proposal 1: latency first ("packet in, transaction out")

Date: 2026-10-14. Author angle: latency purist. The design starts at the NIC and ends at the leader's ingress. Everything else (discovery, validation, exits, operations) is designed to feed the hot path with a small, pre-computed decision table, and to keep live trading switched off until the edge is proven.

Tags follow the research docs: **VERIFIED** (primary source or our own measurement), **LIKELY**, **UNVERIFIED**. **MEASURED HERE** means I ran it in this session; the code is in the session scratchpad and is not committed.

## 0. Position in one paragraph

Two facts decide the shape of the system. First, the strategy has **no proven edge yet**. The guru's headline rule is negative EV, break-even is 43-48% hits at TP +100%, and being 2nd or 3rd sniper raises it further (skeptic-factcheck §2). So the first deliverable is a validation pipeline, not a sniper. Second, the latency-critical path is **tiny**: decode one transaction, look up one key, patch roughly 200 bytes, compute one signature, write one socket. On this VM that costs about 50 µs. Every millisecond that matters is spent in geography, the feed and the leader's batching, not in our code. So the design is:

- a single-threaded, run-to-completion, allocation-free hot loop in Rust;
- fed by whichever feed is available: raw shreds, Yellowstone, or a free websocket;
- governed by a decision table exported from Python;
- defaulting to paper mode;
- with every action recorded, so that paper, shadow and live runs all produce the same measurements.

## 1. What I verified in this session (on top of the lead's ground truth)

| Item | Result |
|---|---|
| SIMD-0385 v1 wire format | Version byte 129. Legacy header, then `TransactionConfigMask u32`, a 32-byte lifetime, `u8` instruction count and `u8` address count, the addresses, the config values, the instruction headers `(u8 program, u8 n_accounts, u16 data_len)`, the payloads, and **signatures last**. No ALTs. Max size 4096 bytes. Priority fee is **total lamports**. An **unset CU limit means 0**. **VERIFIED** (spec fetched). |
| Real v1 create tx bytes | 4 of 8 recent creates fetched as raw base64 were v1. They start `81 02 00 0f`, are **1,521-1,893 bytes long (above the old 1,232 limit)**, and carry CU limit 500k/600k, a total priority fee of 5,000 lamports, and a loaded-accounts limit of 64 MiB. My hand decode matched the spec byte for byte (two 64-byte signatures at the tail). **MEASURED HERE**. |
| `agave-transaction-view` | 7.0.0 requires **rustc 1.98.1** and the solana-message/transaction **5.x** line, so it is incompatible with our stack. **6.1.0** already parses v1 (`InstructionsFrame::V1`, `transaction_config()`), sits on the 4.x line and has MSRV 1.89. **VERIFIED** (crate source). |
| Dependency set | `agave-transaction-view =6.1.0`, `solana-message 4.6`, `solana-transaction 4.3`, `solana-keypair 3.1.2`, `solana-nonce 3.4`, `pump-rust-client =0.4.1` (no default features), `yellowstone-grpc-client 15.0.0`, `yellowstone-grpc-proto 14.0.1`, `tonic 0.14.6`, `quinn 0.11.12`, `rtrb 0.4`, `core_affinity 0.8`. This set resolves (455 packages) and **builds in release on rustc 1.97.0** in 2m12s. `solana-rpc-client` 4.3.1 is excluded because it needs 1.97.1; it is used only by the cold tool. **MEASURED HERE**. |
| Zero-copy decode speed | `TransactionView::try_new_unsanitized` plus a scan for a top-level `create_v2` on 8 real fixtures took **38-130 ns per transaction**. The signers (mint #0, user #5) were always static keys, including in v0 transactions that used an ALT. **MEASURED HERE** (2.1 GHz Xeon VM). |
| Launcher CPI | 1 of 8 creates had no top-level `create_v2`. It was a 47-account call to the unlabelled program `6Vo3245…`, which does the create by CPI. A shred-level decoder therefore needs per-launcher layouts. **MEASURED HERE**. |
| Signing and PDA cost | ed25519 signature over 760 bytes: **21.7 µs**. Two `find_program_address` calls (bonding curve + Token-2022 ATA): **19.9 µs**. **MEASURED HERE** (same VM; expect about 2x faster on a 5 GHz Ryzen, LIKELY). |
| Yellowstone payload | `SubscribeUpdateTransactionInfo` carries a protobuf `Message` (`account_keys` as repeated bytes, an optional v1 `config = 7`, and `index = 5` for the in-block position), not wire bytes. A zero-copy gRPC path therefore needs a raw codec (§3.2). **VERIFIED** (proto 14.0.1 source). |
| `pump-rust-client` v3 | `V3Trade::accounts()` derives `bonding_curve`, `associated_base_bonding_curve`, `associated_quote_bonding_curve = ATA(bc, SPL-Token, WSOL)` and `associated_base_user = ATA(user, Token-2022, mint)`. `buy_exact_quote_in_v3` takes `(spendable_quote_in, min_tokens_out, partial_fill=true)`. **VERIFIED** (source). |
| Leader classes | Taken from critique-speed §3.1: about 63% of stake is batch-sequenced (BAM 37.9%, Harmonic FBA 25.1% with 50 ms windows ordered by priority fee), and 68% of stake advertises a proxy TPU. I adopt this, not the single-scheduler model. |

## 2. Component diagram

```
                       FRA box (hot)                                 Ohio box (cold, existing EC2)
 ┌──────────────────────────────────────────────────────────┐   ┌──────────────────────────────────┐
 │ NIC ─AF_XDP/recvmmsg─► [HOT-A core2] shred RX            │   │ crawler (Python): creates,        │
 │                           │ streaming deshred            │   │  getBlock s..s+3, funding, trades │
 │ gRPC (Yellowstone) ─────► [HOT-B core3] raw codec        │   │        │                          │
 │ PumpPortal / RPC ws ────►  (free mode, cold runtime)     │   │        ▼                          │
 │                           ▼                              │   │ SQLite rugsnipe.db                │
 │              rs-decode: TxView(legacy|v0|v1) → CreateHit │   │        │                          │
 │                           │  Watchlist (ArcSwap)◄────────┼───┼── export: watchlist.json,         │
 │                           │  DecisionTable (ArcSwap)◄────┼───┼──   operators.json, bids.json,    │
 │                           ▼                              │   │   routing.json (epochly)          │
 │                rs-risk gate (mode, caps, kill)           │   │        ▲                          │
 │                           ▼                              │   │ walk-forward scoring, backtest,   │
 │          rs-tx: patch template → sign v0 ──► socket write├─► │ bid model, paper eval, reports    │
 │                           │ SPSC                         │   │        ▲                          │
 │               [FANOUT core4] sign v1..vK ──► sockets     │   └────────┼─────────────────────────┘
 │ [IO core7] responses, reconnect, /ping warmers           │            │ events-YYYYMMDD.jsonl.zst
 │ [STATE core5] slot, blockhash, nonce values, balances    │            │ (rsync every 60 s)
 │ [EXIT core6] positions, trade stream, sell templates     │            │
 │ [REC core8] SPSC ← all threads → JSONL + Prometheus ─────┼────────────┘
 │ heartbeat UDP mesh ◄──────► AMS / NYC / TYO boxes (same binary, same tables)
 └──────────────────────────────────────────────────────────┘
 senders: Helius /fast, Nozomi fra2, Astralane fr, Jito BE (region of leader), NextBlock QUIC,
          direct QUIC to advertised tpuQuic (BAM node / Harmonic Remote TPU / validator)
```

## 3. The hot path

### 3.1 Threading model

**Run to completion.** The thread that reads a packet also decodes it, decides, signs the primary variant and writes it to the socket. Cross-thread hand-offs are used only for work that is *not* on the critical path: secondary variants, recording and response reading. Each hot thread is pinned (`core_affinity`) to an isolated core (`isolcpus nohz_full rcu_nocbs`, SMT off, governor `performance`, C-states ≤ C1), busy-polls, and never blocks or allocates after warm-up.

| Thread | Core (16-core FRA box) | 4-vCPU fallback | Work |
|---|---|---|---|
| HOT-A | 2 | 1 | Shred RX (`recvmmsg` busy-poll; AF_XDP via `xdp` 0.8 later), streaming deshred, decode, decide, sign v0, write |
| HOT-B | 3 | 1 (merged) | Yellowstone stream on a `current_thread` tokio runtime pinned to the core; same decide/sign/write code |
| FANOUT | 4 | 1 (merged) | Pops `FireJob` from an `rtrb` SPSC ring, signs variants 1..K, writes them |
| STATE | 5 | 2 | Yellowstone account and slot subscriptions: nonce values, blockhash, wallet balance, slot clock, leader of the next 8 slots. Publishes a `SeqLock<HotState>` |
| EXIT | 6 | 2 | Position book, trade and transfer stream for open mints, sell triggers (run to completion) |
| IO | 7 | 2 | Reads HTTP/QUIC responses, reconnects, `/ping` warmers every 5 s, and the RTT table |
| REC | 8 | 3 | Drains SPSC rings from all threads into JSONL plus HDR histograms; Prometheus exporter |
| cold | 0-1 | 0 | tokio multi-thread runtime: config reload, table reload, heartbeat mesh, RPC, kill switch |

**Dedupe between feeds.** A `FiredSet` is an open-addressed `[AtomicU64; 8192]` keyed by the first 8 bytes of the mint. The first thread to win the `compare_exchange` fires, and the other records a "late duplicate" together with its delay. That gives feed-versus-feed latency measurements for free.

**Allocation-free proof.** A `#[global_allocator]` counting wrapper is compiled into tests. The replay benchmark (§10, M2) asserts **0 allocations** between `on_packet()` and `socket.write()` once warm.

### 3.2 Ingest adapters (all produce `&[u8]` wire-format transactions or a `CreateHit`)

1. **Raw shreds** (DZ Edge, Helius shred delivery, SVS, Everstake). `rs-shred` parses merkle data shreds itself: common header, then data header (`parent_offset`, `flags`, `size`), then payload. It does **not** pull in `solana-ledger` (too heavy, and it pins Agave). It keeps a per-`(slot, fec_set_index)` reassembly buffer. **Streaming deshred** walks the batch payload as contiguous data shreds arrive: `u64 entry_count`, then per entry `num_hashes u64, hash [32], u64 tx_count`, then transactions. A transaction is handed to the decoder as soon as its bytes are complete. For v1 the address block starts at byte offset 42 (1 + 3 + 4 + 32 + 1 + 1), so the create classifier can run before the trailing signatures arrive (mint is #0 and the user is a static signer). Reed-Solomon recovery (`reed-solomon-erasure 6.0`) runs only when a gap blocks the walk. **Alpenglow** (epoch 1064, Oct 21): `entry_count == 0` introduces `u16 marker_version` plus a marker (`BlockHeaderV1`, `UpdateParentV1`, `BlockFooterV1`). We parse markers explicitly, track `(slot, parent_block_id)`, and on `UpdateParent` mark hits from the abandoned bank as `Orphaned`. Those are not fired, or are left to their nonce burn (§3.6). Shred signatures are not verified on the hot path; the feed is trusted and its source IP is pinned.
2. **Yellowstone gRPC** (`processed`, filter `account_include=[pump program, mint-authority, launchers…]`, `vote=false, failed=true`). A custom `tonic::codec::Codec` returns the raw frame as `bytes::Bytes`. A small protobuf walker (`rs-feed::pbwalk`: varint, tag and length only) jumps to `transaction.message.account_keys` (field 2), `instructions` (field 4) and `config` (field 7) without `prost` allocations. The prost-decoded path stays as the reference implementation, and a test checks that both decoders agree on recorded frames. `SubscribeDeshred` (Triton) uses the same walker.
3. **Helius `preconfSubscribe`**. We use only the 18-byte frame header layout (`ver u8 | slot u64 | idx u64 | status u8`) and decode the body with `agave-transaction-view`. Never bincode, which fails on v1 (critique-speed C10).
4. **Free feeds**: PumpPortal `subscribeNewToken` and public RPC `logsSubscribe(mentions mint-authority, processed)`. These produce a `CreateHit` with no in-slot information. They are used for paper trading and dead-man fallback only (§3.9).

### 3.3 Decode and classify (`rs-decode`)

```
fn on_tx(bytes: &[u8], src: FeedId, t_rx: u64) -> Option<CreateHit>
  v = TransactionView::try_new_unsanitized(bytes)?            // legacy | v0 | v1, 40-130 ns
  keys = v.static_account_keys()
  if !keys.contains_any(PUMP_PROGRAM, MINT_AUTHORITY) && !LAUNCHERS.matches(keys): return None
  for ix in v.instructions_iter():
     match (keys[ix.program], disc8(ix.data)):
       (PUMP, CREATE_V2)  => layout = CreateV2 { mint:0, bc:2, abc:3, user:5, tail:16.. }
       (PUMP, CREATE)     => layout = CreateLegacy { mint:0, bc:2, abc:3, user:7 }   // 0.7% SPL-Token mints
       (L, d) if LAUNCHERS[L].disc == d => layout = LAUNCHERS[L].layout          // learned offline
       _ => continue
     if ix.account_index(layout.mint) >= keys.len(): drop (ALT-resolved: impossible for signers)
     args = parse_create_args(ix.data)        // name/symbol/uri skipped by length; creator, is_mayhem
     quote = if ix.accounts.len() > 16 { Other(keys[ix.accounts[16]]) } else { Sol }
     dev = scan same tx for pump buy*/buy_exact*_in (v2/v3/legacy) → (lamports_in or tokens_out)
     aqbc_hint = account 7 of a dev buy_v3, if present (saves one PDA derivation)
     return CreateHit { mint, bc, abc, user, creator_arg, fee_payer: keys[0], quote, is_mayhem,
                        dev, aqbc_hint, version: v.version(), slot, entry_idx, tx_idx, src, t_rx }
```

- The `LAUNCHERS` table (`launchers.json`) maps program id → (discriminator, account index of the mint, user index, dev-buy arg offset). It is learned offline by `analysis/rugsnipe_analysis/launchers.py`, which joins `getTransaction` inner instructions (CPI `create_v2`) against outer account positions. Unknown launchers that touch `MINT_AUTHORITY` are recorded as `UnknownLauncher` misses and alerted on.
- Non-SOL quotes, mayhem coins and legacy creates are classified and **skipped by default**. Mayhem coins use different fee recipients and see agent flow. Non-SOL quotes carry fees of up to 3.95% per side.
- **Bundle observation.** In the shred path, the transactions of the same and next entries are scanned for pump buys on `hit.mint` (Jito bundles execute back to back; LIKELY they share an entry). This yields an `ObservedBundle { n_tx, lamports_in_est, signers }` within the same loop iteration, before we fire. In the gRPC path the bundle usually arrives as separate updates; we wait at most `bundle_wait_us` (default 0, i.e. fire on create).

### 3.4 Watchlist match

`Watchlist` is an immutable `ahash` map `[u8;32] → WatchEntry`, swapped atomically through `arc-swap` 1.9 by the cold runtime. A hit is checked against `fee_payer`, `user` and `creator_arg`, because 18.9% of creates have creator ≠ signer (critique C5). Entries carry an `expires_slot`, so TTL is enforced on the hot path without mutation. A miss costs about 20 ns. **Every** create (hit or miss) is recorded with its timestamps, so the paper evaluator can score the population as well as the watchlist.

### 3.5 Pre-signed `buy_exact_quote_in_v3` template

**Format.** Templates exist in two formats.

- **Legacy, the default.** This is the format of the decoded competitor snipe (durable nonce + compute budget + v3 buy + tip), so it is proven to land.
- **v1, behind a flag.** It is preferred once milestone M3 proves `v1 + durable nonce` per path (critique gap G4). It needs config bits 0-3 set: total priority lamports, CU limit, and a tight loaded-accounts size.

**Instructions:**

1. `AdvanceNonceAccount` (must be first).
2. `SetComputeUnitLimit` and `SetComputeUnitPrice` (legacy only).
3. `CreateIdempotent` for the Token-2022 ATA.
4. `buy_exact_quote_in_v3` (17 accounts).
5. Tip `SystemProgram::Transfer`.

There is one signer: the trading wallet is fee payer, nonce authority and buyer. The size is about 900 bytes, below 1,232.

**Key layout.** Message keys are laid out so that **every mint-dependent key sits at a fixed offset** inside the "writable non-signer" class. Legacy messages impose no order within a class, so instruction account indexes never change. At start-up the template is built once, through `pump-rust-client` (`PumpSdk::buy_exact_quote_in_v3_instruction`) and `solana-message`, for a dummy mint. The builder then records a `PatchTable` and a unit test checks that byte-patching reproduces the SDK's message for 1,000 random mints.

| Patched at… | Field | Bytes | Source |
|---|---|---|---|
| nonce change (STATE thread, before any create) | lifetime / blockhash = nonce value | 32 | nonce account data via gRPC |
| each slot (STATE) | CU price (legacy) or total priority fee (v1); tip lamports | 8 + 8 | bid table × leader class of the slot (§3.7) |
| variant build (startup) | tip account key | 32 | one per sender (never in an ALT) |
| **detection** | `base_mint` | 32 | create #0 |
| **detection** | `bonding_curve`, `associated_base_bonding_curve` | 64 | create #2, #3 (no derivation) |
| **detection** | `associated_quote_bonding_curve` | 32 | dev-buy hint, otherwise `ATA(bc, SPL, WSOL)` (~10 µs) |
| **detection** | `associated_base_user` | 32 | `ATA(user, T22, mint)` (~10 µs) |
| **detection** | `spendable_quote_in`, `min_tokens_out` | 16 | operator preset + curve math (§3.7) |
| sign | signature | 64 | ed25519 over message (~11-22 µs) |

**Fixed-length framing.** The base64 length depends only on the transaction length, which is fixed per template. Each sender's HTTP/1.1 request (`POST /fast`, `Content-Length` precomputed, JSON body with `"encoding":"base64","skipPreflight":true,"maxRetries":0`) is therefore a **fixed buffer with one patch region**. Encoding takes about 1 µs (`base64` 0.23). Sockets are pre-connected TCP with `TCP_NODELAY`, kept warm with `/ping` (Helius) or idle-timeout probes. QUIC variants (NextBlock :11100, direct TPU) use `quinn` 0.11 with pre-opened connections to the advertised `tpuQuic` of the current and next 2 leaders.

**One-time setup per wallet.** Pre-create the `user_volume_accumulator` (0.00184 SOL) and the nonce pool, so the hot path never pays rent or extra CU for them.

### 3.6 Durable-nonce pool

Durable-nonce transactions **never expire**. That gives us cross-variant and cross-region exclusivity, and also the risk that a stale variant lands seconds later. The pool manages both.

```
struct NonceSlot { pubkey, value: [u8;32], state: Ready{since_slot} | InFlight{mint8, fired_slot} | Unknown }
pool: [NonceSlot; 256] per trading wallet (rent ≈ 0.0015 SOL each, refundable)

select(mint):                                  // identical on every box: depends only on mint + chain state
  h = xxh3(mint) ; for i in 0..8: s = pool[(h+i) % N]
     if s.state == Ready && slot_now - s.since_slot >= 2: return s     // ≥2 slots: all boxes agree
  return None → skip + record NoNonce

on fire: s.state = InFlight{mint8, slot}
STATE thread, on nonce account update: s.value = new; s.state = Ready{slot}   // landed (success or fail) → advanced
burn task (cold, every slot): for s in InFlight where slot_now > fired_slot + land_deadline_slots (default 3):
   send AdvanceNonce-only tx (5,000 lamports + small priority) via 2 senders → kills every outstanding variant
   alert if a variant of s lands after the deadline (the slippage bound still caps the damage)
```

The pool is refilled from chain state, so nothing is persisted locally. A box that restarts marks every slot `Unknown` until it has read the nonce accounts again.

### 3.7 Fee, tip and slippage model ("hardcore mode" as a forecast)

**Leader class.** `routing.json` (rebuilt every epoch and, during the BAM maintenance and Alpenglow cutover, every 5 minutes) maps each leader identity to:

- its class: `JitoAgave | Bam | BamMaintenance | HarmonicFba | HarmonicFifo | Rakurai | Agave`, from DZ `validators-metadata`;
- its advertised `tpuQuic` IP and metro, from `getClusterNodes`;
- its Jito block-engine region (`GetConnectedLeadersRegioned`);
- whether it publishes to DZ.

**Bid table.** `bids.json` is built in Python from our own `getBlock` crawl of slots s..s+3 around every create. For each `(class, operator_tier)` it holds:

- quantiles of the **minimum bid that landed ahead of all slippage-failed reactive buys**;
- the expected number of rival reactive buyers `N_hat`;
- their median size.

```
on CreateHit h matched to operator o, target slot ts (= h.slot, or h.slot+1 if the create was late in a
window-final slot), leader L = leader(ts), class c = routing[L].class:
  // 1. entry gate (anti-farm): curve after dev buy + observed bundle
  V = 30e9 + dev_lamports_net + observed_bundle_net ; T = k / V
  if V > o.max_entry_vsol: skip(BundleTooBig)            // e.g. 70 SOL → floor loss 81.6%
  // 2. expected rivals ahead of us
  V_exp = V + N_hat[c][o.tier] * rival_size_med
  min_tokens_out = floor( tokens_out(o.size_net, V_exp, k/V_exp) * (1 - o.slip_bps/1e4) )
  // 3. bid: forecast quantile, capped by EV and by a hard cap
  q = o.bid_quantile (default 0.75; raised by +0.05 after each loss-on-slippage, decays daily)
  B = min( Q_q(D[c][o.tier]) * 1.1, o.ev_cap_lamports, cfg.hard_cap_lamports /*0.025 SOL*/ )
  // 4. split by class: what the sequencer actually ranks
  JitoAgave:    prio = 0.7B over CU, tip = 0.3B (Jito guidance: 70/30 priority/tip for sendTransaction)
  HarmonicFba:  prio = B, tip = sender minimum      // Harmonic ranks priority fee; tips ≠ bid
  Bam/BamMaint: prio = 0.5B, tip = 0.5B              // score formula UNVERIFIED → learn online
  // tip reverts on failure; priority fee does not. Failures are slippage losses (46% of reactive buys)
```

The CU limit comes from simulation, not folklore. M3 runs `simulateTransaction` (`sigVerify=false`, `replaceRecentBlockhash=true`, a funded mainnet address as payer) on the exact template against live curves, and sets `cu_limit = p99 × 1.05` and the loaded-accounts size from the reported loaded bytes. The guessed 62-65k limit is **not** used without that measurement, because pump's docs recommend about 100k.

### 3.8 Leader-window scheduler across regions

Every box runs the same binary with the same `routing.json` and `boxes.toml`, which holds the box→metro RTT matrix measured by `deploy/latency_probe.sh`. Ownership is computed independently on each box, and the results agree:

```
owner(ts) = argmin_b ( rtt[b][routing[leader(ts)].ingress_metro] + class_penalty[b] )
backup(ts) = second best
fire if  me == owner(ts)
     or  (me == backup(ts) && heartbeat[owner].age_ms > 150)       // UDP mesh, 50 ms period
     or  cfg.redundant_fire                                       // safe: same nonce → at most one lands
```

Because every box picks the nonce deterministically from the mint, **redundant firing is cheap**: the losing variants fail the nonce check before execution and pay nothing (LIKELY, standard nonce semantics; verify in M8). The owner rule exists to respect sender rate limits (Jito 1 req/s/IP/region) and to keep fee accounting clean, not for safety. The site order follows speed-geography and critique-speed: **FRA (TeraSwitch) first, then AMS, then TYO or NYC**. Ohio never fires.

### 3.9 Degradation ladder

| Mode | Feed | Box | What is valid | Expected landing |
|---|---|---|---|---|
| L0 shreds | DZ Edge / Helius shreds + gRPC | FRA (+AMS) | everything | slot 0 on ~65-80% of creates (model, critique §3.9) |
| L1 gRPC | LaserStream / SVS / Triton `processed` | FRA | everything; bundle known from meta | slot 0 late or slot +1 |
| L2 free | PumpPortal + public RPC ws | Ohio or FRA | **paper only**: decisions, timing, virtual fills | slot +1..+3 (PumpPortal median 183 ms after slot notification) |
| L3 dark | none for > 3 slots | any | nothing; hot path disarmed | n/a |

A watchdog on the STATE thread disarms firing when `slot_now - last_feed_slot > 3`, or when the feed's lag to the slot clock exceeds `max_feed_lag_ms`. In L2 the hot path still runs the full decode→patch→sign code with a paper key, so timing and correctness are exercised daily for free.

### 3.10 Latency budget (create executed at the leader → our primary variant at the sequencer)

| Step | FRA box, L0, FRA/AMS leader | Source |
|---|---|---|
| Leader execute → first shred out | 1-5 ms | LIKELY (speed-data-feed) |
| Shred → our NIC (DZ in-building / in-metro) | 0.1-3 ms | UNVERIFIED |
| NIC → userspace (busy-poll `recvmmsg`; AF_XDP later) | 5-20 µs | LIKELY |
| Shred parse + streaming deshred to tx bytes | 2-20 µs (+1-5 ms if FEC recovery waits for coding shreds) | estimate; benchmark in M6 |
| `TransactionView` parse + classify, per tx | **0.04-0.13 µs** | MEASURED HERE |
| Watchlist lookup + risk gate | < 0.2 µs | estimate |
| Up to 2 PDA derivations (aqbc, user ATA) | **~20 µs** VM / ~8-10 µs bare metal | MEASURED HERE / LIKELY |
| Curve math + byte patch | < 1 µs | estimate |
| ed25519 sign (primary variant) | **21.7 µs** VM / ~10 µs bare metal | MEASURED HERE / LIKELY |
| base64 + `write()` on warm socket | 3-10 µs | estimate |
| **Our code total (target p99)** | **≤ 60 µs bare metal, ≤ 100 µs VM** | M2 replay benchmark |
| Box → sender / proxy TPU, same TeraSwitch building | 0.1-0.3 ms (other FRA DC: 0.5-2 ms) | UNVERIFIED |
| Sequencer wait: Jito-Agave continuous / Harmonic FBA / BAM / Jito BE bundle | 0 / avg 25, max 50 / 20-50 / ≤ 50 ms | critique-speed §3 |
| **Total, Jito-Agave leader** | **≈ 2-9 ms** | |
| **Total, batched leader (≈ 63% of stake)** | **≈ 25-60 ms** | |
| L1 gRPC in-metro instead of shreds | + 10-80 ms | UNVERIFIED (vendor) |
| L2 PumpPortal from Ohio | ≈ 180 ms + ~50 ms one way to FRA | VERIFIED relative (speed-data-feed §5) |

Conclusion: our software is 1-3% of the budget. Spend effort on placement (TeraSwitch FRA), feed quality (in-metro leader shreds) and per-class bidding, in that order. Do not hand-tune assembly.

## 4. The rest of the system (cold paths, enough to build)

### 4.1 Discovery and operator scoring (Python, reuses `analysis/`)

- **`crawler.py`** (new; reuses `rpc.py`, `dataset.build_create_index`, `dataset.creation_record` and `events.py`). It runs continuously on the Ohio box, because **public `getBlock` keeps only ~1.5 days**. Any day not crawled is lost. Every 10 minutes it:
  - indexes creates via the mint-authority signatures;
  - fetches `getBlock` for slots s..s+3 of every create (`maxSupportedTransactionVersion: 1`);
  - stores every pump transaction in those slots with tx index, version, signer, fee payer, CU, priority fee, tip (provider by tip account, `labels.py`), error code and decoded `TradeEvent`s.

  A full crawl costs about 4 `getBlock` per create × 46.6k creates/day, which is too much for public RPC. The crawler therefore uses a **stratified sample**: 100% of watchlist and operator creates, plus a 10% random sample. The ratio is recorded so estimates can be reweighted.
- **`operators.py`** clusters creators into operators with union-find over strong links:
  - same funder at the same amount ±0.0005 SOL (`funding.siblings_from_index`);
  - a shared bundle wallet (slot-0 adjacent signer);
  - profit recycling (`via_launchpad`) into a fresh creator;
  - the same launcher plus an identical dev-buy amount.

  Weak links (metadata host, CU/tip fingerprint) only break ties. Output tables: `operators` and `operator_members`.
- **`walkforward.py`** scores *policies*, not just operators. Per-operator n rarely reaches the 100-170 needed within an operator's lifetime, so the unit of validation is a **selection rule** applied to all operators. An example rule: "fresh CEX-funded creator, operator ≥ 5 prior launches, prior hit-rate Wilson LB ≥ 0.3, observed bundle ≤ 25 SOL". It is evaluated on every qualifying launch after its discovery date:

```
for day d in test_days:                       # discovery uses only data with outcome horizon < t
  feats(o, t) computed from tokens with create_time + horizon < t
  for each launch x of operator o on day d passing rule R:
      entry = curve after dev + bundle + k rivals (k from slot_txs, or N_hat when unknown)
      pnl  = exit_sim(trades(x), entry, exit_profile, costs(class, bid))
  report per R: n, hit@TP, Wilson 95% CI, mean pnl, bootstrap CI, Benjamini-Hochberg q over all R tried
promote R only if: n ≥ 100 and EV lower bound > 0 after costs, on ≥ 2 disjoint test windows
```

- **Regime flags.** Data from before Oct 9 (400 ms slots), before Sep 15 (no v1) and around Oct 21 (Alpenglow) is tagged, and never pooled without an explicit flag.

### 4.2 Pre-creation watchlist from funding-graph triggers (`rs-watch`, cold runtime in the bot)

Sources:

- Paid mode: a Yellowstone transaction subscription with `account_include` = hot CEX wallets (24 hot + 34 warm from `labels_verified.json`), FixedFloat, ChangeNOW, MoonPay, and every tracked mother.
- Free mode: public `logsSubscribe(mentions=[wallet])`, one subscription per source, plus `getTransaction`. At about 940 tx/h for Binance `5tzFki…`, this is within public limits.

Freshness test: the recipient's pre-balance is 0 in the transaction meta.

```
rule = { source, amount_ranges[≤5], fresh_only, ttl_s (default 1800; median lag is 160 s), keep_address,
         mode: Exact|LastTransfer, inherit_preset, operator_id }
on transfer(src→dst, lamports, slot):
  for r in rules[src]: if in_ranges(lamports, r) && (!r.fresh_only || dst_prebalance==0):
      watch.add(dst, operator=r.operator_id, preset=r.inherit_preset, expires=slot + r.ttl_s/0.215)
      if !r.keep_address: rules.remove(r)
  if mode==LastTransfer && src is tracked && post_balance(src) < 1% of pre:   // wallet emptied itself
      rules.retarget(src → dst)                                               // follow the money
on expiry without a create: watch.remove(dst); stats[r].no_launch += 1 (remove-if-no-launch)
on create by dst: stats[r].launch += 1; precision(r) = launch/(launch+no_launch) → exported to Python
```

Known serial wallets (37% of recent launches come from 50+-coin wallets) enter the watchlist directly from the operator table. A watchlist update is a new snapshot built and swapped in by the cold runtime, never an in-place edit on the hot path.

### 4.3 Exit engine (`rs-exit`, EXIT thread)

**Position opening.** A position is opened only when **our** transaction is seen executed, i.e. a `TradeEvent` with `user = us` (after Alpenglow, `confirmed` arrives about 150 ms later and is final). The position then subscribes to:

- the bonding curve;
- the mint (`TransferChecked` lists the mint);
- the ATAs of the bundle wallets and the creator. Plain Token-2022 `Transfer` omits the mint, so watching ATAs is the only way to see consolidations.

**Sell templates.** These are pre-built per position: `sell_v3` for the curve, and PumpSwap `sell_v2` against the canonical pool `pool(0, pool_authority(mint), mint, WSOL)` for after completion. A sell nonce pool of 64 allows a 2-sender fan-out. Triggers are evaluated on every update for the mint, in priority order:

| Trigger | Rule (defaults; per-operator override) |
|---|---|
| any-bundle-wallet sell | a sell by any slot-0 adjacent signer, creator, fee payer or funding-linked wallet → sell 100% |
| consolidation | Σ tokens transferred into one owner within 2 slots ≥ 20% of supply → sell 100% (front-run sell) |
| big buy | a single buy within `[lo, hi]` SOL (per operator, e.g. 2.8-3.2) → sell 100% |
| TP ladder | up to 5 `(multiple, fraction)` levels, e.g. `(2.0, 0.5), (3.0, 0.5)` |
| trailing | after activation at ×1.5, sell if price < peak × (1 − 25%) |
| phantom SL | SL at −40% armed only after 10 s |
| no activity | no trade for 150 s → sell |
| max hold | 30 min |
| completion | `CompleteEvent` → switch to the PumpSwap path. Hold until the pool exists (synthetic migration under v3), then apply the same rules |

**Sell priority.** The priority fee escalates on retries (×2, at most 3 attempts per slot). `min_sol_output` comes from the current reserves × (1 − 10%) for dump triggers and × (1 − 3%) for the TP ladder. Exit triggers are recorded with lead times so Python can measure the precision and recall of each signal (critique-claims gap 3).

### 4.4 Anti-farm logic (all on the hot path, all O(1))

- **Gate.** Skip when the observed post-bundle vSOL exceeds the operator's p90 (`max_entry_vsol`).
- **Consecutive losses.** After N losses on one operator (default 3), auto-buy is disabled until a human or the walk-forward re-promotes it.
- **Supply share.** Skip when our buy would push the bundle's share below its historical p25 (the rugger's "too many snipers, dump" trigger).
- **Snipe delay.** Optional: re-enter at slot +N if the launch survived, for operators whose slot-0 sells exceed 10%.
- **Presence-conditioned stats.** Python compares slot-0/1 sells on launches we bought against matched launches we skipped. A significant difference flags the operator as farming.
- **Size jitter.** ±10% so our pattern is not a constant.

### 4.5 Backtester (Python)

`backtest.py` reuses `firstslot.token_metrics` (entry definitions eos0 and eoc0), `curve.py` and `pumpapi.trades_before`. It adds:

- an **entry-position model**: our fill after the k-th rival, where k ~ `N_hat` sampled from `slot_txs`, or worst-case k = observed rivals;
- `costs.py`: fees of 1.25% per side, priority fee and tip from `bids.json` by class, a failed-attempt rate, ATA rent (0.002 SOL, refunded on close), and the sell overhead;
- `exit_sim.py`: the §4.3 rules replayed on trade streams, with sell slippage when several wallets exit in the same slot.

### 4.6 Paper-trade recorder and landing measurement

Every `CreateHit` produces a `decision` record carrying:

- `t_rx`, `t_decide`, `t_signed`, `t_written` (monotonic ns, plus realtime from chrony against Jito's regional NTP);
- feed lag against the slot clock;
- the action and its reason code.

**Paper and shadow modes** build and sign variants but do not send them. `paper_eval.py` later joins each decision to the chain:

1. It locates the create's tx index and every rival buy (from `slot_txs`).
2. It estimates **where we would have landed**, with this model: `arrival = t_written + rtt/2(box, ingress) + class wait`. It then interpolates the position from rival landings with known send paths. This is an estimate, labelled as such.
3. It computes a virtual fill at that position, then exit P&L.

**Live mode** replaces step 2 with truth: the landed slot, tx index, rivals ahead and realized fill (§11).

### 4.7 Metrics, alerts, operations

- **Prometheus** (`metrics` 0.24 + `metrics-exporter-prometheus` 0.18):
  - HDR histograms (`hdrhistogram` 7.6) per stage;
  - feed lag;
  - nonce pool Ready count;
  - sender errors;
  - landed position;
  - P&L;
  - per-operator state.
- **Alerts** via webhook (Telegram or Discord) on:
  - feed dark;
  - kill switch;
  - daily loss at 50% and 100% of the cap;
  - a nonce variant landing after burn;
  - an unknown launcher spike;
  - sender error rate above 20%;
  - an operator auto-disabled;
  - the routing table older than 1 epoch.
- **Daily report.** Python writes an HTML report each day. The user is not watching in real time, so a report plus alerts is the interface.

### 4.8 Safety

- **Modes.** `mode` ∈ {`off`, `paper`, `shadow`, `live`}. The default is `paper`. `live` additionally requires `RUGSNIPE_LIVE=1` in the environment **and** the per-operator flag `live = true`.
- **Hard caps,** checked on the hot path with atomics:
  - `max_snipe_lamports`;
  - `max_open_positions`;
  - `daily_loss_lamports`;
  - `max_snipes_per_hour`;
  - per-operator `max_consec_losses`;
  - wallet-balance floor.
- **Kill switch.** Any one of these disarms the hot path within one poll, and re-arming is manual:
  - the file `/run/rugsnipe/KILL`;
  - `SIGUSR1`;
  - a heartbeat-mesh broadcast;
  - the daily-loss breach.
- **Keys.** Keypair files live outside the repo (`/etc/rugsnipe/keys/*.json`, mode 0600). Config holds paths only, and `.gitignore` already blocks `.env`/`*.pem`; add `keys/`. The hot wallet is funded with one day's budget, and profits are swept to a cold address by a daily job.

## 5. Crates and modules

Rust workspace `crates/` (edition 2021, `rust-version = "1.97.0"`; this set builds on 1.97.0, MEASURED HERE):

| Crate | Responsibility | Key dependencies |
|---|---|---|
| `rs-core` | `Pk = [u8;32]`, program ids, discriminators, PDA seeds, integer curve math (golden vectors shared with `curve.py`), `SeqLock`, `FiredSet`, clock | `solana-pubkey 4.4` |
| `rs-decode` | `TransactionView` wrapper (legacy/v0/v1), create/launcher classifiers, trade/transfer decoders, TradeEvent decoder, bundle observer | `agave-transaction-view =6.1.0` |
| `rs-shred` | merkle shred parser, FEC-set buffers, streaming deshred, Alpenglow `BlockComponent` markers, UDP/XDP RX | `reed-solomon-erasure 6.0`, `socket2 0.6`, `xdp 0.8` (feature `xdp`) |
| `rs-feed` | Yellowstone raw codec + `pbwalk`, prost reference path, Deshred, preconf header, PumpPortal/RPC ws, dedupe | `yellowstone-grpc-client 15.0.0`, `-proto 14.0.1`, `tonic 0.14.6`, `bytes 1.12`, `tokio-tungstenite 0.30` |
| `rs-tx` | template builder (legacy and v1), `PatchTable`, signer, variant set, nonce pool, burn | `solana-message 4.6`, `solana-transaction 4.3`, `solana-keypair 3.1`, `solana-nonce 3.4`, `pump-rust-client =0.4.1` (startup + tests only), `base64 0.23` |
| `rs-send` | fixed-buffer HTTP/1.1 senders (Helius, Nozomi, Astralane, bloXroute), Jito JSON-RPC, NextBlock and direct-TPU QUIC, warmers, response parsing | `quinn 0.11.12`, `socket2` |
| `rs-sched` | routing table, owner/backup, heartbeat mesh, RTT table | `serde`, `toml 1.1` |
| `rs-risk` | modes, caps, kill switch, operator state | atomics only |
| `rs-exit` | positions, triggers, sell templates (curve + PumpSwap) | `rs-tx` |
| `rs-watch` | funding rules, watchlist builder, last-transfer retargeting | `rs-feed` |
| `rs-record` | SPSC rings → JSONL.zst, HDR histograms, Prometheus | `rtrb 0.4`, `zstd 0.14`, `hdrhistogram 7.6`, `metrics 0.24` |
| `rugsnipe-bot` (bin) | wiring, thread pinning, config load and validation | `core_affinity 0.8`, `tokio 1.53`, `arc-swap 1.9`, `mimalloc` |
| `rugsnipe-cli` (bin) | fixture fetch, `simulate`, nonce-pool create/close, template dump, replay bench | `solana-rpc-client =4.3.1` (after the toolchain is at 1.97.1) |

Python additions under `analysis/rugsnipe_analysis/`: `store.py` (SQLite schema), `crawler.py`, `launchers.py`, `operators.py`, `walkforward.py`, `backtest.py`, `exit_sim.py`, `costs.py`, `bidmodel.py`, `routing.py` (DZ metadata + `getClusterNodes` + `stake_geo`), `paper_eval.py`, `export.py`, `report.py`.

## 6. Data model

**Hot structs (`#[repr(C)]`, `Copy`, no heap):**

```rust
struct CreateHit { slot: u64, entry_idx: u32, tx_idx: u32, t_rx_ns: u64, src: FeedId, version: TxVer,
                   mint: Pk, bc: Pk, abc: Pk, user: Pk, fee_payer: Pk, creator_arg: Pk, launcher: u16,
                   quote: QuoteKind, is_mayhem: bool, dev_in_lamports: u64, aqbc_hint: Option<Pk>,
                   bundle: ObservedBundle /* n_tx:u8, lamports_est:u64 */ }
struct WatchEntry { operator: u32, preset: u16, expires_slot: u64, flags: u16 }
struct Preset { size_lamports: u64, max_entry_vsol: u64, slip_bps: u16, tier: u8, bid_quantile: f32,
                ev_cap_lamports: u64, exit_profile: u16, max_consec_losses: u8, live: bool }
struct FireJob { hit: CreateHit, nonce_idx: u16, variants: u8, bid: Bid, t_signed0_ns: u64 }
struct Position { mint: Pk, tokens: u64, cost_lamports: u64, entry_vsol: u64, opened_slot: u64,
                  peak_vsol: u64, bundle_wallets: [Pk; 16], n_bw: u8, ladder_done: u8, state: PosState }
```

**SQLite `data/rugsnipe.db` (WAL; gitignored):**

| Table | Columns |
|---|---|
| `creates` | sig PK, slot, tx_idx, time, mint, creator_arg, signer, fee_payer, launcher, quote, mayhem, version, dev_lamports, bc |
| `slot_txs` | slot, tx_idx, sig, version, signer, mint, kind (buy/sell/transfer), lamports, tokens, cu_used, cu_limit, prio_lamports, tip_lamports, tip_provider, err_code, leader, leader_class |
| `funding_edges` | dst, src, lamports, slot, sig, src_label, dst_fresh |
| `wallets` | address, first_seen, n_sigs, label, class |
| `operators` | id, first_seen, last_seen, n_launches, status |
| `operator_members` | operator_id, wallet, role (creator/bundle/mother), link_kind, confidence |
| `policy_scores` | rule_id, as_of, window, n, hits_tp100/150/200, wilson_lo/hi, ev_mean, ev_lo, bh_q, regime |
| `watch_rules` / `watch_events` | rule fields / added, expired, launched, precision |
| `decisions` | create_sig, box, mode, matched, operator, action, reason, t_rx..t_written, feed_lag_ms |
| `orders` | decision_id, variant, sender, sig, nonce, prio, tip, sent_ns, response |
| `fills` | order_sig, landed_slot, tx_idx, rivals_ahead, tokens, lamports, status/err |
| `positions` / `exits` | open/close, trigger, lead_ms, pnl_lamports |
| `paper_fills` | decision_id, est_position, est_fill_vsol, pnl_sim |
| `leaders` | epoch, identity, class, tpu_ip, metro, jito_region, dz_publisher |

## 7. Config schema (`config/rugsnipe.example.toml`)

```toml
[bot]
mode = "paper"                 # off | paper | shadow | live (live also needs RUGSNIPE_LIVE=1)
box_id = "fra1"
keypair_path = "/etc/rugsnipe/keys/trader.json"   # never inside the repo
paper_keypair_path = "/etc/rugsnipe/keys/paper.json"
tables_dir = "/var/lib/rugsnipe/tables"           # watchlist.json operators.json bids.json routing.json launchers.json

[cores]
hot_a = 2
hot_b = 3
fanout = 4
state = 5
exit = 6
io = 7
rec = 8

[risk]
max_snipe_lamports = 250_000_000
max_open_positions = 3
daily_loss_lamports = 1_000_000_000
max_snipes_per_hour = 20
max_consec_losses = 3
min_wallet_lamports = 200_000_000
hard_cap_bid_lamports = 25_000_000
kill_file = "/run/rugsnipe/KILL"

[feeds.shreds]
enabled = false
bind = "0.0.0.0:7733"
source = "dz_edge"
max_fec_wait_us = 3000

[feeds.grpc]
enabled = false
endpoint = "https://laserstream-mainnet-fra.helius-rpc.com"
x_token_env = "RUGSNIPE_GRPC_TOKEN"
commitment = "processed"
raw_codec = true

[feeds.free]
enabled = true
pumpportal = "wss://pumpportal.fun/api/data"
rpc_ws = "wss://api.mainnet-beta.solana.com"

[watchdog]
max_slot_gap = 3
max_feed_lag_ms = 400

[tx]
format = "legacy"              # legacy | v1 (after M3 proves v1 + nonce per sender)
cu_limit = 0                   # 0 = refuse to start until filled from `rugsnipe-cli simulate`
loaded_accounts_bytes = 0
nonce_pool = 256
land_deadline_slots = 3
variants = ["helius_fra", "nozomi_fra", "astralane_fr", "jito_region"]

[senders.helius_fra]
kind = "http"
url = "http://fra-sender.helius-rpc.com/fast"
min_tip = 1_000_000
swqos_only = false

[senders.nozomi_fra]
kind = "http"
url = "http://fra2.nozomi.temporal.xyz/"
api_key_env = "RUGSNIPE_NOZOMI_KEY"   # auth parameter format: check vendor docs
min_tip = 1_000_000

[senders.jito_region]
kind = "jito"
region = "auto"                # leader's own region via GetConnectedLeadersRegioned
min_tip = 1_000

[senders.direct_tpu]
kind = "quic"
enabled = false

[sched]
boxes = "config/boxes.toml"
redundant_fire = false
heartbeat_ms = 50
owner_timeout_ms = 150

[exit.default]
tp = [[2.0, 0.5], [3.0, 0.5]]
trail_activate = 1.5
trail_pct = 25
phantom_sl_pct = 40
phantom_after_s = 10
no_activity_s = 150
max_hold_s = 1800
consolidation_pct = 20
big_buy_lamports = [0, 0]      # [lo, hi]; 0 = off

[operators.default]
size_lamports = 100_000_000
max_entry_vsol = 70_000_000_000
slip_bps = 1500
bid_quantile = 0.75
live = false

[alerts]
webhook_env = "RUGSNIPE_ALERT_WEBHOOK"

[metrics]
listen = "127.0.0.1:9184"
```

Config load **validates** the file and refuses `live` if any of the following holds:

- a cap is 0;
- `cu_limit` is unset;
- the key file is group- or world-readable;
- the tables are older than their TTL;
- the clock offset is greater than 5 ms.

## 8. Failure modes and mitigations

| Failure | Effect | Mitigation |
|---|---|---|
| Feed stall or lag | blind or late snipes | slot-gap and lag watchdog disarms; failover shreds → gRPC → free (paper) |
| Unknown launcher or new create layout | missed snipes | `UnknownLauncher` counter + alert; `launchers.py` relearns; CI diff of the vendored `pump.json` |
| v1 parse surprise / Alpenglow marker | silent drops | explicit marker parser; decode-failure counter > 0 alerts; fixtures from devnet (Alpenglow live there) |
| Create in an `UpdateParent`-cleared bank | buy of a nonexistent mint | mark `Orphaned`; tx fails cheaply; nonce burn |
| Stale nonce variant lands late | buy at a bad price | `min_tokens_out` bound; burn at deadline; alert |
| Two boxes pick different nonces | double buy | deterministic select, 2-slot Ready rule, per-mint `buy_once` cap; loss bounded by `max_snipe_lamports` |
| Landing behind rivals | slippage failure (46% of reactive buys) | tip in-tx (reverts); N_hat-based bound; record and raise q |
| Rugger farms us | repeated −60..−80% | anti-farm gate, consecutive-loss stop, presence test, size jitter |
| Sell fails during dump | full loss | pre-built sells, escalating priority, 2-sender fan-out, `min_sol_output` loosened on dump triggers |
| Curve completes while held | sell path invalid | `CompleteEvent` → PumpSwap template; wait for pool |
| Sender 429 / rate limit (Jito 1/s/IP/region) | dropped variant | owner rule; per-sender token bucket; IO thread marks a socket down |
| BAM maintenance / client migration week | wrong ingress | routing refresh every 5 min Oct 18-28 |
| Key compromise | loss of hot funds | one-day budget, cold sweep, no keys in repo or config |
| Clock skew | wrong latency attribution | chrony to Jito regional NTP; refuse live at offset > 5 ms |
| Runaway firing loop | fee bleed | `max_snipes_per_hour`, per-mint `FiredSet`, kill switch |

## 9. Build plan (most valuable testable artifact first)

| # | Milestone | Testable artifact (with public data only unless noted) |
|---|---|---|
| M0 | **Start the crawler today** on the Ohio box (systemd timer). Public `getBlock` history expires in ~1.5 days. | `slot_txs` growing; daily row counts in the report |
| M1 | **Go/no-go analysis**: `operators.py`, `walkforward.py`, `backtest.py`, `exit_sim.py`, `costs.py` on cached data + crawl | report with policy-level Wilson CIs, EV lower bounds and the BH-corrected list; the explicit answer "is any rule positive after costs?" |
| M2 | `rs-core` + `rs-decode` + replay bench | unit tests on real legacy/v0/v1 create, launcher and trade fixtures (`rugsnipe-cli fetch-fixtures`); Rust↔Python curve golden vectors; **0-allocation test**; p99 decode→signed ≤ 100 µs on this VM |
| M3 | `rs-tx` templates + `rugsnipe-cli simulate` | byte-equality with `pump-rust-client` for 1,000 mints; `simulateTransaction` on live curves gives the CU and loaded-bytes numbers; v1 template simulated |
| M4 | `rugsnipe-bot` paper mode on free feeds (Ohio, then FRA) | 24 h run: every create decoded, decisions recorded, `paper_eval.py` virtual fills; feed-lag histograms |
| M5 | `rs-watch` funding watcher (free `logsSubscribe`) | precision and lead time per rule against later creates; watchlist hit rate |
| M6 | `rs-exit` in paper + `rs-shred` (deshred tests on synthetic shreds built from real block entries; marker tests) | exit-signal precision, recall and lead time on paper positions; deshred = getBlock tx set |
| M7 | *(paid, ~$150-500/mo)* gRPC in FRA, shadow mode | real detection lag vs rivals' landed positions; `bids.json` calibrated per class |
| M8 | *(funded, user approval)* senders + nonce pool + **micro-live at 0.01-0.05 SOL** | land rate, slot/position, realized cost, v1+nonce acceptance per sender, redundant-fire safety |
| M9 | *(paid)* leader shreds (DZ Edge ~$3k/mo FRA, or Helius shreds), AMS box, scheduler | same-slot rate by leader class; A/B shreds vs gRPC |
| M10 | Scale gates | raise size only for rules with live n ≥ 100 consistent with M1 |

## 10. What cannot be validated without paid infrastructure or real money

1. **True same-slot landing rate and position against rivals.** This needs real sends (M8) from a FRA box. Paper "landing" is a model.
2. **Shred-feed timing and completeness per leader**: DZ coverage of 52.95% of stake, "86% needing repair", GRE on AWS. Needs a paid feed (M9).
3. **Per-class ordering rules**: BAM score formula and cadence, Harmonic FBA strategy per validator, whether Jito-account tips count. Public `getBlock` gives only correlational evidence.
4. **v1 + durable nonce acceptance and priority per sender**, Nozomi's nonce deprioritisation, and whether redundant variants are really free.
5. **Proxy TPU admission** (BAM nodes, Harmonic Remote TPUs) for unstaked QUIC during hot launches.
6. **Rugger response to our presence** (farming conditional on us), and exit fills in real dumps with our size.
7. **`partial_fill = true` semantics** under contention on `buy_exact_quote_in_v3`. Simulation shows only the uncontended case.
8. **Alpenglow behaviour on mainnet** after Oct 21: UpdateParent frequency and Yellowstone `processed` from cleared banks. Devnet helps but is not mainnet load.
9. **In-building RTTs at TeraSwitch FRA** to Jito, Helius, Harmonic and BAM endpoints. They require a trial box.

What **can** be validated now, for free: decoding correctness on real legacy/v0/v1 bytes, template correctness and CU by simulation, hot-path CPU latency, discovery and walk-forward edge estimates on public data (with realistic entry from crawled rival positions), the precision and lead time of funding triggers, exit-signal value on historical trade streams, and a full paper-trading loop on free feeds.
