# Critique of the speed research package (2026-10-14)

**Reviewed in full:**
- `speed-data-feed.md`
- `speed-tx-landing.md`
- `speed-geography.md`
- `speed-reference-impls.md`

**Cross-read for consistency:** `pumpfun-mechanics-2026.md` and `skeptic-factcheck.md`.

**Labels:**
- **VERIFIED** means a primary source or a measurement I made today.
- **LIKELY** means a strong inference or a single credible source.
- **UNVERIFIED** means a vendor claim or a guess.
- "Critic check" marks something I re-fetched or computed myself while writing this.

The helper scripts are in the session scratchpad (`crit/`, `dzagg.py`, `v1scan.py`) and are not committed. Every number below cites the endpoint it came from, so it can be re-run.

---

## 0. Verdict

1. **The core recommendation survives every correction below.** Put the hot path in Frankfurt, preferably in a TeraSwitch building, not in Ohio. I re-ran the geography model with the corrections that follow and the site ranking did not change (§3.9).
2. **The package models the leader wrongly.** It treats every leader as one continuous scheduler where arrival time wins. In fact **about 63% of stake sequences transactions through an external proxy that batches them**:
   - **BAM:** AgaveBam 36.3% plus FireBAM 1.6%.
   - **Harmonic:** HarmonicAgave 18.1% plus HarmonicFiredancer 7.0%, for 25.1%. Harmonic is never quantified in any of the four docs.
   - Harmonic's default strategy is a **50 ms frequent batch auction (FBA) ordered by priority fee**. Arrivals inside the same window are treated as simultaneous.

   This changes four things in the docs: the "colocation > fan-out > CU trimming > fees" ordering (tx-landing §8), the `c = 5 ms` overhead in the geography model, the `Race` score, and the "hardcore mode" bid policy.
3. **Three defects would break the build:**
   - (a) `speed-reference-impls.md` recommends `helius-rust-sdk` `src/preconf.rs` as "a complete decoder". It decodes with **bincode**, and Helius's own docs say bincode **fails on transaction v1**. In a 20-block sample, about 88% of pump.fun transactions were v1.
   - (b) `speed-tx-landing.md` §5 recommends **v1 transactions and an ALT together**, but v1 has no ALTs. It also never mentions that in v1 an unset compute-unit limit defaults to **0**.
   - (c) After Alpenglow (epoch 1064, about Oct 21, seven days away), shred batches carry **block markers** (header, footer, UpdateParent). The recommended `sol-shred-sdk` path parses batches with `wincode::deserialize_exact::<Vec<Entry>>(..).ok()`, which will most likely reject and **silently drop** them.
4. **One cost is probably wrong.** DoubleZero Edge seats are priced **per epoch** on-chain ($100 USDC per epoch in FRA/AMS). Epochs halved to about 24 h on Oct 9, so FRA is **≈ $3,000/mo**, not the $1,500 in data-feed.
5. **Several "open questions" in the docs can already be answered:**
   - Alpenglow's activation epoch.
   - Whether Harmonic delays shreds by about 300 ms. It does not: shreds stream continuously.
   - The Nozomi `fra1`/`fra2` host question. Both resolve to the same IP.
   - Which block-engine region a leader uses. Jito's `GetConnectedLeadersRegioned` answers it.
   - The `buy_v3` buyback recipient.

---

## 1. What I checked, and how

| Check | Source / method | Result |
|---|---|---|
| All pump instruction, event and account discriminators | `sha256("global:"/"event:"/"account:" + name)[..8]`, computed locally | All 19 values in data-feed and reference-impls match. **VERIFIED** |
| `buy_v3` accounts and buyback recipient | [TRADE_V3.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/instructions/TRADE_V3.md) | 17 accounts, as documented. Account 14 is "one of the 8 buyback fee recipient wallets… SOL-paired coin: pass the wallet itself… **Must exist**". **VERIFIED** |
| `create_v2` optional tail | [COIN_CREATION.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/instructions/COIN_CREATION.md) | 1-based accounts 17–19 are `quote_mint`, `associated_quote_bonding_curve` and `quote_token_program`, and all three must be present together. Pump-coin quotes then add `quote_control`, the quote coin's bonding curve, and its pool and vaults. **VERIFIED** |
| Transaction v1 format | [SIMD-0385](https://raw.githubusercontent.com/solana-foundation/solana-improvement-documents/main/proposals/0385-transaction-v1.md) | No ALTs. Max size 4096 bytes. Priority fee is **total lamports** (mask bits 0–1). An unset CU limit means **0**. An unset loaded-accounts data size means 0 at runtime, costed as 32 KiB. Signatures at the tail. Version byte 129. **VERIFIED** |
| SIMD-0296 | [proposal](https://raw.githubusercontent.com/solana-foundation/solana-improvement-documents/main/proposals/0296-larger-transactions.md) | Titled "Larger Transaction Size". It is not the v1 format. **VERIFIED** |
| Alpenglow and slot times | [solana.com/upgrades/alpenglow](https://solana.com/upgrades/alpenglow), [reduced-slot-times](https://solana.com/upgrades/reduced-slot-times) | Epoch 1064 starts Wed Oct 21 at about 15:00 UTC. TowerBFT continues for the first 5,000 slots; Alpenglow consensus starts at **slot 459,653,000**. "Firedancer and Frankendancer are not supported during the migration"; validators must run Agave, Jito-Solana or Harmonic Salsa on Agave 4.3.0. Blockhash window is 150 blocks, **30 s**. Leader span is still 4 slots. An epoch is still 432,000 slots, about 24 h. **VERIFIED** |
| Jito | [lowlatencytxnsend](https://docs.jito.wtf/lowlatencytxnsend/), [lowlatencytxnfeed](https://docs.jito.wtf/lowlatencytxnfeed/), [searcher.proto](https://raw.githubusercontent.com/jito-labs/mev-protos/master/searcher.proto) | 50 ms auction ticks. 1 req/s per IP per region. ShredStream shuts down Sep 5, 2026, with migration to DZ Edge. `GetConnectedLeadersRegioned` and `NextScheduledLeaderResponse.next_leader_region` exist. **VERIFIED** |
| Harmonic | docs.harmonic.gg: [scheduling-strategies](https://docs.harmonic.gg/concepts/scheduling-strategies.md), [streaming-mode](https://docs.harmonic.gg/concepts/streaming-mode.md), [architecture](https://docs.harmonic.gg/concepts/harmonic-architecture.md), [endpoints](https://docs.harmonic.gg/run-a-validator/endpoints.md), [faq](https://docs.harmonic.gg/reference/faq.md) | "**Shreds always stream continuously.**" Default strategy is `fba`, with 50 ms double-buffered batches ordered by priority fee plus tips. "Harmonic tips are compute-unit priority fees." Each validator designates a **Remote TPU** as its proxy TPU. **VERIFIED** |
| BAM | [bam.dev docs](https://bam.dev/docs/bam/bam-overview/), [maker plugin page](https://bam.dev/docs/bam/maker-plugin/how-it-works/), [Solana Compass on the Jito changelog](https://solanacompass.com/news/jito-details-bams-alpenglow-plan-maintenance-mode-for-all-mainnet-nodes-no-firebam-during-the-transition) | The BAM node "sequence[s] transactions according to programmatic, stable rules". The docs say "every auction round (~50 ms)", where bundles and transactions "compete on the same priority score". The changelog says the cadence is "derived from slot length", 25 → 20 ms. BAM goes into **maintenance mode "in the days leading up to activation"**, during which validators "revert to ordinary Jito-Agave Block Engine mode". The page body has no Alpenglow date. **VERIFIED** (the two primary-ish sources disagree on the cadence) |
| DZ Edge | [subscribe docs](https://docs.doublezero.xyz/solana/edge/subscribe/), the DZ data API (`/api/v1/openapi.json`, `/edge/shreds/{overview,devices,publishers/leaders,client-seats}`, `/solana/validators-metadata`) | See §3.5. **VERIFIED** (critic check) |
| Client mix by stake | `data.doublezero.xyz/api/v1/solana/validators-metadata` (673 validators, `software_client`) | AgaveBam 36.28%, JitoLabs 27.24%, **HarmonicAgave 18.12%, HarmonicFiredancer 6.97%**, Firedancer 6.85%, FireBAM 1.63%, Rakurai 1.27%, Agave 1.12%, Frankendancer 0.30%. **VERIFIED** (critic check) |
| TPU ingress vs validator IP | `getClusterNodes` plus `getVoteAccounts` (api.mainnet-beta), joined to the client mix, with ipinfo on the proxied TPU IPs | §3.2. **VERIFIED** (critic check) |
| v1 share, and which senders land v1 | 20 finalized blocks (slots 456,777,367–456,777,500, step 7) via `getBlock` with `maxSupportedTransactionVersion: 1`. Tip accounts from Jito `getTipAccounts` plus `sol-trade-sdk/src/constants/swqos.rs` | §3.6. **VERIFIED** (small sample) |
| Helius preconf decoding | [preconf-subscribe docs](https://www.helius.dev/docs/pre-confirmations/preconf-subscribe.md) vs [helius-rust-sdk `src/preconf.rs`](https://raw.githubusercontent.com/helius-labs/helius-rust-sdk/main/src/preconf.rs) | Docs: "`bincode` fails on v1 payloads". SDK: `use bincode::deserialize; … deserialize(&transaction_bytes)`. **VERIFIED** |
| v1 support in the Agave 4.2.2 shred stack | Crate sources: solana-entry 4.2.2 requires `solana-transaction >=4.1.4,<4.2.0` with `wincode` and `solana-message >=4.2.3,<4.5.0`. solana-transaction 4.1.6 and solana-message 4.4.1 have a `V1` variant and wincode v1 framing. sol-shred-sdk 4.0.4 decodes with `wincode::deserialize_exact::<Vec<Entry>>` | The 4.2.2 stack **can** decode v1. It does **not** handle Alpenglow `BlockComponent` markers (§3.7). **VERIFIED** (source reading, not executed) |
| Nozomi hosts | [endpoints](https://use.temporal.xyz/nozomi/endpoints.md) plus DNS | The docs list `fra2` as direct. `fra1` and `fra2` both resolve to **64.130.47.135**, and `pit1` equals `ewr1` (208.91.110.41). **VERIFIED** |

---

## 2. Contradictions between the documents, and how each resolves

| # | Topic | What the docs say | Resolution |
|---|---|---|---|
| C1 | Alpenglow activation | data-feed: epoch 1064, Oct 21 (VERIFIED). tx-landing §0: "activation expected with 4.3 but not confirmed" (LIKELY). | **data-feed is right.** It is confirmed on solana.com. Add: TowerBFT runs for 5,000 more slots, and Alpenglow consensus starts at slot 459,653,000. |
| C2 | Harmonic shred timing | tx-landing: shreds are released "only after an auction of about 300 ms… block-0 sniping cannot work against Harmonic leaders" (LIKELY/UNVERIFIED, from Blockworks). | **Wrong today.** Harmonic's docs say "Shreds always stream continuously… differences between strategies are about per-transaction inclusion timing". Harmonic leaders are snipeable, but the default strategy is a 50 ms FBA ordered by fee. |
| C3 | BAM batch cadence | data-feed: "20 ms auction rounds since Oct 9 (previously 25 ms)" (LIKELY). tx-landing: "batch interval is not published". The bam.dev maker-plugin page says "~50 ms". | **Unresolved** between 20 ms (changelog, slot-derived) and ~50 ms (docs page, possibly stale). Both docs are wrong to call it unpublished or to treat 20 ms as settled. |
| C4 | BAM ordering rule | tx-landing: the validator "executes FIFO with respect to account locks". | The BAM docs say transactions and bundles "compete on the same priority score" inside each auction batch. Ordering is by priority within a batch, not FIFO. tx-landing's own data shows 58% fee-consistent pairs on BAM, which fits batching. |
| C5 | Leader-class taxonomy | tx-landing: "non-BAM Agave 50%, BAM Agave 33%, Firedancer 16%". data-feed: "26.x… Firedancer 15.4%". | Both docs **lump Harmonic (25.1%) into "Agave" and "Firedancer"**. "Firedancer 16%" includes HarmonicFiredancer at 7%. tx-landing's striking "**non-BAM Firedancer: 87.1% fee-ordered**" is most plausibly **Harmonic FBA** behaviour, not Firedancer `pack`. The ordering study must be re-run with the client key from DZ `validators-metadata`. |
| C6 | v1 SIMD number | tx-landing §5.4: "v1 transactions (SIMD-0296…)". | v1 is **SIMD-0385**. SIMD-0296 is "Larger Transaction Size". |
| C7 | v1 and ALTs | tx-landing §5.4 says "Use v1" and §5.5 says "Use an ALT only for the static pump accounts". data-feed: v1 has "no address lookup tables". | **Mutually exclusive.** v1 has no ALT support, by spec. With v1's 4096-byte limit an ALT is unnecessary anyway: the legacy buy is 834 B (reference-impls). |
| C8 | v1 status | reference-impls: "Treat v1 as live (LIKELY)", citing the Helius README "not activated". data-feed: live since epoch 1035 (VERIFIED). | **Live.** My 20-block scan found v1 transactions in every block. The Helius README is stale. |
| C9 | v1 share | data-feed: 9 of 19 creates. mechanics: 53% of creates. tx-landing: 41% of block-0 buys. | Critic sample: **~88% of pump-program transactions were v1** (351 of 400 provider-tagged rows). The share is rising, so v1 must be first-class on both the decode side and the send side. |
| C10 | Helius preconf decoder | data-feed: decode with `agave-transaction-view`, not bincode. reference-impls: `helius` `src/preconf.rs` "is a complete decoder for the binary frame". | **data-feed is right.** `preconf.rs` uses bincode and will fail on most pump transactions. Copy only the 18-byte header layout and decode the body with `agave-transaction-view`. |
| C11 | DZ Edge stake coverage | data-feed: "413 publishers representing **57.3% of stake**". | That ratio counts every publisher. Validators **publishing leader shreds** hold **52.95%** of stake (373 nodes). For a block-0 feed, the 52.95% figure is the one that matters. |
| C12 | DZ Edge price | data-feed: FRA/AMS $1,500/mo, LON/NY/SGP/TYO $900, others $450 (from docs). | On-chain prices are **per epoch**: `price_per_epoch_dollars` = 100 for FRA/AMS, 60 for LON/NYC/SIN/TYO, 30 for others such as cmh, was and slc. The docs' monthly figures equal 15 epochs, which matches the old ~48 h epochs. At ~24 h epochs, **FRA ≈ $3,000/mo and TYO ≈ $1,800/mo**. LIKELY: confirm with DZ before budgeting. |
| C13 | Top hosting operators | data-feed §7: "TeraSwitch, Latitude.sh or Cherry, the top three operators by stake (26.2%, 9.5% and 7.2%)". geography: TeraSwitch 26.2 + 1.7%, Latitude ≈ 13%, **Allnodes 8.7%**, Cherry ≈ 8.8%. | The two docs group ASNs differently. Use geography's table: Allnodes (FRA/Offenbach) is missing from data-feed's list. |
| C14 | FRA/AMS leader-slot share | data-feed: 32.5% / 18.9% (city). geography: 35.3% / 21.0% (metro). tx-landing: 36% / 19% (1,600-slot sample). | These are grouping differences. Use geography's metro table as canonical. |
| C15 | Blockhash validity | tx-landing: "about 32 s". | solana.com: 150 blocks, **30 s**. Minor. |
| C16 | Overhead `c` in the latency model | geography: `c = 5 ms`, with a sensitivity case at 20 ms. data-feed budget: "5–15 ms, up to ~35 ms with a BAM round". tx-landing: Jito BE adds up to a 50 ms tick. | The model ignores the 50 ms batching on Harmonic and on Jito bundles, and BAM's 20–50 ms rounds. Re-run (§3.9): P_same drops by 0.07–0.14 in absolute terms, but site rankings do not change. |
| C17 | Priority of spend | tx-landing §8: "colocation > fan-out > CU trimming (free priority) > fees", because "arrival time dominates". | On Jito-Agave leaders (≈ 27%), arrival dominates, so that ordering holds. On Harmonic FBA leaders, fee decides position inside a 50 ms window; on BAM it does inside a 20–50 ms round. The bid policy must be **per leader class**. |
| C18 | Fastest send path | tx-landing §7: our own box "sending direct QUIC to the leader TPU or BAM node" is LIKELY fastest. | 68% of stake advertises a **proxy** TPU (§3.2): BAM nodes, Harmonic Remote TPUs, and a few Jito relayers. Whether those proxies give unstaked QUIC senders fair treatment at hot launches is **UNVERIFIED** (gap G5). |
| C19 | Firedancer routing | tx-landing: "Firedancer leaders: fee per CU matters more, and bundles are FIFO". | Moot from Oct 21 for at least the migration. Firedancer, Frankendancer and FireBAM are unsupported; Harmonic validators must run Salsa (Agave). About **15.7% of stake** (Firedancer 6.85 + HarmonicFiredancer 6.97 + FireBAM 1.63 + Frankendancer 0.30) changes client this week. |
| C20 | Senders' minimum tips | `sol-trade-sdk` constants: Helius 0.0002 SOL (already flagged in reference-impls) and Jito 0.00001 SOL. | The vendors say Helius Max 0.001 SOL or `swqos_only` 0.000005 SOL, and Jito 1,000 lamports (0.000001 SOL). Trust the vendor docs. |
| C21 | Nozomi FRA host | tx-landing: "Nozomi `fra1`/`ams1`". geography: "`fra2` direct… not Cloudflare `fra`". | Same IP (64.130.47.135). The docs name `fra2`, so use it. Cosmetic. |
| C22 | `buy_v3` buyback account | data-feed: "1 of 8 constants". reference-impls: "the wallet itself on SOL curves". | Consistent once read carefully. It is **one of the 8 buyback wallets, passed as the wallet itself (not its ATA)** for SOL-paired coins. The program checks it on every trade and **never creates it**. |
| C23 | `create_v2` quote tail | data-feed and mechanics: 3 accounts. reference-impls: `[quote_mint, quote_vault, quote_token_program, (quote_control)]`. | Both are partly right. The tail is 3 accounts, plus `quote_control`, the quote coin's bonding curve and its pool/vaults for pump-coin quotes (COIN_CREATION.md §"pump coin" quote). |
| C24 | Latency measurements from this container | tx-landing: "None of our latency timings are usable" (`time_connect` = 127.0.0.1). geography and data-feed use `appconnect − connect` as a relative RTT. | Both positions are defensible. Through an HTTPS CONNECT proxy, `appconnect − connect` includes the proxy-to-origin TLS handshake, so it ranks origins. It is not an absolute RTT. Geography's ordering matches WonderNetwork, which supports using it relatively. |

---

## 3. What the package is missing (critic checks)

### 3.1 The leader-class mix, as it actually is
Source: DZ `/solana/validators-metadata`, `software_client`, 673 validators, by active stake. **VERIFIED**.

| Class | Stake | How it orders transactions (primary source) | What the bid should be |
|---|---|---|---|
| AgaveBam + FireBAM | 37.9% | BAM node batches every ~20–50 ms and ranks bundles and transactions on one "priority score" ([bam.dev](https://bam.dev/docs/bam/maker-plugin/how-it-works/)). **Maintenance mode this week**: the validators fall back to Jito-Agave block-engine mode. | Fee per CU, plus tips via the block engine. Exact score formula is UNVERIFIED. |
| JitoLabs (Jito-Agave) | 27.2% | Agave priority scheduler on continuous arrival, plus Jito bundle auctions on 50 ms ticks ranked by tip per CU. | Arrival first, fee second (tx-landing's 54% result). |
| HarmonicAgave + HarmonicFiredancer | 25.1% | Remote TPU feeds a builder. **Default `fba`**: 50 ms double-buffered windows sorted by priority fee and tips. `fifo` and `mrev` exist. Per-validator strategy is not published. | "Be in the right window, then outbid". Harmonic tips **are** priority fees. |
| Firedancer / Frankendancer | 7.2% | `fd_pack`, reward per CU. Must switch to Agave for Alpenglow. | n/a after Oct 21 (temporarily) |
| Rakurai | 1.3% | Its own block builder. Not studied. | UNVERIFIED |
| Plain Agave / other | ≈ 1.2% | Agave scheduler | Arrival |

### 3.2 68% of stake receives transactions at a proxy, not at the validator
Source: `getClusterNodes.tpuQuic` vs `gossip` IP, joined to client and stake. **VERIFIED**.

| Client | Stake whose TPU IP ≠ gossip IP |
|---|---|
| AgaveBam | 36.28% (all of it) |
| HarmonicAgave | 17.89% of 18.12% |
| HarmonicFiredancer | 6.92% of 6.97% |
| FireBAM | 1.63% (all of it) |
| JitoLabs | 5.21% of 27.24% |
| Firedancer | ≈ 0% |
| **Total** | **67.96%** |

- **Harmonic proxies.** These validators point their TPU at Harmonic's regional Remote TPU and shred-receiver IPs, all on TeraSwitch:

  | Metro | IP | Stake |
  |---|---|---|
  | FRA | 198.13.140.105 | 10.0% |
  | LON | 64.130.46.242 | 6.2% |
  | AMS | 63.254.166.232 | 4.0% |
  | EWR | 64.130.45.196 | 2.1% |
  | TYO | 198.13.133.200 | 1.0% |
  | SGP | 198.13.130.10 | 0.5% |

- **BAM proxies.** These match the BAM-node shares in tx-landing:

  | Metro | IP | Stake |
  |---|---|---|
  | AMS | 63.254.167.197 | 10.2% |
  | FRA | 198.13.140.227 | 5.1% |
  | Šiauliai | 88.216.192.51 (Cherry) | 4.4% |
  | TYO | 63.254.163.37 | 4.1% |
  | SIN | 198.13.131.139 | 3.7% |
  | Ashburn | 208.91.105.53 | 3.5% |

- **Geography still mostly holds.** At metro level, only about 5% of stake has an ingress in a *different* metro from the validator: DUB→LON 1.2%, PDX→Kent/Seattle 1.3%, WAS→NYC 0.8%, CHI→WAS/NYC 0.9%, STO→FRA/SQQ 0.4%. Note that a different Harmonic validator in, say, AMS may advertise the LON Remote TPU. **The per-leader routing table must use the advertised `tpuQuic` address (refreshed every epoch, and during BAM maintenance), not the validator's city.**
- **TeraSwitch dominates further.** Nearly every BAM node and Harmonic Remote TPU is on TeraSwitch (AS20326), which strengthens geography's TeraSwitch-FRA recommendation.

### 3.3 Harmonic in particular (primary docs)
- **Shreds stream continuously.** This refutes the "300 ms" claim; see C2.
- **Default `fba`.** The builder runs two 50 ms buffers: arrivals fill one while the previous one drains in fee order. Our reaction transaction therefore cannot be sequenced until the window in which it arrives has closed. That costs about 25 ms on average and up to 50 ms, on about 25% of stake.
- **Bids.** "Harmonic tips are compute-unit priority fees… Validators receive 100%." A Jito-style tip transfer to a Jito tip account is not the Harmonic bid. Whether Harmonic's FBA counts Jito-account tips in "fees and tips" is **UNVERIFIED**.
- **Bundles.** Searcher bundles need a whitelist (Typeform intake). The gRPC is backward-compatible with Jito's searcher protos.

### 3.4 BAM in particular
- **Cadence:** ~50 ms per the docs page, or 20 ms per Jito's changelog (C3).
- **Within a batch,** transactions and bundles compete on one priority score.
- **Maintenance mode** starts "in the days leading up to activation", which is this week. Validators "revert to ordinary Jito-Agave Block Engine mode", and nodes return "gradually" after activation.
- **It is not known** what TPU BAM validators advertise during maintenance (UNVERIFIED). The routing table must re-read `getClusterNodes` at least every few minutes around Oct 18–28.

### 3.5 DoubleZero Edge: coverage, completeness, price and location (DZ data API)
- **Coverage.** 373 validators publishing leader shreds hold **52.95%** of network stake. By DZ metro (share of network stake): fra 19.15%, ams 11.85%, lon 7.34%, sin 3.24%, tyo 2.88%, nyc 2.75%, dub 1.94%, was 0.97%. So **only about 19% of stake publishes in-metro to a FRA subscriber**. The rest of the FRA/AMS leaders (32% + 21% of slots) publish elsewhere or **not at all**. For non-publishers, the "retransmit" groups (EU/APAC/AMER, `233.84.178.12–.14`) carry high-stake Turbine retransmits, which are one Turbine hop later.
- **Completeness.** The API's `slots_needing_repair` field is described as "Number of slots that required repair". For leader-shred publishers it is **247,434 of 286,254 leader slots (86%)**. By metro: ams 95%, fra 91%, nyc 99%, tyo 41%. What "required repair" means for timely detection is **UNVERIFIED**: missing data shreds that FEC can recover, or real gaps.
- **Missing group.** data-feed's group list omits **Root, `233.84.178.16`** ([subscribe docs](https://docs.doublezero.xyz/solana/edge/subscribe/)).
- **Price.** It is per epoch; see C12.
- **Devices.** DZ has **devices in TeraSwitch buildings**: `dz100a-fra2-tsw` (37 seats), `dz100a-ams2-tsw` (21), `dz103a-lon1-tsw`, `dz100a-ewr1-tsw`, `dz115a-tyo2-tsw`, `dz100a-sgp1-tsw` and `dz100a-iad1-tsw`. **VERIFIED** (`/edge/shreds/devices`). A TeraSwitch-FRA box can therefore probably cross-connect in-building. That is LIKELY: the onboarding path (internet GRE tunnel or cross-connect) is not documented.
- **Publisher clients.** The top publisher clients are HarmonicAgave 15.25%, AgaveBam 14.86%, JitoLabs 12.07% and HarmonicFiredancer 5.7%.

### 3.6 Transaction v1, sender compatibility and the cost model
Spec facts the builder must hard-code (SIMD-0385, **VERIFIED**):
- **No ALTs.**
- **4096-byte** limit.
- Priority fee is **total lamports** (8 bytes, mask bits 0–1), not micro-lamports per CU. tx-landing §9's "cup" recipes must be converted.
- **An unset CU limit means 0, and an unset loaded-accounts data size means 0.** The transaction cannot execute unless you set bit 2 and bit 3. For the cost model, an unset size counts as 32 KiB. For v1 that means the 16,384-CU "phantom cost" that tx-landing calls a free saving disappears only if you set a tight bit 3, which you must do anyway.
- `ComputeBudget` instructions are ignored in v1 but still consume CU. Do not include them.

Senders seen landing v1 (critic sample: 20 blocks, rows keyed on tip accounts):

| Provider | legacy / v0 / v1, all txs | Pump-program txs, v1 |
|---|---|---|
| Jito | 46 / 153 / **206** | 10 |
| Helius | 122 / 59 / **165** | 90 |
| Stellium | 0 / 0 / **44** | 38 |
| Astralane | 10 / 14 / **40** | 0 (its 9 pump txs were legacy/v0) |
| Nozomi | 1 / 4 / **32** | 32 |
| BlockRazor | 0 / 0 / 1 | 1 |
| 0slot | 0 / 7 / **0** | 0 |
| NextBlock | not seen | — |
| bloXroute | 0 / 1 / 1 | 0 |

What this shows and does not show:
- **v1 is accepted** by at least Jito, Helius, Nozomi, Stellium and Astralane.
- **0slot and NextBlock acceptance is unproven.** The sample is small, so absence is not proof of rejection.
- A Jito tip inside a v1 transaction does not prove the Jito block engine itself accepted it, because Helius Sender also forwards to Jito.

### 3.7 Alpenglow at the shred level (Agave crate source)
**The new wire format.** `solana-entry` 4.2.2 `block_component.rs` documents the new format:
- `Entry Count (u64)` followed by entries, **or** `Entry Count = 0`, a `Marker Version (u16)` and a marker.
- Marker types are `BlockHeaderV1` (parent slot and parent block id), **`UpdateParentV1`** (parent slot and parent block id) and `BlockFooterV1` (bank hash, producer time, certificates).
- **VERIFIED** (source).

**Why the recommended decoder breaks.** `sol-shred-sdk` 4.0.4 decodes every batch as `wincode::deserialize_exact::<Vec<Entry>>(…).ok()`. A marker batch would most likely fail the exact-length check and be **dropped silently**. The sniper would then never learn that the leader switched parents. That inference is LIKELY: it is based on the code, not on a test. Leftover tick-based "position in slot" estimates also break, because ticks disappear.

**What remains unknown** (UNVERIFIED):
- What happens to a create that executed in a bank that a later `UpdateParent` cleared.
- Whether that create re-executes in the new bank or can simply vanish.
- How our reactive buy then behaves; most likely it fails because the mint does not exist.

### 3.8 Things that are already answerable
- **Which Jito region a leader is connected to.** `searcher.proto` has `GetConnectedLeadersRegioned` (region → validator → slots) and `NextScheduledLeaderResponse.next_leader_region`. This resolves most of geography open item 4. Whether one region forwards to another remains UNVERIFIED, but we can simply send to the leader's own region.
- **Where Harmonic leaders take transactions.** The endpoints are `{ams,ewr,fra,lon,tyo,sgp,slc}.tpu.harmonic.gg`; DNS gives fra 198.13.140.105, ams 63.254.166.232 and lon 64.130.46.242. These also appear as the validators' `tpuQuic` addresses.

### 3.9 The geography model re-run with batching (critic recompute)
**Method.** I used `data/stake_geo.json` (same weights and RTT matrix; it reproduces the published FRA figure, 0.784 vs 0.780). I then gave 63% of leader slots an extra 25 ms of mean overhead (or 50 ms worst case) for batch auctions. This is LIKELY: it assumes the class mix is uniform across metros.

| Sites | P_same, c = 5 ms (published) | 63% batched, +25 ms | 63% batched, +50 ms |
|---|---|---|---|
| Ohio (CMH) | 0.516 | 0.447 | 0.377 |
| FRA | 0.784 | 0.716 | 0.648 |
| FRA + AMS | 0.801 | 0.733 | 0.665 |
| FRA + TYO | 0.872 | 0.799 | 0.727 |
| FRA + AMS + NYC + TYO | 0.932 | 0.860 | 0.787 |

**Conclusion.** Rankings and the "never Ohio for the hot path" verdict hold. Absolute block-0 eligibility is about 9–17% lower than published. On batched leaders, in-metro "race" wins matter less and fee matters more than geography's `Race` score implies.

---

## 4. UNVERIFIED items that affect the build

1. **DZ Edge onboarding:**
   - The exact payload inside GRE. Are they raw Agave merkle shreds, and is there any DZ framing?
   - Whether the subscriber host needs a public IP or BGP: the docs mention "BGP Session Up" and TCP 179 on 169.254/16.
   - Whether AWS EC2 can terminate it.
   - Which device a TeraSwitch-FRA box attaches to, and how.
   - What "required repair" means in practice, and when a slot's first data shreds arrive.
2. **BAM:**
   - The cadence (20 vs ~50 ms) and the priority-score formula (fee per CU? plus tips?).
   - What TPU BAM validators advertise in maintenance mode, and for how long.
   - Whether the BAM node applies SWQoS to inbound QUIC.
3. **Harmonic:**
   - Which validators run `fba`, `fifo` or `mrev`. That is not public, but may be inferable from on-chain ordering.
   - Whether Remote TPUs give unstaked senders fair treatment.
   - Whether Jito-account tips count in FBA ranking.
4. **Alpenglow:**
   - How UpdateParent affects transactions in a cleared bank.
   - Whether the leader window stays at 4 slots under Votor. solana.com's Alpenglow page is silent; only the reduced-slot page says 4.
   - Whether Yellowstone `processed` still emits transactions from cleared banks.
5. **v1 on every send path:** 0slot, NextBlock, bloXroute, Harmonic Remote TPU, BAM node, and durable nonces in v1.
6. **Exact v1 `buy_v3` + ATA (+ nonce advance) CU limit and loaded-accounts size,** read from a `simulateTransaction` with the real loaded bytes. tx-landing's "12–16 MiB" was observed on others' transactions, not simulated on ours.
7. **Helius preconf economics.** Real coverage from `getPreConfCoverage`, and the cost at 300 credits per covered slot per subscription (data-feed estimates about $250/day at 40% coverage, unmeasured). BAM preconfs may vanish during maintenance.
8. **Triton `SubscribeDeshred`:** its p50 (vendor-claimed 6.3 ms), its price, and whether it emits Alpenglow `bank_id`/UpdateParent information.
9. **Unknown tip recipients** in tx-landing (`rapidX…`, `LandX…`, `moon…`) and the 46 "no-tip" block-0 buyers. Those buyers are the competitors we need to profile.
10. **Teraswitch pricing and stock.** It comes from an aggregator, because the vendor page returned 403. The in-building RTT targets (64.130.50.88 / .91, and now also 198.13.140.105 Harmonic FRA and 198.13.140.227 BAM FRA) are unmeasured.

---

## 5. What an engineer still needs for end-to-end block-0 sniping

| Component | Covered by the package? | What is missing |
|---|---|---|
| Watched-wallet create detection from shreds | Partly: create layout, mint = account [0], v1 framing | Detection when the create arrives inside a **launcher/bundler program as a CPI** (tx-landing saw `CreateCoinAndBuyBondingCurveV3` launchers). A shred has no inner instructions, so the parser must key on signers plus the static keys (mint authority `TSLv…`, pump program) and decode launcher instruction layouts. Also: how to add newly funded rugger wallets to filters live (preconf `signerInclude` updates, deshred filter updates), not just a static list. |
| Per-leader routing table | Partly: geography's active-box idea | It must be keyed on: leader identity → client class (`software_client`) → **advertised `tpuQuic` IP and metro** → Jito BE region (`GetConnectedLeadersRegioned`) → DZ publisher yes/no → Harmonic/BAM endpoint. It must refresh each epoch, and every few minutes during BAM maintenance and the Alpenglow cutover. |
| Bid and fee policy per leader class | No: one global policy | Separate policies for Jito-Agave (arrival plus fee), BAM (priority score per batch), Harmonic FBA (priority fee only, window-aware) and Jito bundles (tip per CU per 50 ms tick). The "hardcore" model `D[leader_class]` must use these classes. |
| Transaction builder | Mostly: `buy_v3` + ATA, durable nonce idea | v1 specifics (total-lamport fee, mandatory CU-limit and data-size fields, no ALT). Whether durable nonces work and get priority in v1 on each path. Jito's 1 req/s/IP/region limit when sending K variants. A nonce-pool lifecycle across boxes (who advances, and how the new nonce value is read within one slot). |
| Slippage bound without execution results | Partly | Compute the post-dev-buy curve from the create transaction's dev-buy instruction arguments (`buy_v2` / `buy_exact_quote_in_v2` / `buy_v3`), since shreds carry no meta. Then add the expected rival buys. |
| Alpenglow-safe decode | No | Parse `BlockComponent` markers, track `(slot, bank_id)`, and handle UpdateParent by cancelling or re-arming dependent buys. |
| Feed coverage for non-DZ leaders | No | A second shred source covering the ~47% of stake not publishing to DZ, plus a coverage map per leader. |
| Measurement harness on the real box | Planned (geyserbench, latency_probe) | Per-path *send → land* latency logging, using signature-to-slot/position joins and leader-class tags, so the bid policy can learn online. |
| Copy-trade executor | Not covered by the speed docs | Decoders and builders for the venues a watched wallet uses after the curve (PumpSwap `buy_v2`/`sell_v2`, Raydium, Meteora, Jupiter routes), all v1-aware, and the detection path (preconf `signerInclude` or deshred `account_required`). |

---

## 6. Ranked gaps (each answerable by one follow-up researcher)

**G1. Per-leader-class ordering and ingress model.** This is the most important gap: it decides colocation vs fees and the whole routing table.
- **Question.** For each leader class that will exist on and after Oct 21 (Jito-Agave, BAM in normal and maintenance mode, Harmonic Salsa `fba`/`fifo`/`mrev`, Rakurai):
  1. What is the actual intra-slot ordering rule and batch cadence (BAM 20 vs ~50 ms)?
  2. Where is the ingress endpoint (advertised `tpuQuic`, Jito BE region)?
  3. Does fee or arrival decide position among same-slot pump buyers?
- **Method.** Re-run tx-landing's same-block pair study over at least 24 h, with leaders classified by DZ `software_client`. Split out Harmonic, and infer each Harmonic validator's strategy from its ordering signature.
- **Why it matters.**
  - About 63% of stake is batch-auctioned. If that is confirmed, the "colocation > fees" priority and geography's `Race` score are wrong for most slots, and "hardcore mode" must bid per class.
  - tx-landing's 87% "Firedancer fee-ordered" result is probably Harmonic.
- **Sources.** docs.harmonic.gg (scheduling-strategies, streaming, endpoints); bam.dev docs plus the Jito changelog and Discord; `getClusterNodes`; `data.doublezero.xyz/api/v1/solana/validators-metadata`; `getBlock` scans with `maxSupportedTransactionVersion: 1`; `mev-protos` `GetConnectedLeadersRegioned`.

**G2. Alpenglow cutover at the shred, entry and leader level.** This is time-critical: Oct 21 is seven days away.
- **Question.** At epoch 1064 and slot 459,653,000:
  1. How often do `UpdateParent` markers occur?
  2. Are transactions in a cleared bank re-executed in the replacement bank, or dropped?
  3. Does the leader window stay at 4 slots?
  4. Do yellowstone-grpc 14.0.1/15.0.0, `agave-transaction-view` 7.0.0 and `sol-shred-sdk` 4.0.4 handle `BlockComponent` markers?
  5. What do BAM validators advertise as TPU during maintenance, and when does BAM return?
  6. Where does the ~15.7% Firedancer-family stake land?
- **Method.** Test empirically on devnet, where Alpenglow has been active since Sep 25.
- **Why it matters.** A decoder that drops markers cannot tell that a create's bank was abandoned, so it buys mints that do not exist. Leader classes and TPU addresses also shift during the cutover.
- **Sources.** Agave 4.3 source (`entry/src/block_component.rs`, `ledger` shred and blockstore), the Alpenglow SIMD, solana.com/upgrades/alpenglow, the Jito BAM changelog, devnet `getBlock` and raw shreds.

**G3. Shred-feed coverage and completeness per leader.**
- **Question.**
  1. Per leader metro, how much leader-slot share do the DZ Edge leader-shred publishers cover (52.95% of stake overall; only ~19% of stake publishes in FRA)?
  2. What does `slots_needing_repair` = 86% mean for when a create's first data shreds arrive?
  3. Which source best covers the other ~47%: DZ retransmit groups, Helius Shred Delivery, Everstake Direct Shreds, SVS or Triton Deshred?
  4. What is the real onboarding path for each (auth, GRE/BGP on AWS vs TeraSwitch, the in-building `dz100a-fra2-tsw` device, per-epoch price ≈ $3k/mo in FRA)?
- **Why it matters.** The feed is the single biggest monthly cost and the start of the latency chain. If half of the leaders arrive one Turbine hop late or incomplete, block-0 eligibility roughly halves on those slots.
- **Sources.** `data.doublezero.xyz/api/v1/edge/shreds/{publishers/leaders,devices,client-seats}` and the openapi schema; DZ docs and the DZ Discord `#jito-shredstream`; Helius shred-delivery docs; Everstake Blockspace docs; geyserbench methodology.

**G4. v1 snipe transaction spec and sender compatibility.**
- **Question.**
  1. What are the exact v1 field values (CU limit, loaded-accounts size, total priority fee) for `CreateIdempotent(Token-2022 ATA) + buy_v3 + tip` (+ optional `AdvanceNonce`), measured by `simulateTransaction` with real loaded bytes?
  2. Which send paths accept v1, and v1 plus durable nonce: Jito BE `sendTransaction` and `sendBundle`, Helius Sender (`/fast`, `swqos_only`), Nozomi Batch Send, Astralane, NextBlock QUIC, bloXroute, 0slot, Harmonic Remote TPU, BAM node?
  3. Which paths deprioritize nonce transactions?
- **Why it matters.**
  - About 88% of pump traffic is v1.
  - v1 with unset fields executes nothing.
  - An ALT is impossible in v1.
  - Fan-out dedupe relies on durable nonces.
  - Jito's 1 req/s/IP/region limit caps the number of variants.
- **Sources.** SIMD-0385; vendor docs (Helius "transaction-v1" doc, Nozomi, Astralane, NextBlock, bloXroute); mainnet `simulateTransaction`; a larger `getBlock` scan by tip account and version (starting from §3.6).

**G5. Direct-send viability through BAM and Harmonic proxies, and staked access.**
- **Question.** For unstaked vs staked QUIC senders, do BAM nodes and Harmonic Remote TPUs (68% of stake's advertised TPU) apply stake-weighted QoS, connection caps or allow-lists? What happens to an unstaked connection during a hot launch? What does staked access cost in FRA/AMS (`--staked-nodes-overrides` peering, SWQoS-as-a-service from Helius `swqos_only`, SVS, Astralane)?
- **Why it matters.** tx-landing ranks "own box, direct QUIC to leader or BAM node" as the fastest path, but for about two thirds of stake that endpoint is a third-party proxy with undocumented admission rules.
- **Sources.** bam.dev and the Jito BAM GitHub (bam-client / jito-solana BAM branches); docs.harmonic.gg architecture and FAQ, plus asking the Harmonic team; Agave `streamer` QoS source; Triton's SWQoS write-up; vendor pricing pages.

**G6. Geography model v2 and the provider decision.**
- **Question.** Re-run `deploy/stake_geo.py` with:
  - (a) the advertised `tpuQuic` metro instead of the validator city;
  - (b) a per-class overhead `c` (Harmonic 50 ms FBA, BAM 20–50 ms, Jito BE 50 ms ticks);
  - (c) weights by **pump.fun creates per leader metro** (or by watched-wallet creates) instead of leader slots;
  - (d) the measured in-slot create position (median 48%).

  Then confirm the purchasable bare-metal options in the TeraSwitch FRA/AMS buildings: stock, price, in-building RTT to 64.130.50.88 (Jito), 64.130.50.91 (Helius), 198.13.140.105 (Harmonic FRA), 198.13.140.227 (BAM FRA) and the `dz100a-fra2-tsw` DZ device.
- **Why it matters.** It confirms the server purchase the user is waiting on (FRA first; whether the second box is AMS or TYO) with the corrected model. My quick recompute keeps FRA first.
- **Sources.** `data/stake_geo.json`, `getClusterNodes`, ipinfo, the TeraSwitch sales or test-IP request, Latitude and Cherry pricing, the DZ devices API.

---

## 7. Edits to make in the four documents

- **speed-tx-landing.md**
  - §0: Alpenglow is confirmed for epoch 1064.
  - §1: replace "Harmonic releases shreds after ~300 ms" with Harmonic's continuous streaming and 50 ms FBA. Fix "BAM… FIFO".
  - §1 table: re-key leader classes to include Harmonic.
  - §5.4: SIMD-0296 → SIMD-0385, and delete §5.5's ALT advice for v1. Add "CU limit and loaded-accounts size must be set in v1".
  - §5.7: 30 s, not 32 s.
  - §7–§8: make routing and bidding per class.
- **speed-data-feed.md**
  - DZ coverage: 52.95% leader-publishing stake, not 57.3%.
  - DZ price: per-epoch, FRA ≈ $3k/mo.
  - Add the Root group `233.84.178.16`.
  - BAM cadence: 20 ms (changelog) vs ~50 ms (docs).
  - Firedancer 15.4% includes HarmonicFiredancer.
  - Feed ordering: BAM preconfs pause during maintenance mode.
- **speed-geography.md**
  - Add the class-aware overhead and the advertised-TPU metro.
  - State that the `Race` score assumes pure arrival ordering, which holds for only about 27–35% of stake.
  - Add the Harmonic and BAM FRA proxy IPs to the "ask any provider" ping list.
- **speed-reference-impls.md**
  - Mark `helius` `preconf.rs` as **not v1-safe**: copy only the header and decode the body with `agave-transaction-view`.
  - Mark `sol-shred-sdk` decoding as **not Alpenglow-marker-aware**.
  - Remove "Treat v1 as LIKELY" (it is live).
  - Resolve the buyback recipient wording.
  - The quote tail is 3 accounts, plus `quote_control` and the curve/pool accounts for pump-coin quotes.
