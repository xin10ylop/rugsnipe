# Architecture review, judge 2: a quant who distrusts backtests

Date: 2026-10-14 (epoch 1057, slot ~456.79M). Reviewer angle: a backtest is guilty until proven innocent. The question I score against is **"how fast and how cheaply does this design produce an honest 'no' (or a defensible 'yes') about the edge, without losing money while it finds out?"** Speed only matters once that answer is "yes".

Proposals reviewed:
- P1: `arch-proposal-1-latency-first.md`
- P2: `arch-proposal-2-validation-first.md`
- P3: `arch-proposal-3-operations-first.md`

Tags: **VERIFIED** means I checked it in this session (primary source, crate source, fixture or live RPC). **MEASURED HERE** means I computed it in this session. **UNVERIFIED** means an idea or claim I could not test. Every check is listed in §8.

## 0. Verdict

| | Latency | Correctness 2026 | Validation rigor | Buildable here | Ops safety | Cost | **Weighted total** |
|---|---|---|---|---|---|---|---|
| Weight | 15% | 20% | 25% | 15% | 15% | 10% | |
| **P1 latency-first** | **9.0** | 7.5 | 5.5 | 6.0 | 6.0 | 6.5 | **6.7** |
| **P2 validation-first** | 7.0 | 7.5 | **9.0** | **7.0** | 7.0 | **8.0** | **7.7** |
| **P3 operations-first** | 7.0 | **8.0** | 6.5 | 6.5 | **9.0** | 7.5 | **7.35** |

**Winner: P2 as the spine.** Graft P3's operations layer and P1's hot-path internals onto it. Then fix the ten cross-cutting defects in §2, which all three proposals share.

My reasons, in one paragraph:
- None of the three contradicts the 2026 protocol facts. All of them use 200 ms slots, PumpSwap, 1.25% per side, no ShredStream, the Agave 4.3 component crates with no solana-sdk 5, and legacy/v0/v1 decoding. So correctness separates them only on details.
- What separates them is how much I would trust the number that decides whether money moves. Only P2 fully specifies the backtest as the **same arming code replayed point-in-time** (P3 adopts it by reference), with:
  - immutable operator assignments;
  - a feature whitelist enforced by a test;
  - purge and embargo;
  - a vault;
  - a count of configurations tried;
  - randomized skips that give an unbiased control.
- P1's hot path is the best engineered of the three. But its operator clustering is not point-in-time, and its farming test is confounded.
- P3 is the only design I would leave running unattended. Its edge validation, however, is a thinner copy of P2's.

The prior evidence leans negative. The 13 fresh, exchange-funded creators in `onchain-funding-patterns.md` reached a **median ATH of $4.0k**. An entry at the median post-bundle V_e = 37.5 SOL already costs **$4.7k of market cap** (MEASURED HERE: mcap = V²·S/k), and TP +100% needs about $9.3k. So the design has to be optimised to reach a cheap, fast "no". That is P2's stance.

## 1. Facts I verified beyond the lead's list

| # | Claim in the proposals | Result | Matters for |
|---|---|---|---|
| V1 | "Public `getBlock` keeps only ~1.5 days; every day not crawled is lost" (all three) | **FALSE for `api.mainnet-beta.solana.com`.** These calls returned full data after retrying through 429s: `getFirstAvailableBlock` → `0`; `getBlock(440,000,000)` → blockTime 2026-08-18 05:50 UTC; `getBlock(445,000,000)`; `getTransaction(59kqZN5C…)` → full meta, blockTime 2026-09-07. Only publicnode is short: "Block 450000000 cleaned up… First available block: 456391006". History is **rate-limited, not perishable**. VERIFIED. | Build plan, time to the first honest answer (§2.1) |
| V2 | Public RPC limits | 100 req/10 s per IP; **40 req/10 s per IP per method**; 40 connections; 100 MB/30 s ([solana.com/docs/references/clusters](https://solana.com/docs/references/clusters)). VERIFIED. I hit repeated `429 Too many requests for a specific RPC call` on `getBlock`. | Free-mode feasibility (§2.4) |
| V3 | v1 wire layout: address block at byte 42; config bits 0-3 | My own Python decoder parsed every v1 fixture. Signatures are last (64 B per signer remaining). Mask `0x0f` gives total priority lamports, CU limit and LADS. For example `v1_buy_exact_quote_in_v3`: 212,412 lamports, 68,520 CU, 16 MiB, 787 bytes. `v1_create_v2_buy` is 1,945 bytes, above 1,232. MEASURED HERE. | P1 streaming-deshred early classification is sound |
| V4 | `create_v2` accounts: mint #0, bonding curve #2, assoc. bonding curve #3, user #5; quote tail gives 20 accounts | Matches `v1_create_v2_buy` and `v0_create_v2_quote_tail`. VERIFIED. | All decoders |
| V5 | `buy_exact_quote_in_v3` 17-account set and the mint-dependent keys | Fixture order: global, base_mint, wSOL, T22, SPL, bonding_curve, assoc_base_bc, assoc_quote_bc, user, assoc_base_user, assoc_quote_user, user_volume_accumulator, fee_config, buyback recipient, system, event_authority, program. pump-rust-client `V3Trade::accounts()` agrees. The mint-dependent keys are exactly mint, bc, abc, aqbc and the user base ATA. **No creator vault and no fee recipient.** VERIFIED. | P1's 5-key patch table is complete |
| V6 | The "wSOL question" and `spendable` fee semantics (P2/P3 defer both to simulation) | **Already answered by the fixture.** The v3 buy succeeded with `associated_quote_user` (`DvVa29…`) at 0 lamports, i.e. nonexistent, so no wSOL is needed on SOL curves. TradeEvent `sol_amount` 18,586,732 + `fee` 176,574 + `creator_fee` 55,761 = **18,819,067 = `spendable_quote_in`**. The curve account gained exactly 18,819,067 lamports. So `spendable` is gross, with the fees kept on the curve. VERIFIED. | Template and cost model |
| V7 | `associated_base_user` must be the ATA | In pump-rust-client 0.4.1 `idls/pump.json`, `associated_base_user` and `associated_quote_user` carry **no PDA/ATA constraint**. `associated_quote_bonding_curve` and `user_volume_accumulator` do. The program may still check in code: **UNVERIFIED**, simulate. | Possible CU and latency win (§2.8) |
| V8 | The floor (V ≥ 30 SOL) holds on the curve | **Not on mayhem coins.** The curve of `AvUytJ…pump` (the mint of the v3 fixture) now has vSOL **0.0437 SOL** and real token reserves 1.29e15, which is above the 1e15 supply, and its flag byte after `creator` is `1`. VERIFIED (getAccountInfo). | Excluding mayhem is load-bearing (§2.9) |
| V9 | Create txs contain only the dev's buy | **No.** `v1_create_v2_buy` (slot 456,782,692) has 3 signers: `create_v2`, a legacy buy by creator `8UZ7…`, a `buy_exact_quote_in_v3` by a **second signer `GGd2…`**, and a 0.003 SOL transfer to `D2L6…`. VERIFIED. | Bundle and V_e estimation (§2.6) |
| V10 | solana-message key order | `CompiledKeys` uses a `BTreeMap<Address,…>`, so keys are **sorted within each signer/writable class**. VERIFIED (crate source 4.6.0). | P1's byte-equality test cannot pass (§3) |
| V11 | Crate pins | Verified on crates.io index and in sources: agave-transaction-view 7.0.0 needs rustc 1.98.1 and message/transaction ^5; 6.1.0 has MSRV 1.89, message ^4.5, transaction ^4.2 and V1 instruction frames. litesvm 0.18.0 has MSRV 1.97.1 and depends on agave-feature-set/program-runtime ^4.3. yellowstone-grpc-proto 14.0.1 has `bank_id`, `SLOT_DEAD`, `include_update_parent`, `SubscribeDeshred` and `BlockFooter`. rustc 1.97.1 is installed. VERIFIED. | All three pin correctly |
| V12 | Pump program upgraded 2026-10-13 (P3) | programdata `B5MvUw…` slot field = **456,494,698**, about 17 h before this review. VERIFIED. | P3's upgrade kill is warranted |
| V13 | Epoch 1064 = slot 459,648,000; Alpenglow at +5,000 (P3) | 1064 × 432,000 = 459,648,000. MEASURED HERE. | Freeze window |
| V14 | P2's break-even table and power numbers | My recomputation with `curve.py` (0.25 SOL, 1.25%/side, 0.004 SOL overhead, miss exits at the floor) gives break-even 9.9 / 29.5 / 39.1 / 47.3% at V_e 31 / 37.5 / 45.9 / 70, against P2's 11 / 30 / 39 / 47. n = 170 (0.31 vs 0.40) and n = 106 (0.43 vs 0.55) match. **Bonferroni over 50 gives 264 only for the 0.43/0.55 pair; the 0.31/0.40 pair needs 419.** MEASURED HERE. | Gate sizes |
| V15 | Rent | A Token-2022 ATA costs 1,513,840 lamports (fixture), which matches P2/P3. P1's "0.002 SOL" is stale. VERIFIED. | Cost model |
| V16 | TradeEvents are in logs (free-feed exits) | `Program data: vdt/007mYe…` appears in `logMessages` of the v3 fixture, so `logsSubscribe(mentions: bonding_curve)` carries TradeEvents. VERIFIED. | Free-mode exits work, except consolidation |

## 2. Defects shared by all three (fix these whichever proposal wins)

### 2.1 "Public data is perishable" is wrong, and it costs two weeks

All three ship a forward collector first and then wait about 10 days to accumulate 300 out-of-sample snipes (P2's gate), "≥ 2 disjoint windows" (P1) or "t+1..t+7" (P3).

`api.mainnet-beta.solana.com` serves `getBlock`, `getTransaction` and `getSignaturesForAddress` back to genesis (V1). It is merely rate-limited. So:

- **Backfill now.** Take 21-30 days of the mint-authority signature index:
  - 46.6k/day × 30 ≈ 1.4M signatures, at 1,000 per page ≈ 1,400 calls.
  - Sample 10% at random, plus every create linked to a candidate operator.
  - For each sampled create, call `getSignaturesForAddress(bonding_curve)`, then `getTransaction` for slots s..s+3. That includes failed transactions, so it captures rivals' slippage losses.
  - It is much lighter than P1's full `getBlock` crawl: four whole ~1-2 MB blocks per create.
  - Budget: 40 req/10 s per method per IP, shared with everything else.
- **Tag regimes.** Pre-Oct-9 data (400 ms slots) is unusable for landing and position models, but usable for the E2 (end of slot 0+1) outcome model with an explicit regime flag. Pre-Sep-15 data has no v1.
- **Keep the forward collector.** It is cheaper than backfill and it is the only source of post-Alpenglow microstructure.

Effect: the first honest out-of-sample report can arrive in days, not after a fortnight of waiting.

### 2.2 Nonce-index probing uses box-local state, so the cross-box double buy is still possible

All three use `idx = H(mint) mod N`. P1 probes to the next Ready slot; P2 and P3 go to "idx+1 if busy". "Busy" or "InFlight" is **local** state.

Scenario:
1. Box A (the window owner) fires mint M1 on idx 5.
2. Box B did not fire M1, so on box B idx 5 is still Ready.
3. In the next slot, M2 also hashes to 5, and both boxes fire: as backup, in redundant mode, or after a stale heartbeat.
4. A uses idx 6. B uses idx 5.
5. If M1's transaction did not land, **both M2 buys are valid and both land.**

All three make this worse, because the funding-watcher entries are **box-local and dynamic** (P1 `rs-watch` in each bot, P2 `rs-watch`, P3 `watch_dyn`), so the two boxes do not even agree on which mints are armed.

P3's statements are false under this scenario:
- "Split-brain only costs duplicate send attempts; it cannot cause a duplicate fill" (§12).
- Its I3 invariant ("at most one landed buy per intent"; intent = xxh3(mint‖wallet) is the same on both boxes) is violated.

The chaos harness will only catch this if it runs two boxes.

**Fix:**
- Use a deterministic index with **no probing**. On a collision, skip.
- Compute reservations from the **global create stream and the replicated, versioned ArmSet only**, never from local fire decisions.
- Backup boxes fire only on ArmSet-keyed hits. Hits from the dynamic funding watcher fire from the owner box only.
- The loss is bounded by `max_snipe` either way, but the claim of impossibility has to go.

### 2.3 Stale durable-nonce variants: the slippage bound only bounds over-paying

P1 says "the slippage bound still caps the damage". P3 counts a late landing as a fill.

`min_tokens_out` bounds the price **from above** only. Suppose a variant lands late, through a sender's auto-retry (Nozomi does this), a delayed BAM batch or a burn that loses its race, **after** the dump has started. V is then lower and the stale buy **passes** the bound: we buy into a falling coin.

Burning after 3 slots (~650 ms) narrows the window but does not close it.

**Fixes:**
- Use only senders that honour `maxRetries=0` for nonce transactions. Measure per sender in probe mode.
- Burn on the **first** observed dump signal, not only after the deadline.
- Evaluate an on-chain slot-expiry assertion instruction against the Clock sysvar (for example a Lighthouse-style assertion program). **UNVERIFIED**: check its availability, CU cost and whether it fits the 64-key v1 limit.
- Report late fills as their own outcome class in `fills`.

### 2.4 The free feed cannot do several things the designs ask of it

1. **No wire bytes.**
   - PumpPortal `subscribeNewToken` frames are JSON (mint, traderPublicKey, bondingCurveKey, vSol…).
   - Public `logsSubscribe` gives logs plus a signature.
   - P2 §6.2 and P3 §6.2 run `TxView::parse(ev.bytes)` on every `FeedEvent`. That cannot run on either free source.
   - The free-mode adapter must build `CreateSeen` from the `CreateEvent` in `Program data:` logs. Those logs carry mint, bonding_curve, user, creator, quote_mint and is_mayhem.
   - So "paper mode exercises the same pipeline" is false for the decoder, and the replay tests must cover both paths.
   - P1 gets this right: free feeds emit a `CreateHit` directly.
2. **No in-slot information.** There is no bundle observation, and PumpPortal arrives a median 183 ms after the slot notification. Free-mode results are at best E2 (slot +1) results. That is fine as long as they are never reported as block-0 results.
3. **Consolidation detection is impossible in time.**
   - Wallet-to-wallet Token-2022 transfers do not mention the bonding curve.
   - Their logs carry no amounts.
   - So each bundle-wallet ATA needs its own subscription plus a `getTransaction`, which works only at `confirmed`, i.e. seconds late.
   - Mark consolidation as a paid-feed-only trigger.
   - Bundle-wallet sell, big buy, TP, trailing and no-activity all work from TradeEvents in logs (V16).
4. **The public-RPC budget is overcommitted.**
   - P3's fallback is "`getAccountInfo(bonding_curve)` every slot… within public-RPC limits for ≤ 4 positions". That is 18.6 req/s against a **4 req/s per-method cap** (V2). Even one position polled every slot (4.65/s) is over the cap.
   - P3's guard also polls once per slot.
   - During validation, P3 runs `fra1` units on `ohio1`, so the collector, the funding watcher (`getTransaction` per CEX transfer; Binance `5tzFki…` alone is about 0.26/s), recovery and exit polling all share one IP.
   - **Fix:** one token-bucket RPC budget per IP, shared across processes, with priority exits > recovery > funding > collector. Use `getMultipleAccounts` (≤ 10 keys on publicnode) at most every 2 slots, and round-robin two endpoints.

### 2.5 Paper "landing position" cannot answer the block-0 question

- From Ohio on PumpPortal, our counterfactual arrival is about +183 ms, plus ~50 ms one way to FRA. That is almost never slot 0, so free-mode paper landing measures nothing useful about the strategy.
- P2's rival time model, `slot_start + tx_index/n_tx·slot_ms`, assumes in-block order is arrival time. About 63% of stake (BAM 37.9% + Harmonic 25.1%, critique-speed) orders by priority inside 20-50 ms batches. There, `tx_index` is a **fee rank, not a timestamp**, so the claimed ±20-40 ms precision is not credible.
- P1's model (`t_written + rtt/2 + class wait`, interpolated from rivals with known send paths) is labelled as an estimate, which is honest, but it is equally unvalidated.

**What actually measures landing:**
- (a) **Shadow mode on a paid post-exec feed in FRA** (P1 M7). SVS costs $150/mo.
- (b) Probe buys (P2). These cost about 0.004 SOL each.
- (c) A cheaper idea I have **not verified**: **lock probes**.
  - At the moment we would fire, send a transaction on the same nonce, sender and bid that transfers **1 lamport to the bonding curve**. Crediting lamports to a program-owned account is allowed, and the transfer takes the curve's write lock.
  - It competes with rival buys for the same lock, so its `tx_index` relative to them is our landing position under that bid.
  - It has no token exposure and gives no buy signal to the rugger.
  - It costs the base fee plus the bid, versus about 0.004 SOL plus market risk for a probe buy.
  - Verify by simulation and one live send.

### 2.6 In-transaction bundles

Create transactions can contain buys by **other signers** (V9).

- P1 (`dev = scan same tx for a pump buy`) and P3 (`v_after(c.dev_buy_args)`) take a single buy. They under-estimate V_e and mis-gate. Because the on-chain bound fails safe, the cost is missed or failed snipes, not losses.
- P2 and P3 define bundle wallets as "adjacent idx +1..+4 or shared tip". That misses in-transaction co-signers, so the **any-bundle-wallet-sell** exit, the most important dump signal, watches an incomplete set.

**Fix:** sum every pump buy in the create transaction, and add every in-transaction buyer to `bundle_wallets`.

### 2.7 Statistical defects in all three

1. **Wilson intervals assume independent snipes. Ours are not independent.** Launches by one operator on one day share the rugger's state, the competitors and the market. Use a two-way **cluster bootstrap over operator × day** for both hit rate and EV. P2's day-block bootstrap covers half of this. All three use iid Wilson intervals for hit rates.
2. **The headline metric should be a difference, not a level.** "EV > 0" can come from a hot market. Report **EV(operator-selected) − EV(random launches matched on V_e band, hour and leader class)**, under the same entry and exit models. P2's 5% control sample and randomized skips are the right raw material. Make the difference the go/no-go statistic.
3. **Repeated looks.** Every 6 h the arming and farm-flag logic re-tests every operator. P2's farm flag fires at posterior > 0.8, and P3's threshold is unspecified. Across dozens of operators and 4 looks a day, healthy operators will be disarmed by noise.
   - Power check (MEASURED HERE): 300 snipes with 20% skipped gives SE(Δ_dump) ≈ 0.066, so a 10-point farming effect sits at z ≈ 1.5. It is not detectable per operator.
   - Pool by family, and use SPRT or alpha-spending.
4. **Pre-registration.** Commit the policy grid, gate thresholds, cost scenarios and the vault boundary to git **before** the first out-of-sample read. P2's vault plus `n_configs_tried` comes close. P1's BH over "all R tried" is useless if R keeps growing between looks.
5. **Information-set parity.** The backtest gate may use only what the live feed mode knows at fire time. On gRPC (L1), the bundle is unknown before fire.
   - P1 gates on the observed bundle, which is L0-only, and backtests on realized rivals.
   - P3's live gate uses the historical p75 bundle, but its E1 uses the realized bundle.
   - P2 is the cleanest: the on-chain `v_max` bound "knows" the realized V in both worlds.
6. **Sensitivity to k̂.** Until probes exist, E1's "buyers ahead" is a guess. Every report must show EV across k̂ ∈ {0, 1, 2, 3, 5} at three cost levels, not a point estimate.

### 2.8 Facts already answerable that P2 and P3 defer to M4/M5

- **wSOL and spendable** (V6): settled from the existing fixture.
- **`associated_base_user` constraint** (V7): the IDL does not force the ATA.
  - The decoded competitor snipe used `createAccountWithSeed + initializeAccount3` and 54,877 CU. Our ATA path spends **20,217 CU on CreateIdempotent alone** (67,116 total in the fixture).
  - If the program accepts any user-owned Token-2022 account, a seed account saves about 15k CU, and it replaces a ~10 µs `find_program_address` with one SHA-256.
  - **UNVERIFIED.** Simulate before relying on it. It is worth one M3 test.

### 2.9 Mayhem exclusion is load-bearing, not cosmetic

All three skip mayhem coins in the hot path, which is correct.

The backtest and **base-rate** tables must exclude them too. D1 had 22.3% mayhem coins, and their curves break the floor (V8). Note also that the v3 fixture is a mayhem trade, so it must not become a curve-math golden vector.

### 2.10 Repository hygiene today

`.gitignore` does not yet block `*.json` keypairs, `keys/` or `config/*.toml`. All three say "add it". Do it, plus P2/P3's pre-commit secret hook, **before** any keypair exists on a box that has this clone.

## 3. P1, latency first: 6.7

**Strengths.**
- It is the only proposal that measured its own CPU budget:
  - decode 38-130 ns per transaction;
  - two PDAs 19.9 µs;
  - signing 21.7 µs;
  - a dependency set that builds.
- From those numbers it concludes, correctly, that software is 1-3% of the latency budget.
- The v1 early classifier at byte 42 is right (V3).
- The 5-key patch table is complete (V5).
- It is leader-class aware: Harmonic ranks priority fee, not tips; BAM's score is learned online. The bid is capped by EV and by a hard cap.
- Every create, hit or miss, gets a decision record, so the population is scored, not just the watchlist.
- The degradation ladder is explicit, and free mode is paper only.

**Concrete flaws.**
1. **Look-ahead in operator identity.**
   - `operators.py` runs union-find over all links, including "profit recycling into a fresh creator", which exists only *after* a launch has paid out.
   - Its walk-forward computes features point-in-time but uses those operator labels, so early launches are attributed to operators that later turned out prolific.
   - That is survivorship look-ahead: the classic way a "rugger tracker" backtest prints money.
   - P2's immutable `op_assign(as_of_slot)` is the fix.
2. **Confounded farming test.** "Presence-conditioned stats… against matched launches we skipped." Our skips are not random: we skip when the bundle is too big, when no nonce is free, or when a cap trips. A non-randomized comparison is biased. Use P2's hash-of-mint randomization.
3. **`bids.json` and `N_hat` are not point-in-time.** They are built from the whole crawl and used inside the walk-forward cost model.
4. **The byte-equality test cannot pass** ("byte-patching reproduces the SDK's message for 1,000 random mints", M3).
   - The SDK sorts keys within each class (V10). A fixed-offset template does not.
   - The template itself is valid, because the runtime does not require sorted keys. Only the test is wrong.
   - Compare decompiled `(program, [(key, signer, writable)], data)` tuples instead.
5. **The slippage bound is a tolerance, not an EV boundary.** `min_tokens_out = expected × (1 − 15%)`. P2 ties the on-chain bound to `v_max`, the largest V at which posterior EV ≥ 0.
6. **No crash recovery.**
   - Open positions after a restart, a guard, and reconciliation are all absent.
   - The kill switch "disarms the hot path". Whether exits keep running is unstated.
   - The daily-loss cap appears to count realized P&L only (no mark-to-market is specified). With 3 open positions of 0.25 SOL, the real loss can exceed the cap by up to about 0.75 SOL.
7. **The nonce race in §2.2.** P1 lists "two boxes pick different nonces → double buy" in its failure table, which is honest, but its "redundant firing is cheap and safe" rests on the same broken assumption.
8. **Scope before evidence.** It builds its own merkle-shred parser with FEC/Reed-Solomon, AF_XDP, a protobuf walker codec and a 3-4 box mesh (M6/M9, DZ about $3k/mo) before any edge exists. P1's own budget shows feed placement dominates, and none of this is testable for real without paid feeds.
9. **The crawler** fetches whole blocks for s..s+3 per create. That is the heaviest possible approach, and it rests on the perishability premise in §2.1.
10. **Stale details.** ATA rent is given as 0.002 SOL (it is 0.00151). Only one dev buy per create transaction is counted (§2.6).

**Graft from P1:**
- the run-to-completion hot thread, plus the allocation-counting allocator test;
- the `PatchTable` template, with a semantic-equality test;
- fixed-buffer HTTP framing and pre-warmed sockets;
- the leader-class bid split with an EV cap and a hard cap (§3.7);
- `FiredSet` across feeds, which measures feed-versus-feed latency for free;
- the L0-L3 degradation ladder and the watchdog;
- watch-rule precision `launch/(launch+no_launch)` exported to Python;
- a stratified sample with a recorded sampling ratio;
- refusing `live` if `cu_limit` is unset, key permissions are open or clock skew is above 5 ms;
- the supply-share anti-farm gate;
- `CreateHit` built directly from free feeds.

## 4. P2, validation first: 7.7 (winner)

**Strengths.**
- It treats this as an estimation problem.
- Break-even is conditioned on V_e; I verified the table within 1.5 points (V14).
- The **on-chain slippage bound is the EV boundary**: `min_tokens_out = tokens_out(X, v_max)`, so landing behind too much SOL becomes a cheap failure, not a bad fill.
- It validates the *policy*, pooled across operators, because per-operator n = 170 is out of reach.
- Point-in-time machinery:
  - immutable operator assignments with a merge log;
  - a feature whitelist with a banned-list test (ATH, volume, `complete`, anything fetched after `t_create`).
- Three entry models: E0 is reported but never used, E1 is the decision model, E2 is the pessimistic case.
- The exit fill is pessimistic: everything in slot s, plus half of slot s+1.
- `prequential.py` calls the **same** `arming.compile` as live.
- Purge and embargo, a vault, and an `n_configs_tried` count.
- Empirical-Bayes shrinkage plus SPRT disarm.
- Randomized skips and a size arm, keyed on H(salt‖mint).
- Kelly sizing on `p_lb`, with a ρ cap on supply share.
- A go/no-go gate with an operator-concentration cap.
- A signed `gate_passed` field.
- An rslog schema with a golden file shared by Rust and Python.
- An honest header: "every EV is an upper bound until landing, farming and exit fills are measured".

**Concrete flaws.**
1. **The rival landing-time model is invalid on batch leaders** (§2.5).
2. **Sell bounds are inverted.** Ordinary pre-completion sells use `min_sol_output = 0`, while emergency exits get "model × 0.7" (§6.4). A TP-ladder sell with no bound is the sell most exposed to a same-slot dump or a sandwich. It should be the other way round: bounded TP sells, and a loose bound (not zero) for dump exits.
3. **The blockhash variant "in addition to" the nonce variant** is restricted to a single box (§7). That does not help: a blockhash variant and a nonce variant of the same intent are **not** mutually exclusive on one box either, so it is a double buy by design. Drop it, or give it its own intent and size budget.
4. **`TxView::parse(ev.bytes)` on PumpPortal and logs feeds** cannot work (§2.4). So "replay = live" does not hold for the free-mode decoder.
5. **Duplicate crates.** It proposes `rs-core`, `rs-txview` and `rs-decode`, while `crates/pump-codec` already exists in the workspace (constants, PDAs, bytes, fixtures). That costs about 2 days of duplicate work.
6. **The farm flag over-fires and is under-powered** (§2.7.3): posterior > 0.8, re-tested 4 times a day across many operators.
7. **No crash recovery or reconciliation**, and no guard.
   - The hot box can die with open positions, and nothing sells them.
   - `exits.open(...)` is called right after send, not on a confirmed fill. P1 opens on our own TradeEvent; P3 derives positions from token balances.
8. **Alpenglow is only a freeze.** `FeedEvent` carries no `bank_id`, and there is no `UpdateParent` handling. P3 does both.
9. **The go/no-go gate uses a guessed k̂** before any probe exists. It is acceptable only with the sensitivity grid in §2.7.6.
10. **The perishability premise** (§2.1) builds a ≥ 10-day wait into the gate.

**Graft from P2:** the whole validation core (§5.1-5.7), the ArmSet contract, the schema and rslog with golden tests, probe mode, the live-arming chain, and the report header.

## 5. P3, operations first: 7.35

**Strengths.**
- The chain is the source of truth. Positions come from Token-2022 balances, and P&L comes from transaction meta keyed by signature.
- Recovery runs **before** `READY=1`.
- Intent ids are deterministic, with `UNIQUE(mint, mode)`.
- A guard process takes over exits through a lease plus an epoch compare-and-set. A wedged loop is caught too: the watchdog is fed only when the progress counters advance.
- Kill means buys stop and exits continue. Unkill and arm-live are local only, with a 24 h token that names the gate-passed ArmSet's sha.
- A **program-upgrade and FeeConfig kill** plus an automatic simulation smoke test. This is warranted: pump redeployed about 17 h before this review (V12).
- Daily loss is chain-derived and includes mark-to-market.
- Keys use `LoadCredential`, `zeroize` and `mlockall`.
- Final sells use a blockhash plus `CloseAccount`; partial sells use a nonce. The reasoning is correct, and the close recovers the rent.
- Senders have circuit breakers plus landed-share degradation.
- All feeds run at once, with cross-feed lag health.
- **LiteSVM execution tests** of our real transactions: nonce exclusivity, and an oversell that fails. This is the only offline proof of the money path in any proposal (litesvm 0.18.0 on Agave 4.3 is VERIFIED).
- Chaos invariants I1-I6.
- An Alpenglow plan with exact slots (V13).
- It reuses `pump-codec`.

**Concrete flaws.**
1. **The public-RPC fallback and guard polling exceed the per-method limit** by 4-5× (§2.4.4).
2. **The split-brain claim is false** (§2.2). Test I3 with two boxes.
3. **Arming is thin.** "EV_lb > 0 and n_oos ≥ 30" for probe sizing.
   - At n = 30 and p̂ = 0.45, the Wilson interval is 0.29-0.62 (MEASURED HERE).
   - Selecting operators on an EV lower bound at n = 30 among many candidates mostly selects luck: winner's curse.
   - Use P2's shrunken posteriors plus the pooled gate.
4. **Information-set mismatch.** The E1 backtest uses the realized bundle, while the live gate uses the historical p75 (§2.7.5).
5. **Sell retry costs.** Final sells retry every 2 slots for up to 15 tries with the tip ×2, capped at 0.01 SOL. Wherever the bid is a priority fee (Harmonic ranks on it), failed duplicates still pay it. Add a per-position cap on total sell spend.
6. **Scale before evidence.** Four production binaries, a chaos proxy, LiteSVM, WireGuard and Prometheus/Alertmanager come before the go/no-go: about 15 developer-days in M0-M7. The pieces are good, but most should follow the edge gate. LiteSVM and recovery are the exceptions and should come early.
7. **The shred-ingest sidecar is pinned to Agave 4.2.2** (inherited from reference-impls). It probably needs an upgrade for Alpenglow block markers after Oct 21. **UNVERIFIED.** P1's own marker parser avoids the question.
8. **Its validation section is P2 by reference**, without the vault, the shrinkage or the randomization details. That makes it weaker as a spec.

**Graft from P3:** `state.db` plus recovery plus reconcile; the guard; the upgrade kill; chain-derived daily loss; the systemd unit and credentials; LiteSVM `rs-it`; chaos invariants (with a two-box I3); sender breakers; the Alpenglow table; reuse of `pump-codec`.

## 6. What the merged design must contain (ranked by value per day of work)

1. **This week, before any Rust:** a rate-limited 21-30 day backfill (§2.1) plus P2's `outcome.py`, `operators.py` (point-in-time) and `prequential.py`. The first report must show:
   - EV under E1 and E2 at three cost levels and k̂ ∈ {0, 1, 2, 3, 5};
   - the **difference against a V_e-matched random control**, with an operator × day cluster-bootstrap lower bound;
   - Wilson intervals labelled iid;
   - the qualifying snipes per day;
   - concentration (share from the top 3 operators);
   - regime tags.

   Mayhem, non-SOL and legacy-SPL creates are excluded everywhere. Commit the grid, the thresholds and the vault boundary first.
2. **Codec on `pump-codec`.** Decode every transaction version. The free path builds `CreateSeen` from logs. Sum all in-transaction buys and record every in-transaction buyer.
3. **Templates** (P1's patch table with a semantic-equality test) plus `rugsnipe-sim` and LiteSVM tests:
   - nonce exclusivity;
   - CU p99;
   - LADS;
   - the seed-account test (§2.8).
4. **Paper bot on free feeds** with an explicit RPC budget. Consolidation is marked paid-only. Results are reported as E2.
5. **Operations layer from P3** before any probe: recovery, the guard, the upgrade kill, chain-derived loss.
6. **Landing measurement.** Use paid shadow mode in FRA (SVS gRPC, $150/mo), then lock probes (§2.5, UNVERIFIED), then probe buys, so that k̂ is measured rather than guessed. Re-measure after Alpenglow.
7. **Multi-box only after a live edge**, with deterministic no-probe nonce indices and ArmSet-only backup firing (§2.2).

**Go/no-go bar I would sign:**
- At least 300 out-of-sample snipes from at least 15 operators, with no operator above 40% of P&L.
- The cluster-bootstrap 95% lower bound of [EV(selected) − EV(matched control)] is > 0 at E1 with base costs and the measured k̂.
- EV ≥ 0 at high costs.
- Vault EV > 0.
- Everything above holds on post-Alpenglow data for landing.

Failing any of these means stay in paper mode. That outcome is a success of the system, not a failure.

## 7. Scoring arithmetic

Total = 0.15·latency + 0.20·correctness + 0.25·validation + 0.15·buildability + 0.15·safety + 0.10·cost.

| | Calculation | Total |
|---|---|---|
| P1 | 1.35 + 1.50 + 1.375 + 0.90 + 0.90 + 0.65 | 6.675 |
| P2 | 1.05 + 1.50 + 2.25 + 1.05 + 1.05 + 0.80 | 7.70 |
| P3 | 1.05 + 1.60 + 1.625 + 0.975 + 1.35 + 0.75 | 7.35 |

No proposal received a correctness penalty for the red-flag list (400 ms slots, Raydium, 1% fees, ShredStream, solana-sdk 5, legacy-only decoding): none of them uses any of those. The correctness deductions are for:
- the shared perishability error;
- P1's impossible byte-equality test and stale rent;
- P2's invalid tx_index-to-time model and unbounded TP sells;
- P3's RPC-budget arithmetic.

## 8. Verification log (commands run this session)

- `python3 scratchpad/txdec.py crates/pump-codec/fixtures/tx/{v1_buy_exact_quote_in_v3,v1_create_v2_buy,v0_create_v2_quote_tail,legacy_create_spl}.json v`. This is my own legacy/v0/v1 parser. It produced account layouts, v1 config values and signature tails consistent with the spec.
- `analysis/rugsnipe_analysis/events.extract_events` on the same fixtures, to get the TradeEvent fee arithmetic (V6).
- `getAccountInfo DZ517AnG…` (the mayhem curve, V8) and `B5MvUwXd…` (the programdata slot, V12) on `api.mainnet-beta.solana.com`.
- History on mainnet-beta: `getFirstAvailableBlock`; `getBlock 440000000` and `445000000`; `getTransaction 59kqZN5CgyGy…`; `getBlockTime 440000000`. Then `getBlock 450000000` on publicnode, which returned "cleaned up" (V1).
- crates.io sparse index: `agave-transaction-view`, `litesvm`, `pump-rust-client`. Registry sources: `agave-transaction-view-6.1.0/src/instructions_frame.rs`, `solana-message-4.6.0/src/compiled_keys.rs`, `pump-rust-client-0.4.1/src/sdk/pump_v3.rs` and `idls/pump.json`, `yellowstone-grpc-proto-14.0.1/proto/geyser.proto`.
- `curve.py` recomputation of break-even and power, Wilson intervals, the A/B power figure and the polling rates (V14, §2.7, §2.4).
- solana.com cluster page for the public RPC limits (V2).
