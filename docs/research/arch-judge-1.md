# Architecture judge 1: systems / HFT review of the three proposals

Date: 2026-10-14, about 09:30 UTC (epoch 1057, slot about 456.79M). Reviewer stance: a systems engineer who has shipped latency-sensitive trading bots. I read the three proposals in full, together with the research base they cite (`pumpfun-mechanics-2026`, `skeptic-factcheck`, `critique-speed`, `critique-claims`, `speed-gap-2`, `speed-reference-impls`, `speed-tx-landing`, `speed-data-feed`, `onchain-funding-patterns`, the transcript digest) and the code that exists (`analysis/`, `crates/pump-codec`). The proposals were written by other model agents, so I re-checked their load-bearing claims myself instead of trusting them (§1).

Tags: **VERIFIED-J1** means I checked it myself in this session (the method is given). The other tags follow the research docs.

## 0. Verdict

| Criterion (weight) | P1 latency-first | P2 validation-first | P3 operations-first |
|---|---|---|---|
| Latency (20%) | **9.0** | 6.5 | 7.5 |
| Correctness vs 2026 facts (20%) | **8.0** | 6.5 | 7.5 |
| Validation rigor (20%) | 6.5 | **9.0** | 8.0 |
| Buildability in this session (15%) | 5.5 | **7.0** | 6.0 |
| Operational safety (15%) | 5.5 | 6.5 | **9.0** |
| Cost efficiency (10%) | 7.0 | **8.5** | 7.5 |
| **Weighted total** | **7.05** | **7.28** | **7.60** |

**Winner: P3, as the skeleton.** It wins narrowly, and the synthesis matters more than the ranking. No proposal is shippable as written. Each has a different blind spot:
- **P1** has the best hot path. Its validation can leak future information, and its runtime safety has holes (an uncapped sell-fee escalation, no crash recovery).
- **P2** has the best statistics. Its hot path is the weakest and the least current: no Alpenglow handling, and a send path routed through tokio and reqwest.
- **P3** is the only design that survives being left alone: crash recovery, a guard process, a gate on program upgrades. It delegates its statistics to P2, and it overclaims that a double buy is "impossible by construction".

**Recommended build: P3 process model + P1 hot-path internals + P2 validation core** (§6).

## 1. What I verified myself (do not take the agents' word for these)

| Claim (who) | Method | Result |
|---|---|---|
| Rent: Token-2022 ATA 170 B = 1,513,840 lamports, nonce 80 B = 1,056,640 (P2, P3) vs "0.002 / 0.0015 SOL" (P1) | `getMinimumBalanceForRentExemption` on mainnet-beta | **P2/P3 correct**: 1,513,840 and 1,056,640 (165 B is 1,488,440). Rent was cut. P1's figures are stale. VERIFIED-J1 |
| pump redeployed 2026-10-13 at programdata slot 456,494,698, authority `7gZufw…` (P3) | `getAccountInfo` on the programs, then the raw programdata header | **Correct.** pump deploy slot 456,494,698 (about 17 h ago). PumpSwap 454,596,406. pump-fees programdata is `75Uu23…`. Same authority for all three. Upgrades are frequent, so P3's upgrade gate is justified. VERIFIED-J1 |
| Slot time and epoch | `getRecentPerformanceSamples`, `getEpochInfo` | 210-216 ms; epoch 1057, slotIndex 162,069. Matches the lead. VERIFIED-J1 |
| v1 wire layout: keys at byte 42, signatures last (P1) | Hand decode of `fixtures/tx/v1_*.json` | **Correct.** `0x81` + 3-byte header + `u32` mask + 32-byte lifetime + `u8` instruction count + `u8` key count, then keys at offset 42. The config values (mask `0b1111`: u64 priority lamports, u32 CU limit, u32 loaded-accounts size) and the trailing signatures account exactly for the remaining bytes. v1 creates reach **1,945 B** (above the 1,232 legacy limit). VERIFIED-J1 |
| v3 account derivations (P1's patch table) | Pure-Python PDA/ATA derivation against the accounts of the real v1 `buy_exact_quote_in_v3` (sig `4HvEBD…`) | `bc` = PDA("bonding-curve", mint) ✓, `abc` = ATA(bc, T22, mint) ✓, **`aqbc` = ATA(bc, SPL-Token, WSOL) ✓ (a per-mint derivation)**, `aqu` = ATA(user, SPL, WSOL) ✓ (constant per wallet), `uva` = PDA ✓. pump-rust-client 0.4.1 `sdk/pump_v3.rs` derives aqbc the same way. **P2 and P3 leave aqbc out of their patch call.** VERIFIED-J1 |
| SOL-quoted v3 needs wSOL? (left open by P2 M5 and P3 M4) | Same fixture | **No.** The real sniper buy is only an ATA `CreateIdempotent` + `buy_exact_quote_in_v3`. There is no wSOL account, no ComputeBudget instruction (v1 config instead) and no tip. It used **67,116 CU** against a 68,520 limit, with 212,412 lamports of priority. The competitor's createAccountWithSeed/initializeAccount3 was unnecessary. P1's template is right. VERIFIED-J1 |
| Who receives the tokens | Same fixture | `associated_base_user` = ATA(**`5aR7Xt…`**, T22, mint), **a different wallet from the signer and payer `8Z2tbj…`**. The program accepts a third-party recipient. Bundle-wallet identity therefore cannot be keyed on signers alone (§5). VERIFIED-J1 |
| Buys inside the create transaction | `fixtures/tx/v1_create_v2_buy.json` (sig `Mz4aKh…`) | **3 signers**: payer/dev, mint, and a second buyer. The tx contains `create_v2` + a legacy `buy` (dev) + a `buy_exact_quote_in_v3` signed by the second wallet. The post-create curve must sum **all** in-tx buys. P1 and P3 model a single "dev buy". VERIFIED-J1 |
| Crate versions and compatibility (all) | crates.io sparse index | Every crate named exists. `agave-transaction-view` 6.1.0 needs message ^4.5 / transaction ^4.2 (rv 1.89); 7.0.0 needs message/transaction ^5 (rv 1.98.1). `litesvm` 0.18.0 uses caret ranges on the 4.3 line (rv 1.97.1). base64 0.23.1, zstd 0.14.2, xdp 0.8.0 and reed-solomon-erasure 6.0.0 exist. All three proposals are right here. VERIFIED-J1 |
| Yellowstone proto fields (P1, P3) | `yellowstone-grpc-proto-14.0.1/proto/*.proto` in the cargo registry | `Message.account_keys = 2`, `instructions = 4`, `config = 7`, `SubscribeUpdateTransactionInfo.index = 5`, `include_update_parent`, `cleared_bank_id`, `bank_id` on many messages. **Correct.** VERIFIED-J1 |
| `partial_fill` semantics (P1 lists them as "cannot be validated") | pump-rust-client 0.4.1 `math/bonding_curve.rs` tests | It is about buys **past the remaining curve supply**: fill the remainder (v2), or take the post-completion leg (v3). It does not turn a slippage failure into a partial fill. Irrelevant for block-0 entries. LIKELY (SDK source, not simulated) |
| Hot-path CPU cost (P1: sign 21.7 µs, two PDAs 19.9 µs) | Release microbench, separate target dir, same 4-vCPU Xeon 2.1 GHz VM (load about 3.3) | ATA `find_program_address` **10.5 µs** (≈0.97 extra bumps): matches P1. ed25519 sign of 760 B **32 µs**, which is worse than P1's figure, probably from load. P1's conclusion stands: our code is tens of µs in a budget of milliseconds. VERIFIED-J1 |
| Sample sizes (P2: n=170 / 106; Bonferroni over 50 operators "≈264"; exact power 0.76) | Normal approximation + exact binomial in Python | n = 170 and 106 ✓; exact power at n=170 is 0.759 ✓. **The Bonferroni figure is attached to the wrong comparison.** At α=0.05/50, 0.31→0.40 needs **n≈419**. 264 is the corrected n for 0.43→0.55. VERIFIED-J1 |
| P1 crawler cost | `getBlock` on mainnet-beta | One full block is **2.0 MB raw / 0.70 MB gzip**. `transactionDetails:"signatures"` is 78 KB. The first full call returned **429 "Too many requests for a specific RPC call"**. A 10% plus watchlist crawl (about 20k full blocks/day) is about 14 GB/day gzip against a tight per-method limit. Use signature lists for ordering plus `getTransaction` for pump transactions only. VERIFIED-J1 |
| `analysis` selftest passes (P3) | `.venv/bin/python -I -m rugsnipe_analysis.selftest` | 15 passed ✓. Note that numpy is in system Python 3.13, not in `.venv` (P2 says "Python has numpy"). VERIFIED-J1 |
| `pump-codec` already covers constants/PDAs/curve/events/builders/TxView (P3) | Read the crate | At review start `lib.rs` was a one-line placeholder (`bytes.rs`, `constants.rs`, `error.rs` plus 20 tx fixtures). During the review another workflow rewrote it to declare the `constants/pda/curve/events/instructions/tx` modules. P3's description is the crate's **declared** scope, still being filled in. P1 and P2 ignore the crate and would duplicate it. VERIFIED-J1 |
| UpdateParent arrives at the Oct 21 cutover? (P1 and P3 build cutover logic around it) | `speed-gap-2.md` (FLH feature `FLHoAW…` absent on all clusters; v4.4, ≥ Nov 9) | **No UpdateParent on Oct 21.** What does arrive is BlockHeader/BlockFooter markers and an **8-byte all-zero "abort" batch**. None of the three handles the abort batch. P1's parser ("entry_count == 0 ⇒ read u16 marker version") would read past the end of an 8-byte abort batch. (From the research doc, not re-run by me.) |

## 2. Proposal 1: latency first

**Scores:** latency 9.0, correctness 8.0, validation 6.5, buildability 5.5, safety 5.5, cost 7.0 → **7.05**

### Strengths
- **The most accurate hot path of the three.** Run to completion on one pinned thread. A counting `#[global_allocator]` proves zero allocations between packet and `write()`. Fixed-buffer HTTP/1.1 with a precomputed `Content-Length` and one patch region. The protobuf walker avoids prost allocations, with the prost path kept as the oracle.
- **Its central claim is right.** Our code is about 1-3% of the end-to-end budget, so effort belongs in placement, feed and per-class bidding. My bench confirms the order of magnitude.
- **The patch table is correct, and it is the only one that is complete.** It lists every mint-dependent key: base_mint, bc and abc read from the create, aqbc derived or taken from the dev buy's account 7, and the user ATA. Everything else is per-wallet constant. I verified this on mainnet bytes. The byte-equality test against `pump-rust-client` over 1,000 random mints is exactly the right oracle test.
- **Per-leader-class bid split:**

  | Leader class | Split |
  |---|---|
  | Jito-Agave | 70/30 priority/tip |
  | Harmonic | all priority (Harmonic tips are priority fees) |
  | BAM | learned online |

  Routing goes to the **advertised tpuQuic** metro. This is the only proposal that fully absorbs critique-speed §3.
- **Deterministic nonce selection with a "Ready ≥ 2 slots" rule**, because a nonce cannot advance twice in one slot. It also admits the residual cross-box double-buy risk instead of denying it.
- **Owner per leader window.** This respects Jito's 1 req/s/IP/region limit and keeps fee accounting clean.
- **Two non-obvious correct insights:**
  - Plain Token-2022 `Transfer` does not list the mint, so consolidation detection must watch the **bundle wallets' ATAs**, not the mint.
  - In a v1 create the classifier can run **before the trailing signatures arrive**, because keys start at byte 42.
- **The `FiredSet` cross-feed race** gives feed-versus-feed latency measurements for free.
- **Config load refuses `live`** on a zero cap, an unset CU limit, group-readable keys, stale tables or clock skew.

### Concrete flaws
1. **Look-ahead in operator membership.** `operators.py` runs union-find over all data, and then `walkforward.py` computes features "with create_time + horizon < t". The *membership* still uses links discovered after t: a later funding edge merges earlier tokens into the operator. The backtest therefore credits operators with history it could not have attributed live. It needs P2's immutable point-in-time `op_assign`.
2. **No guard against overfitting the search itself.** BH across rules and "two disjoint windows" are good. But there is no vault, no `n_configs_tried`, and no banned-feature test (the API's `ath_market_cap`, `complete` and volume sit next to D1). Per-operator exit parameters (the big-buy range, the TP ladder) have no stated out-of-sample fitting rule.
3. **The sell-fee escalation is unbounded.** "Priority ×2 on retries, at most 3 per slot" has no total cap and no max tries. `hard_cap_bid_lamports` covers buys only. If sells keep failing (program upgrade, a bound too tight), the fee doubles indefinitely.
4. **Dump exits use `min_sol_output = reserves × 0.9`.** A dump that lands in the same slot moves the price 50-80%. The sell then fails, retries, and escalates (see flaw 3). Use 0 or the floor for dump-type triggers, as P2 and P3 do.
5. **The free mode is described inconsistently.** §3.2 says the free feeds "produce a CreateHit with no in-slot information", which is correct: PumpPortal sends JSON and `logsSubscribe` sends logs. §3.9 then says that in L2 "the hot path still runs the full decode→patch→sign code… exercised daily for free". The wire decoder is **not** exercised on free feeds.
6. **No crash recovery or reconciliation, and nothing to keep exits alive if the process dies.** After a restart only the nonce slots go to `Unknown`. Open positions, the daily-loss state and in-flight intents are unspecified. Nothing sells a position while the binary crash-loops.
7. **Loose defaults:**
   - `max_entry_vsol = 70 SOL`: an 81.6% floor loss and about a 47% break-even.
   - `slip_bps = 1500`.
   - `size = 0.1 SOL`, which is in the regime where fixed costs dominate (skeptic §2).
   - Paper mode is the default, but these are the numbers that would go live.
8. **The Alpenglow parser has gaps.** It does not handle the 8-byte abort batch. It frames UpdateParent as a cutover-day concern, although FLH is ≥ Nov 9.
9. **Stale details:**
   - `rust-version 1.97.0`, and `solana-rpc-client` "excluded", although 1.97.1 is installed and pinned in the workspace.
   - Rent figures (§1).
   - `partial_fill` listed as unvalidatable, although the SDK answers it.
   - It assumes a single dev buy per create (§1: multi-signer creates exist).
10. **The crawler is heavy.** It uses full `getBlock` for s..s+3 (§1 measurement: 0.7 MB gzip per block, 429 at once). Use signature lists for in-block order plus `getTransaction` for the pump transactions.
11. **Scope and duplication.** 13 crates, including `rs-core` and `rs-decode`, which duplicate `pump-codec`. Also a home-grown merkle-shred parser with Reed-Solomon recovery, a custom tonic codec, and AF_XDP. None of the shred work can be tested against real shreds in this session (synthetic shreds only). `rs-shred` has no effort estimate.

## 3. Proposal 2: validation first

**Scores:** latency 6.5, correctness 6.5, validation 9.0, buildability 7.0, safety 6.5, cost 8.5 → **7.28**

### Strengths
- **The right unit of inference.** Certify the selection-and-sizing *policy*, pooled across operators, out of sample. Use per-operator data only for shrunken arming posteriors and sequential disarming. Its arithmetic is right (except the Bonferroni line).
- **Leak-proofing that the other two lack:**
  - point-in-time `op_assign`, immutable once written, with merges recorded and history never rewritten;
  - a banned-feature list enforced by a test;
  - purge plus a 1 h embargo, with outcome horizons closed;
  - a locked 3-day **vault** per review of a frozen `code_sha`;
  - `n_configs_tried` logged and reported;
  - a small policy grid (≤ 24).
- **The same `arming.py` runs live and in backtest.** Rust and Python share one rslog and ArmSet contract with golden fixtures, so what is tested and what runs cannot drift.
- **`min_tokens_out = tokens_out(X, v_max)`: the slippage bound *is* the EV boundary.** The hot path never estimates anything, and landing behind too much SOL fails cheaply. This is the single best idea in the three documents.
- **Hash-of-mint randomized skips plus a small-size arm.** They give an unbiased control for "what happens without us" and dose-response for farming. Nobody else measures farming causally.
- **The state machine and its statistics:**
  - CANDIDATE → SHADOW → PROBE → ARMED → SUSPENDED/RETIRED;
  - empirical-Bayes Beta priors per family and entry band;
  - SPRT disarm, a landing CUSUM, a v_max at the 20th posterior percentile, and quarter-Kelly on the lower bound.
- **Probe mode:**
  - real 0.01-0.02 SOL buys, at most 20 a day, about 0.4 SOL per 100 probes;
  - the cheapest way to replace a guessed k̂ with data.
- **An honest go/no-go gate**, with every EV labelled an *upper bound* until probes exist. The gate:
  - ≥ 300 out-of-sample snipes over ≥ 10 days;
  - block-bootstrap lower bound > 0 under E1 and base costs, and ≥ 0 under high costs;
  - vault > 0;
  - no operator above 40% of P&L.
- **Correct rent figures**, read at startup. It also settles that v1 has no ALT.

### Concrete flaws
1. **No Alpenglow handling in the hot path:** no `bank_id`, no markers, no abort handling. The shred feed is delegated to a separate `shred-ingest` workspace on Agave 4.2.2 / `sol-shred-sdk`, which speed-gap-2 shows **silently drops every marker** and cannot tell an aborted block from an empty one. This is not flagged. `regime_freeze_until_epoch = 1065` is the only Alpenglow measure.
2. **The send path is off the hot thread.** "Tokio runtime for HTTP/2 and QUIC sender connections" with reqwest means a cross-thread wake plus allocations for every variant, including the first. It is tolerable given P1's budget argument, but it is the weakest latency design.
3. **The patch call omits `associated_quote_bonding_curve`** (`tmpl.patch(mint, bonding_curve, ata(us, mint, T22), nonce, tip)`). It is a per-mint derivation (§1). Unless the program skips the check, the tx fails. The latency budget also counts one PDA, not two.
4. **Consolidation detection is blind to plain `Transfer`.** Paid mode subscribes `account_include: [bonding_curve, mint]`, and Token-2022 `Transfer` does not list the mint. Free mode (`logsSubscribe(mentions: bonding_curve)`) sees no transfers at all.
5. **Partial ladder sells retry with re-signed blockhash transactions.** Duplicates can both land when the fraction is ≤ 50%. The example ladder (0.6, then 1.0) is safe only by luck. P3 identified this; P2 did not. The `min_sol_output` rules read inverted: 0 for normal sells, 0.7× for emergency exits.
6. **The free-feed hot path cannot run as written.** `TxView::parse(ev.bytes)` on `PumpPortalFeed` and `RpcWsFeed` events: neither feed delivers wire bytes. It needs a CreateHit adapter built from the `Program data:` CreateEvent/TradeEvent logs.
7. **The leader metro comes from `stake_geo.json`** (validator city), not the advertised `tpuQuic`. About 5% of stake has its ingress in a different metro, and 68% sits behind proxies (critique §3.2).
8. **Over-claims and small errors:**
   - "duplicates are still impossible because of the shared nonce", although idx+1 probing on divergent local state can give one mint two nonces (§5);
   - the Bonferroni n (264 where 419 is right);
   - "Python has numpy" (not in the venv).
9. **The arming model is binary.** EV = p·W − (1−p)·L treats outcomes as hit or miss, but the exit policy (ladder, trailing, dump triggers) produces a continuous P&L distribution. Arm on bootstrapped realized P&L per band, not on a binary hit model.
10. **Alerts depend on Ohio and a 5-minute rsync.** A kill on the hot box may reach the user minutes later, or never if Ohio is down. A/B skips also apply in paper mode, where they only throw away samples.

## 4. Proposal 3: operations first

**Scores:** latency 7.5, correctness 7.5, validation 8.0, buildability 6.0, safety 9.0, cost 7.5 → **7.60**

### Strengths
- **The chain is the source of truth.**
  - Deterministic intent IDs, with `UNIQUE(mint, mode)`.
  - One row per signature, so P&L ingestion is exactly-once.
  - Positions come from Token-2022 balances, never from arithmetic.
  - A reconcile loop every 10 s, with alerts on drift.
- **Restart is a designed path, not an afterthought:**
  - recovery from the chain before `sd_notify(READY=1)`;
  - orphan positions get a default exit policy;
  - a ledger with deferred commits, so no fsync sits on the critical path.
- **A separate `rugsnipe-guard` with an exit lease (compare-and-set epoch).** A crash-looping hot binary never strands a position.
- **`WatchdogSec` is fed only when the hot and exit loops show progress**, so a wedged loop gets restarted, not just a dead process.
- **The program-upgrade and FeeConfig watcher.** On a change, buys go to paper, a simulation smoke test runs, and buys resume automatically only if it passes. I verified that pump was redeployed about 17 h before this review, so this is a weekly event, not a corner case. Nobody else has this.
- **Correct sell lifetimes.** A final 100% sell uses a blockhash: a duplicate fails on the balance and costs a base fee. A partial sell uses a separate nonce sub-pool with the same burn rule.
- **Daily loss is chain-derived, marked to market, and global across boxes** without coordination.
- **The kill latch:**
  - buys stop and exits continue;
  - `/kill` is the only remote command; there is no remote unkill and no remote live;
  - `live` needs a local `ctl arm-live` token tied to the ArmSet sha256 with `gate_passed`.
- **Secrets are handled correctly:** systemd `LoadCredential`, `mlockall`, `zeroize`, and a pre-commit hook that rejects keypair patterns.
- **The Alpenglow plan is the most concrete of the three:**
  - a slot-range regime freeze;
  - `(slot, bank_id)` on every feed event;
  - `include_update_parent`;
  - a post-cutover fixture capture and a re-enable gate.
- **Test design:**
  - LiteSVM execution tests, including nonce exclusivity and the burn;
  - a chaos harness with invariants I1-I6;
  - Rust↔Python exit conformance on 500 tapes.
- **It is the only proposal that maps onto the existing workspace.** It reuses `pump-codec` instead of re-creating three crates.

### Concrete flaws
1. **"Double buy: impossible by construction" is false across boxes.** The nonce index is `xxh3(mint) % N`, but the box probes idx+1 when its *local* state says busy. Two boxes with different local views can put the same mint on two different nonces, and both can land. Two cases:
   - a same-slot collision seen in a different order;
   - an intent that only one box fired.

   The fix is to drop probing (skip on collision) or make the probe depend only on chain state. Keep the per-box cap. Test I3 must cover two boxes with divergent feeds.
2. **gRPC resubscribes with `from_slot = last_slot − 32`, and there is no create-age gate.** After a process restart the signature-dedup ring is empty, so creates from the outage are re-delivered and bought about 7 s late. `v_max` caps the price damage, but every case still pays fees and uses nonces. Add `fire only if current_slot − create_slot ≤ max_age_slots`.
3. **The patch call omits `associated_quote_bonding_curve`** (same defect as P2). The wSOL question is still open in M4; the fixture answers it.
4. **TokXfer has no subscription specified.** The same plain-`Transfer`-omits-mint trap applies: subscribe the bundle wallets' ATAs.
5. **Scope.** 4 binaries, `rs-state`, `rs-chain`, LiteSVM with a 10 MiB programdata dump, a chaos runner plus `chaosproxy`, Prometheus and Alertmanager, WireGuard, and S3 backups. The milestones sum to 17.5 days. Only M0-M2 and parts of M3/M4 fit this session. Its own validation section is looser than P2's (probe arming at n_oos ≥ 30).
6. **The hot path's entry gate uses the operator's p75 bundle** (`v_exp = v_after(dev) + op.bundle_sol_p75 + …`) instead of the bundle actually observed in slot 0. P1 uses the observed bundle on pre-exec feeds. The `min_tokens_out` bound still protects the fill.
7. **Two PumpPortal connections from one IP.** Ohio runs PumpPortal in both the collector and the paper bot, but PumpPortal asks for one connection. Share a connection, or give the bot `logsSubscribe` only. P2 has the same issue.
8. **The optional remote guard needs a copy of the hot key on Ohio.** It is disclosed and off by default. Keep it off; the hot-balance cap is the right mitigation.
9. **The UpdateParent path is built for the cutover, but UpdateParent arrives only with FLH (≥ Nov 9).** The abort batch, which *does* arrive on Oct 21, is not handled.

## 5. Flaws shared by all three (fix these in the synthesis regardless of the winner)

1. **`creator_arg` matching can be spoofed to bait the bot.** All three fire when a watched key equals the create's `creator` argument. That argument is an arbitrary pubkey: anyone can launch a coin naming a watched serial wallet as creator (cost under $1, plus donating a 0.30% fee) and dump on whoever snipes it. **Live firing must require a match on a signer or fee payer. A hit on `creator_arg` alone should be shadow only.** Also exclude shared fee payers (launcher or bundler services such as `J7pour…`) from watch keys, or every launch they serve matches.
2. **Bundle-wallet identity is keyed on signers, but tokens can go to third-party owners.** Verified on a real v3 buy (`5aR7Xt…` received while `8Z2tbj…` paid). The any-bundle-wallet-sell and consolidation triggers must track the **owners of the token accounts** that received slot-0 buys (from `TradeEvent` plus ATA ownership), not just the transaction signers.
3. **The post-create curve must sum every pump buy inside the create tx** (verified 3-signer create). The legacy `buy` carries `amount` (tokens out), so V′ = k/(T − n), while `buy_exact_quote_in_*` carries SOL in. A single-"dev buy" model under-estimates V_entry exactly on the bundled launches we target.
4. **The free feed delivers no wire bytes.** The only free real-time path that yields a usable CreateHit is `logsSubscribe(mentions: TSLvdd…, processed)` → parse the `Program data:` CreateEvent and TradeEvents (the dev buy is included; separate bundle transactions are not). PumpPortal adds about 40 ms of lead but sends JSON. To exercise the wire decoder for free, run a shadow lane that fetches `getTransaction` bytes (confirmed) for every create, and run fixture replay in CI.
5. **D2 is biased by survivorship.** 232 of 349 D2 coins are earlier coins of creators who were sampled *because they were active later*. Using them in a walk-forward assumes operator persistence. **Use only D1 and the random arm for out-of-sample work.** None of the three flags this.
6. **Alpenglow, corrected:**
   - On Oct 21, markers (header and footer) and the 8-byte abort batch arrive; UpdateParent arrives only with FLH.
   - Key state on `(slot, bank_id)`.
   - Treat an abort as "cancel every pending action for that slot".
   - Poll `getAccountInfo(FLHoAW…)` each epoch.
   - Retire all tick-based position-in-slot heuristics: the reference tick stays near 0 until the footer.
7. **Data is perishable, but none of the M0 plans can run on the user's box from here** (no SSH). The only place a collector can run today is this container, for the life of the session. Ship it as a one-command systemd installer plus a `--local` mode, and start it here now.
8. **The nonce pool needs an explicit atomic state machine.** Hot fires, STATE updates and cold burns all touch the same slot. P1 writes it as plain mutable structs.
9. **Simulation strategy.** `simulateTransaction` with `sigVerify=false` can impersonate the authority of **any existing mainnet nonce account**. That lets us test legacy+nonce and v1+nonce legality today without funding a wallet. None of the three uses this; they all defer nonce legality to funded probes.

## 6. Best ideas worth grafting

### From P1 (the hot path)
- A run-to-completion pinned loop, a counting-allocator zero-alloc test, and a replay benchmark as a CI gate (p99 ≤ 100 µs on this VM).
- **PatchTable** at fixed offsets, with byte equality against `pump-rust-client` over 1,000 random mints. Use aqbc from the dev buy's account 7 when present (saves one ~10 µs derivation).
- A fixed-buffer HTTP/1.1 sender, warm sockets, and `/ping` warmers.
- Per-leader-class bid split (Jito-Agave 70/30, Harmonic priority only, BAM learned). Bid quantiles from "the minimum bid that landed ahead of all slippage-failed reactive buys".
- Routing keyed on the **advertised tpuQuic** and the leader's Jito region. One owner box per window, plus backup on heartbeat loss.
- Deterministic nonce choice with the ≥ 2-slot Ready rule, and a burn at 3 slots.
- Watch the **bundle wallets' ATAs** for consolidation, because `Transfer` omits the mint.
- `FiredSet` first-arrival accounting between feeds.
- A degradation ladder L0-L3, where L2 (free) is paper only.
- Config validation that refuses `live`.
- Policy-level walk-forward with BH across rules and ≥ 2 disjoint test windows. Regime tags.

### From P2 (the validation core)
- Point-in-time immutable `op_assign`, a banned-feature test, purge plus embargo, a vault, and `n_configs_tried`.
- **`min_tokens_out = tokens_out(X, v_max)` as the hot-path EV gate.**
- An empirical-Bayes family/band prior, the CANDIDATE…ARMED state machine, SPRT and landing-CUSUM disarm, and quarter-Kelly on the lower bound with a size cap tied to slot-0 SOL.
- Hash-of-mint randomized skips and a small-size arm, **in probe and live only**.
- Entry models E0/E1/E2 with the unfilled rule. Probe mode (≤ 20/day, 0.01-0.02 SOL) to measure real k̂ and costs.
- The rslog and ArmSet contract with golden fixtures, and `gate_passed` signed into the ArmSet.
- The go/no-go gate and the "upper bound until probed" label on every report.

### From P3 (the runtime)
- Chain-as-truth recovery, deterministic intent IDs, `UNIQUE(mint, mode)`, positions from balances, and the reconcile loop.
- A separate guard with a lease, and a keeper for blocking RPC. The watchdog is fed on loop progress.
- The program-upgrade and FeeConfig gate (buys to paper → sim smoke test → auto-resume).
- Final sells on a blockhash, partial sells on a nonce sub-pool, and a fee cap on sell retries (≤ 15 tries, tip ≤ 0.01 SOL).
- A chain-derived daily loss including mark-to-market.
- The kill latch with exits continuing and no remote unkill. Direct alerts from the hot box plus a healthchecks dead-man.
- A feed health state machine (cross-feed slot lag), sender circuit breakers, and an `rpcsend` sell fallback.
- LiteSVM execution tests (nonce exclusivity, the burn, over-selling a balance) and chaos invariants I1-I6.
- A slot-range regime freeze around Alpenglow.
- `LoadCredential`, `mlockall`, `zeroize`, and the pre-commit secret scanner.
- Reuse `pump-codec`; do not duplicate it.

## 7. Recommended synthesis and build order for this session

The skeleton is P3's processes (`hot`, `guard`, `keeper`, `ctl`; Ohio analytics). The internals are P1's hot loop, patch table and bid policy. The decision layer is P2's ArmSet, statistics and probe ladder. The §5 fixes are mandatory.

1. **Collector now, in this container** (P2 M1 / P3 M0), plus the systemd installer for Ohio. Two free sources over a single PumpPortal connection; `ingest_gaps`.
2. **First out-of-sample report** on D1 plus the D2 random arm plus fresh collector data: point-in-time operators, E1 with all in-tx buys summed, the vault, `n_configs_tried`. **This is the go/no-go artifact.** If no policy shows a positive EV lower bound, the hot path stays a paper instrument.
3. **Finish `pump-codec`** (in progress elsewhere), then add:
   - CreateHit extraction: every in-tx buy, the launcher table, the quote and mayhem classifiers;
   - the abort batch and marker parsing;
   - a `Program data:` log path for the free feed.
4. **`rs-tx` PatchTable template** (legacy, nonce, ATA, `buy_exact_quote_in_v3`, tip; v1 behind a flag), tested byte for byte against `pump-rust-client`. Then **`rugsnipe-sim`** against live curves, impersonating an existing nonce account to measure CU, loaded bytes and v1+nonce legality.
5. **`rugsnipe-hot --mode paper`** on free feeds. It must include:
   - the risk gate with a create-age gate;
   - live firing only on a signer or fee-payer match;
   - the funding watcher;
   - the recorder;
   - the zero-alloc replay benchmark.
6. Then P3's state, guard, keeper and chaos work, and only after that the paid feeds, the FRA box and probe mode (user action).

**Not in this session:** raw-shred ingest (no feed to test against), AF_XDP, the multi-box mesh, and Alpenglow mainnet fixtures (they exist only after Oct 21).

## 8. Scoring method

Each criterion is 0-10. The weights follow the brief: speed first for the execution path, correctness and honest validation first for the strategy. Latency 20%, correctness 20%, validation 20%, buildability 15%, safety 15%, cost 10%.

Deductions are for verified defects, not for style. Contradicting a verified fact (rent, layouts, the Alpenglow semantics) costs correctness. A path to unbounded loss or a false safety claim costs safety. Scope that cannot be compiled and tested against real fixtures in this session costs buildability. No proposal contradicts the lead's hard facts: none uses 400 ms slots, Raydium, 1% fees, ShredStream, solana-sdk 5 or legacy-only decoding. The correctness differences come from the finer protocol details in §1.
