# Reference implementations and crates for the Rust hot path (2026-10-14)

This note covers which open-source code to reuse for the sniper and copy-trader, which protocol constants to hard-code, and which crate set compiles together. Every repo below was shallow-cloned (`git clone --depth 1`, so commit dates are real), or its crate was downloaded from `static.crates.io`. Dependency claims were tested with `cargo generate-lockfile` and `cargo check` on rustc 1.97.0 in this container.

Tags: **VERIFIED** means we checked it ourselves (code, cargo or on-chain IDL). **LIKELY** means a strong source but not reproduced. **UNVERIFIED** means a vendor claim.

## TL;DR

1. **Use the official `pump-rust-client` 0.4.1 (MIT, published 2026-10-13) for instruction builders, PDAs and curve math.** pump-fun's own docs repo links to it ([pump-public-docs README](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/README.md)). Its `idls/pump.json` is byte-identical to the official IDL (md5 `4cebf087…`). With `default-features = false`, it **compiles inside our Agave 4.3 stack**, and its `Instruction` feeds a `solana-message` 4.6 `Message` without conversion. VERIFIED (cargo check plus a run).
2. **Every third-party trading/parser SDK pins exact Agave versions, so they cannot share one `Cargo.lock` with a current client stack.** `sol-parser-sdk`, `sol-shred-sdk`, `sol-trade-sdk`, `carbon-*` 2.0 and `helius` 3.0.1 each fail to resolve next to `solana-rpc-client =4.3.1`. They do resolve together on an Agave **4.2.2** stack. VERIFIED (cargo). Use them as **reference code to copy**, not as dependencies.
3. **Jito `shredstream-proxy` hard-codes a shutdown: "Jito ShredStream is deprecated and will shut down on September 5, 2026. Migrate to DoubleZero Edge"** ([proxy/src/main.rs L44, L237](https://raw.githubusercontent.com/jito-labs/shredstream-proxy/master/proxy/src/main.rs)). VERIFIED. Its `deshred.rs` (Apache-2.0) is still a good FEC/deshred reference for raw DZ Edge shreds.
4. **Yellowstone now has `SubscribeDeshred`**: pre-execution transactions, ALTs resolved, server-side `account_include`/`account_required` filters ([geyser.proto](https://raw.githubusercontent.com/rpcpool/yellowstone-grpc/master/yellowstone-grpc-proto/proto/geyser.proto)). This is a cheap way to see a watched wallet's create before execution. It is served only by Triton (paid beta) ([Triton blog](https://blog.triton.one/deshred-transactions-the-fastest-path-to-solana-data/)). VERIFIED (proto); availability is LIKELY.
5. **Pitfalls that would bite us:**
   - The current Agave 4.3.x client crates need **rustc 1.97.1**, and our toolchain is 1.97.0.
   - `solana-sdk` 5.0 is type-incompatible with the Agave 4.3 clients.
   - `bincode` 3.0.0 is a `compile_error!` tombstone.
   - `carbon-pumpfun-decoder` 2.0 has **no v3 trade instructions**.
   - All VERIFIED.

## 1. Repo matrix

| Repo (clone URL) | Last commit (UTC offset as committed) | License | Verdict | Files worth copying |
|---|---|---|---|---|
| `pump-fun/pump-public-docs` | 2026-10-13 ("depth 2 pump coin quotes…, SDKs to pump-sdk") | none stated | **Ground truth.** IDLs plus per-instruction docs. | `idl/pump.json`, `idl/pump_amm.json`, `idl/pump_fees.json`, `docs/instructions/TRADE_V3.md`, `COIN_CREATION.md`, `FEE_RECIPIENTS.md` |
| crate `pump-rust-client` 0.4.1 (no repository field; source from crates.io) | published 2026-10-13. Cadence: 0.1.13 Sep 9 → 0.4.1 Oct 13 | MIT | **Official Rust SDK. Base our builders on it.** Anchor `declare_program!` over the IDLs. Builders "work from keys alone (no fetched state)" (pump_v3.rs doc). | `src/sdk/pump_v3.rs` (17-acct v3 set), `src/sdk/pump_legacy.rs` (16 plus 2 remaining), `src/pda.rs`, `src/constants.rs` (includes mainnet ALT `Hyif6eWb8x88RVrvjPfabsgRYnwkVnyByEXTVTXbUcyP`), `src/math/bonding_curve.rs` (832 lines: v2/v3 quoting incl. post-completion leg), `src/math/fees.rs` |
| `0xfnzero/sol-trade-sdk` (crate 6.0.1) | 2026-10-14 ("Pump depth-2 support and SWQoS routing fixes v6.0.1") | MIT | Best open **multi-sender** reference: 20 SWQoS providers with tip accounts and regional endpoints. It has **no buy_v3** (only legacy/v2/exact-in discriminators). Its `src/perf/kernel_bypass.rs` contains no XDP, io_uring or raw-socket code: it is naming fluff. Its sniper example reacts to the dev buy *after* the gRPC (post-execution) event, so it is not block-0. | `src/constants/swqos.rs` (tip accounts plus endpoints for Jito, NextBlock, 0slot, Nozomi, bloXroute, Astralane, Helius…), `src/swqos/{jito,helius,temporal,zeroslot,nextblock,astralane_quic}.rs`, `src/instruction/pumpfun.rs` L240–320 (legacy buy with the 2 remaining accounts), `src/common/gas_fee_strategy.rs` |
| `0xfnzero/sol-shred-sdk` (crate 4.0.4) | 2026-10-14 | MIT | **Raw UDP shred → Entry → tx → pump event**, using `solana-ledger` Reed-Solomon plus `Shredder::deshred`. It is built for the post-Jito world and states: "Raw shreds do not contain … loaded ALT addresses." Pins Agave =4.2.2. | `src/shred/reassembler.rs`, `src/shredstream/pump_ix.rs` (create/create_v2 quote-tail parser), `docs/PUMPFUN_CREATE_LAYOUT.md`, `tests/fixtures/pump_create_layout.json` (account layout pinned to an IDL commit, with an RPC fixture manifest) |
| `0xfnzero/sol-parser-sdk` (crate 0.7.13) | 2026-10-14 | MIT | Yellowstone/RPC event parser for pump and PumpSwap. Pins Agave =4.2.2, yellowstone 13.5.0. | Event layouts, the `is_created_buy` heuristic |
| `rpcpool/yellowstone-grpc` (client 15.0.0, proto 14.0.1) | 2026-10-14 | client, proto and examples Apache-2.0; geyser plugin AGPL-3.0 ([LICENSING.md](https://raw.githubusercontent.com/rpcpool/yellowstone-grpc/master/LICENSING.md)) | **Feed client of record.** `subscribe_deshred*`, `subscribe_with_reconnect`, builder knobs (`tcp_nodelay`, `http2_adaptive_window`, `buffer_size`, zstd). | `examples/rust/src/bin/client.rs` (L423–735 deshred action), `client_with_reconnect.rs`, `yellowstone-grpc-client/src/lib.rs` |
| `jito-labs/shredstream-proxy` 0.2.13 | 2026-07-13 ("Shredstream deprecation (#71)") | Apache-2.0 | Service is dead. Code is useful. Pins solana 2.2.1 and a jito-solana git branch. | `proxy/src/deshred.rs` (1188 lines, FEC recovery), `examples/deshred.rs` (bincode `Vec<Entry>`) |
| `jito-labs/mev-protos` | 2025-07-23 | Apache-2.0 | `searcher.proto` still defines `SendBundle`, `GetTipAccounts`, `GetNextScheduledLeader`. | `searcher.proto`, `bundle.proto` |
| `jito-labs/jito-rust-rpc` (crate `jito-sdk-rust` 0.3.2) | 2025-06-20 | MIT (crate) | Thin JSON-RPC wrapper on **reqwest 0.11**. Write our own (~100 LOC) on a warm HTTP/2 client. | `src/lib.rs` `send_bundle` (≤5 tx, base64), `send_txn` (`bundle_only`) |
| `helius-labs/helius-rust-sdk` (crate `helius` 3.0.1) | 2026-10-13 | MIT | Sender reference: regional `/fast` endpoints, 10 tip accounts, tip floors, `preconfSubscribe` binary-frame decoder. Pins `solana-client =4.3.0`. | `src/optimized_transaction.rs` L95–130, L449–531; `src/preconf.rs` (frame `ver:u8 | slot:u64le | idx:u64le | status:u8 | bincode(VersionedTransaction)`) |
| `sevenlabs-hq/carbon` (2.0.0) | 2026-09-10 | MIT | Codama-generated decoders. **Stale for pump v3**: there is no `buy_v3`/`sell_v3`/`buy_exact_quote_in_v3` in `decoders/pumpfun-decoder/src/instructions/`. A copy-trader built on it would miss v3 trades. | `decoders/pumpfun-decoder/src/events/*` for borsh layouts only |
| `bloXroute-Labs/solana-trader-client-rust` 0.2.0 | 2025-07-15 | MIT | Stale (solana-sdk 2.2, tonic 0.12). Skip. | — |
| `nhuxhr/pumpfun-rs` (crate `pumpfun` 4.6.0) | 2025-10-01 | MIT/Apache | Stale (solana 2.3, no `bonding-curve-v2` remaining account). Skip. | — |
| `0xfnzero/pumpfun-sdk` 2.4.3 | 2026-06-20 | MIT | Stale (solana 2.1.16). Skip. | — |
| crate `pumpfun-sdk-core` 0.1.1 | 2026-02-13 | MIT/Apache | solana-sdk 3, no v3, no `bonding-curve-v2`. Skip. | — |
| GitHub "pumpfun sniper rust" topic repos (e.g. `mitro-co/copy-trading-bot-…`) | clone failed / not public | — | Mostly promotional with closed source. Do not run them. | — |

Sources for dates and licenses: `git log -1` on each clone, LICENSE files, and the crates.io API `/api/v1/crates/<name>/versions`.

## 2. Protocol constants, cross-checked (all VERIFIED)

**Sources compared:**
- (A) official `pump-public-docs/idl/pump.json`
- (B) `pump-rust-client` 0.4.1 `idls/pump.json`, byte-identical to (A)
- (C) `sol-trade-sdk/idl/pump.json` (different bytes, but the extracted fields are identical) plus its hand-written constants
- (D) `carbon-pumpfun-decoder` 2.0 hand-coded discriminator checks
- (E) `sol-shred-sdk` `pump_create_layout.json`

We also recomputed every instruction discriminator as `sha256("global:<name>")[..8]` and every event discriminator as `sha256("event:<Name>")[..8]`. All matched.

| Instruction | Discriminator (hex) | Agreeing sources | Accounts |
|---|---|---|---|
| `create` (legacy) | `181ec828051c0777` | A B C E | 14: mint(s,w), mint_authority, bonding_curve(w), assoc_bc(w), global, mpl_token_metadata, metadata(w), **user(s,w)=#7**, system, **token_program=#9**, ATA prog, rent, event_authority, program |
| `create_v2` | `d6904cec5f8b31b4` | A B C D E | 16: mint(s,w)=#0, mint_authority, bonding_curve=#2, assoc_bc, global, **user(s,w)=#5**, system, **token_program=#7**, ATA prog, mayhem_program, global_params, sol_vault, mayhem_state, mayhem_token_vault, event_authority, program. Optional quote tail [quote_mint, quote_vault, quote_token_program, (quote_control)] (E, per `@pump-fun/pump-sdk@2.0.0`) |
| `buy` | `66063d1201daebea` | A B C D | IDL 16 (global, fee_recipient(w), mint, bc(w), abc(w), assoc_user(w), user(s,w), system, token_program, creator_vault(w), event_authority, program, global_volume_acc, user_volume_acc(w), fee_config, fee_program) **plus 2 remaining: `bonding-curve-v2` PDA (ro), buyback recipient (w)**. B (`pump_legacy.rs` L100–103) and C (`pumpfun.rs` L314–316) agree, and on-chain txs show the same (see pumpfun-mechanics-2026.md). |
| `buy_exact_sol_in` | `38fc74089edfcd5f` | A B C | same 16 + 2 |
| `sell` | `33e685a4017f83ad` | A B C | IDL 14 + 2 remaining |
| `buy_v2` / `sell_v2` / `buy_exact_quote_in_v2` | `b817ee6167c5d33d` / `5df6823ce7e940b2` / `c2ab1c46684d5b2f` | A B C D | 27 / 26 / 27 |
| **`buy_v3`** | **`07051dc4f5176550`** | A B (C and D lack it) | **17**: global, base_mint, quote_mint (WSOL for SOL coins), base_token_program, quote_token_program, bonding_curve(w), assoc_base_bc(w), assoc_quote_bc(w), user(s,w), assoc_base_user(w, **must exist**), assoc_quote_user(w), user_volume_acc(w), fee_config, buyback_fee_recipient(w; the wallet itself on SOL curves), system, event_authority, program |
| `sell_v3` / `buy_exact_quote_in_v3` | `1c92de7726c469d5` / `e1f7501ed5b38488` | A B | same 17 |
| Events | CreateEvent `1b72a94ddeeb6376`, TradeEvent `bddb7fd34ee661ee`, CompleteEvent `5f72619cd42e9808` | A, sha256 | — |
| Accounts | BondingCurve `17b7f83760d8ac60`, Global `a7e8e8b1c86c727f` | A, C (`GLOBAL_ACCOUNT_DISCRIMINATOR`) | — |

**Measured `buy_v3` data:** built by B with amount 1,000,000 and max 10,000,000, the bytes are `07051dc4f5176550 40420f0000000000 8096980000000000 01`. That is 25 bytes: disc, u64 LE, u64 LE, then `OptionBool(partial_fill=true)`. VERIFIED (cargo run).

**PumpSwap (`pAMMBay6…`)**, where A, B and C agree:

| Instruction | Discriminator | Accounts |
|---|---|---|
| `buy` | `66063d1201daebea` | 23 |
| `sell` | `33e685a4017f83ad` | 21 |
| `buy_exact_quote_in` | `c62e1552b4d9e870` | 23 |
| `buy_v2` | `b817ee6167c5d33d` | 17 |
| `sell_v2` | `5df6823ce7e940b2` | 17 |
| `create_pool` | `e992d18ecf6840bc` | 18 |

**Both programs share the buy/sell and v2 discriminators, so always key on program ID first.**

**PDA seeds** (B `constants.rs`/`pda.rs`; C `utils/pumpfun.rs` agrees for every seed it defines):

| Account | Seeds | Program |
|---|---|---|
| `global` | `b"global"` | pump |
| `bonding_curve` | `b"bonding-curve", mint` | pump |
| `bonding_curve_v2` | `b"bonding-curve-v2", mint` | pump |
| `creator_vault` | `b"creator-vault", creator` (hyphen) | pump |
| `__event_authority` | `b"__event_authority"` | pump |
| `mint_authority` | `b"mint-authority"` | pump |
| `pool_authority` | `b"pool-authority", mint` | pump |
| `global_volume_accumulator` | `b"global_volume_accumulator"` | pump |
| `user_volume_accumulator` | `b"user_volume_accumulator", user` | pump |
| `quote_control` | `b"quote-control"` | pump |
| `fee_config` | `b"fee_config", pump_program_id` | pump_fees `pfeeUxB6…` |
| `sharing_config` | `b"sharing-config", mint` | pump_fees |
| mayhem `global-params` | `b"global-params"` | `MAyhSmzX…` |
| mayhem `sol-vault` | `b"sol-vault"` | `MAyhSmzX…` |
| mayhem `mayhem-state` | `b"mayhem-state", mint` | `MAyhSmzX…` |
| `pool` | `b"pool", u16 LE index, creator, base_mint, quote_mint` | PumpSwap |
| PumpSwap `creator_vault` | `b"creator_vault"` (**underscore**) | PumpSwap |
| `global_config` | `b"global_config"` | PumpSwap |
| `pool-v2` | `b"pool-v2"` | PumpSwap |

The canonical migrated pool is `pool(0, pool_authority(mint), mint, WSOL)`.

**Our derivation check:** our run derived `global` = `4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf`, which matches the address in pumpfun-mechanics-2026.md. VERIFIED.

**Hot-path consequence.** For a **v3 snipe in the create's slot**, every key except the buyback recipient is a pure function of `(mint, user)`. The buyback recipient comes from any listed `Global.buyback_fee_recipients` wallet. That makes pre-signing templates possible: per watched creator we only need the mint, and the mint comes from create account #0. Prepend `create_associated_token_account_idempotent`, because `buy_v3` does not create the user ATA (B `with_user_atas`). Pre-create `user_volume_accumulator` once per wallet so the first buy does not pay its 0.0018 SOL rent and the extra CU.

## 3. Feed-side reference code

- **Yellowstone `SubscribeDeshred`** (client ≥13.2.0, which implements geyser 14.0.0 from 2026-07-02, per CHANGELOG L285–291).
  - The ALT-resolution fix shipped 2026-07-08 (CHANGELOG L243).
  - Each update carries the `solana.storage.ConfirmedBlock.Transaction` plus `loaded_writable_addresses`/`loaded_readonly_addresses`, and no meta.
  - Filter with `account_required = [watched_wallet, 6EF8…]` and `vote=false`, then decode create_v2 #0/#2/#5 from data.
  - Triton claims p50 ≈ 6.3 ms and p90 ≈ 20 ms from shred arrival, with no methodology given. **UNVERIFIED**.
  - "Only available on Triton One's gRPC servers"; it needs Agave 4.0's deshred notifier ([Triton](https://blog.triton.one/deshred-transactions-the-fastest-path-to-solana-data/)). **LIKELY**.
- **Raw shreds (DoubleZero Edge / Helius Shred Delivery).**
  - Copy the reassembly from `sol-shred-sdk/src/shred/reassembler.rs` or `shredstream-proxy/proxy/src/deshred.rs`.
  - Raw shreds lack ALT contents, so keep an ALT cache keyed by table address. We still get create_v2 #0/#2/#5 when those are static keys. `sol-shred-sdk` "skips unresolved instructions" in strict mode.
  - `solana-entry`/`solana-ledger` are heavy (the full Agave tree). Isolate them in a separate workspace/binary if they would force an Agave pin (§5).
- **Helius `preconfSubscribe`**: `src/preconf.rs` is a complete decoder for the binary frame. Coverage is "not continuous" (same file). Cost and coverage are analysed in speed-data-feed.md.

## 4. Sender-side reference code

- **Helius Sender** (`helius` 3.0.1, `optimized_transaction.rs`):
  - Endpoints are `http://{slc,ewr,lon,fra,ams,sg,tyo}-sender.helius-rpc.com/fast`.
  - Minimum tip is **0.001 SOL (Sender Max)** or **0.000005 SOL with `swqos_only=true`**, sent to one of 10 tip accounts (`4ACfpUFo…`, `D2L6yPZ2…`, …).
  - `warm_sender_connection` hits `/ping`. VERIFIED (code).
  - `sol-trade-sdk` hard-codes `SWQOS_MIN_TIP_HELIUS = 0.0002`, which disagrees with Helius's own SDK. Trust the vendor SDK.
- **Jito**:
  - JSON-RPC `sendBundle` (≤5 base64 txs) and `sendTransaction?bundleOnly=` at `https://<region>.mainnet.block-engine.jito.wtf/api/v1/…`.
  - Regions listed in `sol-trade-sdk`: ny, frankfurt, amsterdam, dublin, slc, tokyo, singapore, london (no LA).
  - gRPC `SearcherService.SendBundle` remains in `mev-protos`.
- **Nozomi/0slot/NextBlock hosts** (from `sol-trade-sdk/src/constants/swqos.rs`; vendor docs not re-checked, so **LIKELY**):

  | Provider | Hosts |
  |---|---|
  | Nozomi | `{ewr1,fra2,ams1,lon1,lax1,tyo1,sgp1}.nozomi.temporal.xyz` |
  | 0slot | `{ny,de2,ams,la,jp}.0slot.trade` |
  | NextBlock | `{ny,fra,ams,dublin,slc,tokyo,sgp,london}.nextblock.io` |

  Minimum tips in that file: Nozomi 0.001, NextBlock 0.001, 0slot 0.0001 SOL.
- **Measured transaction size.** A legacy tx with CU-limit, CU-price, ATA-idempotent, `buy_v3` and a tip transfer is **834 bytes**; without the ATA ix it is **792 bytes**. Both are under the 1,232 limit, with no ALT needed. VERIFIED (bincode). Pump's mainnet ALT `Hyif6eWb…` exists if we add more ixs.
- **Transaction v1** (SIMD-0385): CU limit and the *total* priority fee go in the message header, so there are no ComputeBudget ixs. `solana-message` **4.6.0 already has `versions::v1`**, so we do not need 5.x. VERIFIED (crate source). The Helius README says v1 is "not activated on any cluster", but our block scan saw 41% of block-0 buys using v1 (speed-tx-landing.md). Treat v1 as live (**LIKELY**), and verify with `simulateTransaction` before relying on it.

## 5. crates.io versions (sparse index plus API, 2026-10-14)

| Crate | Latest stable (date) | Notes |
|---|---|---|
| solana-sdk | **5.0.0** (09-11); 4.1.0 (07-28) | 5.x moves to signer 4, keypair 4, transaction 5, message 5 |
| solana-client / solana-rpc-client / -api / solana-tpu-client(-next) | **4.3.1** (10-13); 4.3.0 (09-18); 4.2.2 (08-28) | 4.3.x **rust-version 1.97.1**. Betas: 4.4.0-beta.1, 4.5.0-alpha.2 |
| solana-program | 5.1.0 (09-23) | — |
| solana-transaction / -message | 5.1.0 / 5.1.0; 4.x line: 4.3.0 / 4.6.0 | Agave 4.3.1 requires `^4.2` / `^4.5` |
| solana-pubkey / solana-address | 4.4.0 / 2.9.0 | pubkey 4.x re-exports solana-address 2.x |
| solana-instruction | 4.0.0 (09-10); **3.5.1 (09-11)** is a shim depending on `solana-instruction-v4` | That shim is why mixed 3.x/4.x code unifies |
| solana-keypair / -signer | 4.0.0; **3.1.2 / 3.0.1** for the Agave 4.3 line | — |
| solana-compute-budget-interface / solana-system-interface | 3.1.0 / 3.3.0 | — |
| yellowstone-grpc-client / -proto | **15.0.0 (10-07) / 14.0.1 (10-07)** | tonic/prost 0.14, solana-pubkey ^4. Proto uses **protoc-bin-vendored**, so no system protoc is needed |
| jito-sdk-rust | 0.3.2 (2025-06-21) | `jito-protos` is **not on crates.io** (path crate in the proxy repo; its build.rs compiles protoc from source via `protobuf_src`, which needs cmake and a C++ toolchain) |
| anchor-lang / anchor-client | 1.2.1 (10-06); 2.0.0-rc.2 (10-12) | 1.2.1 depends on both solana-pubkey v3 and v4 |
| spl-token-2022 / -interface | 11.1.0 / 3.1.2 | The full program crate pulls zk-elgamal deps; use `-interface` |
| spl-associated-token-account(-interface) | 8.0.0 / 2.0.0 | — |
| borsh | 1.8.1 (08-26) | — |
| tokio | 1.53.3 (10-13) | — |
| tonic / prost | 0.14.6 (05-07) / 0.14.4 (06-07) | Old repos still use 0.12/0.13 |
| carbon-core / -pumpfun-decoder / -pump-swap-decoder / -yellowstone-grpc-datasource | 2.0.0 (09-05) | Pins Agave =4.2.2, yellowstone-grpc-client =13.5.0, proto =12.7.0, rust 1.96.1 |
| pump-rust-client | 0.4.1 (10-13) | anchor `>=0.31,<2`, solana-program `>=2.1.21`; client feature is solana 2.x |
| sol-trade-sdk / sol-parser-sdk / sol-shred-sdk | 6.0.1 / 0.7.13 / 4.0.4 (all 10-14) | Agave 4.2.2 pins |
| helius | 3.0.1 (09-22) | pins `solana-client =4.3.0` |
| bincode | 1.3.x is in use; **3.0.0 = `compile_error!("https://xkcd.com/2347/")`** | Do not upgrade |
| reqwest / quinn | 0.13.5 / 0.11.12 | — |

## 6. Compatibility pitfalls (tested in this container)

1. **MSRV.** `cargo check` with `solana-rpc-client =4.3.1` on rustc 1.97.0 fails with "solana-rpc-client@4.3.1 requires rustc 1.97.1" (plus about 15 sibling crates). Fix: pin `rust-toolchain.toml` to ≥1.97.1. `--ignore-rust-version` builds fine, but do not ship that way. VERIFIED.
2. **solana-sdk 5 ≠ Agave 4.3 clients.** `solana-sdk` 5.0 needs `solana-transaction ^5`/`solana-signer ^4`, while `solana-rpc-client` 4.3.1 uses `solana-transaction ^4.2`/`solana-signer ^3`. These are distinct types, so a 5.x `Transaction` cannot go through `RpcClient::send_transaction`. Stay on the 4.x component line, or on `solana-sdk` 4.1. VERIFIED (index deps).
3. **One workspace equals one lockfile.** Each pair conflicted on an exact pin:

   | Added next to our stack | Conflicting pin |
   |---|---|
   | carbon 2.0 | `solana-account-decoder` 4.2.2 vs 4.3.1 |
   | sol-parser-sdk | `solana-instruction =3.4.1` |
   | sol-shred-sdk | `solana-rpc-client =4.2.2` |
   | sol-trade-sdk | `solana-nonce =3.2.0` |
   | helius | `solana-rpc-client-api =4.3.0` |

   An all-4.2.2 workspace (yellowstone 13.5.0/proto 12.7.0, compute-budget-interface 3.0) **does** resolve all of carbon, sol-parser-sdk and sol-shred-sdk together. VERIFIED (cargo). It gives up yellowstone 14/15 proto features from late September (Alpenglow `include_update_parent`, token-account expansion).
4. **Pump IDL drift.** Carbon has no v3. sol-trade-sdk has no v3. pumpfun-rs and pumpfun-sdk lack `bonding-curve-v2`. Generate from the official IDL, or depend on `pump-rust-client`, and run a CI check that diffs our vendored `pump.json` against pump-public-docs (sol-shred-sdk's `tools/sync_pump_create_layout.py --check` is a model).
5. **Shared discriminators.** pump and PumpSwap share `buy`, `sell`, `buy_v2` and `sell_v2` discriminators. VERIFIED.
6. **curve25519 / zeroize.** The old solana 1.x and 2.0 `curve25519-dalek 3.x` + `zeroize<1.4` conflicts did not appear. Our graph resolved to `curve25519-dalek 4.1.3` and compiled. Avoid the `spl-token-2022` program crate and `solana-zk-sdk` in the hot path anyway. VERIFIED (build).
7. **protoc.** Yellowstone needs none (vendored). Jito's `jito_protos` build needs cmake and a C++ toolchain unless `PROTOC` is set. Install `protobuf-compiler` on build boxes and export `PROTOC=/usr/bin/protoc`. VERIFIED (build.rs).
8. **Resolver/edition.** `edition = "2024"` (resolver 3, MSRV-aware) silently picks older crates if `rust-version` is low. Set `rust-version = "1.97.1"` in `[workspace.package]`. LIKELY.

## 7. Proposed workspace dependency set

This was compiled with `cargo check` (about 58 s cold, 453 lockfile entries). Adding `pump-rust-client` gave 553 entries and also compiled. A run produced a valid `buy_v3` tx. VERIFIED.

```toml
[workspace.package]
edition = "2021"
rust-version = "1.97.1"          # and rust-toolchain.toml channel = "1.97.1" (or newer stable)

[workspace.dependencies]
# Solana components: Agave 4.3 line (do NOT use solana-sdk 5.x)
solana-pubkey = "4.4"
solana-address = "2.8"
solana-keypair = "3.1"
solana-signer = "3.0"
solana-signature = "3.5"
solana-hash = "4.6"
solana-instruction = "3.5"        # 3.5.1 shims to 4.0
solana-message = "4.6"            # has versions::v1
solana-transaction = "4.3"
solana-compute-budget-interface = "3.1"
solana-system-interface = { version = "3.3", features = ["bincode"] }
solana-commitment-config = "3.1"
solana-rpc-client = "=4.3.1"      # cold path only (blockhash, ALT, sims)
solana-rpc-client-api = "=4.3.1"
spl-associated-token-account-interface = "2.0"
spl-token-2022-interface = "3.1"
# pump: official builders/PDAs/math, no RPC feature
pump-rust-client = { version = "=0.4.1", default-features = false }
# feed
yellowstone-grpc-client = "15.0.0"
yellowstone-grpc-proto = "14.0.1"
tonic = { version = "0.14.6", features = ["tls-ring", "tls-native-roots", "zstd"] }
prost = "0.14.4"
# runtime / encoding
tokio = { version = "1.53", features = ["rt-multi-thread", "macros", "net", "time", "sync"] }
borsh = { version = "1.8", features = ["derive"] }
bincode = "1.3"                   # never 3.x
reqwest = { version = "0.13", default-features = false, features = ["http2", "rustls-tls"] }
quinn = "0.11"                    # if we add direct QUIC senders
```

**Layout recommendation:**
- `hot` crate (feed, decode, sign, fan-out): the set above. Keep `pump-rust-client` out of the per-packet path. Use it at startup to build account-meta templates, then patch the mint and PDAs with our own byte-level builder, checked against it in unit tests.
- `shred-ingest` binary in a **separate Cargo workspace** pinned to Agave 4.2.2 with `sol-shred-sdk`/`solana-ledger`. It forwards `(slot, mint, creator, sig, recv_ts)` over a Unix socket or shared memory only if we buy DZ Edge or Helius raw shreds.
- Carbon and sol-parser-sdk: offline backtest only, if at all.
- Senders: our own clients copied from the `sol-trade-sdk/src/swqos/*` patterns and the Helius `/fast` code.

## Open questions

- Does Triton Deshred's server-side `account_required` filtering keep the 6 ms p50 at our filter sizes? This needs a paid trial and a side-by-side against DZ Edge raw shreds.
- Is `pump-rust-client`'s Anchor-generated `ToAccountMetas` path fast enough per packet? We plan to template instead, but a benchmark is still needed.
- Are the Nozomi, 0slot and NextBlock endpoints and minimum tips current? They came from `sol-trade-sdk`, not vendor docs.
- Is v1 transaction activation confirmed on mainnet? Check with `simulateTransaction` on a v1 buy.
