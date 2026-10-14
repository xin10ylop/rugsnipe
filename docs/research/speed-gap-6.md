# Speed gap 6: corrected placement model, and what to buy in the TeraSwitch FRA/AMS buildings

**Question.** Re-run `deploy/stake_geo.py` with four changes:
- (a) use the metro of the advertised `tpuQuic` address instead of the validator's city;
- (b) add a per-class batching overhead (Harmonic 50 ms FBA, BAM 20–50 ms, Jito BE 50 ms ticks);
- (c) weight by pump.fun creates (or watched-wallet creates) per leader instead of by leader slots;
- (d) use the measured position of creates inside their block (median 48%) instead of a uniform one.

Then, for TeraSwitch FRA and AMS bare metal: what can be bought (stock, price, contract)? And what in-building RTT does a box there get to 64.130.50.88 (Jito FRA), 64.130.50.91 (Helius fra-sender), 198.13.140.105 (Harmonic FRA Remote TPU), 198.13.140.227 (BAM FRA) and the `dz100a-fra2-tsw` DZ device?

**Date:** 2026-10-14, about 10:00–11:00 UTC. **Labels:** VERIFIED means primary data or our own measurement today. LIKELY means a strong inference. UNVERIFIED means a vendor claim or a guess.

**Reproduce:**
- Model: `python3 -I deploy/stake_geo_v2.py RAW_DIR --out data/stake_geo_v2.json`. It is stdlib only and reuses the v1 city map and RTT matrix.
- Collectors: `deploy/speed_gap6/` holds `scan_creates2.py`, `pos.py`, `lg.py` and `lgmat.py`.
- Small raw files: `data/speed_gap6/` holds the looking-glass matrix, positions, ipinfo, DZ IPs and the watched-wallet slots.

---

## 0. Answer

1. **The ranking holds; the levels drop.** FRA (or AMS) first and Ohio last survive every variant.
   - FRA single-box block-0 eligibility goes from **0.784 (v1) to 0.74 with direct sends**, 0.71 if BAM rounds are 50 ms, and **0.675 if every send goes through 50 ms Jito bundles**.
   - Ohio (CMH, AWS) goes from 0.49 to **0.38–0.45**. It is never a hot-path site.
   - This matches the critic's 0.65–0.72 range. VERIFIED as a model output; the inputs are measured, the model form is LIKELY.
2. **What each change did on its own:**
   - **(b) batching is the only change that matters**: −4.3 to −10.7 points.
   - **(a) the ingress metro is about neutral at metro level.** It reproduces 0.784; it matters per leader, not per site.
   - **(c) weighting by creates instead of slots**: −0.2 points.
   - **(d) the empirical in-block position**: −0.7 points versus uniform. The position distribution is close to uniform: median 0.53 by our count, 0.48 in tx-landing.
3. **Measured in-building RTTs (VERIFIED, TeraSwitch looking glass).** Pings come from TeraSwitch's own router in **Frankfurt-FRA2** (source 64.130.50.5). Minimum and average RTT:
   - Jito FRA **0.076 / 0.088 ms**
   - Helius fra-sender **0.089 / 0.121**
   - Harmonic FRA Remote TPU **0.084 / 0.088**
   - BAM fra-bam-2 **0.084 / 0.093**
   - DZ `dz100a-fra2-tsw` (public IP **198.13.140.16**, read from the DZ ledger) **0.109 / 0.115**

   All of them are in the same building. FRA2 to the AMS endpoints is **5.8–5.9 ms**.
4. **Amsterdam has three TeraSwitch sites, so pick AMS3 (or AMS2), never AMS1.**
   - AMS3 (64.130.42.0/24) is 0.07–0.09 ms from Helius ams-sender, Harmonic AMS and BAM ams-bam-2. That BAM node is the largest single ingress in the network, at **10.1% of leader slots**.
   - AMS3 is 0.17 ms from Jito AMS and the DZ `ams2-tsw` device.
   - AMS1 is 0.65–0.95 ms from all of these.
5. **What can be bought:**
   - **TeraSwitch:** list price about **$425/mo for a Ryzen 9 9950X, 128 GB, 2×25G**, billed hourly with no minimum (aggregator listing, LIKELY). Stock and the order flow cannot be seen from here: teraswitch.com and the portal return 403. A sales or portal request is required.
   - **In stock today in the same FRA campus: Cherry Servers FRA.** It measures **0.089 ms** from the TeraSwitch FRA2 router. Stock is 2 × EPYC 9375F (32 cores, 3.8 GHz, 384 GB) at **€989/mo or €1.693/h**, and 2 × Threadripper PRO 7975WX at €1,249/mo. Every Ryzen and EPYC-4004 plan shows 0 in FRA and AMS (VERIFIED, Cherry public API).
   - **Latitude.sh FRA** is **0.69–0.74 ms** away, so it is a different building. Example plan: f4.metal.small at $398/mo; stock is not public.
6. **The second box: Tokyo for eligibility, Amsterdam for racing.** Under the corrected model, with direct sends and BAM at 20 ms:
   - **FRA2 + TYO2 = 0.817** eligibility, **+7.8 points** over FRA alone. That is the largest gain of any second site; Ashburn is a close second at 0.814.
   - **FRA2 + AMS3 = 0.752**, only **+1.3 points**.
   - **An AMS box changes the race outcome on about 8% of creates.** Those are creates on Amsterdam-ingress leaders where an AMS-colocated rival beats a FRA box by 5.85 ms: Jito-class 4.5%, BAM 10.0% × 5.85/20 and Harmonic 4.2% × 5.85/50. That rises to about 15% while BAM is in maintenance mode.
   - **Recommendation:** buy FRA2 now and TYO2 second. Then add AMS3 or EWR1/IAD1, depending on where paper trading shows the contested profitable creates sit (§4).

---

## 1. Inputs (all pulled 2026-10-14)

| Input | Source | Result |
|---|---|---|
| Leader schedule | `getLeaderSchedule` for epoch 1057 (current, slot index 173,757) and for epoch 1056 (via slot 456,200,000) | 432,000 slots per epoch. VERIFIED |
| Ingress address | `getClusterNodes` gives `tpuQuic` per identity; the IPs are geolocated with ipinfo.io | 661 leaders and **219 unique ingress IPs**; 90 IPs cover 95.3% of slots. VERIFIED |
| Leader class | DZ [`/solana/validators-metadata`](https://data.doublezero.xyz/api/v1/solana/validators-metadata) `software_client`, joined via `getVoteAccounts` | Slot-weighted: BAM 37.6%, Jito-class 36.9%, Harmonic 25.5%. Create-weighted: BAM 38.8%, Jito 35.1%, Harmonic 26.1%. VERIFIED |
| Creates | `getSignaturesForAddress(TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM)` (the pump.fun mint authority signs every create) over epochs 1056 and 1057, joined to the schedule | **63,537 successful creates**. VERIFIED |
| Watched-wallet proxy | `data/funding_creators.jsonl` (bundled and notable arms) plus gap-1 creators with 3 or more creates | **263 creates**. This is a small sample; treat it as a sanity check only. LIKELY |
| Position in block | 250 random create slots, `getBlock` with `transactionDetails:"accounts"`; position = index among non-vote txs / (count − 1) | n = 269. p10 0.10, p25 0.26, **median 0.53**, p75 0.76, p90 0.89. Deciles are flat (26, 26, 25, 20, 31, 22, 37, 27, 29, 26). Counting vote txs too, the median is 0.59. VERIFIED. tx-landing had p25 24%, median 48%, p75 81% |
| Slot time | `getRecentPerformanceSamples(30)` | **216.7 ms**. VERIFIED |
| RTT, send leg | [lg.teraswitch.com](https://lg.teraswitch.com) (hyperglass, AS20326) pings from **7 TeraSwitch buildings** to the top 90 ingress IPs: 630 pings, 492 answered | Covers **94% of leader slots** per site. The rest fall back to the v1 metro matrix. VERIFIED |
| RTT, feed leg | The same measured RTT when the validator and its ingress share a metro; otherwise the v1 matrix to the validator's city (Stakewiz) | LIKELY |

**Model.** A create lands at position x of a slot of length S. Our transaction reaches the ingress after L = RTT_feed/2 + RTT_send/2 + c₀, with c₀ = 5 ms for decode, sign and relay. It then waits w ~ U[0, W_class] for its batch to close. It is block-0 eligible if x·S + L + w ≤ S.

Each leader is served from whichever box has the lowest L. The leader schedule is known per epoch, so routing per leader is free.

The class mapping:
- **JITO:** JitoLabs, Agave, Rakurai, and the Firedancer and Frankendancer validators, which must run Agave from Oct 21 (critique-speed C19).
- **BAM:** AgaveBam and FireBAM.
- **HARM:** HarmonicAgave and HarmonicFiredancer.

Scenarios:

| Scenario | JITO W | BAM W | HARM W | Notes |
|---|---|---|---|---|
| `v1_published` | 0 | 0 | 0 | Reproduces v1 |
| `direct_bam20` | 0 | 20 | 50 | Direct QUIC or sender; BAM at 20 ms per the [Oct 9 changelog](https://bam.dev/blog/bam-changelog-sept-23-oct-9/). **Primary case** |
| `direct_bam50` | 0 | 50 | 50 | BAM at ~50 ms per the [maker-plugin page](https://bam.dev/docs/bam/maker-plugin/how-it-works/) |
| `bundle_all50` | 50 | 50 | 50 | Everything sent as Jito bundles ([50 ms ticks](https://docs.jito.wtf/lowlatencytxnsend/)) |
| `bam_maintenance` | 0 | 0, re-routed to the validator's own metro | 50 | BAM validators "revert to ordinary Jito-Agave Block Engine mode" |

Harmonic's `fba` is "50 ms double-buffered", per [docs.harmonic.gg scheduling-strategies](https://docs.harmonic.gg/concepts/scheduling-strategies.md).

---

## 2. Results (weights = pump.fun creates, empirical position)

P(block-0 eligible). "≤1 ms" is the share of creates whose best box is within 1 ms one-way (2 ms RTT), which is the in-building race coverage.

| Sites | v1 | **direct, BAM 20** | direct, BAM 50 | all bundles | BAM maintenance | ≤1 ms | mean one-way |
|---|---|---|---|---|---|---|---|
| FRA2-tsw | 0.782 | **0.739** | 0.714 | 0.675 | 0.748 | 0.333 | 40.7 ms |
| AMS3-tsw | 0.787 | **0.742** | 0.717 | 0.678 | 0.749 | 0.185 | 39.8 |
| LON1-tsw | 0.777 | 0.731 | 0.707 | 0.666 | 0.743 | 0.067 | 41.9 |
| EWR1-tsw | 0.560 | 0.514 | 0.490 | 0.450 | 0.530 | 0.046 | 85.5 |
| CMH (Ohio, AWS) | 0.496 | **0.449** | 0.426 | 0.384 | 0.465 | 0 | 100.8 |
| SQQ (Cherry LT) | — | 0.620 | — | 0.557 | — | 0.045 | — |
| TYO2-tsw | 0.146 | 0.131 | 0.119 | 0.112 | 0.134 | 0.056 | 182 |
| FRA2 + AMS3 | 0.795 | **0.752** | 0.728 | 0.689 | 0.761 | **0.518** | 37.4 |
| FRA2 + TYO2 | 0.865 | **0.817** | 0.789 | 0.750 | 0.830 | 0.389 | 22.4 |
| FRA2 + IAD1 | — | 0.814 | — | 0.746 | — | 0.368 | — |
| FRA2 + SGP1 | 0.854 | 0.806 | 0.779 | 0.739 | 0.818 | 0.374 | 24.8 |
| FRA2 + EWR1 | 0.849 | 0.802 | 0.775 | 0.735 | 0.818 | 0.379 | 26.5 |
| FRA2 + AMS3 + TYO2 | 0.876 | 0.830 | 0.802 | 0.762 | 0.842 | 0.573 | 19.7 |
| FRA2 + TYO2 + EWR1 | 0.915 | **0.868** | 0.840 | 0.800 | 0.885 | 0.435 | 12.2 |
| FRA2 + AMS3 + EWR1 + TYO2 | 0.923 | 0.877 | 0.849 | 0.809 | 0.893 | 0.620 | 10.2 |

**The other weightings agree** (`data/stake_geo_v2.json`):
- Weighted by leader slots, FRA2 = 0.742 and FRA2 + TYO2 = 0.822 in the direct case.
- Weighted by watched wallets (n = 263), FRA2 = 0.748, FRA2 + TYO2 = 0.819, and FRA2 + IAD1 = 0.830.
- With a uniform position instead of the measured one, each P is +0.006 to +0.008.

The best pair is AMS3 + TYO2 at 0.821, a statistical tie with FRA2 + TYO2. The best triple is FRA2 + TYO2 + IAD1 at 0.871. VERIFIED (model output).

**Per class, FRA2 alone, direct case:** Jito-class 0.853, Harmonic 0.758, BAM 0.623. BAM scores lowest because about half of BAM's ingress is outside FRA, AMS and LON: Šiauliai, Tokyo, Singapore, Ashburn, Seattle and Newark (BAM-node shares from speed-gap-1 §4). Adding TYO2 lifts BAM to 0.777.

**The TeraSwitch backbone is faster than the internet medians v1 used** (VERIFIED, looking-glass minimums to leader ingress IPs):

| Pair | TeraSwitch measured | v1 matrix |
|---|---|---|
| FRA–AMS | 5.83 ms | 7.4 |
| FRA–LON | 11.2 | 16.8 |
| FRA–Newark | 75.2 | 85.9 |
| FRA–SGP | 148.6 | 156.9 |
| FRA–TYO | **212.7** | 230 |
| TYO–SGP | 64.5 | 68 |
| TYO–LAX | 104 | 105 |
| IAD–Newark | 5.4 | 7.5 |
| FRA–Šiauliai (Cherry) | 30.6 | 24.8 (slower in reality) |

DoubleZero's dedicated `dzd-fra-01:dzd-tok-01` link measures 133.9 ms (`/dz/links/latency`). That route is not available to a plain TeraSwitch box, so it was not used.

### 2.1 How to read eligibility under batching (LIKELY)

On FBA and BAM leaders, any arrival inside the same window lands in the same batch, and then fee decides. So being 0.1 ms or 5 ms from the ingress makes no difference, except when the extra delay pushes us across a window boundary.

The probability of crossing is about Δ/W:
- For a FRA box against an AMS-colocated rival (Δ = 5.85 ms), that is **12% on Harmonic** and **29% on BAM at 20 ms**.
- On **Jito-class leaders (arrival-ordered), Δ > 0 loses every time** against an equally fast rival.

The ≤1 ms column is therefore the race metric for Jito-class leaders, and P(eligible) is the metric for the rest.

---

## 3. In-building RTT (TeraSwitch looking glass, VERIFIED 10:03–10:15 UTC)

These are hyperglass pings from TeraSwitch edge routers: 5 packets, minimum / average in ms. A customer server adds a top-of-rack hop and the host stack, so expect about **0.1–0.2 ms server-to-server** (LIKELY).

| Target | **FRA2** (src 64.130.50.5) | AMS1 (74.118.140.250) | AMS2 (64.130.52.5) | **AMS3** (64.130.42.5) |
|---|---|---|---|---|
| 64.130.50.88 Jito BE FRA | **0.076 / 0.088** | 6.57 | 5.86 | 5.88 |
| 64.130.50.91 Helius fra-sender | **0.089 / 0.121** | 6.48 | 5.79 | 5.84 |
| 198.13.140.105 Harmonic FRA Remote TPU (10.4% of slots) | **0.084 / 0.088** | 6.56 | 5.79 | 5.86 |
| 198.13.140.227 BAM fra-bam-2 (5.3%) | **0.084 / 0.093** | 6.74 | 5.86 | 5.87 |
| 198.13.140.16 DZ `dz100a-fra2-tsw` | **0.109 / 0.115** | 6.55 | 5.80 | 5.88 |
| 64.130.32.214 Helius LaserStream FRA | 0.071 | 6.62 | 5.84 | 5.90 |
| 64.130.47.135 Nozomi fra2 / 64.130.40.42 NextBlock FRA | 0.087 / 0.079 | 6.6 | 5.8 | 5.86 |
| 64.130.57.215 Helius validator TPU (3.6%) | 0.092 | — | — | 5.87 |
| 185.191.117.97 Astralane (**Cherry FRA**) | **0.089** | — | — | 5.90 |
| 104.204.143.114 Allnodes "Offenbach" TPU (1.8%) | 0.106 | — | — | 5.88 |
| 109.94.96.69 Jito relayer / 185.26.10.33 TPU (**Latitude FRA**) | 0.694 / 0.741 | 6.8 | 6.1 | 6.16 / 6.07 |
| 64.130.52.119 Jito BE AMS | 5.81 | 0.856 | **0.118** | 0.168 |
| 64.130.42.91 Helius ams-sender | 5.85 | 0.745 | 0.192 | **0.073** |
| 63.254.166.232 Harmonic AMS Remote TPU (4.0%) | 5.86 | 0.653 | 0.139 | **0.073** |
| 63.254.167.197 BAM ams-bam-2 (**10.1%**, the largest ingress) | 5.85 | 0.722 | 0.153 | **0.082** |
| 64.130.52.8 DZ `dz100a-ams2-tsw` | 5.80 | 0.799 | 0.155 | 0.162 |
| 195.189.96.10 Cherry AMS | 6.08 | — | — | 0.367 |
| 64.130.46.242 Harmonic LON Remote TPU (6.8%) | 11.23 | 6.21 | 5.66 | 5.61 |
| 88.216.192.51 BAM sqq-bam-2 (Cherry Šiauliai, 4.4%) | 30.55 | 37.2 | 37.5 | 34.5 |

**DZ device IPs** come from the DZ ledger, via `getProgramAccounts` on serviceability program `ser2VaTMAcYTaauMrTSfSrxBaUDq7BLNs2xfUugTAGv` at the [DZ mainnet RPC](https://raw.githubusercontent.com/malbeclabs/doublezero/main/config/src/constants.rs). The `Device.public_ip` field sits at byte offset 115 ([device.rs](https://raw.githubusercontent.com/malbeclabs/doublezero/main/smartcontract/programs/doublezero-serviceability/src/state/device.rs)):

| Device | Public IP |
|---|---|
| fra2-tsw | **198.13.140.16** |
| ams2-tsw | 64.130.52.8 |
| lon1-tsw | 63.254.168.48 |
| tyo2-tsw | 198.13.133.48 |
| ewr1-tsw | 141.98.216.64 |

[`/edge/shreds/devices`](https://data.doublezero.xyz/api/v1/edge/shreds/devices) shows `fra2-tsw` with 37 of 37 seats available and `ams2-tsw` with 21 of 21, both at $100 per epoch. VERIFIED.

**Which building is FRA2?** LIKELY Equinix FR2, but this is not confirmed:
- [PeeringDB netfac](https://www.peeringdb.com/api/netfac?net_id=15072) lists TeraSwitch in Equinix FR2 and FR5.
- On DZ links, `fra2-tsw ↔ fr2-dzx-001` has a p50 of **160 µs**, against **374 µs** for `fra2-tsw ↔ dz-fr5-sw01`.

**Amsterdam:** PeeringDB lists Equinix AM3 and AM4 (Science Park), Digital Realty AMS17, NIKHEF and Iron Mountain AMS-1. AMS2 and AMS3 sit 0.15–0.19 ms apart, which looks like one campus (LIKELY). Which facility is which is UNVERIFIED.

---

## 4. Second box: AMS vs TYO under the corrected model

**Where the creates are, by ingress city (create-weighted):**

| Ingress | Jito-class | BAM | Harmonic |
|---|---|---|---|
| Frankfurt | 14.0% | 5.8% | 11.2% |
| Offenbach / Eschborn | 3.4% | — | — |
| Amsterdam | 4.5% | 10.0% | 4.2% |
| London | 2.4% | 0.9% | 7.0% |
| Tokyo | 0.3% | 4.3% | 0.9% |

Singapore adds 4.1%.

**What each candidate adds over FRA2 alone (direct case):**

| Second box | Eligibility gain | In-building share | Creates whose race outcome flips vs a colocated rival |
|---|---|---|---|
| **TYO2** | **+7.8 points** (0.739 → 0.817). Tokyo creates go from P ≈ 0.06–0.26 to about 0.85 | 0.33 → 0.39 | ~5.5% (Tokyo ingress) |
| **AMS3** | +1.3 points | 0.33 → **0.52** | **~7.9%**: 4.5 (Jito) + 10.0 × 0.29 (BAM) + 4.2 × 0.12 (Harmonic). About 15% in BAM maintenance |
| IAD1 or EWR1 | +7.5 / +6.3 points | +0.03–0.05 | ~4% |

**Interpretation (LIKELY).**
- The creates TYO2 adds are ones that **Europe-only snipers cannot reach in block 0 at all**, so only bots with an APAC box compete for them. F-Project claims Tokyo.
- The creates AMS3 converts become **ties settled by fee** against the bots that are already colocated in AMS (F-Project claims Amsterdam).
- Both are worth about 8% of creates, but TYO's creates are less contested.
- AMS3 is also a 5.8 ms hot standby for FRA2, which matters operationally.

**Decision rule:**
- Buy **FRA2 now** and **TYO2 second**.
- Run paper mode for a week, then decide the third box. If the profitable contested snipes cluster on AMS-ingress BAM leaders, buy AMS3; otherwise buy EWR1 or IAD1.
- FRA2 + TYO2 + EWR1 (0.868) beats FRA2 + AMS3 + TYO2 (0.830) on eligibility.
- F-Project's footprint without LA is FRA + AMS + EWR + TYO, which scores 0.877.

---

## 5. What can be bought (stock, price, contract)

| Provider and site | Distance to the FRA2 endpoints | Hardware and price | Contract | Stock | Label |
|---|---|---|---|---|---|
| **TeraSwitch FRA2 / AMS3** | in-building (0.08–0.12 ms router RTT) | Ryzen 9 9950X 16C, 128 GB DDR5, 4 TB NVMe, 2×25G: **$425/mo** (estimate) | Hourly on-demand with no minimum; reserved pricing by quote. 150 TB pooled egress in EU/NA (75 TB in APAC); $10/TB overage; DDoS protection, private VLAN ([whtop listing, Oct 3 2026](https://www.whtop.com/plans/teraswitch.com/158524)) | **UNVERIFIED.** teraswitch.com, `/bare-metal` and portal.teraswitch.com all return 403 here | LIKELY (aggregator) |
| **Cherry Servers FRA** | **0.089 ms** from the FRA2 router, indistinguishable from in-building | **EPYC 9375F**, 32C at 3.8 GHz, 384 GB, 3 Gbps: **€989/mo**, €1.693/h, €10,088/yr. **TR PRO 7975WX**, 32C at 4.0 GHz, 768 GB: €1,249/mo | Hourly, monthly, quarterly, semi-annual, annual or spot | **EPYC 9375F: 2; 7975WX: 2.** Ryzen 7950X (€199), 9900X (€196), 9950X (€349) and EPYC 4564P (€259): **0** in FRA and AMS | VERIFIED ([api.cherryservers.com/v1/plans](https://api.cherryservers.com/v1/plans), about 10:20 UTC) |
| Cherry AMS | 0.37 ms from AMS3 | TR PRO 7975WX €1,249 (8 in stock), TR PRO 7965WX €979 (3) | Same as Cherry FRA | as listed | VERIFIED |
| Latitude.sh FRA / FRA2 / AMS | FRA: **0.69–0.74 ms**, a different building. AMS not measured | f4.metal.small (AMD 4484PX, 12C at 4.4 GHz, 96 GB, 2×10G) **$398/mo** ($0.55/h). f4.metal.medium (4564P, 16C at 4.5 GHz) $555. m4.metal.small $296 | Hourly; reserved plans paid monthly or yearly ([pricing](https://www.latitude.sh/pricing)) | Not public (the API requires a token) | VERIFIED price; stock unknown |
| ERPC FRA (TeraSwitch-adjacent reseller) | — | APP+ 7950X €298, APP++ 9950X €398 | — | **Sold out since Mar 2026** ([ERPC](https://slv.dev/en/news/2026/03/11/erpc-frankfurt-standard-metal-soldout-202603/)) | LIKELY |

**What to send TeraSwitch sales** (or the portal) today:
1. Ask for a 9950X box in **FRA2** and one in **AMS3**, the 64.130.42.0/24 site. Ask about TYO2 as well.
2. Ask for the current stock and delivery date.
3. Ask for a test IP. Its pass criterion is the table in §3: under 0.2 ms to 64.130.50.88, .91, 198.13.140.105, .227 and .16.
4. Ask whether a cross-connect to `dz100a-fra2-tsw` is possible.

**Fallback if TeraSwitch cannot deliver this week:** the **Cherry FRA EPYC 9375F**. It is in stock now, billed hourly, and measured to be on the same campus. Its 3.8 GHz base clock (Zen 5) is fine for a path that costs about 50 µs of CPU.

---

## 6. Caveats

- **The router-ping proxy (LIKELY).** The looking glass pings from TeraSwitch's router, not from a rented server. 138 of 630 targets dropped ICMP; those fall back to the v1 matrix, which covers 6% of slots.
- **Position is measured by index.** Our in-block position is an index fraction, not time. tx-landing showed RPC order is noisy inside entries.
- **The batching wait is assumed to be uniform.** Under FBA, real waits depend on how long the previous batch takes to drain.
- **Watched-wallet sample.** n = 263, so it is a sanity check only.
- **BAM maintenance had not started** as of 10:00 UTC: every BAM node IP was still advertised. Re-run around Oct 18–28 (critique-speed G-list).
- **Alpenglow.** After Oct 21, Firedancer leaders move to Agave, as modelled. The slot time and window cadences need re-measuring after the cutover.
