# Speed gap 2: Alpenglow cutover at the shred, entry and leader level

**Question.** What exactly changes at the shred, entry and leader level at Alpenglow activation (epoch 1064, about Oct 21; Alpenglow consensus from slot 459,653,000)? Specifically:
- How often do `BlockComponent` UpdateParent markers occur?
- Are transactions in a cleared bank re-executed in the replacement bank, or dropped?
- Does the leader window stay at 4 slots?
- Do yellowstone-grpc 14.0.1/15.0.0, agave-transaction-view 7.0.0 and sol-shred-sdk 4.0.4 handle markers?
- What TPU do BAM validators advertise in pre-activation maintenance mode, and when does BAM return?
- Where does the ~15.7% Firedancer-family stake go?

Method: Agave 4.3.1 crate source (crates.io tarballs from 2026-10-13: solana-entry, solana-ledger, solana-core, solana-poh, solana-turbine, agave-votor, agave-votor-messages, agave-feature-set). jito-solana `v4.3.0-jito.1` source via raw.githubusercontent. On-chain feature accounts and leader schedules on mainnet, devnet and testnet. A Rust decode test of real marker bytes. Measurements taken 2026-10-14, 08:57–09:30 UTC.

---

## 0. TL;DR (what changes the build plan)

1. **No UpdateParent markers will appear on mainnet on Oct 21.** VERIFIED.
   - UpdateParent is emitted only on the *sad path of fast leader handover (FLH)*.
   - FLH sits behind a separate feature gate, `alpenglow_fast_leader_handover` = `FLHoAWBDjNh6zwmJ5i1NKK4KyD8otAiv7XxvmnFnVnKH`.
   - That gate is **not present on mainnet, devnet or testnet** today (`getAccountInfo` returns null on all three).
   - solana.com attributes FLH to Agave **v4.4**. The v4.4 schedule puts mainnet feature activation at **Nov 9 at the earliest**.
   - So the "markers on devnet since Sep 25" premise holds only for **BlockHeader/BlockFooter** markers. Devnet has never produced an UpdateParent.
2. **What does arrive on Oct 21 (from slot ≈459,653,000) is two markers per block.** VERIFIED (source).
   - A `BlockHeader` is the first component of every Alpenglow block.
   - A `BlockFooter` is the last.
   - A leader that abandons a slot sends a single "empty entry batch" (8 zero bytes). That batch now formally means "block aborted".
   - Ticks shrink to **one "alpentick" per slot**, and PoH goes to "low power mode".
3. **sol-shred-sdk 4.0.4 silently drops every marker.** It also cannot tell an aborted block from an empty one. VERIFIED (code path; see §4 for the byte test).
   - Transactions are **not** lost: entry batches still decode.
   - We lose: parent slot and block id (header), bank hash and producer time (footer), and the abort signal.
   - We would lose UpdateParent once FLH ships.
4. **When FLH ships, transactions in a cleared bank are re-injected into the leader's banking stage and re-executed in the replacement bank.** VERIFIED (`block_creation_loop.rs::handle_parent_ready`).
   - This is best effort ("this send can evict… may drop a few packets").
   - The pre-marker transactions **remain in the slot's shreds**.
   - A raw-shred consumer will therefore see the same signature **twice in one slot**: once before the marker (invalid) and possibly once after (valid).
   - UpdateParent can appear only in the **first slot of a leader window** and only at a **FEC-set boundary**.
5. **The leader window stays 4 slots, aligned to multiples of 4.** VERIFIED.
   - `NUM_CONSECUTIVE_LEADER_SLOTS = 4`, and Votor's `parent_ready_tracker` uses it.
   - Devnet's Alpenglow leader schedule has every run length a multiple of 4: 65,501 four-slot runs, plus consecutive repeats.
6. **BAM maintenance means the validator advertises its relayer TPU, or else its own TPU.** LIKELY, inferred from the jito-solana code.
   - The validator's gossip client id also flips from **AgaveBam to JitoLabs**, which gives a detectable switch.
   - Today (09:01 UTC) maintenance has **not** started: all 36.2% AgaveBam stake still advertises BAM-node TPUs.
   - BAM returns "gradually… after activation", with no date given.
7. **The Firedancer family (15.75%) moves to Agave-based clients for the migration, then returns.**
   - Firedancer **v26.10.2 (mainnet release, published Oct 14)** supports Alpenglow, but "Migration from Tower to Alpenglow will not be supported".
   - Jito says FireBAM should be on 26.10+ "after activation".
   - Testnet, which is post-Alpenglow, already has **9.45% of stake on Firedancer 26.x**. VERIFIED.

---

## 1. Wire format after activation (VERIFIED, `solana-entry 4.3.1/src/block_component.rs`)

Every *completed data set* is one `BlockComponent`. The ledger comment says "one range in CompletedRanges corresponds to one BlockComponent". There are two shapes:

| Shape | Bytes |
|---|---|
| Entry batch | `u64 count (≠0)` + `count` × bincode `Entry` (identical to the pre-Alpenglow `Vec<Entry>`) |
| Marker | `u64 0` + `u16 marker version (=1)` + `u8 variant` + `u16 byte length` + variant data |
| Abort | exactly `u64 0` (8 bytes) → `BlockstoreError::BlockAborted` |

`BlockMarkerV1` variants, by u8 tag in declaration order:
- `0` **BlockFooter**: `VersionedBlockFooter::V1` (tag 1). Fields: `bank_hash`, `block_producer_time_nanos u64`, user agent (≤255 B), optional finalization cert, skip-reward cert and notar-reward cert.
- `1` **BlockHeader**: `parent_slot u64` + `parent_block_id [32]`.
- `2` **UpdateParent**: `new_parent_slot u64` + `new_parent_block_id [32]`.
- `3` **GenesisCertificate**: the one-time migration cert.

Where they appear:
- **Header.** It goes in front of the first entry batch of every Alpenglow block. `solana-turbine 4.3.1 standard_broadcast_run.rs:424`, gated by `migration_status.should_allow_block_markers(slot)`.
- **Gating.** That gate is `is_alpenglow_block(slot)`, so TowerBFT blocks (including the first 5,000 slots of epoch 1064) carry **no** markers (`agave-votor-messages 4.3.1 migration.rs:293`).
- **Footer.** It is flushed with the single per-slot tick (`PohRecorder::tick_alpenglow(max_tick_height, footer)`). The leader sets `tick_height = max-1` and then writes **one** tick (`block_creation_loop.rs`, "Alpentick, produce the footer"). Any heuristic that infers "position within slot" from tick entries or from the shred `reference_tick` **breaks**: the reference tick stays near 0 until the end of the block (LIKELY, derived from `reference_tick = last_tick_height + ticks_per_slot − max_tick_height`).
- **UpdateParent.** It always starts at **data shred 0 of a FEC set** (`Blockstore::maybe_parse_update_parent`: "look at the 0th shred of the NEXT FEC set"). `SlotMeta.replay_fec_set_index` records it, and `validate_update_parent_slot` rejects it unless `leader_slot_index(slot) == 0`.
- **Abort.** A leader whose slot is interrupted (for example when the window moves on) broadcasts an empty entry batch as last-in-slot (`standard_broadcast_run.rs::finish_prev_slot`, test comment at line 1041).

**RPC does not expose markers.** `getBlock` on devnet slot 510,934,032 returned only the usual fields. Blockstore *strips* markers when building `VersionedConfirmedBlock` (`blockstore.rs:4244`). Only Geyser plugins and raw shreds see them.

---

## 2. UpdateParent semantics (VERIFIED, `solana-core 4.3.1`)

### 2.1 When it is emitted
**FLH.** When replay freezes the previous leader's last block *before* Votor's `ParentReady`, the next leader starts its window on that "optimistic parent" (`ParentSource::OptimisticParent`).
- The candidate filter in `replay_stage.rs:1643` requires `bank.feature_set…alpenglow_fast_leader_handover`.
- **Without the gate, the optimistic path never starts.** The leader waits for ParentReady, and no UpdateParent can be produced.

**Happy path.** ParentReady names the same parent. It is a no-op.

**Sad path** (`handle_parent_ready`). ParentReady names a different parent: the optimistic parent was skipped, or a duplicate version was chosen. The leader then:
1. Shuts down and drains the record receiver, keeping every transaction recorded so far in `accumulated_txs`.
2. Emits `UpdateParentV1{new_parent_slot, new_parent_block_id}` **into the same slot's shred stream**. "Broadcast keeps the original shred-header parent." So the shred header's `parent_offset` still points at the *old* parent. Only the marker carries the truth.
3. Clears the bank (`cleared_bank_id`) and creates a new bank for the same slot on the new parent. The bank budget is carried over (`entry_bytes_consumed`).
4. **Re-injects `accumulated_txs` into banking stage** "for rescheduling". The code comment adds: "technically this send can evict to make room (which may drop a few packets)".

### 2.2 What replayers do
`handle_abandoned_bank` and `try_restart_slot_from_update_parent`:
- They clear the bank and restart replay of the slot **from `replay_fec_set_index`**. Shreds before the marker are never executed in the surviving bank.
- They notify Geyser with `EntryNotification::UpdateParent{slot, cleared_bank_id, parent_slot, parent_block_id}`.
- `get_completed_data_ranges` deliberately waits for all earlier shreds "so earlier conflicting UpdateParent markers are observed before the suffix can execute". So **replay of an FLH slot is delayed** until the prefix arrives.

### 2.3 Answer: re-executed or dropped?
**Re-executed, best effort, against different state.** The leader resubmits every transaction it had recorded on the optimistic bank, and the new bank's status cache does not contain those signatures, so they are not rejected as duplicates. LIKELY, from status-cache semantics.

For us this means:
- **A create seen before an UpdateParent will usually reappear after the marker with the same signature.** It is not guaranteed: it can be evicted, or fail against the new parent's state.
- **Our reactive buy is the dangerous part.** If it landed in the prefix, it is void. It will reappear only if the leader re-injects it, which it does for everything it recorded. Its ordering relative to the create, and the bonding-curve state it executes against, both change.
- **A decoder must invalidate everything in `[0, update_parent_fec_set_index)` of that slot** and then dedupe by signature, keeping the post-marker copy.

### 2.4 How often
| Source | Number | Status |
|---|---|---|
| solana.com/upgrades/alpenglow | FLH (v4.4): "Anza expects it in under 1% of slots" | VERIFIED (quote) |
| solana.com | "Under v4.3, a second bank for one slot arises only from equivocation" | VERIFIED (quote) |
| Feature account `FLHoAW…` | absent on mainnet, devnet and testnet (2026-10-14) | VERIFIED |
| Devnet proxy (Alpenglow, no FLH), last 200k slots | 61 / 50,000 windows (0.12%) end with a skipped tail slot after a produced block. These are exactly the cases where a future FLH leader would have built optimistically and hit the sad path. That gives an upper bound of about **1 in 800 windows**. | VERIFIED (measurement), LIKELY (as proxy) |
| Devnet skip profile, last 100k slots | 0.77% slots skipped. 700 of 773 skipped slots are whole skipped windows (offline leader → no optimistic parent → no UpdateParent). | VERIFIED |
| Mainnet (Tower), last 50k slots | 0.032% skipped | VERIFIED |

The devnet proxy does show something: in all 30 inspected candidate blocks, `getBlock` returned no duplicate signatures and no `MissingTransactionMetadata`. That is consistent with no UpdateParent having occurred.

Mainnet skip rates are about 25× lower than devnet, so **post-FLH UpdateParent frequency on mainnet is likely ≪0.1% of windows**. The impact is concentrated at window boundaries, which matters because the creation slot's leader is in its first slot 25% of the time.

---

## 3. Leader window under Votor (VERIFIED)
- `solana-clock`: `NUM_CONSECUTIVE_LEADER_SLOTS = 4`. `agave-votor 4.3.1 parent_ready_tracker.rs:159,230` fires ParentReady only at `s % 4 == 0`.
- **Devnet**, epoch 1182 (`getLeaderSchedule`, 16 leaders):
  - Run lengths are {4: 65,501; 8: 14,117; 12: 3,329; …}, every one a multiple of 4.
  - Every slot's leader equals the leader of `slot − slot%4` (`True`).
- The epoch is still 432,000 slots.
- **Measured slot time:** mainnet 217.6 ms per slot, including skips (200k-slot window). Devnet 238.9 ms.
- **Projection:** epoch 1064 starts at slot 459,648,000 ≈ **Oct 21 14:19 UTC**. Alpenglow consensus starts at slot 459,653,000 ≈ **14:37 UTC**. solana.com says "approximately 15:00 UTC", and a Solana Compass headline says about 14:50. Track the slot, not the clock.

**Implication.** The "3 cheap later slots in the leader's window" logic carries over. Under Alpenglow, though, if a slot misses notarization the rest of the window is skipped: devnet partial skips were *always* the window tail (61/61). A create in slot k of a window whose later slots get skipped is fine. Only *its own* block matters.

---

## 4. Decoder compatibility

| Component | Markers? | Notes | Status |
|---|---|---|---|
| **sol-shred-sdk 4.0.4**: raw shreds (`reassembler.rs:882`), Jito proxy (`grpc/mod.rs:90`), shredstream client (`client.rs:707`) | **No** | All three paths use `wincode::deserialize_exact::<Vec<Entry>>`. On a marker, the `u64` count 0 parses as an empty Vec, then `deserialize_exact` errors on trailing bytes. The chunk is discarded and `stats.entry_decode_failures += 1`. The abort batch decodes as `Ok(empty)` and is indistinguishable from nothing. | VERIFIED (code). Byte-level test: see below |
| **yellowstone-grpc-proto 14.0.1 / geyser 16.x / client 15.0.0** | **Yes, opt-in** | `SubscribeRequestFilterEntry.include_update_parent` → `SubscribeUpdateEntryUpdateParent{slot, cleared_bank_id, parent_slot, parent_block_id}`, which "invalidates entries for cleared_bank_id, including entries that arrive after this update". For deshred, `SubscribeRequestFilterDeshredTransactions.include_update_parent` → `SubscribeUpdateDeshredUpdateParent{slot, update_parent_fec_set_index, …}`, which "precedes transactions from the data set at this UpdateParent FEC-set boundary", and every deshred tx carries `completed_data_set_starting_shred_index`. `bank_id` appears on account, slot, transaction, transaction-status, block, block-meta and entry messages. `block_footer` filter. New `SlotStatus` values `CREATED_BANK=5`, `DEAD=6`. `DedupStream` fix for "the second Clock sysvar write per slot on Alpenglow". | VERIFIED (proto + CHANGELOG 2026-09-25/09-29) |
| **Agave Geyser interface 4.3** | Yes | `notify_entry_update_parent`, `notify_deshred_update_parent`, `bank_id` on every event. `notify_block_footer` is off unless `block_footer_notifications_enabled` returns true. | VERIFIED (solana.com) |
| **agave-transaction-view 7.0.0** | N/A | It parses one serialized transaction (frames: signature, header, keys, instructions, ALT, `transaction_config_frame` for v1). It never sees entry batches, so markers are irrelevant to it. | VERIFIED (crate file list) |
| Yellowstone RPC note | — | Anza says to run `v16.0.0-rc10+solana.4.3.0`. "Yellowstone's gRPC footer message does not forward certificates yet." | VERIFIED (solana.com) |

**Byte-level test** (scratch crates `mtA`, serialising with solana-entry 4.3.1, and `mtB`, decoding with sol-shred-sdk's exact pins solana-entry =4.2.2 and wincode =0.5.5): **VERIFIED.** Results:

| Input | Bytes | `deserialize_exact` (sol-shred-sdk) | plain `deserialize` |
|---|---|---|---|
| Header | 54 | `Err("Trailing bytes remain after deserialization")` | `Ok(0 entries)` |
| UpdateParent | 54 | same Err | `Ok(0)` |
| Footer | 69 | same Err | `Ok(0)` |
| Abort (8 zero bytes) | 8 | `Ok(0)` | `Ok(0)` |

So every marker is dropped as a decode failure, and the abort batch reads as an empty batch.

The header bytes have this layout: `00×8 | 0100 | 01 | 2900 | 01 | parent_slot u64 | block_id[32]`. That is count 0, marker v1, variant 1, length 41, header V1. UpdateParent has variant byte `02` and the footer has `00`. This matches §1 exactly.

---

## 5. BAM during maintenance

**Code** (jito-solana `v4.3.0-jito.1`, `core/src/proxy/fetch_stage_manager.rs::get_next_tpu_state`, VERIFIED):
1. If BAM is `Connected` and a BAM config exists, gossip advertises **BAM-node TPU (port + 6)**.
2. Otherwise, if a relayer has heartbeated within 1.5 s and the first heartbeat was more than 60 s ago, gossip advertises **the relayer TPU**.
3. Otherwise it advertises **the validator's own TPU**.
- `bam_manager.rs`: whenever BAM is disconnected, `ClientId` is reset to `JitoLabs` (it is `AgaveBam` only while connected).
- The state machine re-evaluates every `DEFAULT_MS_PER_SLOT`.
- The code has no separate "maintenance" state. Jito's wording ("validators connected to BAM revert to ordinary Jito-Agave Block Engine mode") is consistent with BAM nodes stopping their heartbeats or serving. That puts validators on path 2 or 3. LIKELY.

**Today.**
- At 09:01 UTC, AgaveBam is 36.28% of stake, and 36.20% of it has `tpuQuic ≠ gossip`.
- Top TPU IPs are the known BAM nodes: 63.254.167.197 (10.18%), 198.13.140.227 (5.18%), 88.216.192.51 (4.52%), 63.254.163.37 (4.06%), 198.13.131.139 (3.72%) and 208.91.105.53 (3.49%).
- **Maintenance has not started.** VERIFIED (`getClusterNodes` + `getVoteAccounts` + DZ `validators-metadata`).

**What changes when it starts.** For comparison, today only 5.21% of the 27.24% plain JitoLabs stake has `tpu ≠ gossip` (relayer). So expect most of the 36% to flip to the **validator's own IP**, with a minority on Jito relayer IPs. Preconfirmations, MPP and DBI are off during maintenance.

**Timing** (bam.dev changelog Oct 9; readiness post Oct 5, VERIFIED):
- Start: "during the days leading up to activation".
- End: "after activation, BAM nodes are brought back online gradually while performance is monitored".
- Exact timing is "through Discord announcements". No dates are published.
- Cadence after return: 20 ms rounds.

**Detection rule.** Watch gossip `ContactInfo.version.client` (AgaveBam→JitoLabs) and `tpuQuic` per leader. Re-key routing on change instead of on a timer.

---

## 6. Firedancer-family stake (15.75%)

| Class | Stake (Oct 14) | Versions | Expected path |
|---|---|---|---|
| HarmonicFiredancer (Samba) | 6.97% | 26.9.6/26.9.4/26.9.5/26.10.0 | Harmonic **Salsa 4.x (Agave)** through migration. Harmonic docs have no Alpenglow page (UNVERIFIED plan). Remote TPU proxy unchanged → **routing to Harmonic endpoints stays valid** |
| Firedancer | 6.85% | 26.9.6 (5.52%), 26.10.0/.1/.2 (1.13%) | Agave/Jito for migration, then back to **26.10.2** ("mainnet ready… Support for Alpenglow… Migration from Tower to Alpenglow will not be supported"; adds Agave-compatible vote-history files "for safe failover to Agave") |
| FireBAM | 1.63% | 26.9.4–26.10.2 | Jito: "Do not run FireBAM or FrankenBAM during the Alpenglow transition… After activation, use 26.10 or later" |
| Frankendancer | 0.30% | 0.1204.40300 | End of support (cryptobriefing); must go to Agave |

Sources: DZ metadata `software_client` and `software_version`. `getClusterNodes` versions agree: 26.x = 14.23%, 0.1204 = 0.30%; 4.3.0 = 84.21% plus RCs.

**Empirical support.** Testnet has run Alpenglow since epoch 1042 (feature activated at slot 444,620,256). Today it has 89.13% of stake on 4.x and **9.45% on Firedancer 26.x** (29 gossip nodes on 26.10.0/26.10.1). Firedancer does rejoin after migration. VERIFIED.

**Risk.** A Firedancer operator who fails to switch keeps its leader slots in the epoch-1064 schedule but produces nothing, so those slots are skipped. Leader schedules are fixed one epoch ahead. Expect a temporary skip-rate spike concentrated on Firedancer-class leaders (LIKELY). Our `fd_pack`-specific routing (reward per CU, FIFO bundles) is moot from about Oct 20 until operators return, likely days to weeks.

---

## 7. What to build (concrete)

1. **Our own shred decoder** (fork or patch sol-shred-sdk, or write a fresh one):
   - Read the first 8 bytes of each completed data set.
   - `≠0` → entries.
   - `=0` with len 8 → **slot aborted**: cancel every pending action keyed on that slot.
   - `=0` with more bytes → parse the marker:
     - **Header** → record `(slot, parent_slot, parent_block_id)`. Reject a create whose slot's header parent is not on our fork view.
     - **Footer** → slot sealed. Store `bank_hash` and `producer_time_nanos` (useful for latency attribution).
     - **UpdateParent** → mark shreds `< fec_set_index` of that slot invalid. Re-arm on post-marker re-appearance, deduped by signature.
2. **Key state on `(slot, bank_id)`, not `slot`,** for Yellowstone. Subscribe with `include_update_parent: true` on entries and deshred. Handle `SLOT_DEAD` and `CREATED_BANK`.
3. **Ship marker handling before Oct 21 even though UpdateParent won't appear until FLH.** Header, footer and abort are live at about 14:37 UTC Oct 21. FLH is a feature-gate flip we will not get advance notice of in code: **poll `getAccountInfo(FLHoAWBDjNh6zwmJ5i1NKK4KyD8otAiv7XxvmnFnVnKH)` each epoch**.
4. **Routing table.** Refresh on gossip change, not on a timer. Expect three shifts:
   - (a) BAM maintenance: AgaveBam→JitoLabs, TPU→own or relayer.
   - (b) Firedancer→Agave around Oct 20–21.
   - (c) BAM return and Firedancer 26.10 return after activation.
5. **Kill tick-based timing.** Use footer `producer_time_nanos` and our own receive timestamps.
6. **Watch window.** Oct 21 14:19 UTC (epoch start; markers still off) → about 14:37 UTC (slot 459,653,000; markers on). Expect elevated skips. Consider pausing live sniping for the first hour.

---

## 8. Open items
- **Real UpdateParent rate** once FLH activates. Measure it from our own decoder; there is no public cluster with FLH today. **Testnet is the first place FLH is likely to activate** (its v4.4 feature activation started Oct 5, but the FLH account is absent).
- **Jito maintenance timing.** It is announced on Discord only. Our gossip watcher will see it.
- **Harmonic Samba→Salsa plan.** Not documented.
- **Signature duplicates in `getBlock`** for UpdateParent slots. `get_complete_block` reads components from shred 0, so pre-marker transactions might be included and fail `MissingTransactionMetadata`. Untestable until FLH.

## Sources
- crates.io tarballs (static.crates.io): solana-entry/ledger/core/poh/turbine/agave-votor 4.3.1; agave-votor-messages and agave-feature-set 4.3.1; yellowstone-grpc-proto 14.0.1; agave-transaction-view 7.0.0; sol-shred-sdk 4.0.4.
- raw.githubusercontent.com: jito-solana `v4.3.0-jito.1` (`core/src/bam_manager.rs`, `bam_connection.rs`, `proxy/fetch_stage_manager.rs`); yellowstone-grpc `CHANGELOG.md`.
- [solana.com/upgrades/alpenglow](https://solana.com/upgrades/alpenglow).
- [bam.dev changelog Sep 23–Oct 9](https://bam.dev/blog/bam-changelog-sept-23-oct-9/); [BAM readiness](https://bam.dev/blog/update-on-bam-readiness-for-alpenglow-on-mainnet/); [Solana Compass on BAM maintenance](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition); [Solana Compass on the v4.4 schedule](https://solanacompass.com/news/alpenglow-activates-on-solana-testnet-as-frankendancer-era-ends-agave-v44-schedule-targets-november-9-mainnet-activation).
- [Firedancer v26.10.2 notes](https://newreleases.io/project/github/firedancer-io/firedancer/release/v26.10.2); [cryptobriefing on Frankendancer](https://cryptobriefing.com/jump-firedancer-ends-frankendancer-support/).
- RPC: api.{mainnet-beta,devnet,testnet}.solana.com (`getAccountInfo`, `getLeaderSchedule`, `getBlocks`, `getBlock`, `getClusterNodes`, `getVoteAccounts`); data.doublezero.xyz `validators-metadata`.
