# Gap 5: exact snipe and exit transactions, settled by mainnet simulation

**Question.** Use mainnet `simulateTransaction` to settle the minimal snipe and exit transactions:
- (i) worst-case CU of `buy_v3` + Token-2022 ATA idempotent create + durable-nonce advance + tip, across at least 200 mints;
- (ii) whether v1 can use an ALT, and whether Jito, Helius Sender and Nozomi accept v1;
- (iii) what `partial_fill=true` does when `max_sol_cost` is exceeded;
- (iv) which buyback-recipient set mayhem coins need on v3;
- (v) which `create_v2` remaining-account count marks a non-SOL quote, and the byte offset of `is_mayhem_mode`;
- (vi) the layout of the legacy SPL-Token `create`;
- (vii) slots between `CompleteEvent` and PumpSwap pool creation on non-v3 completions, plus the PumpSwap `sell_v2` account list and fee at about 411 SOL market cap.

**Date:** 2026-10-14, 10:59–11:55 UTC. **Method:** public RPC (`solana-rpc.publicnode.com`, `api.mainnet-beta.solana.com`), `simulateTransaction` with `sigVerify:false`, hand-built v1 transactions (serializer from speed-gap-4). The user was a live wallet `EW4dq…Tk2`, which has a nonce `EzeKnQ…`, its UVA and its WSOL ATA. For fresh-wallet and large-size cases the user was the Binance hot wallet `5tzFki…`. **Code and raw output:** `analysis/scripts/gap5/` (`sweep.py`→`sweep_nonce.json`, `partial.py`→`partial.json`, `mayhem_bb.py`, `sellsweep.py`→`sellsweep.json`, `creates.py`+`classify.py`→`creates_raw.json.gz`/`creates_class.json`, `grad.py`→`grad.json`, `amm_sell.py`→`amm_sell2.json`, `feecfg.py`, `glob.py`). **Labels:** VERIFIED = primary source or our own measurement; LIKELY = strong inference; UNVERIFIED = no evidence.

---

## 0. Answers in one screen

| # | Answer | Status |
|---|---|---|
| i | CU is **deterministic per mint**: `62,310 + 1,500·S` with nonce (`62,160 + 1,500·S` without). **S** is the number of missing PDA bumps (255 − bump) summed over 4 per-mint PDAs: `bonding_curve`, `associated_base_bonding_curve`, `associated_quote_bonding_curve` (the curve's WSOL ATA) and the user's base ATA. Across **361 successful SOL-quoted simulations**, the max was **83,312** (S = 14). The fit residual is ≤ 184 CU. **The 72,150 limit from speed-gap-4 would have failed 58/361 (16%) of mints.** A static 88,000 covers 99.93%. Better: a per-mint limit computed from the bumps you already derive. LADS is 12,095,612–12,095,893 B. | VERIFIED |
| ii | **No.** v1 has no ALT section (SIMD-0385; `agave-transaction-view` 6.1.0 hard-codes 0 lookups for v1). v1 is SIMD-**0385**; SIMD-0296 is "Larger Transaction Size", which builds on v1. v1 is live: 207 of 448 creates (46%) in our sample were v1. Jito and Helius landed v1 creates; Nozomi documents v1 explicitly. | VERIFIED |
| iii | **Nothing on slippage.** `TooMuchSolRequired` (6002) fires whether `partial_fill` is `[true]`, `[false]` or omitted, on normal and on mayhem coins. The flag only matters on **mayhem** coins, for buys larger than the curve's remaining tokens. | VERIFIED |
| iv | **The same 8 `Global.buyback_fee_recipients` for every coin, mayhem included.** A reserved recipient, a normal recipient or a buyback wallet's WSOL ATA → `BuybackFeeRecipientNotAuthorized` (6057). | VERIFIED |
| v | **Count alone is wrong.** A 0-account tail means SOL. A 3-account tail with `tail[0]=WSOL` is **also SOL** (30/448 creates). A tail of 3 (USDC), 4 (any listed/custom quote + `quote_control`) or 8 (migrated pump-coin quote) with `tail[0]≠WSOL` is non-SOL. **`is_mayhem_mode` sits at byte `52 + len(name) + len(symbol) + len(uri)`** of the instruction data; it matched `CreateEvent` 448/448. | VERIFIED |
| vi | Disc `181ec828051c0777`; **14 accounts** (mint, mint_authority, bonding_curve, associated_bonding_curve, global, mpl_token_metadata, metadata, user, system, **SPL Token**, ATA program, rent, event_authority, program); args `name, symbol, uri, creator` only (no mayhem flag). A v3 buy on it costs **60,149 CU** with nonce (S = 3), about 6.7k below T22. LADS 10,713,096. | VERIFIED |
| vii | Non-v3 completions (n = 46): **0 slots in 32 (70%)**, otherwise 2–5; max **5**. Never the same transaction. v3 synthetic completions (n = 8): 0–6, median 4. PumpSwap `sell_v2` = **17 accounts** (§7). Fee at < 420 SOL market cap (a fresh pool opens at **410.8 SOL**) = **125 bps**: LP 2, protocol 93 (50% of it buyback), creator 30. At ≥ 420 SOL it is 120 bps (20/5/95). | VERIFIED |

---

## 1. (i) Worst-case CU of the snipe transaction

**Transaction simulated** (v1, mask `0x0F`, lifetime = live nonce value):
`[AdvanceNonce(EzeKnQ…, EW4dq), CreateIdempotent(user, user, mint, token_program), buy_v3(17 accounts, amount ≈ 0.0098 SOL worth, max_sol_cost 0.02 SOL, partial_fill 01), Transfer 0.001 SOL → Jito tip 96gYZG…]`

**Sample.** 442 newest coins from `frontend-api-v3.pump.fun/coins?sort=created_timestamp` (≈ 11:00 UTC), 431 not complete.
- 361 succeeded: 237 SOL non-mayhem and 124 SOL mayhem; 34 of them holder-reward.
- 68 failed `MintDoesNotMatchBondingCurve` (6004). All were USDC or other-quote curves, because WSOL was passed as quote.
- 1 failed `BondingCurveComplete` and 1 `TooMuchSolRequired` (mayhem price moved).
- The buyback recipient rotated over all 8; every one was accepted.

**Measured (VERIFIED, `sweep_nonce.json`):**

| Subset | n | min | p50 | p90 | p99 | max |
|---|---|---|---|---|---|---|
| SOL, non-mayhem | 237 | 62,128 | 68,156 | 74,127 | 81,705 | **83,312** |
| SOL, mayhem | 124 | 61,756 | 67,640 | 72,314 | 78,410 | 81,334 |

**Model (least-squares, residual −183/+141 CU).**
- `total = 62,222 + 1,495·m_bc + 1,502·m_abc + 1,502·m_aqbc + 1,498·m_uata − 409·mayhem`, where `m_x = 255 − bump(x)`.
- The upper envelope is `max(S) = 62,310 + 1,500·S`, exact for every S from 0 to 14 in the data.
- Per instruction:
  - ATA create = `17,116 + 1,500·m_uata` exactly. The ATA program searches the bump.
  - `buy_v3` = `44,806 + 1,500·(m_bc + m_abc + m_aqbc) − 409·mayhem`.
  - Nonce +150, tip +150.
- Holder-reward flag and mint size have no effect.
- The curve's WSOL ATA is derived on chain even though it "does not need to exist" (TRADE_V3.md).

**Tail risk.** Each bump is geometric (P(m ≥ k) = 2^−k), so S ~ NegBin(4, ½), mean 4.06 measured vs 4 predicted.

| Limit (nonce) | Covers S ≤ | Mints that fail |
|---|---|---|
| 72,150 (speed-gap-4) | 6 | **17.2%** theory / 16.1% measured |
| 80,310 | 12 | 1.06% |
| 88,000 | 17 | 0.075% |
| 100,000 (pump docs) | 25 | 7.6e-6 |

**Other measured costs (VERIFIED, `partial.json`):**
- User without a UVA (first trade): +≈2.8k. The test wallet's WSOL-ATA bump was 254, so the split between UVA init and WSOL bump is LIKELY ~1.3k / 1.5k. Pre-create the UVA, and grind wallet keys so the UVA and WSOL-ATA bumps are both 255. One random key in four qualifies.
- A curve-completing synthetic-migration buy costs 79–83k without nonce (pump 62–65k).
- A failing slippage buy burns ~49k.
- LADS: 12,095,612–12,095,893 B with nonce on Token-2022, which confirms speed-gap-4's 12,124,160 setting.

**Recommendation.**
- Set `cu_limit = 62,500 + 1,500·S` (minus 150 without nonce), computed during PDA derivation. The decoder already derives `bonding_curve`, the curve ATA and the user ATA; add the curve WSOL ATA. Add a +1.5k safety margin.
- If per-mint computation is not wanted, a static **88,000** is the floor.
- In v1 the fee is a fixed total, so a tighter limit only improves priority rank (`reward/(cost+1)`). Per-mint vs static 88k saves ~20k requested CU at median S = 4 (~25% better rank per lamport).

## 2. (ii) v1 and ALTs; which senders accept v1

- **SIMD-0385** ([text](https://raw.githubusercontent.com/solana-foundation/solana-improvement-documents/main/proposals/0385-transaction-v1.md)) says: "This new `v1` transaction format notably does not include address lookup tables" (Transaction Constraints: max 4096 B, 64 accounts). VERIFIED.
- **SIMD-0296** ([text](https://raw.githubusercontent.com/solana-foundation/solana-improvement-documents/main/proposals/0296-larger-transactions.md)) is "Larger Transaction Size". It is "only supported by the v1 transaction format as described in SIMD-0385". speed-tx-landing's "SIMD-0296" citation and its ALT-for-v1 advice are wrong. VERIFIED.
- **Agave source.** `agave-transaction-view-6.1.0/src/transaction_frame.rs::try_new_as_v1` builds `AddressTableLookupFrame{num_address_table_lookups:0}` ("Don't have ATL in txv1"), and `TransactionVersion::V1 => unreachable!` in the lookup parser. VERIFIED.
- **On chain, 448 create transactions** (`creates_raw.json.gz`):
  - v1: 207 (46.2%), none with an ALT;
  - v0: 240, of which 211 use an ALT;
  - legacy: 1.
  - Tips in v1 creates: Jito 64, Helius 48, Astralane 2.

  So the Helius README's "v1 not activated" is stale. VERIFIED.
- **Nozomi** documents v1 on every method, up to 4,096 B, with Send Bundle members ≤ 1,232 B ([doc](https://use.temporal.xyz/nozomi/transaction-v1.md); see speed-gap-4, 233 Nozomi v1 landings). VERIFIED.
- **Size.** Our nonce snipe is 880 B and needs no ALT. Keep it ≤ 1,232 B.

## 3. (iii) `partial_fill` semantics (VERIFIED, `partial.json`)

Normal SOL curve `CbDgR6…` and mayhem SOL curve `92bgQ7…`. Each case was run with `partial_fill` = `[true]`, `[false]` and omitted.

| Case | Normal coin | Mayhem coin |
|---|---|---|
| `buy_v3`, `max_sol_cost` = cost/2 | 6002 `TooMuchSolRequired` (all three) | 6002 (all three) |
| `buy_v3`, amount = remaining + 10M tokens, cap 500 SOL | **succeeds for all three**: synthetic migration, 3 events (Trade, Complete, PostCompleteBuy) | `[true]`: succeeds, buys the remainder, completes (2 events). `[false]`/omitted: **6021 `NotEnoughTokensToBuy`** |
| `buy_exact_quote_in_v3`, `min_tokens_out` too high | 6042 `BuySlippageBelowMinTokensOut` even with `[true]` | 6042 even with `[true]` |
| `buy_exact_quote_in_v3`, 200 SOL | succeeds (synthetic) | `[true]` succeeds; `[false]` 6021 |

This matches TRADE_V3.md ("Only used on mayhem coins…") and SYNTHETIC_MIGRATION.md. **`partial_fill` never converts a slippage failure into a partial fill.** The 22–38% slippage-failure rate in gap-2 cannot be fixed with this flag. For SOL non-mayhem snipes the byte is irrelevant, so omit it and save 1 B.

## 4. (iv) Buyback recipient on mayhem coins (VERIFIED, `mayhem_bb.py`)

On-chain `Global` (`4wTV1Ymi…`, 1,088 B, decoded by IDL):
- `buyback_fee_recipients` = the 8 wallets in [FEE_RECIPIENTS.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/FEE_RECIPIENTS.md);
- `buyback_basis_points` = 5000 (half of the protocol fee);
- `fee_basis_points` 95, `pool_migration_fee` 15,000,001, `max_curve_depth` 2, `whitelisted_quote_mints` = [USDC].

Simulated `buy_v3` on 2 mayhem curves and 1 normal curve:

| `buyback_fee_recipient` passed | Mayhem | Normal |
|---|---|---|
| `9M4giF…` (buyback #1) | ok | ok |
| `GesfTA…` / `4budyc…` (reserved) | 6057 | 6057 |
| `62qc2C…` (normal fee recipient) | 6057 | 6057 |
| WSOL ATA of `9M4giF…` | 6057 | 6057 |

All 8 buyback wallets succeeded on mayhem coins in the 124-mint sweep. The "reserved" set applies only to `feeRecipient` on v2 and legacy trades.

## 5. (v) Shred-level classifiers for `create_v2`

From 448 `create_v2` instructions in the latest 450 mint-authority signatures (441 top-level, 7 via CPI; 2 signatures were PumpSwap `CreatePool`, not creates). VERIFIED.

**Remaining accounts after the 16 fixed ones:**

| Tail length | `tail[0]` | n | Meaning |
|---|---|---|---|
| 0 | — | 366 | SOL |
| 3 | WSOL `So111…2` | **30** | **SOL** (COIN_CREATION.md: WSOL → `quote_mint = default`) |
| 3 | USDC | 10 | USDC |
| 4 | USDC | 2 | USDC + `quote_control` |
| 4 | other mint | 37 | Listed/custom quote (xStock-like `Xs…` Token-2022 mints, a few SPL mints, and 3 pump coins treated as listed depth-0 quotes) + `quote_control` `6z6GDdfb…` at `tail[3]` |
| 8 | pump coin | 3 | Migrated pump-coin quote: 4 + curve, pool, 2 vaults |

**Rule:** `is_sol = tail_len == 0 || tail[0] == WSOL`.
- critique-claims' "3 vs 4" framing is wrong both ways: 3 can be SOL, and 4 can be USDC.
- The CREATE_WITH_PUMP_COIN_QUOTE.md layout (group 0–4 including `quote_bonding_curve`) appeared 0 times as a 5-tail; listed quotes skip index 4.
- Non-SOL total: 52/448 (11.6%).

**`is_mayhem_mode` offset.**
- Data layout: `disc(8) | u32 len + name | u32 len + symbol | u32 len + uri | creator(32) | is_mayhem(1) | [is_cashback u8] [creator_fee_bps u64] [is_holder_reward u8]`.
- So the offset is **`8+4+Ln+4+Ls+4+Lu+32 = 52+Ln+Ls+Lu`**. It matched `CreateEvent.is_mayhem_mode` 448/448. Mayhem share was 153/448 = 34%, against D1's 22.3%.
- `OptionBool`/`OptionU64` are plain structs (1 B / 8 B, no tag; IDL).
- The trailing fields are parsed leniently. Observed tails: 10 B (216), 1 B `00` (105), 9 B (43), 0 B (11), and a 2-byte `0001` (6). All 6 `0001` coins have `is_holder_reward = 1` on their curves.
- Read holder-reward and creator-fee from the curve or `CreateEvent`, not from the instruction tail. LIKELY.

**Creator vs signer.** `creator` arg ≠ fee payer in 8/448 (1.8%), ≠ `user` (#5) in 7. Dev-sell detection must use `bonding_curve.creator` **and** the create signer. D1's 18.9% was not reproduced in this window. UNVERIFIED why; possibly D1 counted CPI launchers' fee payers.

## 6. (vi) Legacy SPL-Token `create`

0 of 448 recent creates used it. From fixture `2wMWZkGP…` (slot 456,642,636, v0), VERIFIED by `getTransaction` and the IDL:

- **Discriminator** `181ec828051c0777`.
- **Accounts (14):**
  0. mint (signer; not a `…pump` vanity)
  1. mint_authority `TSLvdd1p…`
  2. bonding_curve
  3. associated_bonding_curve (**SPL Token** ATA)
  4. global
  5. mpl_token_metadata `metaqbxx…`
  6. metadata PDA
  7. user (signer)
  8. system
  9. token_program `Tokenkeg…`
  10. ATA program
  11. rent sysvar
  12. event_authority
  13. program
- **Args:** `name, symbol, uri: String; creator: Pubkey`. Nothing follows, so legacy creates are never mayhem and always SOL.
- **Classifier:** discriminator, or `account[9] == Tokenkeg` with 14 accounts. Mint index #0, user index **#7** (not #5).
- **Same-transaction dev buy** is the legacy 18-account `buy` (16 IDL + `bonding-curve-v2` + buyback recipient).
- **Our snipe on it** is the same v3 template with `base_token_program = Tokenkeg` and the SPL ATA. Simulated: **60,149 CU** with nonce at S = 3 (T22 model: 66,810), LADS 10,713,096.
- The ATA create costs 13,417 vs 17,116 on T22. A static T22 LADS covers it.

## 7. (vii) Completion → pool, and PumpSwap `sell_v2`

**Gap (VERIFIED, `grad.json`).**
- Sample: 81 graduated coins (`data/cache/graduated_window.json`). For each, the last 25 bonding-curve signatures were fetched to find the transaction holding `CompleteEvent` (`5f72619cd42e9808`) and the `Migrate`/`MigrateV2` transaction. 54 pairs found.

| Completion | n | Slots to migrate | Distribution | Same transaction |
|---|---|---|---|---|
| Non-v3, ix `buy` | 42 | p50 **0**, max 5 | 0:32, 3:3, 4:5, 5:2 | 0 |
| Non-v3, `buy_exact_quote_in` | 4 | 2–4 | | 0 |
| v3 synthetic (`PostCompleteBuyEvent`) | 8 | p50 4, max 6 | 0:3, 3:1, 4:3, 6:1 | 0 |

Median blockTime difference is 0–1 s. 49 used `MigrateV2` and 5 legacy `Migrate`.

**Exit rule.**
- Inside that window, curve sells fail with 6005.
- On `CompleteEvent`, pre-sign `sell_v2` against the deterministic pool `PDA(AMM, ["pool", 0u16, PDA(pump, ["pool-authority", mint]), mint, WSOL])` and send it on the first shred of the migrate transaction (`CreatePoolEvent` `b1310cd2a076a774`). Blind-firing at +1 slot fails about 30% of the time.

**`sell_v2`.**
- Discriminator `5df6823ce7e940b2`; args `base_amount_in u64, min_quote_amount_out u64` ([PUMP_SWAP_TRADE_V2.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/instructions/PUMP_SWAP_TRADE_V2.md), IDL).
- **17 accounts**, w = writable:
  1. pool (w)
  2. user (s, w)
  3. global_config `ADyA8hde…`
  4. base_mint
  5. quote_mint (WSOL)
  6. user_base_ta (w)
  7. user_quote_ta (w, **must exist**)
  8. pool_base_ta (w)
  9. pool_quote_ta (w)
  10. base_token_program
  11. quote_token_program
  12. system
  13. user_volume_accumulator (AMM PDA `["user_volume_accumulator", user]`, w)
  14. fee_config `5PHirr8j…`
  15. buyback_fee_recipient = **WSOL ATA** of a buyback wallet (w)
  16. event_authority (AMM `__event_authority`)
  17. program `pAMMBay6…`
- Simulated on 11 pools: ok. AMM CU 49.6k–57.2k. The transaction with a WSOL-ATA idempotent create was 54–71k, 754 B v1. Add a WSOL `CloseAccount` to unwrap.

**Fee (VERIFIED: `5PHirr…` FeeConfig decoded with the `pump_fees` IDL, plus `SellEvent` from simulations):**
- `fee_tiers`, all in bps:
  - from 0 SOL market cap: LP 2, protocol 93, creator 30;
  - from **420 SOL**: 20 / 5 / 95;
  - from 1,470 SOL: 20 / 5 / 90;
  - and so on down to 20 / 5 / 5 at ≥ 98,240 SOL.
- `flat_fees` (non-pump pools): 25 / 5 / 0.
- A fresh SOL pool opens at `(85.005 − 0.015) SOL × 1e15 / 2.069e14` = **410.8 SOL** (constants from on-chain `Global`). It is in tier 0: **125 bps**. A sell at 17.9 SOL showed LP 2, protocol 93, creator 30, buyback_bps 5000 (half the protocol fee). A sell at 920 SOL showed 20 / 5 / 95.
- launchpad's "0.25%" reads `GlobalConfig` lp 20 + protocol 5, which "play no part in on-chain pricing" (pump-rust-client `math/fees.rs`). WRONG.
- mechanics' 1.25% / 1.20% is right.
- The bonding curve itself is flat 125 bps (pump FeeConfig `8Wf5Ti…`: 0/95/30).

## 8. Exit transaction on the curve (VERIFIED, `sellsweep.json`, 30 mints)

- **Transaction:** `[CreateIdempotent, buy_v3, sell_v3 (same 17 accounts, disc 1c92de7726c469d5, amount, min_sol_output), Token-2022 CloseAccount(ix 9)]`.
- **`sell_v3`** = `42,500 + 1,500·S3 − 371·mayhem`, residual ≤ 93, where S3 = missing bumps of bc + abc + aqbc. The user ATA bump does not matter. CloseAccount costs 1,464.
- **Exit limit:** `44,300 + 1,500·S3` (+150 tip, +150 nonce).
- Old curves with a short layout (< 166 B) take a rent top-up and cost more (TRADE_V3.md). All 442 sampled curves were 166 B.

## 9. Corrections to existing files

1. speed-gap-4 §0/§2.3: CU 72,000/72,150 → **per-mint `62,310 + 1,500·S` (+margin)**, or static ≥ 88k. Its "1.5–1.9k per bump, unexplained residual" is resolved: the 4th PDA is the curve's WSOL ATA.
2. speed-tx-landing §5.4/5.5: SIMD-0385, not SIMD-0296; no ALT in v1.
3. critique-claims C23 and mechanics: the SOL test is `tail_len == 0 || tail[0] == WSOL`, not a count.
4. launchpad: PumpSwap fee is 125 bps below 420 SOL and 120 bps above, not 0.25%.
5. arch-proposal-1/2 open item "partial_fill under contention": closed. Set it false or omit it.

## 10. Open items

- CU at the real block-0 state with competing same-slot buys (the formula should hold; confirm on our first live landings by logging `computeUnitsConsumed`).
- Splitting the UVA-init cost from the user-WSOL-bump cost (one more simulation with a ground wallet).
- The `create_v2` lenient trailing-argument parser (2-byte `0001` → holder reward) is inferred from 6 cases.
