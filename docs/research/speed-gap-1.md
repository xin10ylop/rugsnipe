# Speed gap 1: intra-slot ordering by leader class (measured over 24 h)

**Question.** For each leader class as it will exist on and after Oct 21, 2026, what are:
- the intra-slot ordering rule,
- the batch cadence,
- the ingress endpoint (the advertised `tpuQuic` IP and metro, and the Jito block-engine region),
- the deciding factor among same-slot pump.fun buyers: fee or arrival?

The classes are Jito-Agave (27.2% of stake), BAM (37.9%, in normal and maintenance mode), Harmonic Salsa (25.1%, with `fba`/`fifo`/`mrev` strategies) and Rakurai (1.3%).

**Method.** I re-ran tx-landing's same-block pair-ordering study over 24 h. Leaders are classified by DZ `software_client`, Harmonic is split out, and each Harmonic validator's strategy is inferred from its ordering signature.

**Labels.**
- **VERIFIED**: a primary source, or a measurement made today (2026-10-14).
- **LIKELY**: a strong inference.
- **UNVERIFIED**: a guess, or a claim no one has checked.

Scripts and raw data are in the session scratchpad, `g1/` (`fetch.py`, `final.py`, `dist.py`, `perval2.py`, `jl.py`, `blocks*.jsonl`).

---

## 0. Answer

| Class (stake) | Intra-slot ordering rule | Cadence | Ingress: advertised `tpuQuic` → metro | Jito BE region | Deciding factor among same-slot pump buyers |
|---|---|---|---|---|---|
| **Jito-Agave** (27.2%) | Agave priority scheduler over continuous arrival. Fee reorders only transactions buffered together. Bundles come from 50 ms BE auctions (VERIFIED, [Jito docs](https://docs.jito.wtf/lowlatencytxnsend/): "Parallel auctions are run at 50ms ticks") | Continuous. Fee matters only within about 1–2.5% of the block (measured) | 22.0% of stake advertises its own IP. 4.1% uses relayer 109.94.96.69 (Latitude FRA). About 1% uses other relayers (VERIFIED, `getClusterNodes`) | **Specific**, 1–2 regions per validator: frankfurt 9.4%, frankfurt+tokyo 4.0%, amsterdam 3.1%, london 2.1%, slc 1.7%, ny 1.1% … (VERIFIED, `GetConnectedLeadersRegioned`) | **Arrival.** Pair agreement with fee is 0.52 overall and 0.60 for block-0 snipers |
| **BAM, normal** (37.9%) | The BAM node (TEE) sequences each auction round; "bundles and regular transactions compete on the same priority score" ([bam.dev](https://bam.dev/docs/bam/maker-plugin/how-it-works/)). The validator must "execute all received transactions in that exact order" ([bam-overview](https://bam.dev/docs/bam/bam-overview/)) | **20 ms rounds** since the 200 ms-slot change: "cadence is derived from slot length… tighten automatically from 25ms to 20ms" ([BAM changelog, Oct 9](https://bam.dev/blog/bam-changelog-sept-23-oct-9/)). The "~50 ms" on the docs page is stale. The measured fee-ordered span is about 5% of the block, which fits 20 ms rounds | One BAM node per region; table §4 (VERIFIED, `explorer.bam.dev/api/v1/validators` joined to `tpuQuic`) | **All 8 regions** (the BAM node connects to the BE on the validators' behalf) | **Fee, within a round.** Same-round pairs are 86–89% fee-ordered. Block-0 sniper pairs are 0.73 fee-ordered |
| **BAM, maintenance** | "Revert to ordinary Jito-Agave Block Engine mode." MPP and DBI are off (changelog) | As Jito-Agave | From code: when BAM disconnects, the gossip TPU falls back to the relayer TPU, or else to the validator's own TPU, re-checked every slot, and the gossip `clientId` flips to `JitoLabs` (VERIFIED, jito-solana `fetch_stage_manager.rs`, `bam_manager.rs`). That maintenance actually disconnects the BAM session is LIKELY | Becomes specific, like Jito-Agave (LIKELY) | **Arrival**, as Jito-Agave (LIKELY) |
| **Harmonic Salsa** (25.1%; Samba moves to Salsa) | The builder chosen by `--strategy`. Default **`fba`**: 50 ms double-buffered batches "by descending priority fees and tips". `fifo` is strict arrival. `mrev` is proprietary ([scheduling-strategies](https://docs.harmonic.gg/concepts/scheduling-strategies.md), [salsa-4](https://docs.harmonic.gg/run-a-validator/setup/salsa/salsa-4.md)). No public per-validator strategy list exists (VERIFIED, via the GitBook ask endpoint) | `fba`: 50 ms per the docs. **Measured**: the fee-ordered span is only about 5% of the block, the same as BAM; see §2.3 | Harmonic Remote TPU per region: FRA 198.13.140.105 (10.0%) and .111 (0.7%), LON 64.130.46.242 (6.5%), AMS 63.254.166.232 (4.0%), EWR 64.130.45.196 (2.1%), TYO 198.13.133.200 (1.0%), SGP 198.13.130.10 (0.6%). All on TeraSwitch | **All 8 regions** (VERIFIED) | **Fee within the batch, for about 14% of stake.** Inferred: FBA-like 13.9% of stake, FIFO-like 1.7%, undetermined 9.5%. No MREV-distinct signature was seen. Block-0 sniper pairs: 0.69 (Salsa), 0.76 (Samba) |
| **Rakurai** (1.3%) | Jito-solana fork with a "heuristics-based" scheduler. A SOL tip to 8 Rakurai PDAs is a "virtual priority boost" ([docs.rakurai.io](https://docs.rakurai.io)). The formula is UNVERIFIED | Continuous (LIKELY) | Own IP, 100% (VERIFIED) | frankfurt 1.10%, others 0.17% | **Arrival**: 0.51 overall, 0.53 block-0. 49% of its reactive buys failed |

**Bottom line.**
- **Arrival decides on about 30% of stake:** Jito-Agave, Rakurai and Harmonic FIFO. Counting the Firedancer stake that migrates to Agave, it is about 37%. During BAM maintenance it is about two thirds.
- **Fee decides within a 20 ms round on BAM.**
- **Fee decides within an FBA batch on about 14% of stake** (Harmonic). The batch is as narrow as BAM's in block-index terms.
- Across rounds and batches, arrival decides on every class: agreement falls to 0.45–0.55 beyond about 5% of block distance.
- So "colocation first" still holds everywhere. Fee is a tie-breaker inside a ≈20–50 ms window, and it is decisive on BAM and Harmonic-FBA when snipers react to the same shred.

---

## 1. Data and method

- **Window: slots 456,347,600–456,779,600.** That is 432,000 slots, one epoch length. Block times run from **2026‑10‑13 06:45 to 2026‑10‑14 08:55 UTC (26.2 h)**, so the mean slot was ≈218 ms. The first pass covered only 216k slots, about 13 h, because epochs halved; a second pass doubled it.
- **Leaders:** `getSlotLeaders` over all 432k slots.
- **Leader classes:** identity → vote account (`getVoteAccounts`) → DZ [`validators-metadata`](https://data.doublezero.xyz/api/v1/solana/validators-metadata) `software_client`, 673 validators.
  - **New:** `getClusterNodes.clientId` equals DZ `software_client` for 99.84% of stake (VERIFIED). The live routing table can therefore classify leaders straight from gossip.
  - The remaining 0.08% is AgaveBam→JitoLabs, i.e. BAM sessions that are already down.
- **Leader-slot share (first 216k):** AgaveBam 36.28%, JitoLabs 27.06%, HarmonicAgave 18.31%, HarmonicFiredancer 7.05%, Firedancer 6.90%, FireBAM 1.59%, Rakurai 1.23%, Agave 1.06%. The earlier half is within 0.2 pp.
- **Sample: 15,978 blocks from 659 leaders**, fetched with `getBlock` (`maxSupportedTransactionVersion:1`, full):
  - **(a) Stratified per validator:** up to 6+3 slots for BAM, 20+10 for Jito, 24+16 for Harmonic and 40+24 for Rakurai validators, spread over the window. 10,759 blocks.
  - **(b) Harmonic top-up:** 1,429 blocks.
  - **(c) Create slots:** 3,790 blocks. I found every create slot by running `getSignaturesForAddress` on pump's mint authority `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM` (about 50k create signatures in 24 h, about 21/min), then sampled them by class.
- **Ordering signal.**
  - The order of a block's transactions as returned by RPC is noisy, because worker threads interleave. Over all non-vote transactions, the adjacent-pair ascent ratio is 0.46–0.48 for every class, i.e. random.
  - I therefore use only **write-conflicting pairs**: two pump trades on the same bonding curve, from different signers, in the same block. Their relative order is the scheduler's order.
  - Metric: P(the earlier transaction has the higher bid). Each (slot, mint) group gets weight 1, and the CIs are 95% bootstraps over slots.
  - For reference, pure FIFO scores about 0.5 plus any fee–speed correlation, and strict priority scores 1.0.
- **Bid keys tested:**
  - CU price;
  - total priority fee;
  - the Agave estimated priority, `pf/(costUnits + unused CU)`;
  - Jito tip.

## 2. Results

### 2.1 Class level (group-weighted)

| Class | blocks | (slot,mint) groups | P(earlier has higher CU price) | P(earlier has higher total prio fee) | Jito‑tipped tx earlier |
|---|---|---|---|---|---|
| Jito-Agave | 4,249 | 6,971 | **0.518** [0.51–0.53] | 0.564 [0.55–0.57] | 0.49 |
| BAM | 4,465 | 7,570 | **0.595** [0.58–0.60] | 0.617 [0.61–0.63] | 0.48 |
| Harmonic-Salsa | 3,659 | 6,104 | **0.559** [0.55–0.57] | 0.612 [0.60–0.62] | 0.49 |
| Harmonic-Samba | 1,833 | 3,319 | **0.554** [0.54–0.57] | 0.615 [0.60–0.63] | 0.48 |
| Rakurai | 782 | 1,335 | **0.510** [0.48–0.54] | 0.555 [0.53–0.58] | 0.46 |
| Firedancer | 543 | 1,063 | 0.536 [0.51–0.57] | 0.581 [0.56–0.61] | 0.53 |

- **Total fee beats CU price on every class, including Firedancer, whose `fd_pack` provably ranks by reward per CU.**
  - In pairs where the two keys disagree, the CU-price winner comes first only 20–34% of the time.
  - So the total-fee edge is a **confound**, not a scheduler key. Bots that pay more in absolute terms are also the faster ones.
  - Jito-Agave's 0.56 on total fee is the "arrival plus correlation" baseline that every class must be compared against. (LIKELY)
- **No dose–response on the fee gap.** P(earlier has higher CU price) is flat across fee ratios of 1–1.5×, 1.5–3×, 3–10× and ≥10×:
  - Jito-Agave: 0.53, 0.51, 0.51, 0.53.
  - BAM: 0.65, 0.57, 0.58, 0.59.
  - Harmonic-Salsa: 0.58, 0.51, 0.56, 0.57.

  In a batch model, any bid difference wins inside the batch and nothing wins across batches. The flat profile matches that, and does not match a continuous fee auction. (LIKELY)

### 2.2 Distance profile: the cadence signature
P(earlier has higher CU price) as a function of the pair's distance in the block, measured as a fraction of non-vote transactions:

| Class | <1% | 1–2.5% | 2.5–5% | 5–10% | 10–15% | 15–25% | 25–40% | >40% |
|---|---|---|---|---|---|---|---|---|
| Jito-Agave | 0.65 | 0.61 | 0.58 | 0.49 | 0.45 | 0.50 | 0.48 | 0.51 |
| **BAM** | **0.86** | **0.89** | **0.85** | 0.53 | 0.55 | 0.51 | 0.50 | 0.54 |
| **Harmonic-Salsa** | **0.75** | **0.77** | **0.76** | 0.55 | 0.46 | 0.51 | 0.52 | 0.54 |
| Harmonic-Samba | 0.78 | 0.76 | 0.75 | 0.58 | 0.48 | 0.48 | 0.49 | 0.53 |
| Rakurai | 0.71 | 0.54 | 0.51 | 0.51 | 0.49 | 0.48 | 0.48 | 0.48 |
| Firedancer | 0.59 | 0.65 | 0.57 | 0.48 | 0.55 | 0.52 | 0.48 | 0.55 |

(The 7,000+ pairs per class appear in `dist_cup.txt`.)

- **BAM shows a sharp step.** Near-strict fee order holds out to 5% of the block, then falls to arrival order. That is a batch auction (VERIFIED as measured).
  - A block of about 200 ms holds about 10 rounds of 20 ms, each about 10% of the block. Inside one round, pair distances are mostly under 5%.
  - So this fits 20 ms and does not fit 50 ms (LIKELY).
- **Jito-Agave decays smoothly.** It shows a weak fee effect only at the shortest distances: the Agave scheduler's buffer, plus 50 ms BE bundles.
- **Rakurai and Firedancer** behave like Jito-Agave or weaker.

### 2.3 Harmonic: what the signature says

1. **Harmonic's fee-ordered span is about as narrow as BAM's.** Index distance is under 5%, the same as BAM.
   - If 50 ms batches were spread evenly over the block, that would be about 25% of it. Instead, pairs 10–25% apart are at 0.46–0.52.
   - Either the batches cover far fewer transactions than a quarter of the block, or most Harmonic stake does not run FBA at all.
   - The per-validator data (item 2) points to the first: FBA-like validators show the same short span. (LIKELY; RPC exposes no intra-slot timestamps.)
2. **Per-validator inference.** Each validator gets a short-distance (<5%) agreement score with a bootstrap CI. The null is the Jito-Agave validator distribution, median 0.62.

   | Harmonic validators (77 seen, 25.1% stake) | Validators | Stake |
   |---|---|---|
   | **FBA-like** (CI low > 0.63) | 30 | **13.9%** (FRA .105 3.84+0.57, LON 4.01, AMS 2.09, EWR 1.66, TYO 0.92) |
   | **FIFO-like** (median < 0.68, CI high < 0.8) | 7 | **1.7%** (LON 1.26, own IP 0.28) |
   | Undetermined | 40 | 9.5% (mostly FRA .105, 5.6%) |

   Other classes measured the same way:
   - BAM: 107 FBA-like out of 176 (20.7% stake), 0 clearly FIFO.
   - Jito-Agave: 46 FIFO-like (16.4%), plus 10 that look fee-ordered (4.5%; the largest is E1r4Psq8, 2.5%, which LIKELY just has a deep scheduler buffer).
   - Rakurai: 7 of 10 FIFO-like.

   Top Harmonic validators:
   - JUPiTE…, 2.46%, FRA: 0.80 [0.70–0.89], FBA-like.
   - CAo1dC…, 2.15%, LON: 0.84 [0.76–0.91], FBA-like.
   - Awes4T…, 1.46%, FRA: 0.79, FBA-like.
   - 5ZqveV…, 0.93%, FRA: 0.67, FIFO candidate.
3. **MREV cannot be told apart from FBA with this method.** No Harmonic validator shows fee order at long distance; the maximum is 0.69 and the median is 0.50, so nothing sorts the whole slot by revenue. MREV's logic is proprietary. The only MREV constraint is that it is not SFDP-compliant, so SFDP validators run FBA or FIFO. (UNVERIFIED which validators run MREV.)
4. **Block fingerprint.** Harmonic blocks put votes into a median of **2** contiguous runs (Samba 7). Agave, BAM and Rakurai blocks have 93–124 runs. This means a Harmonic leader can be recognised from the block alone (VERIFIED as measured). It disappears once Alpenglow removes vote transactions.

### 2.4 Block 0: snipers reacting to a create in the same slot
3,790 create slots plus every create in the stratified sample: 4,663 successful creates. A "reactive" buy is a buy of the new mint, by a different signer, 4 or more positions after the create. Positions 1–3 are treated as dev or launcher bundles.

| Class | creates | % with ≥1 reactive buy in block | reactive buys (failed) | **P(earlier sniper has higher CU price)** [CI], groups | Jito-tipped sniper earlier |
|---|---|---|---|---|---|
| Jito-Agave | 1,182 | 20.7% | 782 (35%) | **0.60** [0.54–0.66], 129 | 0.36 |
| BAM | 1,363 | 23.1% | 1,041 (38%) | **0.73** [0.69–0.78], 182 | 0.41 |
| Harmonic-Salsa | 971 | 25.1% | 674 (36%) | **0.69** [0.63–0.75], 113 | 0.26 |
| Harmonic-Samba | 545 | 25.7% | 478 (42%) | **0.76** [0.68–0.83], 67 | 0.36 |
| Rakurai | 285 | 24.6% | 296 (49%) | 0.53 [0.42–0.65], 39 | 0.37 |
| Firedancer | 274 | 26.3% | 189 (37%) | 0.66 [0.53–0.77], 36 | 0.62 |

- **Snipers arrive together, so fee matters more for them than for ordinary trading** (0.60–0.76 versus 0.52–0.60).
  - The effect is strongest on BAM and Harmonic, which fits snipers landing in the same 20 or 50 ms batch.
  - Even on Jito-Agave, fee has weak power (0.60): near-simultaneous arrivals sit in the scheduler buffer together.
- **Batching does not reduce block-0 reach.**
  - Harmonic and BAM leaders get same-slot reactive buys at least as often as Jito-Agave (23–26% versus 21%).
  - Creates in the first fifth of the block get reactions 30–38% of the time. Creates in the last fifth get them 7–9% (e.g. Jito-Agave 23/255, Harmonic-Salsa 15/176).
- **Jito-tipped snipers land later** than untipped ones: 0.26–0.41 on every class except Firedancer. These rest on only 21–72 groups per class, so the result is LIKELY. In the first 13 h subsample, the first reactive buyer carried a Jito tip in only 14% of cases (Jito-Agave 21/147).
  - Third-party sender-tipped transactions were also not earlier (0.25–0.49; pair-weighted, indicative).
  - Implication: for block-0 the bundle path costs position. Use the direct or TPU path, with the bid in priority fee. (LIKELY)

### 2.5 Correction: tx-landing's "87% fee-ordered Firedancer"
Re-keyed by DZ client, the 87.1% came from **pure Firedancer** (9 leaders, 91.9% of n=8,850 pairs). It was **not Harmonic**: HarmonicFiredancer scored 57.9%. Moreover, **7,010 of the 8,850 pairs come from one (slot, mint) group.** The figure is an artefact of a single hot mint. On the 24 h group-weighted data, Firedancer scores 0.536 (VERIFIED, recomputed from tx-landing's own `scan/blocks_*.jsonl`).

## 3. Oct 21 onward

- **Firedancer-family stake must move** (critique-speed C19; solana.com/upgrades/alpenglow).
  - HarmonicFiredancer (7.0%) moves to Salsa. Salsa is selected with the same `--strategy` flag, so expect the same strategy mix. (LIKELY)
  - FireBAM (1.6%) is told "do not run" during the transition, so it becomes Jito-Agave or Agave-BAM in maintenance ([changelog](https://bam.dev/blog/bam-changelog-sept-23-oct-9/)).
  - Firedancer (6.9%) moves to Agave or Jito-Agave. Its measured profile already matches Jito-Agave.
- **BAM maintenance mode** runs "during the days leading up to activation" and comes back "gradually" after it. **As of 09:00 UTC Oct 14, BAM is not yet in maintenance**: all 36.28% AgaveBam still advertises BAM-node TPUs.
  - In maintenance, BAM leaders should be treated as Jito-Agave: arrival decides, Jito BE region is specific, and the TPU is their own or a relayer's. That makes about two thirds of stake arrival-ordered for those days.
  - Watch for it via `getClusterNodes` (`clientId` flips AgaveBam→JitoLabs, and the `tpuQuic` IP changes). The flip takes effect within one state-machine tick, `DEFAULT_MS_PER_SLOT` (source above).
- **Alpenglow votes are not transactions,** so the vote-run fingerprint (§2.3, item 4) disappears. Use `clientId` instead.
- **BAM cadence** is tied to slot length, so it moves again if slot time changes (changelog).

## 4. Ingress table (VERIFIED, 2026-10-14 09:00 UTC; refresh each epoch, and every few minutes during maintenance)

**BAM node → advertised `tpuQuic` → metro (stake):**

| BAM node | `tpuQuic` IP | Metro | Stake |
|---|---|---|---|
| ams‑bam‑2 | 63.254.167.197 | Amsterdam | 10.18% |
| fra‑bam‑2 | 198.13.140.227 | Frankfurt | 5.18% |
| sqq‑bam‑2 | 88.216.192.51 (Cherry) | Šiauliai | 4.52% |
| tyo‑bam‑2 | 63.254.163.37 | Tokyo | 4.06% |
| sin‑bam‑1 | 198.13.131.139 | Singapore | 3.72% |
| iad‑bam‑1 | 208.91.105.53 | Ashburn | 3.49% |
| sea‑bam‑1 | 63.254.180.11 | Kent (Seattle) | 1.31% |
| ewr‑bam‑2 | 64.130.51.255 | Newark | 1.05% |
| lon‑bam‑2 | 64.130.46.11 | London | 0.87% |
| dub‑bam‑2 | 64.130.61.195 | Dublin | 0.84% |
| lax‑bam‑2 | 64.130.58.123 | Los Angeles | 0.69% |
| hkg‑bam‑1 | 103.70.2.236 | Hong Kong | 0.66% |
| pit‑bam‑1 | 64.130.54.221 | Pittsburgh | 0.38% |
| slc‑bam‑1 | 64.130.33.195 | Salt Lake City | 0.27% |
| dfw‑bam‑2 | 141.98.218.181 | Dallas | 0.19% |

Every BAM node except sqq and hkg is on TeraSwitch AS20326 (ipinfo).

**Harmonic:** the Remote TPUs listed in §0, which are the `{fra,lon,ams,ewr,tyo,sgp,slc}.tpu.harmonic.gg` endpoints ([endpoints](https://docs.harmonic.gg/run-a-validator/endpoints.md)). The Harmonic docs say "Regular TPU transactions are delayed by 50ms to allow bundles to arrive" ([harmonic-bundles](https://docs.harmonic.gg/searchers/harmonic-bundles.md)). That is UNVERIFIED for FBA versus the in-client fallback.

**Jito regions.** The `GetConnectedLeadersRegioned` query works **without auth** (VERIFIED, `mainnet.block-engine.jito.wtf`, gRPC):
- BAM, Harmonic and FireBAM validators appear in all 8 regions.
- Jito-Agave and Firedancer validators appear in specific regions (§0).
- Rakurai validators are listed in frankfurt.

## 5. What this changes

1. **The "colocation > fan-out > CU trimming > fees" rule holds for position *across* windows on every class.**
   - Inside a window, fee decides on BAM (strictly) and on Harmonic-FBA.
   - For block-0 snipers, who all react to the same shred, the window *is* the race. On BAM and Harmonic leaders (about 63% of stake and of leader slots until maintenance starts) the bid matters as much as latency.
2. **Latency model.** Replace `c = 5 ms` with a per-class model:
   - Jito-Agave and Rakurai: continuous; `c` ≈ 5 ms.
   - BAM: 20 ms rounds, a 10 ms expected wait, and fee ordering within the round.
   - Harmonic-FBA: 50 ms buffers per the docs, a 25 ms expected wait. Measured spans suggest the effective batch is narrower (§2.3, UNVERIFIED).
   - Jito BE bundles: 50 ms ticks.
3. **"Hardcore mode" must bid per class.**
   - **BAM:** CU price, i.e. priority score. Win the round with the CU-trimmed limit, so that CU price × limit stays cheap.
   - **Harmonic:** priority fee ("Harmonic tips are compute-unit priority fees", [FAQ](https://docs.harmonic.gg/reference/faq.md)). Jito-account tips do not buy priority on the TPU path.
   - **Jito-Agave and Rakurai:** arrival first. A modest fee helps only against simultaneous arrivals (0.60).
   - **Never rely on the Jito bundle path for block-0.** It lands later on every class measured.
4. **Routing table key.** Use `getClusterNodes.clientId` and `tpuQuic` live, plus the Jito region from `GetConnectedLeadersRegioned`. There is no need to fetch DZ metadata on the hot path.

## 6. Still open
- **Intra-slot time base.** I could not convert index distance into milliseconds. Votes are a usable clock only on BAM and Agave blocks: there, fee order lasts about 2–5% of votes. The fix is to time our own probe transactions, or DZ shred timestamps, against batch boundaries. This would settle Harmonic's effective batch width and BAM's 20 ms.
- **Which Harmonic validators run MREV,** and whether FBA's "tips" include Jito-account transfers.
- **BAM maintenance.** The real start date, the TPU actually advertised, and whether DZ or gossip `clientId` flips during it. Re-run `tpu.py` and `jl.py` daily from Oct 15.
- **Rakurai.** The virtual-priority formula, and the tip PDAs (docs appendix not fetched). Whether Rakurai runs during the Alpenglow migration.
- **Selection bias.** The pump-trade pairs come from *included* transactions only. Dropped transactions, such as ones outbid or past the CU limit, are invisible.
