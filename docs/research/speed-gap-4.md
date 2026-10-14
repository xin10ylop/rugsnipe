# Speed gap 4: v1 (SIMD-0385) snipe spec and which send paths accept v1 and nonces

**Question.** What is the exact v1 transaction spec for our snipe, `CreateIdempotent(Token-2022 ATA) + buy_v3 + tip`, with and without `AdvanceNonce`? That means the CU limit (mask bit 2), the loaded-accounts data size, or LADS (bit 3), and the total-lamport priority fee (bits 0–1), measured with `simulateTransaction` against real loaded bytes. Which send paths accept v1, and v1 with a durable nonce: Jito BE `sendTransaction` and `sendBundle`, Helius Sender (Max `/fast` and `swqos_only`), Nozomi Batch Send, Astralane, NextBlock QUIC, bloXroute, 0slot, Harmonic Remote TPU and BAM node? Which of them deprioritize nonce transactions?

**Date:** 2026-10-14. **Measurement scripts and raw JSON:** `analysis/scripts/speed_gap4/`. That folder holds `v1build.py`, a hand-rolled v1/legacy serializer that mainnet RPC accepted; `sim.py`, `bound.py` and `multi.py`, the simulations; and `scan4.py` and `an.py`, the block scan.

**Labels.** VERIFIED means measured or read in primary source. LIKELY means strong indirect evidence. UNVERIFIED means no evidence either way.

---

## 0. Answer in one screen

| Field | No nonce | With `AdvanceNonce` (first ix) | Status |
|---|---|---|---|
| Mask | `0x0000000F` (bits 0–3) | same | VERIFIED |
| ConfigValues order | `priority_fee u64 LE`, `cu_limit u32 LE`, `lads u32 LE` (16 bytes, ascending bit order) | same | VERIFIED (SIMD + RPC accepted our bytes) |
| Size | **805 B** (19 keys) | **880 B** (21 keys) | VERIFIED |
| CU consumed (8 fresh SOL-quoted curves) | **61,513 – 71,003** (AyXX…: 65,003) | **+150** | VERIFIED |
| CU limit to set | **72,000** (or a per-mint value, §2.3) | **72,150** | recommendation |
| LADS actually loaded | **12,089,419 – 12,089,448 B** | **+6,216 B** (12,095,664) | VERIFIED |
| LADS to set | **12,124,160** (= 370 × 32 KiB) | **12,124,160** | recommendation |
| Priority fee field | **total lamports** = what you pay above the 5,000 base. Sim: `prio=100,000` gives `fee=105,000` | same | VERIFIED |
| Requested cost (scheduler) | ≈ **78,389** CU | ≈ **78,840** CU | computed |
| v1 with bit 2 unset | `ProgramFailedToComplete` (0 CU meter); **priority fee still charged** | | VERIFIED (sim) |
| v1 with bit 3 unset | `MaxLoadedAccountsDataSizeExceeded`; fee charged | | VERIFIED (sim) |

**Send paths.** I scanned 160 finalized blocks: 48,835 non-vote transactions, of which 17,813 were v1. In that scan, v1 landed through Jito, Helius (both tiers), Nozomi, Astralane, Stellium, **0slot (46)**, BlockRazor, **NextBlock (2)** and bloXroute (1). v1 with a durable nonce landed through Jito, Helius, Astralane, Nozomi, BlockRazor, Stellium and NextBlock, but **not 0slot**.

**Nonce penalty.** No primary source documents a nonce penalty except Nozomi's "may be deprioritized by builders". The Agave and Firedancer schedulers contain no nonce penalty, only per-nonce de-duplication. On chain, nonce density is flat across leader clients (§4).

---

## 1. Spec facts (SIMD-0385 and Agave 4.3 source)

- **Wire layout** ([SIMD-0385](https://raw.githubusercontent.com/solana-foundation/solana-improvement-documents/main/proposals/0385-transaction-v1.md)), in order:
  1. `0x81`
  2. legacy header (3 bytes)
  3. `TransactionConfigMask u32`
  4. lifetime (32 bytes)
  5. `NumInstructions u8`, `NumAddresses u8`
  6. addresses
  7. ConfigValues (4 bytes per set bit)
  8. instruction headers `(u8 prog, u8 nacc, u16 datalen)`
  9. payloads
  10. **signatures last**, with no length prefix.
  - Limits: max 4096 bytes and 64 accounts. **No ALTs.** VERIFIED.
  - `solana-transaction 4.3.0` `versioned/mod.rs` writes the message and then the fixed-length signature array ("V1 transaction signatures are written as a fixed length array without a length prefix"). VERIFIED.
  - Nozomi's doc says `0x81` is "followed by the signature count". Strictly, `0x81` is followed by the legacy header, whose first byte is `num_required_signatures`. So the doc is right in effect but loosely worded.
- **Unset fields default to the minimum.** If bits 0–1 are unset, the fee is 0. If bit 2 is unset, the CU limit is **0**. If bit 3 is unset, LADS is **0**, but it is costed as one 32 KiB page. Setting only one of bits 0–1 is invalid. ComputeBudget instructions are ignored but still burn CU (SIMD).
  - Agave `solana-runtime-transaction 4.3.1` `transaction_meta.rs` (`from_v1_config`): `compute_unit_limit: config.compute_unit_limit.unwrap_or(0)` and `loaded_accounts_data_size_limit: …unwrap_or(0)`. In `try_into_config`, the CU limit is clamped to `MAX_COMPUTE_UNIT_LIMIT` and LADS to 64 MiB. VERIFIED.
- **Priority.** `calculate_priority_and_cost` (Agave `transaction_priority.rs`) computes the cost from the **requested** `compute_unit_limit` and **requested** `loaded_accounts_data_size_limit`. Priority is then `reward × M / (cost + 1)`, and the reward comes from `priority_fee_lamports`. VERIFIED (source).
  - So in v1, trimming the CU limit **does not lower what you pay**: the fee is a fixed total. It only shrinks the cost denominator, which raises priority. In legacy, trimming at a fixed price lowered the fee instead.
- **LADS cost.** It is `ceil(bytes / 32768) × 8` CU ([cost_model.rs](https://raw.githubusercontent.com/anza-xyz/agave/master/cost-model/src/cost_model.rs) `calculate_pages_for_bytes`, `DEFAULT_HEAP_COST`). 64 MiB costs 16,384 CU, and our 12.12 MB costs 2,960 CU, a saving of **13,424 cost units**. VERIFIED.
- **`meta.costUnits` on chain is the *executed* cost**, computed from actual CU and actual loaded bytes (`calculate_cost_for_executed_transaction`). It is not what the scheduler ranked on. For example, a real snipe with limit 48,784 and LADS 14 MiB shows `costUnits` 54,889. That matches the actual 47,928 CU plus ~370 pages, not the requested values. Older docs that infer "priority per cost" from `costUnits` understate the penalty for over-requesting. VERIFIED (getTransaction `2AEywZag…`).

## 2. Measurements (mainnet `simulateTransaction`, `sigVerify:false`)

**Setup.**
- **Fee payer and user:** `EW4dqBdF…Tk2`, a live sniper wallet. It holds 2.2 SOL, its `user_volume_accumulator` exists, its WSOL ATA exists, and it is the authority of nonce account `EzeKnQSY…`.
- **Mint:** a fresh create_v2 Token-2022 curve, `AyXXuvqg…pump`, with 0.00099 SOL real reserves. The user had no ATA for it.
- **Buyback recipient:** `9M4giFFM…` (from a real buy).
- **Instructions:** tip 0.001 SOL to a Jito tip account; `buy_v3` = discriminator `07051dc4f5176550`, amount, `max_sol_cost` 0.02 SOL, partial_fill `01`; the 17 accounts per [TRADE_V3.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/instructions/TRADE_V3.md).
- **Self-check:** our PDA code reproduced the bonding curve, the curve ATAs, the user WSOL ATA and the UVA of a real transaction exactly.

### 2.1 Results (VERIFIED; raw output in `sim_AyXXuv.json`)

| Variant | Size | Result | CU | LADS loaded | Fee field |
|---|---|---|---|---|---|
| v1, loose limits (1.4M, 64 MiB), prio 100,000 | 805 | ok | **65,003** | **12,089,448** | 105,000 |
| v1 + AdvanceNonce (lifetime = live nonce) | 880 | ok | **65,153** | **12,095,664** | 105,000 |
| v1, mask = 0 | 789 | `MaxLoadedAccountsDataSizeExceeded` | 0 | — | 5,000 |
| v1, CU set, LADS unset | 801 | `MaxLoadedAccountsDataSizeExceeded` | 0 | — | 105,000 |
| v1, LADS set, CU unset | 801 | `ProgramFailedToComplete` ("consumed 0 of 0") | 0 | 12,089,448 | 105,000 |
| v1, CU = 65,003 exact, LADS = 12,089,448 exact | 805 | **ok** | 65,003 | — | — |
| v1, CU = 65,002 | 805 | `ComputationalBudgetExceeded` | — | — | — |
| v1, LADS = 12,089,447 | 805 | `MaxLoadedAccountsDataSizeExceeded` | — | — | — |
| v1 `buy_exact_quote_in_v3` (spend 0.01 SOL) | 805 | ok | 66,373 | 12,089,448 | — |
| v1 + nonce, `buy_exact_quote_in_v3` | 880 | ok | 66,523 | 12,095,664 | — |
| legacy, same core + 3 ComputeBudget ixs | 842 | ok | 65,453 | 12,089,534 | 6,400 |
| v1 + 2 ignored ComputeBudget ixs | 859 | ok | 65,303 (+300) | 12,089,534 | 105,000 |
| v1 without the ATA create | 762 | `AccountNotInitialized` (3012) on `associated_base_user` | — | — | — |
| v1 nonce ix with a recent blockhash as lifetime | 880 | `BlockhashNotFound` | — | — | — |

**What the table shows.**
- The limits are **exact**: one CU or one byte short fails.
- The `fee` field for the transactions that **failed to load** still shows base plus priority. Under fee-only commit, a v1 snipe shipped with unset fields would pay to land and do nothing. That is LIKELY. None of the 17,813 v1 transactions on chain had CU or LADS unset (VERIFIED, §3), so in practice nobody ships one.
- **Doc errors to avoid:**
  - Nozomi's Rust v1 sample sets only `with_compute_unit_limit(20_000).with_priority_fee(0)` and **no LADS**, which would fail per the rows above ([Nozomi v1](https://use.temporal.xyz/nozomi/transaction-v1.md)).
  - Helius's example `transactionConfig` uses `loadedAccountsDataSizeLimit: 200000`, which is far below any pump transaction ([Helius v1](https://www.helius.dev/docs/rpc/transaction-v1.md)).

### 2.2 LADS is deterministic, so compute it rather than guess

The sum of `data_len + 64` over every loaded account, plus `programdata_len + 64` for each upgradeable program, equals **12,095,664**. That matches the nonce simulation **exactly**. Accounts that do not exist yet (our new ATA) count 0. VERIFIED (`bound.py`). The dominant terms are:

| Account | Bytes |
|---|---|
| pump programdata | **10,485,760** (10 MiB, preallocated) |
| Token-2022 programdata | 1,382,061 |
| ATA program | 105,032 |
| SPL Token programdata | 108,645 |
| `SysvarRecentBlockhashes` (nonce only) | 6,008 |
| fee_config | 4,097 |
| global | 1,088 |
| mint | 391–420 (varies with metadata) |

Across 8 mints, LADS varied by only 29 bytes. Exact need is 12.09 MB, which is 369 pages; 370 pages is 12,124,160. **Setting 12,124,160 costs 8 CU more than the tightest page and leaves ~28–35 KB of headroom** for mint metadata, a Token-2022 upgrade or an extended programdata.

Observed competitors (§3) set **12–14.2 MiB**, which is 384–455 pages, or 3,072–3,640 cost units. Our tight value saves another 100–700 cost units over the best of them.

**Guard.** Recompute LADS at startup, and whenever any programdata account changes, from `getMultipleAccounts` sizes (or a gRPC account watch). If a program upgrade grows Token-2022 by more than ~28 KB, a stale constant would fail every snipe.

### 2.3 CU varies by about 9.5k per mint, so do not use one tight constant

| Mint | CU | bonding-curve bump | curve ATA bump | user ATA bump |
|---|---|---|---|---|
| GspNMz | 61,513 | 255 | 255 | 255 |
| 6Avq9x | 63,102 | 255 | 255 | 255 |
| 26496Q | 63,131 | 255 | 254 | 255 |
| 7XWwoN | 65,002 | 254 | 254 | 255 |
| AyXXuv | 65,003 | 255 | 254 | 255 |
| FWpgnP | 67,631 | 253 | 255 | 253 |
| 5FQDCh | 69,133 | 255 | 255 | 253 |
| CSZV1A / 2Ctzox | 71,002 / 71,003 | 253 | 252 | 255/254 |

- Each lower bump adds roughly 1.5–1.9k CU. That fits find-PDA iterations at 1,500 CU per `create_program_address`, but the data does not fit a clean model (GspNMz and 6Avq9x differ by 1.6k with identical bumps). VERIFIED data, LIKELY cause.
- **Recommendation:** default **72,000** (or 72,150 with nonce). The cost of the headroom is about 7k CU over the mean, or ~9% priority. A CU shortfall means a failed transaction that still pays and still burns the nonce.
- **Refinement:** an offline per-mint estimate of `61.5k + 1.9k × (sum of missing bumps) + 2k`, gated by shadow-mode calibration against our own `computeUnitsConsumed`. **Never** fall back to the observed competitor value of 48–50k; that is `buy_exact_quote_in_v3` **without** the ATA create.

### 2.4 Things the builder must handle (found during simulation)

- **USDC-quoted curves exist.** 4 of 13 fresh mints failed with `MintDoesNotMatchBondingCurve` (6004) because `bonding_curve` stores USDC (`EPjFWdd5…`) at offset 83. The builder must read the curve's quote mint (zero means SOL) and pass the matching quote mint, quote token program and ATAs. VERIFIED.
- **Pre-create** the `user_volume_accumulator` (137 B; it existed in our test) and keep the WSOL ATA (165 B). If the WSOL ATA is absent, LADS drops by 229 bytes, which is harmless.
- **A v1 snipe without ATA create is 762 B; with it, 805 B; with nonce, 880 B.** All three are **under 1,232 B**. That matters because Astralane QUIC, NextBlock QUIC and Nozomi Send Bundle members all cap at 1,232 even for v1 ([Astralane QUIC](https://astralane.gitbook.io/docs/low-latency/submit-transactions/quic-transaction-submission.md), [NextBlock QUIC](https://docs.nextblock.io/api/quic-transaction-submission.md), [Nozomi v1 table](https://use.temporal.xyz/nozomi/transaction-v1.md)). **Rule: never exceed 1,232 B even in v1.**

### 2.5 Priority fee, converted to total lamports

`total = µL/CU × CU_limit / 1e6`. At 72,000 CU:

| Price | Total |
|---|---|
| 100k µL/CU | 7,200 lamports |
| 1M µL/CU | 72,000 lamports |
| 10M µL/CU | 720,000 lamports |

tx-landing §9 "cup" recipes must be multiplied out this way.

Observed v1 pump `priorityFee` medians (§3): Helius 700, Nozomi 704, Astralane 830, 0slot 1,020, Jito 1,974. The p90 is about 14–15k lamports. Most block-0 snipers bid with tips, not priority fees. A real nonce sniper (`2AEywZag…`) paid **2,000,000 lamports of priority fee plus two 0.004 SOL tips** (Helius and an `ASTz…` account) in one v1 transaction: CU limit 48,784, LADS 14,680,064. VERIFIED.

## 3. Block scan: which paths land v1 and v1 with a nonce (VERIFIED)

**Method.**
- 160 finalized blocks, slots 456,784,651–456,786,400, taking every 11th slot, via `getBlock` with `maxSupportedTransactionVersion:1`.
- Providers were attributed by tip account in the static keys. The tip accounts came from Jito `getTipAccounts` and `sol-trade-sdk/src/constants/swqos.rs`.
- A transaction counts as a nonce transaction when its first instruction is System with data `04000000`.
- Totals: 17,813 of 48,835 non-vote transactions were v1. 53.0% of pump-touching transactions were v1 (2,952 of 5,573). The earlier "88%" was a 20-block, provider-tagged subset. 0 v1 transactions had an ALT. All 17,813 had both CU and LADS set; 3,269 left the priority fee unset.

| Provider | v1 landed | of which pump | v1 + nonce | Pump v1 + nonce | Notes |
|---|---|---|---|---|---|
| Jito | 1,107 | 86 | **78** | 34 | Legacy + nonce 164 |
| Helius | 1,591 (Max-tier 686, swqos 905) | 708 | **47** (Max 15, swqos 32) | 4 | |
| Astralane | 443 | 69 | **47** | 29 | |
| Nozomi | 233 | 212 | **19** | 0 | |
| Stellium | 295 | 261 | 10 | 0 | |
| **0slot** | **46** | 46 | **0** | 0 | 1 legacy and 1 v0 nonce tx |
| BlockRazor | 11 | 10 | 11 | 10 | |
| **NextBlock** | **2** | 0 | **2** | 0 | Sigs `EUdzvHC2…`, `33twyBvk…` |
| bloXroute | 1 | 0 | 0 | 0 | `2k5nC2xG…` |
| No known tip | 14,160 | 1,639 | 732 | 86 | |

**Failure rates.** Most landed v1 pump transactions failed: Helius 1,401 of 1,591, Nozomi 232 of 233, 0slot 45 of 46. I sampled 12 failures from Helius and Nozomi. **All of them were bot-program guard errors** (`7JwTi9am…` custom 7/11, `DhpyNWkd…` custom 13, pump `TooLittleSolReceived`). None were format or budget errors, so these are intentional cheap-fail snipes, not v1 rejections. LIKELY across the whole set.

**Jito endpoint proof.** `bundles.jito.wtf/api/v1/bundles/transaction/<sig>` found **8 of 8** sampled Jito-only v1 transactions and **8 of 8** v1+nonce transactions inside Jito bundles.
- Of 14 v1 bundles, 3 had 2 transactions. A multi-transaction bundle can only come from **`sendBundle`**, so `sendBundle` accepts v1. VERIFIED.
- All 15 v1+nonce bundles were single-transaction. That is either `sendTransaction` (which becomes a one-transaction bundle) or a one-transaction `sendBundle`; the two cannot be told apart. The block engine accepts v1 with a nonce: VERIFIED. Which endpoint carried it: LIKELY both.

**Helius detail.**
- Sender docs say every transaction needs a `setComputeUnitPrice` instruction, and Sender Max needs a priority fee of at least 5,000 lamports ([sender](https://www.helius.dev/docs/sending-transactions/sender.md), [Max](https://www.helius.dev/docs/sending-transactions/sender-max.md)). The docs never mention v1.
- On chain, **563 Max-tier v1 transactions had no ComputeBudget instruction and a priority fee under 5,000 lamports**, and 258 swqos-tier v1 transactions had no ComputeBudget instruction. Sender therefore accepts v1 without the dummy instruction and does not enforce the 5,000 floor on `transactionConfig`. LIKELY: the tier was inferred from the largest top-level transfer.
- 636 swqos v1 transactions carried a no-op `setComputeUnitPrice`. That is one bot's habit, not a requirement.

## 4. Per-path verdict and nonce policy

| Path | v1 docs | v1 accepted | v1 + nonce accepted | Nonce policy (primary source) |
|---|---|---|---|---|
| **Jito BE `sendBundle`** | silent ([docs](https://docs.jito.wtf/lowlatencytxnsend/)) | **VERIFIED** (2-tx v1 bundles) | **VERIFIED** at block-engine level | Docs silent; 78 v1+nonce landed in 1-tx bundles |
| **Jito BE `sendTransaction`** | silent | **LIKELY** (1-tx bundles) | **LIKELY** | Docs silent. Default limit **1 req/s per IP per region** (docs) caps variants: one Jito variant per region per IP per second |
| **Helius Sender Max `/fast`** | silent; requires a CU-price ix and ≥5,000 lamports | **VERIFIED** (686 landed) | **VERIFIED** (15) | Silent |
| **Helius `?swqos_only=true`** | silent | **VERIFIED** (905) | **VERIFIED** (32) | Silent. Keyless 1 rps per IP per region; with a key 50 rps per region |
| **Nozomi Batch Send / API v2 / JSON-RPC** | **explicit:** v1 accepted on every method, up to 4,096 B; Send Bundle members ≤1,232 B ([v1](https://use.temporal.xyz/nozomi/transaction-v1.md), [batch](https://use.temporal.xyz/nozomi/batch-send.md)) | **VERIFIED** | **VERIFIED** (19) | "Block builders **may deprioritize** durable-nonce transactions… benchmark with and without" ([tipping](https://use.temporal.xyz/nozomi/tipping-and-faq.md), [troubleshooting](https://use.temporal.xyz/nozomi/troubleshooting.md)). Send Bundle accepts all-nonce bundles. Priority is tip plus your historical success rate ([FAQ](https://use.temporal.xyz/nozomi/faq.md)); losing variants show as "Expired" — whether that counts as failure is UNVERIFIED |
| **Astralane** (Iris HTTP, binary, QUIC) | silent; QUIC caps at 1,232 B | **VERIFIED** (443) | **VERIFIED** (47) | **Nonce-native:** `sendIdeal` takes two transactions on one durable nonce, and `getNonce` provides managed nonce accounts ([submit](https://astralane.gitbook.io/docs/low-latency/submit-transactions.md)). No deprioritization (LIKELY) |
| **NextBlock QUIC** | silent; ≤1,232 B, fire-and-forget ([QUIC](https://docs.nextblock.io/api/quic-transaction-submission.md)) | **VERIFIED, small n** (2) | **VERIFIED, small n** (2) | Silent. Which NextBlock interface carried them is unknown |
| **bloXroute** | silent | **LIKELY** (1) | UNVERIFIED | `submit-snipe` tells you to make the pair exclusive "by using the same durable nonce" ([snipe](https://docs.bloxroute.com/solana/trader-api/api-endpoints/transaction-submisson/submit-snipe.md)), so no penalty (LIKELY) |
| **0slot** | docs unreachable (`docs.0slot.trade` returned 502 through the proxy) | **VERIFIED** (46 landed) | **UNVERIFIED** (0 in sample) | Unknown. Do not put 0slot on the shared nonce until an A/B test |
| **Harmonic Remote TPU** | silent ([architecture](https://docs.harmonic.gg/concepts/harmonic-architecture.md)) | Cannot attribute: no tip account | Harmonic-led blocks (44) held 288 v1+nonce transactions | Remote TPU path itself UNVERIFIED (LIKELY, since it forwards raw packets to Agave/FD-based builders) |
| **BAM node** | not fetched (bam.dev returned 404/502) | 60 BAM-led blocks held 6,111 v1 | **314 v1+nonce in BAM-led blocks** | The leader side is VERIFIED; direct BAM-node submission is UNVERIFIED |

**Scheduler source (VERIFIED).**
- Agave `receive_and_buffer.rs` only **de-duplicates by nonce**: it drops a new nonce transaction if a queued one of equal or higher priority uses the same nonce, and evicts the queued one if the new one is higher.
- Firedancer `fd_pack.h` keeps one transaction per `(nonce account, authority, blockhash)` and replaces it on higher priority (`NONCE_PRIORITY`). It gives nonce transactions a synthetic 500-slot lifetime.
- **Neither applies a priority penalty to nonce transactions.**

**On-chain check (LIKELY; confounded).**
- Nonce transactions per 1,000 by leader client: BAM 48.4, JitoLabs 51.9, HarmonicAgave 55.4, HarmonicFiredancer 50.6, Firedancer 44.4, Rakurai 48.9.
- Median relative block position: nonce 0.574 vs non-nonce 0.566 (all); pump-successful 0.656 vs 0.572 (n = 180 vs 1,390).
- There is no systematic exclusion. There is perhaps a slight lateness for nonce pump wins, but that is confounded by bot type.

**Other.** Hofund (not on our list) reportedly rejects nonce transactions on its `sendTransaction` route, per a docs.thornode.io search snippet. UNVERIFIED.

## 5. Nonce mechanics for cross-sender and cross-box dedupe

- **The next nonce value can be computed locally.** It equals `sha256("DURABLE_NONCE" ‖ previousBlockhash of the slot where the advance landed)`.
  - Source: `solana-nonce 3.4.0` `state.rs` (`DurableNonce::from_blockhash`).
  - **Checked on mainnet:** nonce `EzeKnQ…` = `8Y2sqSnC…` matched `previousBlockhash` of slot 456,789,161, its last advance. It did **not** match that slot's own `blockhash`.
  - Every box can therefore re-arm with no RPC read, as soon as it sees our transaction land in slot N; it already has slot N's parent hash. VERIFIED.
- **A failed transaction still advances the nonce.** A landed but reverted variant (slippage, guard) still advances the nonce and kills the other variants. That is LIKELY (Agave commits fee plus nonce on execution failure). Design for it: a reverted snipe is a lost snipe.
- **Costs:** AdvanceNonce adds +150 CU, +6,216 LADS bytes, +75 B, and +1 write lock (300 cost units). That is ≈ +451 requested cost units, about 0.6% priority.
- **Rate limits for fan-out:**

  | Path | Documented limit |
  |---|---|
  | Jito | 1 req/s per IP per region |
  | Helius keyless | 1 req/s per IP per region |
  | Helius with key | 50 req/s per region |
  | Nozomi | 5 req/s per key per region; send each variant once per key and spread across regions |
  | Astralane QUIC | drops silently over limit |

  With one durable nonce, the practical fan-out per snipe is **one variant per (provider, region, IP)**. The Jito limit makes multiple boxes or IPs the only way to get more than one Jito variant per region.

## 6. What changes in our docs and code

1. **tx-landing §5.4 / §5.5:** use SIMD-0385, not SIMD-0296 (Nozomi's docs make the same naming mix-up). Remove ALTs. Set mask `0x0F` with `prio_total, 72_000 (72_150), 12_124_160`. Remove ComputeBudget instructions.
2. **Priority logic:** bid **total lamports** directly. In v1, CU trimming changes rank, not cost.
3. **Builder:** read the curve quote mint (USDC curves exist), and keep the transaction ≤1,232 B. Compute LADS from live account sizes, and alert if Token-2022 or pump programdata changes.
4. **Calibration:** log `computeUnitsConsumed` and loaded bytes for every one of our landings. Keep the CU limit at the observed max + 1.5k, recomputed daily.
5. **Shared-nonce set:** Jito, Helius (both tiers), Nozomi, Astralane, Stellium, BlockRazor and NextBlock. **0slot and bloXroute** go on a non-nonce or A/B lane until our own landings show v1+nonce success.
6. **A/B test (do this before mainnet capital):** send the same snipe template with and without nonce through each provider, using separate nonces per test so variants do not cancel. Nozomi itself says racing on a shared nonce makes the comparison meaningless. Measure landed-in-slot-0 rate and block index.

## 7. Open items

- Whether 0slot accepts v1 with a nonce, and how Harmonic Remote TPU and direct BAM-node submission behave on v1 (docs unreachable or silent; needs our own sends).
- Whether Nozomi's "Expired" outcome from losing nonce variants lowers our priority score.
- Whether Jito counts `sendTransaction` and `sendBundle` against one shared 1 rps bucket.
- A precise CU model per mint; the residual ~1.6k is unexplained by bumps.
