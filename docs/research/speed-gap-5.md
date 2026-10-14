# Speed gap 5: proxy-TPU admission rules, unstaked vs staked sends, and what staked access costs

**Question.** Do BAM nodes and Harmonic Remote TPUs apply stake-weighted QoS, connection caps or allow-lists to inbound QUIC? They are the advertised `tpuQuic` for 68% of stake. How do unstaked direct sends fare during hot launches compared with staked ones? What does staked access realistically cost in FRA/AMS: `--staked-nodes-overrides` peering with a validator, Helius `swqos_only`, SVS staked RPC, Astralane VIP tiers?

**Labels.** VERIFIED means read in primary source or measured. LIKELY means strong indirect evidence. UNVERIFIED means no evidence either way. Date: 2026-10-14. SOL = $106.37 (CoinGecko, 10:05 UTC). Total active stake = 438.1M SOL (`getVoteAccounts`), supply 635.8M (`getSupply`), inflation 3.60% (`getInflationRate`).

---

## 0. Answer in brief

1. **Neither operator documents its inbound admission rules.** Harmonic's docs say nothing about QoS, caps or allow-lists for senders; we confirmed this through their own docs Q&A endpoint. The BAM node is closed source. VERIFIED (that the rules are undocumented).
2. **Both proxies come from Jito's relayer line, and that relayer is open source with stake-weighted QoS.** Harmonic's `--remote-tpu-url` flag accepts `--relayer-url` as an alias. Helius calls BAM nodes "transparently visible on the network, similar to existing Jito relayers". `jito-relayer` runs the stock Agave `solana-streamer` QUIC server, with stake taken from `getVoteAccounts`, **500 unstaked and 2,000 staked connection slots by default, 8 connections per peer, 64 new connections per IP per minute, and no sender allow-list**. Its TPU-forwards port **rejects every unstaked connection**. VERIFIED for jito-relayer. LIKELY that BAM and Harmonic inherited it; their actual values are UNVERIFIED.
3. **The Agave-family admission code never refuses a new unstaked connection because the table is full.** It evicts the least recently *used* connection, then admits the new one. A low-rate sender is therefore never locked out. The worst cases are (a) its idle warm connection gets evicted, which costs a fresh handshake, about 1 RTT (sub-millisecond to a few ms in the same metro), and (b) the per-IP connection rate limit returns close code 2 "disallowed" if it reconnects too often: 8 per minute on Agave 3.x/4.x, 64 on jito-relayer. Unstaked per-connection throughput (100–200 TPS) is far above a sniper's needs. VERIFIED (code).
4. **On-chain, the untipped path is not shut out at proxy-fronted leaders, and not during hot launches either.** Untipped transactions win first place in block 0 about as often on BAM (30–33%) and Harmonic (32–49%) leaders as on JitoLabs leaders that receive at their own TPU (26–32%). Hot launches (≥6 reactive buy attempts in the create slot) do not reduce that share. VERIFIED measurement on our 3,581-create sample. "Untipped" mixes direct, staked-RPC and unknown senders, so this is LIKELY, not proof, that unstaked direct sends work.
5. **Staked access is cheap only as a paid relay. Staking our own identity is expensive.** A self-staked QUIC identity needs **≥8,762 SOL (≈$932k)** to count as staked at Agave leaders. At a proxy using jito-relayer's 2.2 defaults it needs **≥17,525 SOL (≈$1.86M)**. The forgone yield is about **$4k per month**. Peering through `--staked-nodes-overrides` adds a validator hop plus the forwarding stage, so it is slower than direct for block 0. Retail staked paths cost $300–1,300 a month (SVS, Everstake) or a per-transaction tip: Helius `swqos_only` 0.000005 SOL, Astralane VIP3 0.00001 SOL. VERIFIED prices.
6. **What this means for the design.** Keep "own box, direct QUIC to the advertised `tpuQuic`" as the primary low-latency leg. Always fan out in parallel to one or two paid senders under a **shared durable nonce**. Do not buy stake. Run the fingerprint probe in §5 from the Frankfurt box, which settles items 2 and 3 for the real proxies in about an hour.

---

## 1. What the operators publish

### Harmonic Remote TPU (25.1% of stake; 24.8% advertises a Harmonic TPU, per critique-speed §3.2)

- **Architecture.** "The Remote TPU proxies TPU transaction flow to builders and validators… The validator and any connected block builders then subscribe to the relayer and receive forwarded packets" ([harmonic-architecture.md](https://docs.harmonic.gg/concepts/harmonic-architecture.md)). VERIFIED.
- **Lineage hint.** Salsa 4.x: "`--remote-tpu-url` … Harmonic Remote TPU gRPC endpoint… **`--relayer-url` is accepted as an alias**." The Remote TPU address "is advertised in gossip as the validator's TPU", and when the scheduler stops the validator "restores its original TPU addresses in gossip" ([salsa-4.md](https://docs.harmonic.gg/run-a-validator/setup/salsa/salsa-4.md)). VERIFIED.
- **Who is whitelisted.** Only *validators* are: "Harmonic will not accept connections from unregistered validators" ([salsa-4.md](https://docs.harmonic.gg/run-a-validator/setup/salsa/salsa-4.md), [samba.md](https://docs.harmonic.gg/run-a-validator/setup/samba.md)). *Searchers* need whitelisting only for the authenticated bundle service. A public bundle service also exists with "lower rate limits" ([harmonic-bundles.md](https://docs.harmonic.gg/searchers/harmonic-bundles.md)). VERIFIED.
- **Harmonic points single-transaction flow at the Remote TPU.** Bundles with revert protection require a high landed rate, and "crank arb spam… gets you blacklisted… **Send through the Remote TPU instead.** Remote TPU is the right path for non-atomic, single-transaction, and speculative flow… failed attempts cost you fees" ([harmonic-bundles.md](https://docs.harmonic.gg/searchers/harmonic-bundles.md)). So Harmonic intends the Remote TPU as a public ingress for exactly our kind of traffic. VERIFIED.
- **Inbound QoS.** We asked the docs' own answer endpoint (`docs.harmonic.gg/…md?ask=`) whether the Remote TPU applies SWQoS, connection caps, rate limits or allow-lists. The answer: "The docs **do not specify**… They also don't confirm that unstaked senders receive the same connection or forwarding treatment." VERIFIED (silence). We have not contacted the Harmonic team directly; that remains a to-do.
- Regional Remote TPUs: ams, ewr, fra, lon, tyo, sgp, slc. The shred receiver IP is the same host as the advertised TPU, e.g. FRA `198.13.140.105`, port 9007 in gossip ([endpoints.md](https://docs.harmonic.gg/run-a-validator/endpoints.md); `getClusterNodes` today). VERIFIED.

### BAM node (37.9% of stake)

- The BAM node "comprises a transaction processing unit (TPU) and a transaction scheduler, operating within a TEE". Users "have the option to send transactions directly to BAM instances". "BAM nodes are transparently visible on the network, **similar to existing Jito relayers**". The QUIC certificates are generated inside the TEE ([Helius BAM explainer](https://www.helius.dev/blog/block-assembly-marketplace-bam)). LIKELY (secondary source quoting Jito).
- bam.dev: "Direct TPU flow: Transactions sent directly to your TPU port are processed through BAM before execution" ([bam-overview](https://bam.dev/docs/bam/bam-overview/)). The validator learns the TPU it should advertise from the BAM node's `ConfigResponse.bam_config.tpu_sock` (`jito-solana v4.3.0-jito.1 core/src/bam_manager.rs::update_tpu_config`). VERIFIED.
- **The code is closed.** "the BAM codebase remains closed-source" ([Helius](https://www.helius.dev/blog/block-assembly-marketplace-bam)). The node-operator page says "We anticipate releasing operator specifications and deployment code in the coming months" ([bam.dev/node-operators](https://bam.dev/node-operators/)). The open `jito-labs/bam-client` is the *validator* side (Immunefi scope lists "BAM Validator Client" and "BAM Proto Definitions"). VERIFIED. **BAM's inbound QUIC admission therefore cannot be read from source.**
- The only documented BAM allow-list is the maker plugin's **separate** UDP "Plugin TPU", with static enrollment by the node operator. It is "isolated from the standard TPU" ([maker plugin](https://bam.dev/docs/bam/maker-plugin/how-it-works/)). That is not a restriction on the normal TPU. VERIFIED.
- **Alpenglow caveat.** In BAM maintenance mode, validators flip to JitoLabs and advertise their own TPU or a relayer (speed-gap-2 §5). During that window the BAM share drops out of this question entirely.

---

## 2. The open reference implementation: jito-relayer (what the proxies most likely run)

Source: `jito-foundation/jito-relayer@master`, read via raw.githubusercontent. Pinned to `solana-streamer = "=2.2"`, crate version 0.4.2.

| Knob | Value | Where |
|---|---|---|
| Stake source | `getVoteAccounts` current **and delinquent**, refreshed every 5 s, plus optional `--staked-nodes-overrides` | `core/src/staked_nodes_updater_service.rs` |
| QUIC server | stock `solana_streamer::quic::spawn_server`, i.e. the full Agave SWQoS | `core/src/tpu.rs` |
| `--max-unstaked-quic-connections` | **500** (default) | `transaction-relayer/src/main.rs` |
| `--max-staked-quic-connections` | **2,000** | same |
| Connections per peer | 8 (`MAX_QUIC_CONNECTIONS_PER_PEER`) | `tpu.rs` |
| New connections per IP per minute | **64** | `tpu.rs` |
| TPU-forwards port | `max_unstaked_connections: 0` ("Prevent unstaked nodes from forwarding transactions") | `tpu.rs` |
| Sender allow-list | **none**. `--allowed-validators` gates only validators subscribing over gRPC | `main.rs` |

From the streamer 2.2 code (`agave v2.2.20 streamer/src/nonblocking/{quic.rs,stream_throttle.rs}`, `quic.rs`). VERIFIED:

- `DEFAULT_MAX_STREAMS_PER_MS = 250`. Unstaked connections share 20%: 250 × 100 ms × 0.2 / 500 = **10 streams per 100 ms, about 100 TPS per unstaked connection**, fixed.
- The staked threshold is `1/(max_streams_per_ms × 100)` = **1/25,000 of stake = 17,525 SOL** today. Below that, a staked key is treated as unstaked.
- When the unstaked table is full, `prune_unstaked_connection_table` calls **`prune_oldest`** to bring the table down to 90%, evicting the connections whose `last_update` is oldest. Then the new connection is **always added**. **`last_update` advances only when a stream (transaction) finishes**, not on QUIC pings (`quic.rs` line ~1184). An idle pre-warmed connection is therefore the first one evicted.
- Staked peers sit in their own table and are pruned by `prune_random(2, stake)`. They can be evicted only by a higher-stake peer.

What Triton reports about the regime of 500 unstaked slots: "just 62 entities (500 / 8) could occupy all unstaked slots… constant evictions and delayed, or even entirely failed transaction landing" ([Triton](https://blog.triton.one/evolution-of-solanas-stake-weighted-quality-of-service-from-the-agave-side/)). LIKELY. That is the regime a proxy running relayer defaults would be in, and a proxy is busier than one validator, because it is the "next leader" for dozens of validators (BAM AMS fronts 67 identities, 10.2% of stake).

### For comparison: what Agave validators run today (direct-TPU leaders, 27% JitoLabs)

| | Agave v4.3.1 (deployed) | Agave master 4.5.0-alpha.2 |
|---|---|---|
| Unstaked / staked connection slots | 2,000 / 2,000 | 3,000 / 2,000 |
| Connections per unstaked / staked peer | 8 / 16 | 8 / 16 |
| New connections per IP per minute | **8** | 8 |
| Unstaked TPS per connection | **200**, fixed (`MAX_UNSTAKED_TPS`) | 400, falling to 100 above 70% load |
| Staked throttling | only above 95% load | above 76% load |
| Staked threshold | 1/50,000 = **8,762 SOL** | same |

Sources: `streamer/src/quic.rs` and `nonblocking/stream_throttle.rs` at tags `v4.3.1` and `master`, plus `nonblocking/swqos.rs` on master (prune_oldest to 90% kept). VERIFIED.

- **Agave PR #10666** proposed **parking unstaked connections at MAX_STREAMS = 0 when the TPU is saturated**. The page shows it **Closed, not merged**, last activity Sep 2026, with a `--tpu-swqos-mode` flag that defaults to `sleep` ([PR](https://github.com/anza-xyz/agave/pull/10666)). VERIFIED. If a later version ships it, unstaked senders get 0 TPS during saturation. Triton says saturation (≥95% of about 450k TPS) "was never triggered" in a three-week mainnet test. LIKELY irrelevant at pump.fun scale.
- **Forwarding.** An Agave validator forwards only packets flagged `is_from_staked_node()` (`core/src/forwarding_stage.rs::initial_packet_meta_filter`, v4.3.1), to the next 3 leaders, in batches of 128, capped at 12 MB/s. So unstaked packets that reach a non-leader are **dropped, never forwarded**. Sending only to "the next leader" is fragile around slot boundaries; send to the current leader and the next one or two. VERIFIED.

---

## 3. Unstaked vs staked on chain: block-0 winners by leader class

**Data.** gap-2's 3,581 creates (rows.json), with reactive non-owner buys in the create slot. Each leader is classified by today's `getClusterNodes.clientId`: AgaveBam/FireBAM → BAM, HarmonicAgave/HarmonicFiredancer → Harmonic, JitoLabs, Firedancer/Frankendancer. "Untipped" means a *successful* buy with no transfer to any of the 150 known sender tip accounts. It includes direct TPU sends, staked-RPC sends and private senders. Failed transactions are excluded, because a failed transaction's tip never moves and so it always looks untipped. "Hot" means ≥6 reactive buy attempts in slot 0. Script: `scratchpad/g5/untipped2.py`.

| Leader class (ingress) | creates with a slot‑0 winner | **first winner untipped** | top‑3 winners untipped | all successful buys untipped |
|---|---|---|---|---|
| BAM (proxy), cold / hot | 194 / 77 | **33% / 30%** | 33% / 31% | 33% / 35% |
| Harmonic (proxy), cold / hot | 138 / 37 | **32% / 49%** | 29% / 35% | 29% / 33% |
| JitoLabs (mostly own TPU), cold / hot | 170 / 38 | **26% / 32%** | 22% / 27% | 22% / 30% |
| Firedancer (own TPU), cold / hot | 45 / 24 | 42% / 29% | 36% / 33% | 35% / 35% |
| **All**, cold / hot | 556 / 178 | **31% / 34%** | 28% / 31% | 28% / 33% |

- **Proxy-fronted leaders do not penalize the no-tip path.** If anything, its share is higher than on JitoLabs. VERIFIED (measurement). Hot launches do not cut that share. This argues against "unstaked connections are evicted exactly when it matters", but cannot prove it, because staked-RPC traffic is in the same bucket. LIKELY.
- Untipped first winners still pay: the median priority fee is **0.002 SOL** (p75 0.018), and the median CU price is about 25.8M µL/CU. Tipped first winners pay a median of 0.005 SOL. On BAM and Harmonic, the ordering inside a batch is by fee (critique-speed §3.1). Admission only gets you into the auction: as SVS puts it, SWQoS "doesn't put you in the front of the line… If you have low fees it will likely still be slow to land" ([SVS](https://docs.solanavibestation.com/services/solana-node-services/staked-rpc-services.md)).
- The only published staked-vs-other landing study is old and weak: Chorus One, Nov 2024, one validator's slots, time-to-inclusion only, not congestion-specific ([Chorus One](https://chorus.one/reports-research/transaction-latency-on-solana-do-swqos-priority-fees-and-jito-tips-make-your-transactions-land-faster)). It is not usable for block 0.

**Live probe from this container: not possible.** UDP egress returns only DNS (intercepted). A QUIC handshake to `cloudflare-quic.com:443` and to all 7 TPU targets timed out. The Ohio box is not reachable from here (no key, port 22 blocked). The probe was written and smoke-tested instead (§5).

---

## 4. What staked access costs (FRA/AMS)

| Option | What it buys | Latency effect for block 0 | Cost | Status |
|---|---|---|---|---|
| **Self-staked QUIC identity**: a vote account whose `node_pubkey` is our client key, with real stake delegated; no need to vote | Staked table at every Agave leader (stake read from epoch `staked_nodes()` of all delegated vote accounts, `runtime/src/bank.rs`), and at relayer-lineage proxies (`getVoteAccounts`, delinquent included) | None added; we still send direct | **≥8,762 SOL ≈ $932k** (Agave threshold 1/50,000). **≥17,525 SOL ≈ $1.86M** to clear relayer 2.2 defaults (1/25,000). A non-voting account earns nothing: forgone yield ≈ 3.60%/0.689 ≈ **5.2%/yr ≈ 456 SOL ≈ $48.5k/yr (≈$4.0k/mo)** on 8,762 SOL. Plus warm-up of 1–2 epochs | Mechanism VERIFIED (code). Proxies' thresholds UNVERIFIED |
| **`--staked-nodes-overrides` peering** with a validator | The validator treats our key as staked *at its own TPU* and then **forwards** our packets with its identity to the next 3 leaders (`forwarding_stage.rs`) | **Adds a hop plus forwarding batching** (RPC/client → validator → leader proxy), so it is slower than direct in the same metro | No public price. The solana.com guide says such deals are "made directly between RPC operators and Validators" ([guide](https://solana.com/developers/guides/advanced/stake-weighted-qos)). ERPC sells it as a **+€100/mo** dedicated-node add-on "with 1,000 SOL assigned" (Frankfurt), or shared at "1 TPS per 4.2 elSOL held" on €42–598/mo plans ([ERPC](https://erpc.global/en/staked-connection/)) | Price VERIFIED (vendor page). Revocable at the validator's will |
| **Helius Sender `?swqos_only=true`** (`fra-sender`, `ams-sender`) | "Single fast SWQOS path". Helius's validator HEL1… holds 3.61% of stake, on Agave 4.3 with its own TPU | One extra hop in the same metro (HTTP) | **0.000005 SOL tip + priority fee**, no credits; 1 tx/s keyless, 50 tx/s per region with a key. **Helius itself recommends Sender Max (0.001 SOL) for "competitive launches"** ([Sender](https://www.helius.dev/docs/sending-transactions/sender.md), [swqos-only](https://www.helius.dev/docs/sending-transactions/sender-swqos-only)) | VERIFIED. Whose stake it uses is LIKELY Helius's own |
| **SVS staked RPC** | RPC → "primary staked validator partner" (0.1 ms away), ">500,000 SOL in stake weighted peering" | RPC hop plus peering hop. Partner location not stated (SVS lists ATL and AMS) | **$300 / $500 / $1,000 a month** for 25 / 50 / 100 sendTransaction r/s ([SVS staked RPC](https://docs.solanavibestation.com/services/solana-node-services/staked-rpc-services.md)). Lightspeed adds a 0.001 SOL minimum tip, `?only=swqos` ([Lightspeed](https://docs.solanavibestation.com/api-reference/lightspeed/lightspeed.md)) | VERIFIED |
| **Everstake SWQoS** | "stake-based priority lane", RPC or QUIC endpoint | One hop; regions not named | PAYG **0.0005 SOL/tx minimum tip**; Base $500/mo, Pro $1,300/mo ([Everstake](https://everstake.com/products/swqos)) | VERIFIED price |
| **Astralane Iris** (fr, fr2, ams, ams2) | FIFO router; QUIC available on all tiers | One hop in the same metro | Free: 0.001 SOL min tip, 5 TPS. VIP1/2: 0.0001 SOL, 20/40 TPS. **VIP3: 0.00001 SOL, 80 TPS** at 8 SOL of tips per rolling 7 days (the table says VIP1 = 4 SOL, the FAQ says 3 SOL). **Instant upgrade by prepaying the threshold.** "All tiers receive the same scheduling priority" ([tiers](https://astralane.gitbook.io/docs/low-latency/send-txn-fee-tiers.md)) | VERIFIED |

**Cost reading.**
- Stake capital is out of proportion to the benefit. Admission at 1/50,000 gives no ordering priority, and the evidence in §3 says the unstaked path already wins blocks.
- **The best-value staked fan-out is a per-transaction tip: Helius `swqos_only` (5,000 lamports) plus Astralane VIP3 (10,000 lamports).** These cost about $0.0005 and $0.001 per attempt, against $0.106 for the 0.001 SOL senders.
- Astralane VIP3 is effectively a one-time 8 SOL (≈$850) prepay that is then consumed as tips.
- The monthly staked RPCs (SVS, Everstake, ERPC) are fine backups and sell paths, but each adds a hop. They are not the block-0 leg.

---

## 5. How to settle the rest in one hour from the Frankfurt box

`deploy/tpu_admission_probe.py` (aioquic) is committed to the repo. It sends **no transactions**: it opens QUIC connections with ALPN `solana-tpu` and records Agave/relayer close codes (1 `dropped` = evicted, 2 `disallowed` = rate-limited or unstaked-disallowed, 4 `too_many`).

1. **Rate-limit fingerprint** (identifies the code lineage):
   ```
   python3 tpu_admission_probe.py burst 70 198.13.140.227:5010     # BAM FRA
   python3 tpu_admission_probe.py burst 70 198.13.140.105:9007     # Harmonic FRA
   ```
   - Refused at the 9th connection → Agave ≥3.x streamer (8/min).
   - Refused at about the 65th → jito-relayer 2.2 defaults (64/min).
   - Never refused → custom limits.
   - Refused at the 1st → allow-list.
   - Run it from a secondary IP, since it burns that IP's per-minute budget.
2. **Eviction pressure**:
   ```
   hold 900 <6 proxy TPUs> <3 direct Agave/FD TPUs>
   ```
   Run it at a quiet hour and again at 14:00–20:00 UTC, the peak launch hours. If the proxies close idle connections with code 1 within minutes, the table is under pressure. Then keep one tiny stream fresh, or reconnect just in time; see the playbook.
3. **Tpu-forwards check.** `hold 5` against a proxy's `tpuForwardsQuic`. Code 2 confirms the relayer behaviour "unstaked disallowed on forwards". Never send unstaked traffic there.
4. **Landing A/B, in production.**
   - For every snipe, send the same intent through direct QUIC, Helius `swqos_only` and Astralane, all signed against one **shared durable nonce**, so that only one can land (speed-gap-4).
   - Make each path's priority fee differ by 1 µL/CU, so that the landed transaction identifies its path.
   - Tabulate wins by leader class and hotness. This turns §3's LIKELY into VERIFIED for *our* box.
5. Ask Harmonic (Discord or intake form) and Jito (BAM Discord) to state publicly whether their proxies accept unstaked QUIC senders. Ask for the `max_unstaked_connections`, per-IP rate and staked threshold values.

---

## 6. Playbook implications for the sender

- **Primary leg.** Keep a warm QUIC connection to the current leader's advertised `tpuQuic` and the next 1–2 (BAM node, Harmonic Remote TPU or validator). Present a stable self-signed ed25519 identity: the table key is the pubkey when a cert is presented, so the per-peer cap of 8 applies per key. Hold **1–2 connections per proxy, not 8**, so we stay a small target.
- **Eviction-safe sending.** Assume an idle connection may have been pruned. On send, write to the warm connection; if it has been closed (we see `ConnectionTerminated` code 1), re-handshake at once. That costs about 1 RTT in the same metro, which is small against BAM's 20–50 ms rounds and Harmonic's 50 ms FBA windows.
  - Do **not** reconnect in a loop. Agave allows 8 new connections per IP per minute per server; beyond that the server closes with "disallowed".
  - Spread the reconnect budget across two source IPs.
- **Always fan out in parallel** (shared nonce) to Helius `fra-sender`/`ams-sender` `swqos_only`, and to Astralane `fr`/`ams` at VIP3. Bump to Sender Max or Nozomi (0.001 SOL) only on launches where the expected edge exceeds about $0.1. The fan-out covers exactly the failure we cannot rule out: an unstaked eviction or rate-limit at the moment of a hot launch.
- **Do not buy stake. Do not pay monthly for staked RPC as the block-0 path.** Revisit only if the §5 probe shows code-2 refusals at the first connection (an allow-list), or a ≥10-point landing gap between direct and swqos in the A/B.
- During **BAM maintenance mode**, about 36% of stake moves back to the validators' own Agave 4.3 TPUs or Jito relayers. The 2,000-slot unstaked tables there (relayers: 500) improve unstaked odds further. Re-key the routing table on the `clientId` flip (speed-gap-2).

---

## 7. Open items

- The real admission parameters of the BAM node and the Harmonic Remote TPU: unstaked slots, per-IP rate, staked threshold, possible allow-lists. Closed code, silent docs. Settle with the §5 probe and by asking the operators.
- Whether untipped block-0 winners are unstaked direct senders or staked RPC traffic. On-chain data cannot separate them; the §5 A/B can.
- Whether Agave 4.4 (Alpenglow, scheduled for Nov 9 mainnet activation) changes streamer QoS defaults. Re-read `streamer/src/quic.rs` at the v4.4 tag when it is cut.
- How many new connections per minute a jito-relayer Remote TPU tolerates after any operator tuning.

## Sources

- Harmonic docs: [architecture](https://docs.harmonic.gg/concepts/harmonic-architecture.md), [faq](https://docs.harmonic.gg/reference/faq.md), [endpoints](https://docs.harmonic.gg/run-a-validator/endpoints.md), [salsa-4](https://docs.harmonic.gg/run-a-validator/setup/salsa/salsa-4.md), [samba](https://docs.harmonic.gg/run-a-validator/setup/samba.md), [harmonic-bundles](https://docs.harmonic.gg/searchers/harmonic-bundles.md), plus a docs `?ask=` query, 2026-10-14.
- BAM: [bam-overview](https://bam.dev/docs/bam/bam-overview/), [maker plugin](https://bam.dev/docs/bam/maker-plugin/how-it-works/), [validators](https://bam.dev/validators/), [node-operators](https://bam.dev/node-operators/), [Helius BAM explainer](https://www.helius.dev/blog/block-assembly-marketplace-bam), [Immunefi BAM client scope](https://immunefi.com/bug-bounty/jito-bam-client/scope).
- Code (raw.githubusercontent): `jito-foundation/jito-relayer@master` (`transaction-relayer/src/main.rs`, `core/src/tpu.rs`, `core/src/staked_nodes_updater_service.rs`, `Cargo.toml`); `anza-xyz/agave` at `v2.2.20`, `v4.3.1` and `master` (`streamer/src/quic.rs`, `streamer/src/nonblocking/{quic.rs,stream_throttle.rs,swqos.rs}`, `core/src/forwarding_stage.rs`, `core/src/staked_nodes_updater_service.rs`, `runtime/src/bank.rs`); `jito-foundation/jito-solana@v4.3.0-jito.1` (`core/src/bam_manager.rs`); [agave PR #10666](https://github.com/anza-xyz/agave/pull/10666).
- [Triton SWQoS blog](https://blog.triton.one/evolution-of-solanas-stake-weighted-quality-of-service-from-the-agave-side/), [solana.com SWQoS guide](https://solana.com/developers/guides/advanced/stake-weighted-qos), [Chorus One](https://chorus.one/reports-research/transaction-latency-on-solana-do-swqos-priority-fees-and-jito-tips-make-your-transactions-land-faster).
- Vendors: [Helius Sender](https://www.helius.dev/docs/sending-transactions/sender.md), [Helius swqos-only](https://www.helius.dev/docs/sending-transactions/sender-swqos-only), [SVS staked RPC](https://docs.solanavibestation.com/services/solana-node-services/staked-rpc-services.md), [SVS Lightspeed](https://docs.solanavibestation.com/api-reference/lightspeed/lightspeed.md), [Astralane tiers](https://astralane.gitbook.io/docs/low-latency/send-txn-fee-tiers.md), [ERPC](https://erpc.global/en/staked-connection/), [Everstake](https://everstake.com/products/swqos).
- Live RPC: `getClusterNodes`, `getVoteAccounts`, `getSupply` and `getInflationRate` on api.mainnet-beta.solana.com; CoinGecko SOL/USD; gap-2 `rows.json`, reanalyzed by `scratchpad/g5/untipped2.py`.
