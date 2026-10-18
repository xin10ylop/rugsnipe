# Alpenglow (Oct 21 2026) and validator-side options for a Frankfurt landing-race operator

Research date: 2026-10-18. Audience: single Frankfurt bare-metal box, budget a few hundred to a few thousand USD/month. All claims dated; vendor claims marked "(vendor claim)". "Scheduled" = announced by Anza/Solana Foundation; "speculative" = inference or unconfirmed.

Headline corrections to the brief's assumptions:
1. Slots are **already 200 ms** (not 400 ms) since epoch 1053 on Oct 9 2026 under SIMD-0525, a rollout separate from Alpenglow. Alpenglow does not change slot cadence.
2. Oct 21 activates **Votor only**. Rotor (the shred-propagation half) is "not yet scheduled" and needs its own SIMD; Turbine stays.
3. **Jito ShredStream was shut down on Sept 5 2026**; Jito points users to DoubleZero Edge ($1,500/month per machine in Frankfurt). "Jito ShredStream access tiers" no longer exist.
4. **Paladin is effectively dead** (two validators as of Feb 2026, website blank Oct 2026); there is no Paladin bot a trader can pay into.
5. Firedancer/Frankendancer are **not supported for the Oct 21 migration**; ~15.8% of stake on Firedancer-family clients must switch to an Agave-4.3-based build or the 82% genesis-certificate threshold is at risk.

---

## KQ1. What exactly changes on Oct 21: slot time, Votor finality, leader behaviour, Rotor/shreds, prerequisites, slip risk

### Takeaway
Oct 21 (epoch 1064, ~14:50–15:00 UTC) activates feature gate `A1pengvuM6JEcyNuTnMqepBKhwHE3N6PmUrdATGawhJS`; TowerBFT keeps running for 5,000 more slots (~18 min at 200 ms), then user transactions pause briefly while a genesis certificate (82% stake) forms, then Votor takes over at slot 459,653,000 with 100–150 ms target finality. Slot time stays 200 ms, leader windows stay 4 slots (800 ms), Turbine/shred format are unchanged because Rotor is not shipping. The main slip risk is client share: Firedancer-family stake (~15.8%) must migrate to Agave 4.3 builds.

### Cited Findings

Timing and gate
- Mainnet activation: epoch 1064, Wednesday Oct 21 2026 at approximately 15:00 UTC (Foundation page, estimate because epoch start drifts). Feature gate `A1pengvuM6JEcyNuTnMqepBKhwHE3N6PmUrdATGawhJS`; TowerBFT keeps running for the first 5,000 slots (~18 min); Alpenglow consensus takes over at slot 459,653,000 — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Anza (X post Oct 13 2026): Alpenglow "goes live on mainnet-beta Wednesday, October 21, ~14:50 UTC"; Anza has not stated whether 14:50 is the gate or the handoff; Solana Compass' arithmetic puts epoch 1064 start at ~14:55–15:10 UTC and the handoff at ~15:10–15:30 UTC (estimate) — [Solana Compass, Oct 2026](https://solanacompass.com/news/anza-sets-alpenglow-mainnet-date-solanas-consensus-switch-is-scheduled-for-october-21-at-about-1450-utc)
- Devnet activated at epoch 1167, Sept 25 2026 19:30 UTC — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Testnet: Anza said on Sept 22 2026 that Alpenglow activates on testnet "this week"; Frankendancer's testnet run ended with it — [Solana Compass, Sept 2026](https://solanacompass.com/news/alpenglow-activates-on-solana-testnet-as-frankendancer-era-ends-agave-v44-schedule-targets-november-9-mainnet-activation)
- Related SIMDs: 0326 (consensus), 0337 (fast-leader-handover markers), 0357 (Validator Admission Ticket, activated mainnet July 22 2026), 0384 (migration), 0387 (BLS pubkeys in vote account, activated mainnet July 8 2026); SIMD-0232 (custom commission collector) — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- SIMD-0326 passed governance in Aug 2025 with 98.27% approval and 52% stake participation — [Solana Compass, Aug 9 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster)

Migration procedure (what a sender experiences)
- Step sequence per Anza: (1) gate activates at epoch boundary, "nothing migrates immediately"; (2) 5,000 slots later, blocks stop carrying user transactions and hold only votes; (3) validators wait for one block past the boundary to receive votes from 82% of stake, forming a genesis certificate (Anza expects TowerBFT to produce one more block first); (4) validators roll back blocks after the genesis block, hand consensus to Votor and resume accepting user transactions; Anza expects the handoff to take "hundreds of milliseconds". SIMD-0384 says liveness impact is "optimistically only one slot past the migration boundary slot". If the certificate does not form, validators keep running TowerBFT on vote-only blocks and resend genesis votes — [Solana Compass, Oct 2026](https://solanacompass.com/news/anza-sets-alpenglow-mainnet-date-solanas-consensus-switch-is-scheduled-for-october-21-at-about-1450-utc)
- Anza describes the switch as happening in a single slot: validators roll back everything after the genesis block, hand consensus to Votor and start accepting user transactions again — [CoinMarketCap Academy citing Anza, Oct 2026](https://coinmarketcap.com/academy/article/solana-alpenglow-mainnet-oct-21-anza)

Slot time and leader window (unchanged by Alpenglow; changed by SIMD-0525)
- SIMD-0525 phases on mainnet: 350 ms Aug 21 2026 (epoch 1020, slot 440,640,000); 300 ms Aug 28 (epoch 1024); 250 ms Sept 18 (epoch 1037); 200 ms Oct 9 2026 ~14:35 UTC (epoch 1053, slot 454,896,000). Each step had its own feature gate; skip rate was the gating criterion. Leader span of four slots unchanged. Blockhash window still 150 blocks ≈ 30 s (was ~60 s). Epoch = 432,000 slots ≈ 24 h — [Solana Foundation reduced-slot-times page](https://solana.com/upgrades/reduced-slot-times)
- Foundation Alpenglow page: slot time "cut separately from 400 ms to 200 ms in four stages, completed October 2026" — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Per-block compute limits were halved so capacity stays near 250M CU/s; observed average slot times ~222 ms after the 200 ms step (news report) — [BitcoinWorld, Oct 2026](https://bitcoinworld.co.in/solana-activates-200ms-slots-doubling-block-output-ahead-of-alpenglow/); [Everstake blog](https://everstake.com/resources/blog/solana-block-time-200ms-slots)
- Helius' (2025) explainer describes Alpenglow with a fixed ~400 ms block time and Δblock ≈ 400 ms; this predates SIMD-0525 and is superseded by the 200 ms rollout — [Helius, Alpenglow explainer](https://www.helius.dev/blog/alpenglow)

Votor finality mechanics
- Fast path: ≥80% of stake notarizes in round one → Fast-Finalization Certificate, target ~100 ms. Slow path: ≥60% in round one, then ≥60% in round two → Finalized Certificate, target ~150 ms. Both paths run concurrently — [Helius, Alpenglow explainer](https://www.helius.dev/blog/alpenglow); [Solana Compass, Aug 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster)
- Votes move off-chain (peer-to-peer), aggregated into one certificate per block; vote transactions leave blocks, so transaction counts and TPS will drop; validator participation is read from block-footer certificates — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- "The 150 ms is an expectation, not a constant", measured against stake distribution at testing time — [Solana Compass, Oct 2026](https://solanacompass.com/news/anza-sets-alpenglow-mainnet-date-solanas-consensus-switch-is-scheduled-for-october-21-at-about-1450-utc)
- Community test cluster (Aug 2026): 96 validators, 96% of blocks finalized via fast path; first "Alpenswitch" May 9 2026 — [Solana Compass, Aug 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster)
- `confirmed` and `finalized` converge once Alpenglow is live; `confirmed` is removed in a later release; switch to `finalized` only after activation (before that it still means ~12.8 s). Detect activation with `getAgGenesisCert` (returns `null` on TowerBFT; older RPC nodes return error -32601); `@solana/kit` 8.3.0+ exposes it; gate on this call, not dates — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Skip votes: a validator sends a SkipVote after a timeout, missing block, or invalid block; ≥60% SkipVotes form a Skip Certificate (canonical empty slot). If the first slot of a leader's window fails, validators skip the remaining slots in that window. Timeouts per slot: now + Δtimeout + slotIndex × Δblock, never reset; Δtimeout value not published in the explainer — [Helius, Alpenglow explainer](https://www.helius.dev/blog/alpenglow)
- Leaders can begin sending the next block once they have ingested the parent, while votes are still arriving (pipelining) — [Helius, Alpenglow explainer](https://www.helius.dev/blog/alpenglow)
- Proof of History no longer paces block production; tick entries (64 per slot) leave the shred stream; `Clock` timestamp set by the leader between parent timestamp and parent + 2× elapsed slot duration — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- One slot can contain multiple banks: Geyser/gRPC consumers should buffer by `(slot, bank_id)`; `bank_id` arrives in Agave 4.3; fast leader handoff (Agave 4.4) expected to affect <1% of slots; `bank_id` is node-local, reconcile across providers on blockhash. Geyser `update_account`, `notify_transaction`, `notify_entry`, `notify_block_metadata` deprecated; add UpdateParent handling, opt into `notify_block_footer` — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- SIMD-0337 (Fast Leader Handover) markers `BlockHeader` and `UpdateParent`: a leader can start building optimistically on a parent it expects to be notarized and restart if the network skips that parent; claimed ~120 ms earlier block production start (claim from social/news, not SIMD text). Foundation says fast leader handoff ships in Agave v4.4 — [ChainCatcher](https://www.chaincatcher.com/en/article/2213471); [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Agave v4.4 tentative schedule: branch Sept 22 2026; testnet adopts Sept 28; testnet feature activation Oct 5; mainnet volunteer rollout begins Oct 19; general adoption recommended Nov 2; mainnet feature activation resumes Nov 9 2026 — [Solana Compass, Sept 2026](https://solanacompass.com/news/alpenglow-activates-on-solana-testnet-as-frankendancer-era-ends-agave-v44-schedule-targets-november-9-mainnet-activation)

Rotor / shreds
- Rotor (Phase 2) "replaces Turbine with a single relay layer. Not yet scheduled; planned for a later release after Votor is adopted" — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Helius (Agave 4.3 post): 4.3 introduces Votor and keeps Turbine; Rotor was explicitly left out of SIMD-0326 and needs its own SIMD — [Helius Agave 4.3 (es)](https://www.helius.dev/es/blog/agave-v4-3)
- Rotor design (whitepaper via Helius): single hop (~2δ) with fanout 200; leader sends each shred to a stake-weighted relay which broadcasts to all nodes (next leader first); blocks split into slices, Reed-Solomon encoded; only one erasure-coded version of each shred, no separate data/recovery shreds; leader signs a Merkle root, each shred carries Merkle path + signature; compatible with multicast systems such as DoubleZero; relay rewards unspecified — [Helius, Alpenglow explainer](https://www.helius.dev/blog/alpenglow)
- Helius shreds post: under Rotor "a validator's stake will still influence which peers it broadcasts to" — [Helius, Solana Shreds](https://www.helius.dev/blog/solana-shreds)
- Today (Turbine): unstaked nodes are always arranged at the bottom of the broadcast tree (Solana PR #20480); unstaked validators are "the last in Turbine's fanout to receive shreds" — [solana-labs PR #20480](https://github.com/solana-labs/solana/pull/20480); [Helius, Solana Shreds](https://www.helius.dev/blog/solana-shreds)

Prerequisites and slip risk
- Validators must run Agave, Jito-Solana or Harmonic Salsa built on Agave 4.3.0; Firedancer and Frankendancer are not supported during the migration; vote account needs ≥0.8 SOL above rent-exempt minimum for the VAT (0.8 SOL per epoch, burned, first charged at start of epoch 1064 for epoch 1065) — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Firedancer v26.10.0 (Oct 7 2026) and v26.10.1 (Oct 9) are testnet-only releases that add Alpenglow support but state "migration from Tower to Alpenglow will not be supported"; the validator aborts if run in Alpenglow mode on a non-Alpenglow cluster — [Firedancer v26.10.0](https://github.com/firedancer-io/firedancer/releases/tag/v26.10.0); [Firedancer v26.10.1](https://github.com/firedancer-io/firedancer/releases/tag/v26.10.1); [firedancer PR #12006](https://github.com/firedancer-io/firedancer/pull/12006)
- Solana Compass (Oct 13 2026): ~15.8% of activated stake on Firedancer-family labels; if none of it votes in the genesis step, ~84% remains, about two points above the 82% threshold; labels can lag actual switches; operators have eight days to switch — [Solana Compass, Oct 2026](https://solanacompass.com/news/anza-sets-alpenglow-mainnet-date-solanas-consensus-switch-is-scheduled-for-october-21-at-about-1450-utc)
- Agave v4.3.0 released Sept 18 2026 as the stable mainnet build — [Solana Compass](https://solanacompass.com/news/anza-issues-live-upgrade-call-agave-v43-is-the-recommended-version-for-all)
- Yellowstone gRPC v16.0.0-rc10 is a release candidate; final v16.0.0 pending; its footer message does not yet forward certificates (as of the Foundation page) — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Agave 4.3 RPC breaking changes (Triton checklist via Solana Compass): base58 encoding cannot carry v1 transactions (explicit error); `getBlock`/`getTransaction` need `maxSupportedTransactionVersion ≥ 1` and base64 encoding; Yellowstone minimum versions raised — [Solana Compass](https://solanacompass.com/news/agave-43-hits-mainnet-three-breaking-changes-for-rpc-providers-and-dapp-developers)
- Consensus set capped at 2,000 validators (mainnet has ~700); a depleted vote account drops a validator from consensus and re-entry takes multiple epochs — [Solana Compass, Aug 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster)
- Alpenglow bug bounty 50,000 SOL pool through Aug 19 2026 — [Solana Compass, Aug 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster)

### Inferences
- For a sniper, Oct 21 changes *finality*, not *propagation* or *inclusion*: the shred stream you consume today (Turbine, same shred format minus tick entries) is what you will consume on Oct 22. Only the vote transactions disappear from blocks and 64 tick entries per slot leave the stream.
- Removal of vote transactions frees block space/CU previously consumed by votes (quantity not sourced here); the per-block CU ceiling was already halved on Oct 9, so net effect on your inclusion odds is unquantified.
- The genesis-certificate step is the only hard pause for user transactions: budget for several hundred ms to a few slots of no user transactions around ~15:10–15:30 UTC on Oct 21, plus the possibility that it stretches if the 82% threshold is not reached promptly. Do not trade through the window; resume only after `getAgGenesisCert` returns non-null on your own node.
- Slip risk is real but bounded: the gate activates at the epoch boundary regardless; what can slip is the handoff (TowerBFT on vote-only blocks until 82% genesis votes). Firedancer-family stake migrating late is the identified lever; Anza would presumably coordinate a client switch rather than abandon the epoch, but no official contingency is published.
- Agave 4.4 (fast leader handover, Nov 9 target) is the next sender-relevant change: a leader may rebuild its block mid-slot on a new parent (`UpdateParent`), meaning a transaction you saw in shreds for a bank could be dropped when the bank is discarded, in <1% of slots. Decoders must key on `(slot, bank_id)`.

### Gaps
- Δtimeout (block-arrival timeout) value for mainnet not published in any fetched source.
- No published figure for how much block CU vote transactions consumed pre-Alpenglow, so the freed-capacity effect is unquantified.
- No official Anza contingency plan if the 82% genesis threshold is not reached at epoch 1064 (what happens to epoch 1065).
- Rotor has no SIMD number or target date in any source found; "2027" is speculation.

---

## KQ2. What have the clients and MEV/sender/feed vendors published about Alpenglow readiness

### Takeaway
Anza and the Foundation have published full operator and RPC checklists; Jito has a dated BAM plan (maintenance mode, Agave-based builds only, no FireBAM); Firedancer has Alpenglow code only in testnet releases and does not support the migration; Helius, Triton and the Foundation have RPC migration notes; DoubleZero, Temporal/Nozomi, 0slot, Astralane and BlockRazor have published nothing Alpenglow-specific that I could find. ShredStream is gone (Sept 5 2026) and BAM's auction cadence already moved to 20 ms for 200 ms slots.

### Cited Findings

Anza / Solana Foundation
- Operator checklist: Agave-4.3.0-based client (Agave, Jito-Solana, Harmonic Salsa), ≥0.8 SOL in vote account; RPC/dApp notes: `(slot, bank_id)` buffering, `confirmed`→`finalized`, vote-tx disappearance, tick removal, `getAgGenesisCert` — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Anza's Oct 13 2026 X announcement of Oct 21 ~14:50 UTC — [Solana Compass, Oct 2026](https://solanacompass.com/news/anza-sets-alpenglow-mainnet-date-solanas-consensus-switch-is-scheduled-for-october-21-at-about-1450-utc)
- Agave 4.1 shipped Alpenglow readiness components (BLS keys, VAT, SIMD-0337 markers, 200 ms slot prep) — [Solana Compass](https://solanacompass.com/news/agave-41-ships-alpenglow-readiness-components-and-lays-groundwork-for-200ms-slots); [Helius Agave 4.1](https://www.helius.dev/blog/agave-v4-1)

Jito (Block Engine, BAM, ShredStream)
- All mainnet BAM nodes enter Maintenance Mode "in the days leading up to activation" (no exact dates; Discord carries timing); connected validators revert to ordinary Jito-Agave Block Engine mode and keep producing blocks and accepting bundles; Maker Priority Plugin (MPP) and Direct Bundle Ingest (DBI) unavailable; DBI users use the legacy Block Engine bundle path; validators, FullSend users and standard bundle senders need no action; after activation BAM nodes return gradually — [Solana Compass, Oct 2026](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)
- Builds: Jito-Solana v4.3.0-jito.0 (Oct 2 2026) named as mainnet build; action list points to v4.3.0-jito.1 pre-release (Oct 8) as currently recommended; FireBAM and FrankenBAM must not run during the transition; FireBAM v26.09 "is not Alpenglow-ready", 26.10 or later recommended after activation; 12 FireBAM validators (7.3M SOL, 4.6% of BAM stake) must switch to Agave-based builds — [Solana Compass, Oct 2026](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)
- BAM scale: 164M SOL across 385 validators ≈ 37% of stake at epoch 1050 — [Solana Compass, Oct 2026](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)
- BAM auction cadence derived from slot length: 25 ms → 20 ms with 200 ms slots, no config change — [Solana Compass, Oct 2026](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition); [TokenPost](https://www.tokenpost.com/news/technology/31530)
- ShredStream: Jito docs state the service is deprecated and "scheduled to shut down on September 5, 2026", recommending DoubleZero Edge (free trial) and the DoubleZero migration guide; legacy regions listed Amsterdam, Dublin, Frankfurt, London, New York, Salt Lake City, Singapore, Tokyo — [Jito docs, ShredStream](https://docs.jito.wtf/lowlatencytxnfeed/)
- Jito announced the deprecation in July 2026 with 60 days' notice — [OrbitFlare migration guide](https://orbitflare.com/blog/developers/jito-shredstream-shutdown-migration-guide)
- No Jito statement on ShredStream/Alpenglow or on bundle-landing effects under Alpenglow was found — [Solana Compass, Oct 2026](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)

Firedancer / Jump
- v26.10.0 (Oct 7 2026, testnet pre-release, "not suggested for Mainnet use") adds Alpenglow support, enabled in default testnet/devnet configs; "migration from Tower to Alpenglow will not be supported". v26.10.1 (Oct 9) improves repair rate and snapshot validation for Alpenglow edge cases — [v26.10.0](https://github.com/firedancer-io/firedancer/releases/tag/v26.10.0); [v26.10.1](https://github.com/firedancer-io/firedancer/releases/tag/v26.10.1)
- Frankendancer's testnet run ended at testnet Alpenglow activation; neither Firedancer nor Frankendancer had a Votor implementation in Sept 2026 — [Solana Compass, Sept 2026](https://solanacompass.com/news/alpenglow-activates-on-solana-testnet-as-frankendancer-era-ends-agave-v44-schedule-targets-november-9-mainnet-activation)

Helius
- Alpenglow explainer (2025) and Agave 4.3 post (Votor only, Turbine kept) — [Helius Alpenglow](https://www.helius.dev/blog/alpenglow); [Helius Agave 4.3 (es)](https://www.helius.dev/es/blog/agave-v4-3)
- Products: LaserStream (gRPC, Yellowstone drop-in, regional endpoints, 48 h replay, free trial on request, price not stated), Preprocessed Transactions (up to 8 ms before `processed`), Raw Shreds via a "shred-delivery-seats" dashboard (price not public) — [Helius, Solana Shreds](https://www.helius.dev/blog/solana-shreds)
- Helius dedicated plans from $2,900/month with staked connections default on paid plans (competitor comparison page, secondary) — [RPC Fast comparison](https://rpcfast.com/blog/helius-alternatives-solana)

Triton One
- Agave 4.3 RPC migration checklist (three breaking changes) — [Solana Compass](https://solanacompass.com/news/agave-43-hits-mainnet-three-breaking-changes-for-rpc-providers-and-dapp-developers)
- Shred Streaming (with DoubleZero/Malbec Labs) live in beta from New York, London, Amsterdam, Frankfurt, Tokyo; access by contacting Triton to whitelist servers; no price given; Riptide gRPC priced same as Dragon's Mouth; "Riptide delivers first 81.8% of the time" (vendor claim, methodology not given); no Alpenglow/Rotor mention — [Triton, Before the Block (updated Aug 4 2026)](https://blog.triton.one/before-the-block-get-the-fastest-streams-on-solana/)
- SWQoS default at no extra cost; Cascade Marketplace retired Feb 28 (2026); no Alpenglow mention — [Triton, SWQoS for everyone (updated May 5 2026)](https://blog.triton.one/swqos-for-everyone-why-we-retired-cascade-marketplace/)

DoubleZero
- Edge subscriber docs give pricing, onboarding and ToS but do not mention Alpenglow or Rotor — [DoubleZero docs, Edge subscribe](https://docs.doublezero.xyz/solana/edge/subscribe/)
- Rotor design is "compatible with multicast systems such as DoubleZero" (whitepaper via Helius) — [Helius Alpenglow](https://www.helius.dev/blog/alpenglow)
- Sept 9 2026 removal of unauthorized retransmitters; Austin Federa claim of "leader shreds on DoubleZero beating all competitors by 70%+" (vendor dashboard claim); Triton May 2026 analysis said DoubleZero multicast wins ~90% of slots (vendor-adjacent) — [Solana Compass, Sept 11 2026](https://solanacompass.com/news/doublezero-removes-unauthorized-shred-retransmitters-claims-70-plus-lead-over-all-competitors)

Temporal/Nozomi, 0slot, Astralane, BlockRazor
- No Alpenglow- or 200 ms-specific statements found. Only vendor-adjacent latency data: from Amsterdam, Nozomi 0.101 ms, BlockRazor 0.107 ms, Astralane 0.124 ms, 0slot 0.561 ms (0slot hostnames resolve to shared CDN anycast IPs so true distance is unmeasurable) — [OrbitServers sender latency guide](https://orbitservers.io/blog/solana-transaction-senders-latency-guide)
- Harmonic (block-building aggregator over Jito, Temporal, JitoBAM, Paladin proposals) is named as an Alpenglow-supported client via Salsa (Agave fork) — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow); [RPC Fast client comparison](https://rpcfast.com/blog/solana-validator-client-comparison-jito-firedancer-harmonic-rakurai)

### Inferences
- Sender endpoints (Jito Block Engine, Nozomi, 0slot, Astralane, BlockRazor, Helius Sender) should be unaffected on Oct 21: the SVM, transaction format, fees and TPU/QUIC path are unchanged; the vendors' silence is consistent with "nothing to change". The exception is any sender that infers leader timing from PoH ticks or assumes 400 ms slots (already broken by Oct 9).
- BAM Maintenance Mode means that for some days around Oct 21, ~37% of stake that normally runs BAM reverts to classic Block Engine ordering. If your strategy relies on BAM-specific behaviour (e.g., 20 ms batch auctions or MPP-fed market makers), expect a temporary regime change.
- With ShredStream gone, Jito-relayer-adjacent shred access is a historical artefact; the shred market is now DoubleZero Edge, licensed resellers (Triton, Helius raw shreds, others) and unlicensed retransmitters that DoubleZero is actively cutting off.

### Gaps
- No English Helius Agave 4.3 post fetched (only es/pt-BR variants surfaced).
- Jito has not published the maintenance-mode start time or its final minimum Jito-Solana version.
- No statement from Temporal, 0slot, Astralane or BlockRazor on Alpenglow found via search.

---

## KQ3. Leader-schedule mechanics, skip-rate behaviour, and how fast inclusion is known (exit / failure-detection design)

### Takeaway
Leader schedule (stake-weighted, 4-slot windows of 800 ms) is unchanged. What changes is that a block is final ~100–150 ms after it is produced instead of ~12.8 s, and skips become explicit certificates where a failed first slot collapses the leader's whole window. A sniper should keep detecting inclusion from shreds (`processed`), treat `finalized` as arriving ~1 slot later, stop using `confirmed`, and treat a skip certificate on the target leader's first slot as "resend to the next leader now".

### Cited Findings
- Leaders still get four consecutive slots; at 200 ms a turn lasts 800 ms (was 1.6 s) — [Solana Foundation reduced-slot-times page](https://solana.com/upgrades/reduced-slot-times); [Coinpaprika](https://coinpaprika.com/news/solana-cuts-block-time-200-milliseconds-final/)
- Under Alpenglow each leader owns a four-slot window; if the first slot fails, validators skip the remaining slots in that window; skip certificate = canonical empty block, no competing fork — [Helius Alpenglow](https://www.helius.dev/blog/alpenglow)
- Leaders can send the next block once the parent is ingested while votes are still arriving — [Helius Alpenglow](https://www.helius.dev/blog/alpenglow)
- Blockhash expiry still 150 blocks ≈ 30 s at 200 ms — [Solana Foundation reduced-slot-times page](https://solana.com/upgrades/reduced-slot-times)
- Finality target ~150 ms (100 ms fast path); `confirmed` and `finalized` converge; `confirmed` removed later — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Fast leader handover (Agave 4.4, Nov 9 target): leader can build optimistically on an expected-notarized parent and restart on `UpdateParent`; <1% of slots expected — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow); [ChainCatcher](https://www.chaincatcher.com/en/article/2213471)
- Skip votes earn the same reward weight as notarization votes (may change) — [Helius Alpenglow](https://www.helius.dev/blog/alpenglow)
- Skip-rate data points: FireBAM averaged 0.05% skip rate vs 0.13% for AgaveBAM over epochs 1032–1039 — [Solana Compass, Oct 2026](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)
- "Processed" commitment is not discussed in the Alpenglow explainer — [Helius Alpenglow](https://www.helius.dev/blog/alpenglow)

### Inferences
- Inclusion detection: unchanged. You learn inclusion from the leader's shreds (processed level) within a few ms of production; that stays the fastest signal. Finality then arrives ~100–150 ms later (≈ next slot), versus 2/3-vote "confirmed" after 1–2 slots today. Exit logic that waited for `confirmed` before selling can wait for `finalized` instead with little added delay, and rollback risk on a finalized fill drops to near zero.
- Failure detection gets a new hard signal: a Skip Certificate for the target leader's first slot means that leader's remaining 3 slots (600 ms) will not contain your transaction; re-route immediately to the next leader rather than waiting out the window. Today's analogue (missing shreds / fork) is softer and slower.
- Leader-window economics tighten: 800 ms windows mean a transaction that misses the current leader lands with the next leader at most ~0.8 s later (Gulf Stream forwarding is unchanged). 30 s blockhash expiry means fewer retries per blockhash than before.
- Ticks are gone: any sub-slot timing or "where in the slot are we" heuristic that read tick entries must switch to a local clock keyed to first-shred arrival.
- `confirmed` must be removed from code now (it is slated for removal); before activation it still means ~12.8 s, so gate on `getAgGenesisCert`.

### Gaps
- No published measurement of mainnet skip rate before/after Alpenglow (activation has not happened); community-cluster data is 96% fast-path but no skip-rate figure.
- No source on how `processed`-level RPC/Geyser notifications behave during the 5,000-slot vote-only phase or the rollback.

---

## KQ4. SWQoS leases / "staked connections as a service": who sells, prices, lead times, documented benefit

### Takeaway
There is no open marketplace for leasing stake-weighted bandwidth in 2026: Triton killed Cascade (Feb 2026) and gives SWQoS to all customers free; what remains are RPC vendors bundling SWQoS into plans (Everstake $500–1,300/mo, ERPC Frankfurt shared endpoint tied to elSOL holdings or +€100/mo for a 1,000 SOL allocation on a dedicated node, Helius/QuickNode/RPC Fast on paid plans). The technical basis is the validator's `--staked-nodes-overrides` YAML ("virtual stake"), so a private deal with a Frankfurt validator is possible but no public price exists. No vendor publishes a measured landing-rate uplift.

### Cited Findings
- Mechanism: validator loads a YAML mapping RPC identity pubkey → virtual stake in lamports (`staked_map_id`); listed RPCs get prioritized QUIC connections; the RPC forwards with `--rpc-send-transaction-tpu-peer HOST:PORT`; SWQoS gives stake-weighted priority to 80% of a leader's TPU capacity; feature since v1.14; the guide says RPCs should not be staked — [Solana docs, Stake-weighted QoS](https://solana.com/developers/guides/advanced/stake-weighted-qos)
- Agave splits bandwidth between 2,000 staked connections and 500 unstaked; the unstaked pool is shared at 200 TPS across up to 8 connections; at p50 latency staked and unstaked perform similarly (no numbers); under Solana 4.0 (planned March 2026) SWQoS becomes a congestion-only backstop that activates when a block producer's TPU reaches 90% capacity, replacing the static 80/20 split — [Triton, SWQoS for everyone (updated May 5 2026)](https://blog.triton.one/swqos-for-everyone-why-we-retired-cascade-marketplace/)
- Triton: Cascade Marketplace retired, all standing bids cancelled Feb 28; SWQoS default for every Triton customer at no extra cost; former Cascade users keep their landing rates (no figures) — [Triton, SWQoS for everyone](https://blog.triton.one/swqos-for-everyone-why-we-retired-cascade-marketplace/)
- Everstake SWQoS: Pay-As-You-Go (RPC, 0.0005 SOL tip per tx, no subscription), Base $500/mo (QUIC), Pro $1,300/mo (QUIC, custom latency tuning, dedicated infra), Custom on request; stake behind it "huge stake volume" (unquantified); TPS "custom"; regions "multiple locations worldwide" (Frankfurt not named); no measured landing improvement; no lead time or contract terms (vendor page) — [Everstake SWQoS](https://everstake.com/products/swqos)
- ERPC: shared SWQoS endpoint in Frankfurt; 1 TPS per 4.2 elSOL held (10 elSOL → 2 TPS), capped by plan maximum; dedicated RPC can add a 1,000 SOL staking allocation for +€100/month; plans Developer €42/mo, Business €298/mo, Pro €598/mo (discounted from €98/€698/€1,280); same-region placement advised; no measured landing uplift; no lead time stated (vendor page) — [ERPC staked connection](https://erpc.global/en/staked-connection/)
- RPC Fast: staked connections included; custom dedicated from $2,200 (vendor comparison) — [RPC Fast](https://rpcfast.com/blog/helius-alternatives-solana)
- Helius: staked connections default on paid plans; dedicated from $2,900/mo (competitor comparison, secondary) — [RPC Fast](https://rpcfast.com/blog/helius-alternatives-solana)
- QuickNode routes via its own staked validator connections when priority fees are at recommended or higher; Ankr SWQoS in premium tiers (comparison blogs) — [Chainstack comparison](https://chainstack.com/learn/compare/solana-rpc-hft-market-making-providers-2026/)
- A guide argues the "lease SOL for priority" framing is misleading: client-side flags cannot enable it and delegating SOL to an RPC is not the model; virtual stake exists only when both validator and RPC are configured for the peering — [RPC Edge, SWQoS explained](https://rpcedge.com/blog/stake-weighted-qos-explained)
- One blog estimates a minimum of ~5,000 SOL to be competitive running your own staked validator for SWQoS (single blog estimate) — [RPC Fast](https://rpcfast.com/blog/helius-alternatives-solana)

### Inferences
- If Triton's description of Agave 4.0 behaviour holds on mainnet (SWQoS only kicks in at 90% TPU capacity), a lease buys insurance against congestion spikes (exactly the moments a launch-sniper cares about), not a steady-state latency edge; at p50 staked and unstaked are reported similar.
- A direct validator deal is feasible technically (one YAML line on the validator) but requires trust: the validator exposes its TPU prioritisation to you; expect to approach Frankfurt operators privately. No public rate card exists; the closest priced proxy is ERPC's +€100/mo per 1,000 SOL allocation.
- For a few-hundred-USD budget, the cheapest "staked" path is a Frankfurt-local SWQoS RPC (ERPC shared endpoint, or any Frankfurt-hosted vendor with staked connections) rather than a bespoke lease.

### Gaps
- No independent, measured landing-rate uplift from SWQoS in 2026 found; every figure is a vendor claim or unquantified.
- No confirmation in Agave release notes (only Triton's FAQ) that the 90%-capacity SWQoS backstop shipped as described in 4.0.
- No public price for a private `staked-nodes-overrides` entry from an individual validator.

---

## KQ5. Running a node in Frankfurt (unstaked/lightly staked, voting or non-voting): cost, lead time, hardware, and whether it beats a paid shred feed

### Takeaway
A non-voting Frankfurt node costs roughly €400–1,500/month in bare metal (Agave: 12c/24t 2.8 GHz minimum, 24+ cores 3.5 GHz+ and 256–512 GB RAM recommended) and no vote fees or VAT, but as an unstaked node it sits at the bottom of Turbine and receives shreds last; a voting validator additionally burns 0.8 SOL/epoch (≈0.8 SOL/day at 24 h epochs) plus needs millions of SOL to matter in SWQoS or tree position. Paid multicast feeds (DoubleZero Edge $1,500/mo Frankfurt) are designed to beat Turbine's edge; competitors' benchmarks at TeraSwitch FRA2 show DoubleZero at p50 1.69 ms and Helius raw shreds at p50 2.31 ms (competitor measurements).

### Cited Findings
Hardware
- Solana HCL Agave minimum: 2.8 GHz, 12 cores/24 threads, AVX2 and SHA extensions; higher clock preferred over more cores — [Solana HCL, Agave](https://www.solanahcl.org/category/agave)
- Guides converge on 24+ cores at ≥3.5 GHz; Agave 256 GB ECC minimum, 512 GB recommended; Firedancer 384–512 GB — [Everstake hardware vs cloud 2026](https://everstake.one/resources/blog/solana-validator-node-hardware-vs-cloud-2026); [The Good Shell](https://thegoodshell.com/solana-validator-requirements/)
Cost
- Bare-metal estimates: $800–1,000/mo entry Agave (24-core EPYC); $300–500 budget / $800–1,500+ premium DCs; $400–1,200+/mo (secondary guides) — [Cherry Servers](https://www.cherryservers.com/blog/solana-node-cost); [Hivelocity](https://www.hivelocity.net/kb/solana-validator-infrastructure/); [The Good Shell](https://thegoodshell.com/solana-validator-cost/)
- ERPC Frankfurt bare metal: Ryzen 9950X "Metal Premium APP++" €320→€390/mo (Oct 2025, citing DDR5/NVMe cost); ~2 month lead time in Frankfurt; Ryzen 7950X tier out of stock; ShredStream-focused Ryzen Premium €599/mo (Aug 2025); new pricing from Feb 2026 not published in the notices — [ERPC price adjustment Oct 2025](https://erpc.global/en/news/2025/10/17/erpc-premium-metal-appplusplus-price-update/); [ERPC FRA restock Jan 2026](https://erpc.global/en/news/2026/01/31/erpc-fra-baremetal-restock-202601/); [SLV restock Aug 2025](https://slv.dev/en/news/2025/08/20/erpc-frankfurt-premium-restock/)
- TeraSwitch is the largest provider by stake (~124M SOL) — [Solana Compass on Fidelity FCAT, Oct 17 2026](https://solanacompass.com/news/fidelitys-fcat-finds-solana-more-reliable-with-stake-hosting-and-client-software-still-concentrated); no public FRA2 price list found.
Voting costs
- Pre-Alpenglow vote fees ~1.1 SOL/day; ~2.16 SOL/day during the 200 ms transition; post-Alpenglow VAT ~0.8 SOL per epoch charged in advance from the vote account — [Solana Compass, Aug 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster); [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- Consensus set capped at top 2,000 validators by stake — [Solana Compass, Aug 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster)
Shred receipt as an unstaked node
- Unstaked nodes always placed at the bottom of the Turbine tree (PR #20480) — [solana-labs PR #20480](https://github.com/solana-labs/solana/pull/20480)
- "Unstaked validators are the last in Turbine's fanout to receive shreds" (vendor with a feed to sell) — [Helius, Solana Shreds](https://www.helius.dev/blog/solana-shreds)
- Vendor estimate: a high-stake validator may see a shred 50–200 ms before a node at the edge of the tree (no methodology) — [OrbitFlare, Shreds explained](https://orbitflare.com/blog/fundamentals/shredstream)
- Repair fallback exists but adds a request round trip after gap detection — [RPC Edge, Turbine](https://rpcedge.com/blog/solana-turbine-block-propagation)
Paid feed alternatives measured at Frankfurt
- Shreder benchmarks at TeraSwitch FRA2 (competitor-run, dates not given): DoubleZero p50/p95/p99 1.69/15.06/24.45 ms with Shreder first 81.48% of 100k txs; Helius Raw Shred Delivery p50 2.31 ms, p95 12.7 ms vs Shreder 78 µs/563 µs; Jito ShredStream (pre-shutdown) p50 4.61 ms; Helius LaserStream p50 2.92 ms; Triton Geyser p50 160.5 ms; QuickNode Geyser p50 20.6 ms; Alchemy Geyser p50 19.55 ms — [Shreder benchmarks](https://shreder.xyz/benchmarks/)
- DoubleZero Edge: $1,500/month per machine in Frankfurt or Amsterdam; $900 London/New York/Singapore/Tokyo; $450 elsewhere; apply at doublezero.xyz/edge/subscribe, contact within 1–3 business days, pay invoice, run `doublezero connect multicast --subscribe-feed solana-shreds-full`; requires GRE, BGP (TCP 179), PIM, UDP 7733 inbound; shreds arrive GRE-encapsulated; ToS 2(e): data for internal use only, may not be retransmitted — [DoubleZero docs, Edge subscribe](https://docs.doublezero.xyz/solana/edge/subscribe/)
- Earlier per-epoch pricing ($100/$60/$30 USDC per epoch) is superseded — [OrbitFlare migration guide](https://orbitflare.com/blog/developers/jito-shredstream-shutdown-migration-guide); [DoubleZero docs](https://docs.doublezero.xyz/solana/edge/subscribe/)
- Kairos Research (April 2026): DoubleZero Edge delivering shreds first on ~93% of slots from DZ Edge leaders (vendor-adjacent) — [Kairos Research on X](https://x.com/Kairos_Res/status/2045584160077889866)
- DefiLlama: Edge subscription revenue $236.72K in Q2 2026, $168.11K in Q3 2026 — [DefiLlama DoubleZero](https://defillama.com/protocol/doublezero)
- Helius: running your own Yellowstone node means hardware, config, maintenance, single point of failure, no replay; LaserStream pitched as not requiring millions in stake (vendor) — [Helius, Solana Shreds](https://www.helius.dev/blog/solana-shreds)

### Inferences
- An unstaked non-voting Frankfurt node is a *worse* shred receiver than a licensed multicast feed in the same facility: Turbine places it last, whereas DoubleZero injects at the PoP nearest the leader and multicasts. The node still has value as (a) a private TPU/QUIC submission point with your own leader-schedule tracking, (b) a local RPC for simulation and state reads, and (c) a repair-backed fallback feed.
- Lightly staking a validator does not fix this: tree position and SWQoS share are stake-proportional, and a few hundred SOL is noise against ~443M SOL active stake (epoch 1050 figure). The VAT (0.8 SOL/epoch ≈ $100+/day at 2026 prices, not sourced) plus hardware exceeds the brief's budget for no measurable landing benefit.
- Lead time: 1–3 business days for DoubleZero Edge onboarding vs. ~2 months for a popular Frankfurt bare-metal SKU (ERPC Oct 2025 figure); hardware procurement is the binding constraint, not feed access.
- Rotor (unscheduled) would not obviously rescue the unstaked node: relays are stake-weighted and Helius expects stake to still shape broadcast.

### Gaps
- No TeraSwitch FRA2, Equinix FR5 or Interxion price lists found; no 2026 Frankfurt lead-time data other than ERPC.
- No independent (non-vendor, non-competitor) measurement of unstaked-node Turbine latency vs DoubleZero/Helius in Frankfurt.
- DoubleZero Edge eligibility: docs do not say whether the host must be in a DoubleZero-connected facility or which Frankfurt sites are on-net; the topology map was not fetched.
- Helius raw-shred seat price not public.

---

## KQ6. Geography of stake-weighted leader slots in late 2026 and whether Alpenglow moves the optimum

### Takeaway
Frankfurt alone produced ~35% of leader slots in the Sept 2026 snapshot (Germany 36.1%, Europe 72.9%); Frankfurt + Amsterdam produced 52% of blocks and held 53% of stake across 310 of ~675 validators (Validators Solutions/Glassnode, early Sept), while Fidelity's Oct 16 report (Sonda IP geolocation) puts FRA+AMS at ~40% of stake and the largest ASN at ~29%. Frankfurt is the right place; Votor does not change propagation, and Rotor is unscheduled.

### Cited Findings
- Latest snapshot (article Sept 9 2026): Europe 72.9% of leader slots; Germany 36.1%; Frankfurt 35.3%; Frankfurt + Amsterdam 52% of block production. July 2026 comparison: Europe 67–68.5%, Germany 26.7%, Frankfurt 25.9%. Stake (Validators Solutions, early Sept 2026): Germany 34.2% across 177 validators; Frankfurt 28.3% across 110 validators; FRA+AMS 53% across ~310 of ~675 validators. Latency: ~72 ms Frankfurt vs ~140 ms US East (earlier snapshots) — [Crypto Briefing, Sept 9 2026](https://cryptobriefing.com/europe-solana-leader-slots-germany-dominance/)
- Glassnode: Frankfurt 33%, Amsterdam 19% of blocks; 310 of 675 validators in the two cities, 46% by count, 53% by stake — [Glassnode on X](https://x.com/glassnode/status/2097611752615952407)
- Fidelity FCAT "The State of Solana" (Oct 16 2026, Sonda Q3 data, IP geolocation caveat): ~59% of stake in Germany + Netherlands + US; ~40% in Frankfurt + Amsterdam; largest ASN ~29% (validators.app Oct 2026) vs Foundation Delegation Program ceilings of 25% per ASN and 15% per data center; providers by stake: TeraSwitch ~124M SOL, Cherry Servers ~62M, Latitude ~53M; 5 of the top 10 validators by stake are in Frankfurt (Solana Compass, Oct 17 2026); Nakamoto coefficient 18 (unchanged since epoch 1012); 678 active validators (epoch 1060) — [Solana Compass on FCAT, Oct 17 2026](https://solanacompass.com/news/fidelitys-fcat-finds-solana-more-reliable-with-stake-hosting-and-client-software-still-concentrated)
- Client share of stake (Sonda via FCAT): Jito BAM 36.2%, Jito standard 26.9%, Harmonic variants ~24%, Firedancer variants ~15%, plain Agave 1.2% — [Solana Compass on FCAT](https://solanacompass.com/news/fidelitys-fcat-finds-solana-more-reliable-with-stake-hosting-and-client-software-still-concentrated)
- Active stake at epoch 1050: 442.9M SOL — [Solana Compass, Oct 2026](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)
- Shred feeds are offered in Frankfurt by DoubleZero Edge, Triton Shred Streaming, Shreder (TeraSwitch FRA2 benchmarks) and ERPC's shared SWQoS endpoint — [DoubleZero docs](https://docs.doublezero.xyz/solana/edge/subscribe/); [Triton](https://blog.triton.one/before-the-block-get-the-fastest-streams-on-solana/); [Shreder](https://shreder.xyz/benchmarks/); [ERPC](https://erpc.global/en/staked-connection/)
- Rotor is "compatible with multicast systems such as DoubleZero" and selects relays by stake-weighted sampling — [Helius Alpenglow](https://www.helius.dev/blog/alpenglow)

### Inferences
- The 40% vs 53% discrepancy is methodological (IP geolocation and date), not a trend reversal; by leader slots, Frankfurt's share rose from ~26% (July) to ~35% (Sept 2026). A Frankfurt box is co-located with roughly a third to a half of all leader slots.
- Which Frankfurt facility matters less than being on a short path to TeraSwitch (largest stake provider) and to DoubleZero's Frankfurt PoP; Shreder and DoubleZero both benchmark at TeraSwitch FRA2, which suggests that is where the latency-sensitive crowd sits. Equinix FR5 / Interxion share is not quantified in any source found.
- Votor does not move the optimum: block propagation is unchanged on Oct 21. Rotor, when it ships, concentrates relaying in high-stake nodes (FRA/AMS) and is multicast-friendly, which if anything strengthens Frankfurt/Amsterdam as the place to receive shreds.

### Gaps
- No London, Tokyo, Singapore or US-regional leader-slot percentages found for Sept/Oct 2026; sources only give Europe/Germany/Frankfurt/Amsterdam.
- No per-facility (FRA2 vs FR5 vs Interxion) stake breakdown found; validators.app/stakewiz data-center tables were not fetched directly.

---

## KQ7. Paladin / Harmonic and other validator-side MEV: can a trader pay for inclusion, and at what cost

### Takeaway
No. Paladin is effectively defunct (last client release Dec 7 2025, two validators as of Feb 2026, website blank Oct 4 2026), so the P3 paid lane is not a live option. Harmonic (~18–24% of stake/scheduling) is a validator-side block-building aggregator whose inputs are Jito, Temporal, JitoBAM and Paladin proposals; it offers no documented direct trader lane or price in the sources found.

### Cited Findings
- Paladin: last client release v3.1.14 on Dec 7 2025; BlockRazor review (Feb 2026) found two validators, website down, X ceased; Oct 4 2026 paladin.one blank and docs.paladin.one not resolving; no formal shutdown notice; P3 described as a direct lane to the leader for high-fee trades with a minimum fee (amount not given); validators were to lock PAL to enable P3 — [Solana Compass Paladin project page](https://solanacompass.com/projects/paladin)
- Paladin economics (2025, stale): Palidators keep 90% of bot MEV, 10% to PAL (7.5% validators / 7.5% stakers); bot runs only when the validator is leader; P3 was a UDP endpoint with a whitelist used by bloXroute — [Chorus One](https://chorus.one/reports-research/paladins-quest-for-fair-mev); [buffalu analysis](https://buffalu.substack.com/p/understanding-paladin-an-analysis)
- Harmonic: open block-building aggregator collecting proposals from Jito, Temporal, JitoBAM and Paladin; April 2026 upgrade added streaming block delivery, configurable scheduling strategies, Salsa (Agave) and Samba (Firedancer) clients; Syndica April 2026: Harmonic 18% of scheduling vs Agave-Jito 36% and JitoBAM 28%; Jan 2026: Harmonic 15–20% above typical validators on priority fees — [RPC Fast client comparison](https://rpcfast.com/blog/solana-validator-client-comparison-jito-firedancer-harmonic-rakurai); [Syndica April 2026](https://blog.syndica.io/deep-dive-solana-onchain-activity-april-2026/); [Syndica Jan 2026](https://blog.syndica.io/deep-dive-solana-onchain-activity-january-2026/)
- Harmonic Salsa is an Alpenglow-supported client for Oct 21 — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow)
- FCAT: Harmonic variants ~24% of stake — [Solana Compass on FCAT](https://solanacompass.com/news/fidelitys-fcat-finds-solana-more-reliable-with-stake-hosting-and-client-software-still-concentrated)

### Inferences
- The trader-facing way to reach Harmonic validators is through the builders it aggregates (Jito Block Engine bundles, Temporal/Nozomi), i.e., the sender subscriptions the operator already has; there is no separate Harmonic fee lane to buy.
- "Validator-side MEV access" in 2026 therefore collapses to: Jito bundles/BAM (37% of stake; MPP/DBI are for market makers and bundle submitters, not snipers), Temporal, and ordinary priority fees.

### Gaps
- No Harmonic documentation on direct order flow, pricing or whitelist found (docs.harmonic.gg not fetched).
- No confirmation whether Paladin's P3 port is still accepting traffic on its two remaining validators.

---

## KQ8. Ranking validator-side options against buying a shred feed plus sender subscriptions (synthesis for the operator)

### Takeaway
For a few-hundred-to-few-thousand USD/month Frankfurt operator, the only validator-side items with a measurable, purchasable edge are a licensed multicast shred feed (DoubleZero Edge $1,500/mo, or Triton/Helius/Shreder resold feeds) and Frankfurt-local SWQoS-backed submission (bundled RPC plans from ~€100–$1,300/mo). Running a node, staking, leasing stake privately, Paladin and Jito relayer tiers either do not exist any more, cost more than the budget for no documented landing benefit, or are dominated by the paid feed.

### Cited Findings
- DoubleZero Edge Frankfurt $1,500/mo/machine, 1–3 business day onboarding, internal-use-only ToS — [DoubleZero docs](https://docs.doublezero.xyz/solana/edge/subscribe/)
- Competitor benchmark at FRA2: DoubleZero p50 1.69 ms, Helius raw shreds p50 2.31 ms, Jito ShredStream (defunct) p50 4.61 ms — [Shreder benchmarks](https://shreder.xyz/benchmarks/)
- SWQoS plans: Everstake $500/$1,300; ERPC Frankfurt shared endpoint via elSOL or +€100/mo per 1,000 SOL on dedicated; Triton free with service; Helius dedicated from $2,900 — [Everstake](https://everstake.com/products/swqos); [ERPC](https://erpc.global/en/staked-connection/); [Triton](https://blog.triton.one/swqos-for-everyone-why-we-retired-cascade-marketplace/); [RPC Fast](https://rpcfast.com/blog/helius-alternatives-solana)
- Node hardware €390–€1,500/mo, ~2 month Frankfurt lead time (ERPC), unstaked nodes last in Turbine — [ERPC Oct 2025](https://erpc.global/en/news/2025/10/17/erpc-premium-metal-appplusplus-price-update/); [PR #20480](https://github.com/solana-labs/solana/pull/20480)
- Voting validator: 0.8 SOL/epoch VAT plus hardware, consensus set capped at 2,000 — [Solana Foundation upgrade page](https://solana.com/upgrades/alpenglow); [Solana Compass Aug 2026](https://solanacompass.com/news/what-solanas-alpenglow-upgrade-changes-validator-costs-finality-and-96-fast-path-finalization-from-the-test-cluster)
- Paladin defunct; ShredStream shut down Sept 5 2026 — [Solana Compass Paladin](https://solanacompass.com/projects/paladin); [Jito docs](https://docs.jito.wtf/lowlatencytxnfeed/)

### Inferences (ranking, highest expected benefit per USD first)
1. Licensed multicast shred feed in Frankfurt (DoubleZero Edge $1,500/mo, or a cheaper reseller such as Triton Shred Streaming/Helius raw shreds/Shreder, prices on request): the only item with repeated (if vendor/competitor) measurements showing single-digit-ms p50 shred arrival at FRA2. Lead time days. Legal: internal use only, no retransmission.
2. Frankfurt-local SWQoS-backed sender/RPC (ERPC shared endpoint, Everstake Base, or staked connections bundled with an existing vendor): low cost, insurance for congestion moments; no measured uplift published; benefit may be mostly during TPU saturation if the Agave 4.0 backstop description is accurate.
3. Own non-voting Frankfurt node (€400–1,500/mo, months of lead time for popular SKUs): worth it for private TPU/QUIC submission, local simulation and as a fallback feed, not as a primary shred source.
4. Private `staked-nodes-overrides` lease from a Frankfurt validator: technically trivial, no public market or price, trust-dependent; only pursue if a relationship exists.
5. Own voting validator with meaningful stake: outside budget (VAT 0.8 SOL/epoch, hardware, millions of SOL to move tree position) and no landing benefit at small stake.
6. Paladin/P3, Jito ShredStream tiers: not available in Oct 2026.
- Oct 21 itself does not reorder this list; it changes finality (exit logic) and briefly pauses user transactions. Reassess only when Rotor gets a SIMD/date (unscheduled) and after Agave 4.4 (Nov 9 target) introduces mid-slot `UpdateParent` restarts.

### Gaps
- No head-to-head, independent measurement of landing rate (not shred latency) across these options exists in the sources found; the ranking rests on mechanism and vendor/competitor latency data.
- Reseller shred-feed prices (Triton, Helius, Shreder) are quote-only; the $1,500 DoubleZero figure is the only public Frankfurt price.
