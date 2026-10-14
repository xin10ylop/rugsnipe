# Speed research: fastest ways to observe pump.fun creates/buys/sells on Solana (2026-10-14)

Labels: **VERIFIED** means a primary source or our own live measurement. **LIKELY** means a strong inference or a single credible source. **UNVERIFIED** means a vendor claim or a guess. The live measurements were taken today from this container, which egresses in **Columbus, Ohio** (ipinfo.io: AS396982 Google, Columbus OH). It runs through an HTTPS proxy, so absolute RTTs are inflated and only relative comparisons hold. Its location is close to the user's us-east-2 box.

---

## 0. TL;DR. Four facts that change the plan this week

1. **Jito ShredStream has been shut down.** Jito's docs say: "The service will be completely shut down in 60 days (September 5, 2026)", and they point users to DoubleZero Edge ([docs.jito.wtf/lowlatencytxnfeed](https://docs.jito.wtf/lowlatencytxnfeed/)). **VERIFIED** (the announcement). F-Project's "Jito ShredStream" claim is therefore stale marketing, or they have migrated. Do not build on `shredstream-proxy`.
2. **Slots are now 200 ms.** The reduction completed at epoch 1053 on Oct 9, 2026, and the blockhash expiry window is now 30 s ([solana.com/upgrades/reduced-slot-times](https://solana.com/upgrades/reduced-slot-times)). **VERIFIED**. Our measurements: `getRecentPerformanceSamples` gave 268 to 277 slots per 60 s (about 217 to 224 ms per slot). Our websocket slot stream gave a median of **207 ms/slot**. A leader still gets **4 consecutive slots**: `getSlotLeaders` over 200 slots returned run lengths of {4: 49, 2: 2}. A leader window is therefore about 800 ms, and "block 0" now means a 200 ms target.
3. **Alpenglow (Votor) activates at epoch 1064, about Wed Oct 21, 2026, 15:00 UTC** ([solana.com/upgrades/alpenglow](https://solana.com/upgrades/alpenglow)). **VERIFIED** (it shows "Mainnet: Not activated" today). What it changes for us:
   - PoH tick entries disappear from shreds.
   - A slot can carry more than one bank. Geyser and Yellowstone add `bank_id` and `UpdateParent` markers.
   - `confirmed` becomes the same as `finalized`, at about 150 ms.
   - Turbine is unchanged until Rotor, which is not yet scheduled.
   - "Pre-confirmation streaming is unaffected."
   - Jito puts BAM into "maintenance mode" around activation, which means plain Block Engine mode with no MPP or DBI ([solanacompass](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)). **LIKELY**.
4. **Transaction v1 (SIMD-0385) is live** (epoch 1035, about Sep 15, 2026; [solanacompass](https://solanacompass.com/news/transaction-v1-simd-0385-is-live-on-solana-mainnet-at-epoch-1035)). In our sample, **9 of 19 recent pump.fun create transactions were v1.** **VERIFIED** (our `getTransaction` sample, below). In v1, signatures sit at the **tail**, the first byte is `0x81`, there are no address lookup tables, and the compute budget lives in a `TransactionConfigMask` ([SIMD-0385](https://raw.githubusercontent.com/solana-foundation/solana-improvement-documents/main/proposals/0385-transaction-v1.md)). **VERIFIED**. A decoder built on solana-sdk 2.x/3.x will choke. The old `shredstream-proxy` pins `solana-entry =2.2.1` ([Cargo.toml](https://raw.githubusercontent.com/jito-labs/shredstream-proxy/master/Cargo.toml)). **VERIFIED**. It will therefore probably fail to deserialize entry batches that contain v1 transactions. **LIKELY** (inference).

---

## 1. The block-production path, and where a transaction becomes visible

**Ingress.** Transactions reach the current or next leader over the TPU (QUIC). Stake-weighted QoS gives staked peers reserved capacity. Most stake is Jito-enabled: 98.7% of active stake has `is_jito` = true ([Stakewiz](https://api.stakewiz.com/validators), computed today). **VERIFIED**. So transactions also arrive through the Jito Block Engine as bundles. On BAM validators they arrive through the BAM node, which runs **20 ms auction rounds** since Oct 9 (previously 25 ms) ([solanacompass](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition)). **LIKELY**. BAM covers about 34 to 37% of stake: 34.1% at the preconfirmation launch on Sep 9, and 37% at epoch 1050 (same source). **LIKELY**.

**There is no public mempool.** A watched wallet's `create_v2` is invisible to us until one of these happens:
- (a) the leader's preconfirmation export fires, which is Helius preconf at execution or BAM preconf at commit-to-execute;
- (b) the leader shreds the entry containing it;
- (c) a downstream node replays it ("processed").

**At the leader.**
1. The banking stage executes a batch.
2. PoH records an entry.
3. The broadcast stage turns entries into shreds, in FEC sets of data and coding shreds of about 1,228 bytes each.
4. The shreds go to the Turbine root.

The gap between execution and the first shred is a few ms (**LIKELY**; no primary number found).

**Turbine.** Shreds fan out through a stake-weighted tree. High-stake validators get them first, and unstaked RPC nodes sit in the deepest layer. Triton says that by default you wait "an extra 15-80 ms (or even >200 ms for certain slots)" for propagation and reconstruction ([Triton guide](https://blog.triton.one/complete-guide-to-solana-streaming-and-yellowstone-grpc/)). **UNVERIFIED** (vendor claim).

**At an RPC node.** The replay stage executes entries as they arrive, slot-incrementally. The Geyser `notify_transaction` call then makes the transaction available at the `processed` level (Yellowstone `CommitmentLevel::PROCESSED`). Processed means only "executed in a bank". It can belong to an abandoned fork or bank, and post-Alpenglow it can belong to an `UpdateParent`-cleared bank ([Alpenglow page](https://solana.com/upgrades/alpenglow)). **VERIFIED**.

**JSON-RPC websocket** (`logsSubscribe`/`blockSubscribe`) is the slowest of these feeds. See the measurement in §5.

**Where the leaders are.** This determines where to put boxes. We computed the current epoch's `getLeaderSchedule` (432,000 slots), joined to Stakewiz IP geolocation (0.02% of slots unmapped). **VERIFIED, computed today:**

| City | Share of leader slots |
|---|---|
| Frankfurt | **32.5%** |
| Amsterdam | **18.9%** |
| London | 8.7% |
| Tokyo | 5.7% |
| Šiauliai, LT | 4.5% |
| Singapore | 4.2% |
| Washington | 3.8% |
| New York City | 2.5% |
| Los Angeles | 2.1% |
| Dublin | 2.1% |
| Stockholm | 1.9% |
| Newark | 1.6% |
| Boardman, OR (AWS us-west-2) | 1.3% |
| Chicago | 1.0% |

Grouped by stake: **Europe 73.8%**, APAC 10.9%, US-East 8.4%, US-West/other 4.6%, US-Central 1.2%. Ohio is not a validator hub.

**Inter-metro RTT medians** from DoubleZero's own probes, last 24 h, internet RTT ([data.doublezero.xyz `/dz/metro-pairs/latency`](https://data.doublezero.xyz/api/v1/docs)). **VERIFIED**:

| Pair | Internet RTT |
|---|---|
| FRA–AMS | 7.4 ms |
| AMS–LON | 6.4 ms |
| FRA–LON | 16.8 ms |
| FRA–WAS | 87.1 ms |
| LON–NYC | 70.7 ms |
| CMH(Columbus)–NYC | 17.4 ms |
| FRA–TYO | 235 ms (134 ms on DoubleZero) |
| SIN–TYO | 72 ms |

So Ohio to Frankfurt is roughly **100 to 105 ms RTT** (**LIKELY**, summed from these pairs). That is half a slot each way, and it lands on about 60% of all leader slots.

---

## 2. Jito ShredStream, and what replaced it

**Status.** It was sunset on Sep 5, 2026. **VERIFIED** (announcement; we did not test whether a shutdown connection is refused). For the record, this is how it worked:

- **Protocol** ([mev-protos/shredstream.proto](https://raw.githubusercontent.com/jito-labs/mev-protos/master/shredstream.proto)):
  - `Shredstream.SendHeartbeat(Heartbeat{socket, regions})` returns `HeartbeatResponse{ttl_ms}`. Up to 2 regions.
  - Raw shreds arrived over UDP (default port 20000).
  - The proxy exposed `ShredstreamProxy.SubscribeEntries(SubscribeEntriesRequest{})` as a stream of `Entry{slot, entries}`, where `entries` is bincode `Vec<solana_entry::Entry>`. That means full, deserializable transactions, before replay, with no filters ("tbd").
- **Auth:** a keypair allow-list approved via Discord (`--auth-keypair`).
- **Block engine:** `https://mainnet.block-engine.jito.wtf`.
- **Regions:** Amsterdam, Dublin, Frankfurt, London, New York, Salt Lake City, Singapore, Tokyo. The hostnames are `https://{amsterdam,dublin,frankfurt,london,ny,slc,singapore,tokyo}.mainnet.block-engine.jito.wtf`, with shred IPs such as Frankfurt 64.130.50.14 and NY 141.98.216.96 ([lowlatencytxnsend](https://docs.jito.wtf/lowlatencytxnsend/)). **VERIFIED**. The Block Engine regions still exist for **sending**.
- **Cost:** not stated publicly.

**Replacements (all pre-execution shreds):**

**DoubleZero Edge.** Validators publish leader shreds into IP multicast over DZ fiber, one hop to subscribers ([docs](https://docs.doublezero.xyz/solana/edge/subscribe/)). **VERIFIED**.
- Multicast groups: leader shreds `233.84.178.1:7733`; retransmit groups EU/APAC/AMER `233.84.178.12/.13/.14:7733`.
- Traffic is **GRE-encapsulated** on interface `doublezero1`. You strip GRE yourself, for example with XDP.
- Connect with `doublezero connect multicast --subscribe-feed solana-shreds-full`.
- **Price per machine per month:** FRA/AMS **$1,500**; LON/NY/SGP/TYO **$900**; all other metros **$450**. Approval takes 1 to 3 business days.
- Footprint per the DZ data API today: 31 metros, 85 enabled devices, 413 publishers representing **57.3% of stake** (`total_publisher_stake/total_network_stake`). **VERIFIED**. The publisher stake is concentrated in FRA, AMS and LON. A DZ metro exists in **Columbus (cmh)**.
- Vendor scoreboard, Jul 6, 2026: 73.5% first-seen win rate; 11.6 ms p50 faster than Jito ShredStream; 20.7 ms p50 and 209 ms p95 faster than Turbine ([doublezero.xyz/jito-shredstream](https://doublezero.xyz/jito-shredstream)). **UNVERIFIED** (vendor).
- Running it on AWS needs GRE (IP protocol 47) allowed in the security group. **LIKELY** to work; we did not test it.

**Helius Shred Delivery.** Raw UDP shreds at **$1,000 per IP per month** ($800 on the Pro plan). "Preprocessed Transactions" are the same data deshredded, served over the `preprocessedSubscribe` websocket at 0.1 credits per message on any paid plan ([helius docs](https://www.helius.dev/docs/shred-delivery)). **VERIFIED**.

**Everstake Blockspace.**
- ShredStream: 7 regions (FRA, AMS, TYO, NY, LON, DUB, SGP), UDP to your IP:port, "sub-10ms in Frankfurt".
- Direct Shreds: per-customer link from Everstake validators, "3 ms p50 in-region", source-IP allow-list ([docs](https://docs.blockspace.everstake.com/shredstream/quickstart)).
- **VERIFIED** (the docs exist); the latency figures are **UNVERIFIED**.

**Solana Vibe Station.** Raw shreds from **$250/mo in AMS and $300/mo in ATL**; Yellowstone gRPC from $150/mo; co-located VPS ([pricing](https://www.solanavibestation.com/pricing)). **VERIFIED** (pricing page).

**Shyft RabbitStream.** Shred-derived, pre-execution transactions with Yellowstone-style filters. Their own geyserbench run against Jito ShredStream, as "Jito P50 behind": FRA 6.7 ms, NY 5.3 ms, AMS 3.8 ms ([docs](https://docs.shyft.to/solana-shredstreaming/rabbitstream-vs-jito-shredstream-benchmarks.md)). **UNVERIFIED** (vendor). Their pipeline diagram wrongly puts execution after shredding: at the leader, execution happens first.

**ERPC** sells shared and dedicated shredstreams. Pricing goes through Discord ([erpc news](https://erpc.global/en/news/2025/12/01/erpc-premium-vps-shreds-new-pricing-202512/)). **UNVERIFIED**.

**Do shreds let us see a create "before the block is produced"?** They let us see it before the block is **complete**. A 200 ms slot streams out entry by entry, and the leader keeps building for up to 3 more slots, so a reaction can still land in the same leader window and often in the same slot. **LIKELY**. They never show a create before the leader has executed it.

---

## 3. Feeds that come before shreds: preconfirmations

**Helius `preconfSubscribe`** on `wss://beta.helius-rpc.com` (the Gatekeeper host) merges two sources ([guide](https://www.helius.dev/docs/pre-confirmations/preconf-subscribe.md)). **VERIFIED**:
- **Helius preconfirmations**, emitted "the instant the leader executes the transaction", with execution status. These come only from validators that forward to Helius.
- **BAM preconfirmations**, emitted "when the validator commits to executing a transaction, before it runs". Their status is always 2 (unknown), and the same signature may arrive twice.

Features:
- Server-side filters: `signerInclude` (our watched creator wallets, up to 10k), `accountInclude`, `instructions[{programId, memcmp, dataSize}]` for top-level instructions only, and `regionInclude` (slc, fra, lon, pit, sgp, ewr, tyo, ams, dal, dub, mia, lax, iad, sea, hkg, sqq).
- Payloads are binary frames in Solana wire format, including v1. Helius explicitly says to decode with `agave-transaction-view`, not bincode.
- **Coverage is not continuous.** Use `getPreConfCoverage` to check it.

**Cost:** Professional plan ($999/mo) at 10 credits per message, with a **minimum of 300 credits per covered slot per subscription starting today, Oct 14, 2026**. **VERIFIED**. Credits cost about $5 per 1M (200 credits = $0.001, per [plans](https://www.helius.dev/docs/billing/plans)). If about 40% of 432k daily slots are covered, that is about 52M credits/day, or **roughly $250/day**. **LIKELY** (our arithmetic; coverage assumed). Validate against `getPreConfCoverage` before committing.

**Where this fits.** For **copy-trading a watched wallet**, this is the earliest signal available to anyone not inside the leader. It covers only the slots led by Helius-forwarding or BAM validators, and BAM may be in maintenance mode around Oct 21.

---

## 4. Yellowstone gRPC / Geyser (post-execution, `processed`)

**Proto** ([geyser.proto, master](https://raw.githubusercontent.com/rpcpool/yellowstone-grpc/master/yellowstone-grpc-proto/proto/geyser.proto)). **VERIFIED**:
- `Subscribe` with `SubscribeRequestFilterTransactions{vote, failed, account_include, account_exclude, account_required}`, `from_slot` replay, and an `entry` stream.
- **`SubscribeDeshred`**: pre-execution transactions with ALT-resolved addresses and no meta. The README says it is "only available on Triton extension servers"; open-source servers return `UNIMPLEMENTED`.
- `bank_id` fields for Alpenglow.

Anza's Alpenglow note says to upgrade the plugin to `v16.0.0-rc10+solana.4.3.0`.

**Providers (all Yellowstone wire-compatible):**

**Helius LaserStream.** Mainnet requires Business ($499) or Professional ($999). Endpoints are `laserstream-mainnet-{ewr,pitt,slc,lax,lon,ams,fra,tyo,sgp}.helius-rpc.com`. It supports zstd and 48 h replay ([docs](https://www.helius.dev/docs/laserstream/grpc)). **VERIFIED**. It pairs with **Helius Sender**, which fans out across Helius, Jito, Harmonic, Rakurai and others: the Max tier has a 0.001 SOL minimum tip, and the SWQoS-only tier 0.000005 SOL ([sender](https://www.helius.dev/docs/sending-transactions/sender)). **VERIFIED**.

**Triton (Dragon's Mouth).** Usage-based at about $0.08/GB, per their own table. It is the only provider of `SubscribeDeshred` ([Triton guide](https://blog.triton.one/complete-guide-to-solana-streaming-and-yellowstone-grpc/)). **VERIFIED** (their page).

**QuickNode.** Included on Scale plans and up. Port 443 plus compression is required; legacy port 10000 is sunset. Metered at 15 credits per 0.1 MB from Oct 1, 2026. It needs `yellowstone-grpc-proto ≥ 12.6.0` to receive the v1 `config` field ([docs](https://www.quicknode.com/docs/solana/yellowstone-grpc/overview)). **VERIFIED**.

**Others:**
- Chainstack: Yellowstone add-on from its Growth plan ([pricing](https://chainstack.com/pricing/)).
- Shyft: x-token or IP allow-list.
- SVS: $150/mo, ATL/AMS.
- ERPC, InstantNodes: pricing not public. **UNVERIFIED**.

**Independent benchmarks.** None credible exist. Every published number is vendor-authored ([Subglow's survey notes](https://subglow.io/solana-grpc-latency-benchmark) that almost nobody publishes p50/p99). The tool to measure it ourselves is **[geyserbench v1.2.4](https://raw.githubusercontent.com/solstackapp/geyserbench/master/README.md)**. It supports Yellowstone, Shredstream, Thor, Jetstream and others, and reports first-detection share and P50/P95/P99. Run it from our own boxes.

**Ordering of feeds, earliest first.** **LIKELY**, based on the pipeline above plus the vendor deltas:
1. BAM preconf
2. Helius preconf
3. In-metro raw leader shreds (DZ Edge, Direct Shreds)
4. Shared shred relays
5. `SubscribeDeshred`
6. gRPC `processed`, typically 10 to 80 ms after shreds
7. PumpPortal and RPC websockets

---

## 5. Running our own node, and the free fallbacks

**Own Agave RPC with a Geyser plugin.** Anza's requirements for an RPC node: 16 cores / 32 threads at ≥2.8 GHz with SHA extensions; 512 GB RAM for full account indexes (256 GB minimum for a validator); 3 NVMe drives (accounts 1 TB, ledger 1 TB, snapshots 500 GB); 1 Gbit/s symmetric for an unstaked node. Anza warns that cloud VMs need "significantly greater operational expertise" ([requirements](https://docs.anza.xyz/operations/requirements)). **VERIFIED**. Bare metal of that class costs about $800 to $2,000/mo (**UNVERIFIED** market estimate). An **unstaked** node sits at the bottom of Turbine, so it is not faster than a paid shred feed. Its value is local state (bonding curves, our balances, blockhash) with no network hop.

**Recommendation: do not run a full node in phase 1.** Run a lightweight **deshredder** on the shred feed, and use LaserStream gRPC for the state we need.

**Firedancer.** Clients on `26.x` versions (FireBAM 26.09 per Jito) hold **15.4% of stake** (Stakewiz). **LIKELY** that 26.x means Firedancer/Frankendancer. Neither client is supported at Alpenglow activation, so that stake moves to Agave builds this week.

**Jito relayer / "shred receiver".** Shred receiving is no longer offered by Jito. The relayer is TPU-side infrastructure for validators and does not help us observe.

**PumpPortal (`wss://pumpportal.fun/api/data`, `subscribeNewToken`)** is free and worked without an API key today ([docs](https://pumpportal.fun/data-api/real-time)). **VERIFIED**.

We measured it today from the Columbus container against the public RPC websocket over 180 s, with 20 creates matched by signature. **VERIFIED, relative numbers**:
- PumpPortal arrived a median **183 ms** (p90 320 ms) after the public RPC's `slotNotification` for the create's slot.
- Public RPC `logsSubscribe(processed, mentions=mint-authority)` arrived a median 239 ms after it.
- PumpPortal was a median **39 ms earlier** than public RPC logs (range −179 to +98 ms).

Conclusion: PumpPortal sits roughly one full slot or more behind shreds. Use it only as a dead-man fallback and as a cross-check feed. It cannot achieve block 0.

---

## 6. What to parse from a create, and what we can know in advance

**Programs.**
- Pump program: `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`.
- Mint authority PDA: `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM`, a cheap filter key that appears in every create.

**Discriminators** from the current IDL ([idl/pump.json](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/idl/pump.json)). **VERIFIED**:

| Instruction or event | Discriminator |
|---|---|
| `create_v2` | `d6904cec5f8b31b4` |
| legacy `create` | `181ec828051c0777` |
| `buy` | `66063d1201daebea` |
| `buy_v2` | `b817ee6167c5d33d` |
| `buy_v3` | `07051dc4f5176550` |
| `buy_exact_quote_in_v3` | `e1f7501ed5b38488` |
| `sell_v3` | `1c92de7726c469d5` |
| CreateEvent | `1b72a94ddeeb6376` |
| TradeEvent | `bddb7fd34ee661ee` |

**`create_v2` accounts** ([COIN_CREATION.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/instructions/COIN_CREATION.md)). **VERIFIED**:
- [0] `mint`: a new **Token-2022** mint and a **signer**.
- [2] `bonding_curve` = PDA(`"bonding-curve"`, mint).
- [3] `associated_bonding_curve`.
- [5] `user` (the payer).
- [7] token_program = `TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb`.
- [9..13] Mayhem accounts.
- Optional remaining accounts [16..18]: `quote_mint`, `associated_quote_bonding_curve`, `quote_token_program`. These appear for non-SOL quotes (USDC or a pump coin, up to depth 2, since Oct 13).

**`create_v2` arguments:** `name`, `symbol`, `uri`, `creator: Pubkey`, `is_mayhem_mode: bool`, then optional `is_cashback_enabled` (must be false), `creator_fee_bps`, `is_holder_reward`.

**Our buy should be `buy_v3`.** It takes 17 accounts ([TRADE_V3.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/instructions/TRADE_V3.md)). **VERIFIED**. Every one of them is derivable from **mint + our wallet + constants**:

| Account | Source |
|---|---|
| global | constant |
| base_mint | the mint |
| quote_mint | wSOL |
| base_token_program | Token-2022 |
| quote_token_program | SPL Token |
| bonding_curve | read from the create, account [2] |
| associated_base_bonding_curve | read from the create, account [3] |
| associated_quote_bonding_curve | derived (unused for SOL) |
| user | our wallet |
| associated_base_user | must exist, so prepend `CreateIdempotent` for a Token-2022 ATA |
| associated_quote_user | our wSOL ATA address (unused) |
| user_volume_accumulator | PDA(`"user_volume_accumulator"`, us), precreate once, otherwise 0.0018444 SOL rent |
| fee_config | `8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt` |
| buyback_fee_recipient | 1 of 8 constants ([FEE_RECIPIENTS.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/FEE_RECIPIENTS.md)) |
| system_program, event_authority, program | constants |

**v3 needs no creator, `creator_vault` or `fee_recipient`.** This shrinks the transaction and removes the parse dependency on the `creator` argument. The legacy `buy` and `buy_v2` instructions still need `creator_vault` = PDA(`"creator-vault"`, creator). Skip coins whose quote is not SOL, or route them separately.

**Live sample of recent creates.** 19 transactions referencing the mint authority, fetched via `getTransaction`. **VERIFIED**:
- 100% used `create_v2`; legacy `create` did not appear.
- 9 were v1, 9 were v0, and 1 was legacy (a PumpSwap transaction).
- Most bundle `create_v2 + ATA + dev buy` (`buy`, `buy_v2` or `buy_v3`) in one transaction.
- They consumed 113k to 330k CU.
- Fees ran from 10,000 to 1,010,000 lamports.
- About 40 creates per 669 slots, roughly one every 17 slots (0.28/s).

**Is the mint known before confirmation?** **No.** It is a fresh keypair that signs the create, often a vanity address ending in "pump" that pump.fun issues. Nothing derives it in advance. **VERIFIED** (the mint is a signer in the IDL). The earliest we can learn it is the first preconfirmation or shred carrying the create. Everything after that (bonding curve, ATAs, our transaction) is precomputable within microseconds.

---

## 7. Recommended ingestion stack and latency budget

**Location.** Choose **Frankfurt bare metal first**: TeraSwitch, Latitude.sh or Cherry, the top three operators by stake (26.2%, 9.5% and 7.2%). AWS eu-central-1 is the fallback.
- Leaders in FRA (32.5%), AMS (18.9%) and LON (8.7%) are reachable within ≤17 ms RTT, which covers **~60% of leader slots**.
- Adding Šiauliai, Dublin and Stockholm covers about 70%.
- Second box: NY/Newark (US-East 8.4%). Third box: Tokyo (5.7%), plus Singapore 4.2% at 72 ms.
- Ohio is a poor place to *observe*. Keep it only for non-latency services.

**Feeds**, in parallel, deduplicated by signature, first one wins:
1. DZ Edge FRA leader-shred multicast at $1,500/mo. A cheaper alternative to start and A/B test against: Helius raw shreds at $800 to $1,000 per IP, or SVS AMS at $250.
2. Helius `preconfSubscribe` with `signerInclude` set to the watched wallets, Pro plan.
3. LaserStream gRPC `fra` at `processed`, for confirmations, our own fills, bonding-curve account state and blockhash.
4. PumpPortal as a fallback and sanity check.

**Hot path.** Use a pinned core and busy-poll with `recvmmsg` or AF_XDP. Strip GRE, parse the shred header, then filter.
- Filter pre-deshred: entries are built from data shreds, and the first data shreds of an entry carry its transactions. A byte scan for the pump program ID or the mint-authority key can trigger before the full FEC set arrives. **LIKELY**; needs testing.
- Run FEC recovery only when packets are missing.
- Use a zero-copy v0/v1 transaction view.
- Fill a pre-built `buy_v3` template, sign it, and fan it out to several senders.

| Step (FRA box, FRA/AMS leader) | Budget |
|---|---|
| Leader execution → first shred out | 1–5 ms (LIKELY) |
| Leader → us (DZ Edge in-metro) | 0.5–3 ms (UNVERIFIED; FRA–AMS RTT 6 ms) |
| GRE strip, shred parse, deshred, filter, decode | 0.05–0.3 ms (LIKELY) |
| ATA/PDA derivation, template fill, ed25519 sign | 0.03–0.08 ms (LIKELY) |
| Send → leader (QUIC/SWQoS or Jito BE/Sender, in-metro) | 0.5–3 ms network, plus relay or auction (BAM 20 ms rounds) |
| **Observe → our transaction at the leader** | **≈ 5–15 ms, up to ~35 ms with a BAM round** |
| Same chain from Ohio to a FRA leader | **≈ 110–130 ms** (adds about 100–105 ms RTT) |
| Same, via LaserStream `processed` in-metro | ≈ 20–100 ms (UNVERIFIED) |

At 200 ms slots, a create executed in the first ~80% of a slot can be followed in the **same slot** from Frankfurt. **LIKELY**. From Ohio, only creates in the first ~35 to 40% of the slot can be followed in the same slot.

**Rust crates**, latest stable on crates.io today. **VERIFIED**:

| Purpose | Crates |
|---|---|
| gRPC | `yellowstone-grpc-client 15.0.0`, `yellowstone-grpc-proto 14.0.1` (≥12.6.0 is required for v1 config), `helius-laserstream 0.10.0`, `tonic 0.14.6`, `prost 0.14.4` |
| Shreds and entries | `solana-ledger 4.3.1` (`Shred::new_from_payload`, merkle recovery), `solana-entry 4.3.1`, `solana-perf 4.3.1` |
| v1-aware zero-copy parsing | `agave-transaction-view 7.0.0` |
| Solana types | `solana-pubkey 4.4.0`, `solana-message 5.1.0`, `solana-transaction 5.1.0`, `solana-keypair 4.0.0`, `solana-signer 4.0.0`, `solana-hash 4.7.0`, `solana-signature 3.6.0`; `solana-sdk 5.0.0` exists but avoid the meta-crate |
| Sending | `solana-tpu-client-next 4.3.1`, `quinn 0.11.12` |
| Reference only | `pump-rust-client 0.4.1` |

Version alignment: the Yellowstone workspace pins Agave `4.3.0` crates, together with `solana-pubkey 4.0.0`, `solana-transaction 4.0.0` and `solana-message 4.4.1`. **Align on Yellowstone's pins** to avoid duplicate `Pubkey` types.

For Jito, use the proto file at `jito-labs/mev-protos/shredstream.proto`. `jito-protos` is a path crate inside the repo and is not on crates.io. It is legacy only.

---

## 8. Open questions and next measurements

1. Run geyserbench from FRA and NY for 7 days. Compare DZ Edge, Helius raw and preprocessed shreds, SVS, Everstake, LaserStream `processed` and Triton `SubscribeDeshred` on first-seen pump.fun creates.
2. Measure `getPreConfCoverage` per region and the real monthly cost under the new 300-credit minimum.
3. Confirm that DZ Edge's GRE multicast works on AWS EC2 versus bare metal, and whether the cmh metro gives anything useful.
4. Re-test shred and entry decoding after Alpenglow, which arrives about Oct 21: no ticks, `UpdateParent`, multiple banks per slot.
5. Measure where in the slot creates land (entry index or tick position) to estimate the true same-slot hit rate.
6. Check whether F-Project still claims ShredStream after Sep 5. If they do, their feed is no longer what they advertise.
