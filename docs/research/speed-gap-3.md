# Speed gap 3: DoubleZero Edge coverage per leader metro, what "repair" means, and covering the other 47%

**Question.** For each leader metro, what share of leader slots do DoubleZero (DZ) Edge leader-shred publishers cover? Only 52.95% of stake publishes leader shreds, and only about 19% publishes in Frankfurt (FRA).

Follow-ups:
- What does the API field `slots_needing_repair` (86% of publisher leader slots) mean for when a create's first data shreds reach a FRA subscriber?
- Which source best covers the other ~47% of stake: DZ retransmit/root groups, Helius Shred Delivery, Everstake Direct Shreds, SVS, or Triton `SubscribeDeshred`?
- For each source: what is the concrete onboarding, and what is the true monthly price?

**Method.**
- DZ data API, pulled 2026-10-14 between 09:08 and 09:12 UTC: `/openapi.json`, `/edge/shreds/{overview,devices,client-seats,subscribers,publishers/leaders}` with `epochs=1`, `slots=5000` and `slots=500`.
- Mainnet RPC `getEpochInfo`, `getLeaderSchedule` (epoch 1057, all 432,000 slots) and `getRecentPerformanceSamples`.
- Stakewiz `/validators` (`ip_city`), used to map each leader to a metro.
- Source code: jito-solana `v4.3.0-jito.1` (`turbine/src/broadcast_stage.rs`, `turbine/src/retransmit_stage.rs`, `core/src/multicast_shred_check_service.rs`) and agave `v4.3.1` (`ledger/src/shred.rs`, `turbine/src/cluster_nodes.rs`), all from raw.githubusercontent.
- Vendor docs: DZ, Helius, Everstake (`llms-full.txt`), SVS and Triton.
- Scripts and raw JSON are in the session scratchpad (`cov.py`).

---

## 0. TL;DR

1. **Coverage is uneven by metro.** Leader slots this epoch whose leader publishes leader shreds to DZ (VERIFIED, computed):
   - FRA: **55.6%**, all of it in-metro.
   - AMS: 56.3%. LON: 84.9%. TYO: 49.7%. NYC: 62.1%. SIN: 64.6%. DUB: 93.7%.
   - Šiauliai (Cherry, 4.5% of all slots): **4.4%**.
   - WAS, LAX and CHI: about 16%.

   Network-wide that is **53.0% of slots**, and **19.2% of all slots are in-metro to a FRA seat**. About **24% of all slots** are FRA or AMS leaders that do not publish. Those slots are the gap that matters.
2. **The on-chain price of $100 per epoch is a legacy, frozen system. The real DZ price is $1,500/mo for a FRA seat.**
   - The DZ docs say the CLI/on-chain seat system "is being decommissioned on August 30, 2026". New seats are "billed per month, per machine": FRA/AMS **$1,500**, LON/NY/SGP/TYO $900, others $450.
   - The API agrees. Every device shows `granted_seats = 0`. No seat has `funded_epoch` later than 1027 (the current epoch is 1057), and the last escrow activity was 2026-09-24.
   - So **critique-speed C12 (≈$3,000/mo) is wrong, and data-feed's $1,500 is right.** VERIFIED.
   - Epochs are about 26 h, not 24 h: 432,000 slots at about 218 ms/slot (268–280 slots per 60 s sample). Even under the legacy system, FRA would have been about $2,750/mo.
3. **`slots_needing_repair` almost certainly means "at least one shred of the slot was missing from multicast". It does not mean "the feed is late or partial by design".**
   - VERIFIED from the code: a publishing Jito-Agave leader mirrors *every* shred it broadcasts (data and coding) to `233.84.178.1:7733`, in the same send pass as its Turbine root packets.
   - LIKELY from the data: about 0.15% packet loss reproduces the 86–88% figure. Shreds per slot are the same on 41%-repair and 100%-repair devices.
   - Merkle FEC sets are 32 data + 32 coding shreds, and any 32 recover the set. **If our deshredder does eager Reed-Solomon recovery, a "repair" slot costs about tens of µs, not a Turbine hop.**
4. **The DZ seat already includes something aimed at the other 47%.** jito-solana 4.3 auto-mirrors shreds a node receives *as Turbine root* (`root_distance == 0`) to the Root group `233.84.178.16`. The `solana-shreds-full` subscription carries leader + root + retransmit groups. VERIFIED (code and docs). How much root traffic actually flows is **UNVERIFIED**, because the API does not report root publishers, and `publishing_retransmitted` is false for all 805 rows.
5. **Best complement for the gap: Helius raw shreds in FRA ($1,000/IP/mo; $800 on Pro).** Self-serve, plain unicast UDP, works on EC2. Helius is the #2 stake node (3.61%), sits in FRA on TeraSwitch, and **does not publish to DZ**.
   - By Turbine math it receives every shred at hop 1 or 2. LIKELY: simulated P(root or layer 1) ≈ 1.0.
   - Everstake Direct Shreds costs **$4,000/IP/mo** for a 1.76%-stake FRA node, which is poor value.
   - SVS is $250/mo but AMS only, sourced from RPC nodes and peers. It is a cheap A/B baseline.
   - Triton Shred Streaming (FRA) aggregates DZ Edge + Triton + partners + Turbine, but the price is unpublished.
   - Triton `SubscribeDeshred` waits for `CompletedDataSets` (p50 6.3 ms after shred arrival, vendor figure), so it is too slow to be primary.
6. **Recommended feed:** one TeraSwitch FRA bare-metal box with a **DZ Edge FRA seat ($1,500)** plus **Helius raw shreds ($1,000)**. Take the first arrival per `(slot, fec_set, index)` and do eager FEC. That is **≈$2,500/mo**. Run a 7-day first-seen test per leader class before adding a third source.

---

## 1. Per-leader-metro coverage (epoch 1057)

How it was computed:
- Leader schedule for epoch 1057: 432,000 slots, 0 unmapped.
- Each leader identity is mapped to a metro from Stakewiz `ip_city`, by city-to-metro rules (for example Frankfurt/Rüsselsheim/Limburg → fra; Amsterdam/Diemen/Rotterdam → ams).
- A slot counts as "covered" when its leader has `publishing_leader_shreds = true` in `/edge/shreds/publishers/leaders?epochs=1`.
- Sanity check: for DZ publishers, the Stakewiz metro and `dz_metro_code` agree on **95.3%** of slots. Mismatches are mostly hkg↔sin and unmapped cities. VERIFIED.

| Leader metro | Share of all slots | DZ-covered (any metro) | DZ in same metro | Uncovered, % of all slots |
|---|---|---|---|---|
| fra | 34.07% | **55.6%** | 55.6% | **15.12** |
| ams | 20.94% | 56.3% | 56.3% | **9.14** |
| lon | 8.73% | 84.9% | 84.9% | 1.31 |
| tyo | 5.70% | 49.7% | 49.7% | 2.87 |
| other / unmapped | 5.19% | 37.4% | – | 3.25 |
| sqq (Šiauliai) | 4.48% | **4.4%** | 4.4% | **4.29** |
| nyc | 4.39% | 62.1% | 62.1% | 1.66 |
| sin | 4.22% | 64.6% | 64.6% | 1.49 |
| was | 3.83% | 16.0% | 16.0% | 3.22 |
| lax | 2.05% | 15.3% | 15.3% | 1.74 |
| dub | 2.05% | 93.7% | 93.7% | 0.13 |
| sto | 1.87% | 21.5% | 14.9% | 1.47 |
| chi | 0.97% | 16.7% | 16.7% | 0.81 |
| **All** | 100% | **53.00%** | FRA seat in-metro: **19.21%** | **47.0** |

Notes:
- Slot share tracks the stake numbers, so critique-speed §3.5's 52.95% / 19.15% are confirmed **on a slot basis**. data-feed's **57.3%** comes from `total_publisher_stake/total_network_stake`. That counts all 413 multicast-connected publishers, including 185 hot-spare (`is_backup`) rows and 247 rows that do not publish leader shreds. **Overstated for block-0 use.** VERIFIED.
- **Publishers really do publish on their slots.** In the current-epoch window, publishers' observed `leader_slots` / scheduled slots = **82,113 / 84,707 = 96.9%**. The remaining 3.1% is consistent with skipped slots. VERIFIED (computed).
- **Where the gap is.** 15.1% of all slots are non-publishing FRA leaders and 9.1% are non-publishing AMS leaders, about **24% of all slots** within 3 ms of us. Another 4.3% sit in Šiauliai, where almost nobody publishes, at about 25 ms from FRA.
  - These 24% are the slots that matter for block-0. A FRA subscriber gets them only via Turbine (hop 1 as root, or hop 2 as layer 1), or through the DZ root group if a DZ-connected root caught the shred.
- **Not in-metro but on DZ fiber:** AMS publishers are about 3 ms one-way away. Use FRA–AMS internet RTT 6 ms from data-feed; DZ fiber is similar or better. LON is about 7 ms. TYO is about 67 ms (FRA–TYO 134 ms RTT on DZ, per speed-geography). These are LIKELY one-way estimates (half the RTT).

### Snapshot caveat
Publisher membership changes every epoch. The `/publishers/leaders?epochs=1` total is 805 rows / 601 unique nodes. Re-pull it every epoch and key the routing table on `node_pubkey` (critique-speed §5).

---

## 2. What `slots_needing_repair` means for timeliness

**What the API says.** The schema description is only "Number of slots that required repair" (`EdgeShredsPublisher.slots_needing_repair`, `openapi.json`). No public doc defines it further. A web search found nothing. The data-API source is not public on raw.githubusercontent under a guessable path. The definition itself is **UNVERIFIED**.

**What publishers send.** VERIFIED from source, jito-solana `v4.3.0-jito.1`, `turbine/src/broadcast_stage.rs::broadcast_shreds`:
- The leader builds `all_packets` from the Turbine root peer (plus the next leader) for each shred.
- It then appends `for &addr in external_addrs { for shred in shreds { push(shred, addr) } }`. `external_addrs` includes the `multicast_receiver_address`, which is auto-set to `233.84.178.1:7733` by `MulticastShredCheckService` when a `/32` route to it exists, plus any `--shred-receiver-address`.
- The comment reads: "Mirror this validator's own broadcast shreds to external receivers".
- It sends the Turbine batch first, then the external batch, through `shred_receiver_socket` (or XDP), **in the same call**.
- So DZ gets **all** data and coding shreds of the leader's block, microseconds after the root peers do.
- Firedancer/Frankendancer use `additional_shred_destinations_leader` (DZ publish docs).

**What the data says.** Computed from `/publishers/leaders`:

| Window | Leader slots | Needing repair | Share |
|---|---|---|---|
| epochs=1 | 84,717 | 74,233 | 87.6% |
| slots=5000 | 2,656 | 2,301 | 86.6% |
| slots=500 | 265 | 212 | 80.0% |

- **It varies by the publisher's DZ device**, not by client:
  - `dz115a-tyo2-tsw` 41.1%, `dgt-dzd-lon-ths` 39.3%, `dz100a-iad1-tsw` 42.8%, `allnodes-fra1` 69.3%.
  - `dz-fra-01` 93.6%, `dz100a-fra2-tsw` 96.4%, `dz-ny7-sw01` and `dz103a-lon1-tsw` 100%.
  - The per-publisher median is 1.0.
- **Shreds per slot do not differ** between low-repair and high-repair devices: 1,260 vs 1,418–1,542, which tracks block size. So "repair" is not "half the slot missing".
- Per client, repair ranges from 79.6% (FireBAM) to 96.3% (JitoLabs). All clients deliver about 1,300–1,500 unique shreds per slot. Agave 4.3.1 has `DATA_SHREDS_PER_FEC_BLOCK = CODING_SHREDS_PER_FEC_BLOCK = 32`, so about 1,350 shreds is a full block of about 21 FEC sets.

**Interpretation (LIKELY).** The field counts slots where the DZ monitor did not receive *every* shred over multicast.
- With about 1,350 shreds per slot, independent loss of **p ≈ 0.15%** gives 1 − (1 − 0.0015)^1350 ≈ 87%. That matches the figure.
- The device dependence points to the publisher's GRE-over-internet on-ramp. DZ docs: "All DZDs are using DIA for user on-ramps."

**Consequence for a FRA subscriber (LIKELY):**
- **Loss is sparse.** Each 64-shred FEC set needs any 32. At p = 0.15%, P(unrecoverable set) is about 0. P(the one data shred holding our create is lost) is about 0.15%.
- **Recovery is immediate.** Data and coding shreds of a merkle FEC set are created together (the merkle root covers all 64) and sent in the same burst. If the create's data shred is lost, the next 32 shreds of that set arrive within the same burst, roughly 60 µs at 10 Gbit/s. Eager RS recovery then rebuilds it. So **"needs repair" ≠ "one Turbine hop late"**. That holds **only if** our deshredder:
  - (a) recovers as soon as 32 of 64 are in, and
  - (b) does not wait for Agave-style repair.

  The `rs-shred` plan in arch-proposal-1 (`reed-solomon-erasure`) is correct. Make "recover at 32/64" a test case.
- **Open risk.** If the loss is bursty (microbursts at the on-ramp), whole sets could drop. Measure by subscribing: log the per-set received count and the recovery rate per publisher. DZ Discord `#jito-shredstream` was not accessible from here.

---

## 3. Sources for the other ~47%: mechanics, onboarding, price

### 3.1 DZ Root and retransmit groups (included in the DZ seat)
- **Root group.** jito-solana 4.3 `retransmit_stage.rs::retransmit_shred` adds `multicast_root_receiver_address` (`233.84.178.16:7733`) **only when `root_distance == 0`**. That means the node received the shred directly from the leader as the Turbine root. The address is auto-enabled by a second `MulticastShredCheckService` when a route exists, and it can be disabled with `--disable-multicast-shred-check`. VERIFIED (code).
  - For a non-publishing leader, each shred's root is a stake-weighted random node. **If root publishers held about 50% of stake, about half of every non-DZ leader's shreds would hit multicast after one internet hop**, from wherever that root sits.
  - With FEC (any 32/64), you can recover a set with roughly the median root path. That is the lowest-latency path possible for non-publishers, short of being the root yourself.
  - **UNVERIFIED:** who publishes root. The API has no root field. The publish docs do not ask validators to join the root group.
- **Retransmit groups** EU/APAC/AMER (`233.84.178.12–.14`): "high-stake Retransmit Shreds" per the subscribe docs. That is Turbine hop 2 (leader → root → layer-1 node). The API flag `publishing_retransmitted` is **false for all 805 rows**, so the API shows no qualifying publisher. Measure it.
- **Onboarding** (DZ subscribe docs, VERIFIED):
  - Hardware: x86_64 Ubuntu, "public IP address (no NAT)" (setup docs).
  - Install `doublezero` + `doublezero-solana`.
  - Firewall: GRE (IP proto 47) out, BGP tcp/179 on 169.254/16 over `doublezero1`, PIM out, udp/7733 in on `doublezero1`, udp/44880 in on `doublezero0`.
  - Apply at doublezero.xyz/edge/subscribe: pick a metro and a DZ ID pubkey (no IP binding; access can move between IPs in the metro). Approval in 1–3 business days. Invoice, then `doublezero connect multicast --subscribe-feed solana-shreds-full`.
  - Packets arrive GRE-encapsulated; strip them in XDP. ToS forbid retransmitting the data.
- **Cross-connect.** No user cross-connect product is documented; users on-ramp by GRE over DIA ("CYOA … DIA", glossary/requirements). A box in the TeraSwitch building of `dz100a-fra2-tsw` (37-seat capacity, 0 granted on-chain) reaches the DZD over TeraSwitch's own internet edge, probably a sub-ms in-building path (LIKELY). A physical cross-connect would need a private arrangement with DZ/TeraSwitch (UNVERIFIED).
- **AWS.** EC2 public IPs are 1:1 NAT, which conflicts with "no NAT". The tunnel also needs PIM/BGP over GRE. **Plan for bare metal**; an EC2 attempt is UNVERIFIED and low priority.
- **Price:** **$1,500/mo per machine in FRA/AMS**, monthly invoice (subscribe docs). VERIFIED. The $100/epoch figure belongs to the legacy on-chain system (frozen, see §0.2).

### 3.2 Helius Shred Delivery (raw UDP)
- **Source.** Helius validator: 3.61% stake (#2 by stake), Frankfurt, TeraSwitch; **not a DZ publisher** (Stakewiz + DZ API).
  - Turbine `DATA_PLANE_FANOUT = 200` (agave 4.3.1 `cluster_nodes.rs`), and the top 201 nodes hold 85.3% of stake.
  - Simulated weighted shuffle: a 3.6% node is root or layer-1 for ≈100% of shreds, against about 25% for a 0.05% node.
  - So Helius receives **every leader's every shred at hop ≤2**, and its own slots (about 3.6%) at hop 0. LIKELY (simulation, A-ES approximation of Agave's shuffle).
- **Onboarding.** Dashboard → Shreds → Add Seat. Pay by card or USDC. Enter `IP:PORT`. Shreds arrive in about 5 s. Each seat binds to one IP; changing the IP issues a new token. Helius auto-detects the closest region. **Plain unicast UDP**, so EC2 and bare metal both work. VERIFIED (raw-shreds docs).
- **Price:** **$1,000/IP/mo**, or $800 on the Pro plan. No refunds mid-cycle. VERIFIED. The Pro plan's own price was not checked here.

### 3.3 Everstake
- **ShredStream (shared proxy).** 7 regions including FRA. "#2 globally in Frankfurt", sub-10 ms (vendor). Auth is `x-token`. You give a region and IP:port list; there is a 1-hour trial. Tiers are Connect (one region) and Ultra (FRA+AMS+TYO+NY dedicated lane). **Price is shown only in the dashboard.** UNVERIFIED.
- **Direct Shreds.** Dedicated per-customer link from Everstake validators, "3 ms p50 in-region", source-IP allow-list, about 30 minutes provisioning. **$4,000/mo per dedicated IP** (`llms-full.txt`). VERIFIED.
- The Everstake validator holds 1.76% stake in Frankfurt (Allnodes) and is not a DZ publisher. It also sits in layer 1, so coverage is the same Turbine position class as Helius, with half the root probability. **Not worth 4× Helius.**

### 3.4 SVS (Solana Vibe Station) raw shred forwarding
- **Sources:** "shared RPC nodes, validator peers and some Jito block-engine locations". There is no leader position at all.
- **Locations:** Amsterdam (buy in Discord `#purchase-services`: **$25/day, $75/week, $250/month**) and Atlanta (**$300/mo**, only with an SVS VPS). Delivered to UDP 20000. Cancellation by ticket, pro-rated. VERIFIED.
- SVS's own test (2026-10-13): median lead of 7 ms over its gRPC `processed` stream. That is vendor data and says nothing about lead versus other shred feeds.
- **No FRA location; useful only as a cheap AMS baseline.** data-feed's "$150/mo" is outdated.

### 3.5 Triton
- **Shred Streaming (raw UDP).** Regions: NY, LON, AMS, **FRA**, TYO. It aggregates "Triton validators", **"DoubleZero Edge: through a special partnership"**, partner validators, and Turbine, deduped first-copy-wins, over Triton's own multicast network. Onboarding: ask support in the Customer Portal, add IP:port per region, allow source `67.209.53.15`. VERIFIED (docs.triton.one/chains/solana/shred-streaming).
  - **Price not published.** UNVERIFIED.
  - It adds at least one relay hop over a direct DZ seat for publisher slots. It could be the best *single* feed for the non-DZ slots if its partner and Turbine ingest beats Helius's L1 position. Measure it.
- **`SubscribeDeshred` (gRPC).** On "all plans", shared and dedicated. It hooks at **`CompletedDataSets`** and reports p50 **≈6.3 ms / p90 ≈20 ms from shred arrival** (Triton blog, updated 2026-10-05; vendor).
  - Streaming is bandwidth-billed at about $0.08/GB according to third-party comparisons. UNVERIFIED for Deshred.
  - Because it waits for a completed data set, it is **structurally later** than our own streaming deshred on raw shreds. Use it as a fallback or for validation, not as primary.

### 3.6 Summary

| Source | Covers non-DZ leaders? | Hop for non-DZ FRA leader | FRA? | Onboarding | Works on EC2 | $/mo |
|---|---|---|---|---|---|---|
| DZ Edge leader group | No (53% of slots) | – | yes | apply + invoice, GRE/BGP/PIM | unlikely (no-NAT) | 1,500 |
| DZ Root group | Partly, unknown share | 1 (via a DZ root) | yes | same seat | same | incl. |
| DZ retrans EU | All, if any publisher | 2 | yes | same seat | same | incl. |
| Helius raw | All | 1–2 (L1 always) | yes | self-serve, ~5 s | yes | 1,000 (800 Pro) |
| Everstake Direct | All | 1–2 | yes | dashboard, ~30 min, IP allow-list | yes | 4,000 |
| Everstake ShredStream | All | proxy + 1–2 | yes | dashboard, token | yes | unpublished |
| Triton raw | All (DZ + Turbine + partners) | ≥1 + relay | yes | via support | yes | unpublished |
| Triton Deshred | All | + CompletedDataSets wait | yes | plan | yes | bandwidth-billed |
| SVS AMS | All (RPC/peers) | ≥2 | AMS only | Discord | yes | 250 |

---

## 4. What this means for the build

1. **Feed stack v1 (≈$2,500/mo):** a FRA bare-metal box on TeraSwitch, in the same building as `dz100a-fra2-tsw` and Helius's validator.
   - (a) A DZ Edge FRA seat covers 55.6% of FRA-leader slots and 53% overall at hop 0. It also includes the root and retransmit groups.
   - (b) Helius raw shreds go to the same box and cover 100% of leaders at hop ≤2.
   - Dedupe on `(slot, fec_set_index, index, data/coding)` and keep the first arrival.
2. **Eager FEC is mandatory.** It turns "86% needing repair" into a non-issue. It also turns Turbine hop-2 feeds into roughly "median-root" latency, because a set completes once its 32 fastest shreds arrive.
3. **Block-0 eligibility model.** On about 53% of slots (DZ publishers), detection can be at about leader + 1–3 ms in FRA/AMS. On about 24% of slots (non-DZ FRA/AMS), it is Turbine root/L1 timing: a few ms extra, LIKELY. On about 23% (other metros), it is dominated by geography regardless of the feed. Feed choice therefore moves the needle only on the ~24% FRA/AMS non-publisher slots. That is where an A/B test of Helius vs Triton vs DZ-root must focus.
4. **Correct the earlier docs.**
   - data-feed: coverage 57.3% → **53.0% of slots**; the $1,500 price stays.
   - critique-speed C12: **retract** the ≈$3,000/mo figure.
   - arch-proposal-1 M9: "DZ Edge ~$3k/mo" → $1,500.
5. **Routing table** (critique-speed §5) per leader: add `dz_leader_publisher`, `dz_metro`, and the per-publisher repair ratio. Refresh each epoch.

## 5. Open questions (measure on day 1 of the paid feed)
- Root-group (`233.84.178.16`) volume, and which non-DZ leaders it covers. Is the EU retransmit group populated?
- The per-publisher distribution of received shreds per FEC set: is loss independent or bursty?
- First-seen deltas for DZ vs Helius vs Triton raw, by leader class (DZ-FRA, DZ-AMS, non-DZ-FRA, non-DZ-AMS, sqq), over 7 days.
- Whether DZ will grant a physical cross-connect at TeraSwitch FRA2. Whether GRE/PIM tunnels come up on EC2 at all (low priority).
- Triton raw-shred and Everstake ShredStream prices (sales quotes).

## Sources
- DZ API: https://data.doublezero.xyz/api/v1/openapi.json; `/edge/shreds/overview`, `/devices`, `/client-seats`, `/subscribers`, `/publishers/leaders?epochs=1|slots=5000|slots=500` (pulled 2026-10-14 ~09:10 UTC).
- DZ docs: https://docs.doublezero.xyz/solana/edge/subscribe/ ; /solana/edge/subscribe-cli/ (legacy, "decommissioned August 30, 2026"); /solana/edge/publish/ ; /setup/ ; /reference/glossary/ ; /contributors/requirements/.
- jito-solana v4.3.0-jito.1 (raw.githubusercontent.com/jito-foundation/jito-solana/v4.3.0-jito.1/…): `turbine/src/broadcast_stage.rs`, `turbine/src/retransmit_stage.rs`, `core/src/multicast_shred_check_service.rs`.
- agave v4.3.1: `ledger/src/shred.rs` (32/32 FEC), `turbine/src/cluster_nodes.rs` (`DATA_PLANE_FANOUT = 200`).
- Solana RPC: `getEpochInfo` (epoch 1057, slotIndex 159,418 of 432,000), `getLeaderSchedule`, `getRecentPerformanceSamples` (268–280 slots per 60 s).
- Stakewiz: https://api.stakewiz.com/validators (ip_city, stake).
- Helius: https://www.helius.dev/docs/shred-delivery ; https://www.helius.dev/docs/shred-delivery/raw-shreds
- Everstake: https://docs.blockspace.everstake.com/shredstream/quickstart ; /shredstream/pricing ; /llms-full.txt (Direct Shreds $4,000).
- SVS: https://docs.solanavibestation.com/services/raw-shred-forwarding
- Triton: https://docs.triton.one/chains/solana/shred-streaming ; https://blog.triton.one/deshred-transactions-the-fastest-path-to-solana-data/
