# Where to put the servers: Solana leader geography and region choice (2026-10-14)

Labels: **VERIFIED** means primary data or our own measurement. **LIKELY** means a strong inference or a single credible source. **UNVERIFIED** means a vendor claim or a guess.

Reproduce with `python3 -I deploy/stake_geo.py --out data/stake_geo.json` (stdlib only; it fetches Stakewiz and the public RPC). The endpoint scan is `python3 -I deploy/infra_endpoints.py data/infra_endpoints_geo.json`. This doc complements `speed-data-feed.md`, which covers feeds; this one covers placement.

---

## 0. TL;DR

1. **About 56% of all leader slots are produced in Frankfurt plus Amsterdam, and about 74% in Europe.** Ohio is not a validator hub: it holds 0% of stake, and its nearest real hub is Chicago at 1.3%. **VERIFIED** (computed below).
2. **Almost the whole sending and feed industry sits inside one provider, TeraSwitch (AS20326).** Every Jito block engine, every Helius Sender and LaserStream region, all Nozomi Direct PoPs, NextBlock, bloXroute, BlockRazor, Triton FRA/AMS and 0slot `de2` resolve to TeraSwitch IPs. TeraSwitch also hosts **26–28% of stake and leader slots**. **VERIFIED** (DNS + ipinfo, §2).
3. **Best single box: bare metal in TeraSwitch Frankfurt.** The model puts AMS level with FRA on same-slot probability (0.782 vs 0.780). FRA wins on in-metro coverage (34% vs 20% of leader slots within 2 ms), and that is what decides a race against co-located bots.
4. **Ohio (us-east-2) has an expected same-slot rate of 0.51, against 0.78 for FRA.** It adds about 58 ms to mean RTT to the leader and about 90 to 107 ms on every FRA/AMS slot. It has **0% in-metro coverage**, so against a co-located competitor it wins essentially no European races. Use it for analytics and backtests, never for the hot path. **LIKELY** (model on measured RTTs).
5. **Build order:**
   - 1 site: FRA.
   - 2 sites: FRA+AMS, which gives 56% in-metro (best for racing). FRA+TYO gives the best raw block-0 rate, 0.867.
   - 3 sites: FRA+AMS+TYO or FRA+AMS+NYC.
   - 4 sites: FRA+AMS+NYC+TYO, which already matches F-Project's claimed footprint on every metric that matters.
   - 6 sites: add LON and SIN, which **beats** F-Project's claimed set (81% vs 70% in-metro).

---

## 1. Stake and leader geography (data)

**Data:**
- Stakewiz `/validators`, fetched 2026-10-14 10:14 UTC+2: 683 active validators with stake and 438.63 M SOL. The public RPC `getVoteAccounts` reports 438.13 M SOL, so coverage is complete.
- Current-epoch `getLeaderSchedule` for epoch 1057: 432,000 slots, of which only 176 (0.04%) belong to identities missing from Stakewiz.
- Geolocation is Stakewiz's `ip_city`/`ip_asn` for each validator's IP.
- **VERIFIED** (computed; raw summary in `data/stake_geo.json`).

**Leader-slot share by metro.** Metro groups: FRA includes Rüsselsheim, Limburg and Strasbourg; AMS includes Diemen, Rotterdam and Haarlem; NYC includes Newark, Secaucus, Piscataway, Toronto and Montreal at a penalty; WAS includes Ashburn and Pittsburgh.

| Metro | Leader slots | | Metro | Leader slots |
|---|---|---|---|---|
| **FRA** | **35.3%** | | SIN | 4.2% |
| **AMS** | **21.0%** | | LAX | 2.1% |
| LON | 9.2% | | DUB | 2.1% |
| TYO | 6.1% | | STO | 2.0% |
| NYC/NJ | 5.4% | | PDX (AWS Boardman) | 1.3% |
| SQQ (Šiauliai, LT) | 4.5% | | CHI | 1.3% |
| WAS/Ashburn | 4.2% | | HKG 0.7%, SLC 0.5% | |

**By region:**

| Region | Share |
|---|---|
| EU | 73.9% |
| APAC | 10.9% |
| US-East | 9.8% |
| US-West | 4.1% |
| US-Central (Chicago) | 1.3% |
| **Ohio** | **0%** |

**By hosting:**
- Bare metal or colo: 90.3%.
- Clouds: AWS 4.3% (mostly one AWS us-east-1 operator at 2.8% of slots), OVH 2.3%, Vultr 1.8%, Alibaba 1.2%, Hetzner 0.06%.

**Top ASNs by stake:**

| ASN | Stake |
|---|---|
| TeraSwitch | 26.2% + 1.7% |
| Allnodes | 8.7% |
| Latitude.sh (AS396356, including resold "Saveincloud" and "Megaport UK" blocks) | ≈13% |
| Cherry Servers (AS16125/214159/213896) | ≈8.8% |
| Galaxy | 3.2% |
| Servers.com | 4.8% |

**Leader slots by building, a level below metro:**

| Location and provider | Leader slots |
|---|---|
| TeraSwitch FRA | 10.8% |
| TeraSwitch AMS | 7.5% |
| Allnodes FRA | 6.7% |
| Latitude FRA | 7.8% |
| Cherry Šiauliai | 4.1% |
| TeraSwitch TYO | 3.5% |
| Cherry FRA ("Private Customer" AS213896) | 2.9% |
| AWS us-east-1 | 2.8% |

Across all metros, TeraSwitch hosts **28.0% of leader slots**. **VERIFIED.**

This matches the independent leader-schedule join in `speed-data-feed.md` (FRA 32.5%, AMS 18.9% by city only).

---

## 2. Where the latency-critical infrastructure physically lives

We resolved every published regional endpoint and geolocated the IPs (ipinfo.io), on 2026-10-14. **VERIFIED** (`data/infra_endpoints_geo.json`):

| Provider | Regions (source) | Host network |
|---|---|---|
| **Jito Block Engine** | AMS, DUB, FRA, LON, NY (resolves to Newark), SLC (Bluffdale UT), SGP, TYO, plus global ([docs.jito.wtf/lowlatencytxnsend](https://docs.jito.wtf/lowlatencytxnsend/)) | **All TeraSwitch** |
| Jito ShredStream | Sunset Sep 5, 2026; DZ Edge is the successor (see `speed-data-feed.md`) | n/a |
| **Helius Sender** | `slc, ewr, lon, fra, ams, sg, tyo` ([helius sender docs](https://www.helius.dev/docs/sending-transactions/sender)) | **All TeraSwitch** |
| **Helius LaserStream** | `ewr, pitt, slc, lax, lon, ams, fra, tyo, sgp` ([laserstream grpc docs](https://www.helius.dev/docs/laserstream/grpc)) | **All TeraSwitch** |
| **Nozomi (Temporal)** | pit1, ewr1, ash1, lax1, fra2, ams1, lon1, tyo1, sgp1 ([use.temporal.xyz/nozomi/endpoints](https://use.temporal.xyz/nozomi/endpoints)). `pit1` resolves to the same Newark IP as `ewr1`. | **All TeraSwitch** |
| **0slot** | ny, de1, de2, ams, la, jp (hostnames from [sol-trade-sdk constants](https://raw.githubusercontent.com/0xfnzero/sol-trade-sdk/main/src/constants/swqos.rs), which comments "de2 for TSW, de1 for OVH") | de2 is TeraSwitch FRA; de1 is OVH FRA; the others sit behind Cloudflare anycast |
| **NextBlock** | ny, fra, ams, london, tokyo, slc (same SDK) | All TeraSwitch |
| **bloXroute** | submit-only: uk, ny, germany, amsterdam, tokyo, global ([docs.bloxroute.com](https://docs.bloxroute.com/solana/trader-api/introduction/regions)) | All TeraSwitch |
| Astralane | fr, ams, ny, jp, la, sg | fr = Cherry FRA, jp = Latitude TYO, ams/ny = TeraSwitch |
| BlockRazor, Triton (fra/ams.rpcpool.com) | FRA, NY / FRA, AMS | TeraSwitch |
| DoubleZero Edge | 31 metros, including Columbus (`cmh`) | own fiber (see feed doc) |

**Implication (LIKELY).** In-building cross-connect RTT to Jito, Helius and Nozomi FRA plus 10.8% of leader slots is around 0.1–0.3 ms. From any other FRA datacenter it is about 0.5–2 ms (UNVERIFIED; measure it). TeraSwitch Frankfurt is therefore the one building that gives both the senders and the largest single block of leaders at the same time.

---

## 3. Inter-metro RTT (published matrices)

**Sources:**
- **W**: WonderNetwork, median of 336 hourly 30-ping averages over 14 days ([wondernetwork.com/pings/A/B](https://wondernetwork.com/pings/Frankfurt/Columbus)).
- **D**: DoubleZero data API, 24 h internet p50 ([data.doublezero.xyz `/api/v1/dz/metro-pairs/latency`](https://data.doublezero.xyz/api/v1/docs)).
- **C**: CloudPing AWS inter-region p50 over 1 year ([cloudping.co/api/latencies](https://www.cloudping.co/api/latencies?percentile=p_50&timeframe=1Y)).

All fetched 2026-10-14. **VERIFIED** as published values.

**RTT in ms from candidate box (row) to leader hub (column).** Ohio = Columbus = us-east-2.

| From \ To | FRA | AMS | LON | TYO | NYC | WAS | SIN | CHI | LAX |
|---|---|---|---|---|---|---|---|---|---|
| **Ohio (us-east-2)** | 107.9 W (103.6 C) | 97.4 W | 90.1 W | 145.8 W (136.7 C) | 15.9 W | 13.9 C | 233.3 W | 12.9 W | 60.3 W |
| Virginia (us-east-1) | 87.9 W (93.1 C) | 87.2 W | 75.3 W | 149.4 C | 7.5 W | — | 213.6 W | 19.7 W | 56.4 W |
| NYC / Newark | 85.9 W | 76.3 W | 70.0 W | 173.1 W | — | 7.5 W | 220.0 W | 18.0 W | 66.4 W |
| **Frankfurt (eu-central-1)** | — | 7.4 D (10.7 W) | 16.8 D | 230.0 W | 85.9 W | 87.9 W | 156.9 D | 106.9 W | 145.8 W |
| Paris (eu-west-3) | 10.2 C (12.6 W) | 13.5 W | 7.2 W | 221 C | 72.9 W | 78.5 W | 150.6 W | 96.6 W | 127.7 W |
| Amsterdam | 7.4 D | — | 6.4 D | 235.8 W | 76.3 W | 87.2 W | 155.7 W | 91.6 W | 131.9 W |
| Tokyo (ap-northeast-1) | 230 W (228 C) | 235.8 W | 214 C | — | 173.1 W | 149.4 C | 68.0 W | 148.3 W | 105.1 D |

**Other useful pairs:**

| Pair | RTT (ms) |
|---|---|
| FRA–Šiauliai | 24.8 D |
| FRA–Stockholm | 23.9 C |
| FRA–Dublin | 25.4 W |
| AMS–Dublin | 17.8 D |
| AMS–LON | 6.4 D |
| SIN–TYO | 68 W |
| SIN–HKG | 30 D |
| FRA–TYO over DoubleZero fiber | 133.9 D, against 235 D over the internet |

**Our own check from this container.** It egresses in Columbus, OH (ipinfo: AS396982 Google), behind a proxy. We measured TLS handshake time (`appconnect − connect`, roughly 2 RTT plus proxy overhead) to each Jito engine:

| Jito engine | Handshake (ms) |
|---|---|
| NY | 114 |
| SLC | 240 |
| LON | 258 |
| AMS | 278 |
| FRA | 310 |
| TYO | 418 |
| SGP | 571 |

The ordering matches W. The FRA−NY difference of about 196 ms works out to an RTT gap of about 98 ms; WonderNetwork gives 92 ms. **VERIFIED** (relative only).

**Ohio sits 13–16 ms from NYC, Ashburn and Chicago, and about 90–108 ms from every European hub.**

---

## 4. The "leader is random" problem: math and numbers

The leader changes every 4 slots, but the schedule is **known for the whole epoch in advance** (`getLeaderSchedule`). The leader is therefore not random at decision time; it is only spread across metros.

To land in the creation slot, our transaction must reach the **same leader** before its block for that slot closes. The creator's create executes at that leader. We see it through shreds, which go leader → us, and our buy travels us → leader. The geography cost is therefore the **round trip between our box and the leader's metro**.

**Model** (implemented in `deploy/stake_geo.py`):

- `S` = slot duration. Measured with `getRecentPerformanceSamples(30)`: **217 ms**.
- `c` = processing, relay and auction overhead. We use **5 ms**. BAM rounds can add up to 20 ms; see the feed doc.
- `w_m` = the fraction of leader slots in metro `m`.
- `r_m(D) = min over s in D of RTT(s, m)`, plus the intra-metro offset for satellite cities.
- The create lands at a uniform time `u` in `[0, S)`, and we make the slot iff `u + r + c < S`. So:

```
P_same(D)   = Σ_m  w_m · max(0, 1 − (r_m(D) + c) / S)
Coverage_t  = Σ_m  w_m · 1[r_m(D) ≤ t]        (t = 2 ms ≈ "in-metro", 10 ms ≈ "neighbour metro")
Gain(s | D) = P_same(D ∪ {s}) − P_same(D)
Race(D vs D') ≈ Σ_m w_m · (1 if r_m(D) < r_m(D') ; ½ if tie ; 0 otherwise)
```

**Why `Race` matters.** Being first among snipers is decided only between bots that can make the same slot. Two bots in the same metro race on software and fees. A bot 7 ms away loses almost every race on that leader.

**Results** (script output, epoch 1057 schedule; **LIKELY**, since the model assumes uniform create timing and medians):

| Set | Mean RTT to leader | P_same | ≤2 ms (in-metro) | ≤10 ms |
|---|---|---|---|---|
| **Ohio alone (current)** | 101.9 | **0.511** | 0.000 | 0.000 |
| Virginia/WAS alone | 88.5 | 0.569 | 0.038 | 0.084 |
| NYC alone | 86.5 | 0.579 | 0.044 | 0.082 |
| **FRA alone** | 43.8 | **0.780** | **0.341** | 0.554 |
| AMS alone | 43.7 | 0.782 | 0.202 | 0.639 |
| LON / Paris alone | 45.1 | 0.770 / 0.772 | 0.088 / 0.000 | 0.302 / 0.088 |
| TYO alone | 198.5 | 0.136 | 0.057 | 0.058 |
| FRA+TYO (best 2 for P_same) | 23.7 | 0.867 | 0.398 | 0.612 |
| FRA+AMS (best 2 for racing) | ≈38 | ≈0.80 | **0.56** | 0.64 |
| FRA+NYC+TYO (best 3 for P_same) | 13.0 | 0.917 | 0.442 | 0.694 |
| FRA+AMS+TYO | — | ≈0.887 | 0.624 | — |
| AMS+FRA+NYC+TYO (best 4 for racing) | 10.6 | 0.928 | 0.643 | 0.782 |
| AMS+FRA+NYC+SIN+TYO (best 5 for P_same) | 7.6 | 0.942 | 0.685 | 0.824 |
| **F-Project's claimed set: NY, LA, AMS, FRA, TYO** | ≈7.5 | ≈0.942 | 0.700 | — |
| FRA+AMS+LON+NYC+TYO+SIN (our 6) | ≈5.8 | ≈0.949 | **0.812** | — |
| Every metro (16 boxes) | 2.0 | 0.967 | 1.0 | 1.0 |

"≈" rows come from the metro-level approximation, a second pass using rounded weights that may be ±0.005 off the script.

**Race scores** against F-Project's claimed set, assuming equal software and fees (parity = 0.50):

| Our set | Race score |
|---|---|
| Ohio | 0.013 |
| FRA | 0.21 |
| FRA+AMS | 0.37 |
| FRA+AMS+TYO | 0.425 |
| FRA+AMS+NYC+TYO | 0.48 |
| FRA+AMS+LON+NYC+TYO+SIN | **0.56** |

LON (9.2%) and SIN (4.2%) are the metros their claimed footprint leaves uncovered. **LIKELY.**

**Marginal gains:**
- Ohio → FRA: **+0.27 P_same**, a 53% relative increase in block-0-eligible creates.
- FRA → +TYO: +0.087.
- → +NYC or WAS: +0.05.
- → +SIN: +0.013.
- → +AMS: +0.02 on P_same, but **+0.21 in-metro coverage**.
- After about 5 sites, P_same flattens at 0.94–0.95. Further boxes buy only race wins (in-metro coverage), not block-0 eligibility.

**Sensitivity.**
- With `c = 20 ms` (a BAM round plus a slow relay): FRA 0.72, FRA+NYC+TYO 0.85, Ohio 0.48.
- With 400 ms slots (the pre-reduction duration): FRA 0.88, Ohio 0.75.
- Ohio's penalty therefore **grows as slots get shorter**. With Alpenglow arriving about Oct 21 and 200 ms slots already live, geography matters more than it did when F-Project's videos were made.

**Operational consequence: an "active box per leader window" scheduler.** Each epoch, every box loads the leader schedule and the identity→metro map from this script. For each 4-slot window it marks the box with the lowest RTT to that leader as **primary**; the others stay silent or act as a backup with a lower fee.

To make duplicates harmless when two boxes fire, sign every snipe with a **durable nonce** taken from a shared pool. Only one transaction that advances a given nonce can ever land, so cross-region double-buys become impossible. **LIKELY** (this is standard durable-nonce semantics. The cost is a nonce-advance instruction and one more writable account; both should be measured against the ~60–70k CU target).

---

## 5. Bare metal vs cloud

**Why bare metal wins for this workload (LIKELY):**

1. **Colocation.** The senders (§2) and 28% of leader slots are in TeraSwitch buildings. AWS eu-central-1 is in Frankfurt but in different buildings, so you add an internet or IX hop of about 0.5–2 ms each way (UNVERIFIED; measure with `deploy/latency_probe.sh`). In a same-metro race that hop decides the outcome.
2. **NIC path.**
   - On bare metal you get a dedicated 10/25/100 GbE NIC with kernel bypass (AF_XDP or DPDK), busy-poll and full IRQ control.
   - EC2 ENA sits behind the Nitro virtual switch and the VPC/IGW NAT path. That adds tens of µs per packet and makes tail latency noisy under load.
   - AWS itself notes that ENA Express (SRD) only helps **instance-to-instance traffic inside one Region**, and adds "tens of microseconds" median at light load ([AWS docs](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/ena-express.html)). It is **useless for internet traffic to leaders and senders.** **VERIFIED.**
3. **Noisy neighbours and CPU.** Bare metal gives no steal time, a fixed boost clock and the ability to isolate cores. Consumer-class high-clock parts (Ryzen 9950X, EPYC 4004 series, EPYC 9275F) beat cloud vCPUs for single-thread signing and parsing. **LIKELY.**
4. **Traffic cost.** A full shred feed is high-bandwidth. Ingress is free on both, but egress is $10/TB over 150 TB on TeraSwitch versus about $0.09/GB on AWS. **LIKELY.**
5. **Multicast and GRE.** DoubleZero Edge delivers GRE-encapsulated multicast. That is trivial on bare metal. On EC2 it needs IP protocol 47 allowed and is unproven. **UNVERIFIED.**

**Providers** (prices are list prices, monthly, pre-tax):

| Provider | Solana relevance | Metros | Example price | Status |
|---|---|---|---|---|
| **TeraSwitch** | Hosts Jito, Helius, Nozomi, NextBlock, bloXroute, Triton; 28% of leader slots | FRA, AMS, LON, DUB, NJ, Ashburn, Pittsburgh, SLC, LAX, TYO, SGP, Stockholm, France | Ryzen 9 9950X 16C, 128 GB, 2×25G: **$425**; EPYC Milan 7313P: $177; EPYC 9254: $460; hourly billing, 150 TB egress pooled ([whtop listing, Oct 3 2026](https://www.whtop.com/plans/teraswitch.com/158524)) | LIKELY (aggregator; teraswitch.com blocked our fetch with a 403) |
| **Latitude.sh** | About 13% of stake, Astralane jp | FRA, FRA2, AMS, LON, LON2, NYC, ASH, CHI, LAX, TYO3/4, SGP, plus 15 more ([locations](https://www.latitude.sh/locations)) | m4.metal.small 6C 2×10G: $296; f4.metal.small EPYC 4484PX 12C @ 4.4 GHz: **$398**; f4.metal.medium 16C @ 4.5 GHz: $555; 20 TB egress ([pricing](https://www.latitude.sh/pricing)) | VERIFIED |
| **Cherry Servers** | About 8.8% of stake (Šiauliai 4.1% of slots), Astralane fr, DoubleZero partner | Lithuania, AMS, FRA, Stockholm, CHI, SGP, TYO ([pricing page](https://www.cherryservers.com/pricing/dedicated-servers)) | Plan table did not render for us | VERIFIED (locations) |
| OVH | 2.3% of stake; 0slot de1 | FRA (Limburg), Gravelines, Roubaix, etc. | Cheap; network path is not colocated with TeraSwitch | LIKELY |
| Servers.com | 4.8% of stake (AMS) | AMS and others | Quote | LIKELY |
| Vultr | 1.8% of stake | Many metros, bare metal and cloud | Hourly | LIKELY |
| **Hetzner** | 0.06% of stake | FSN/NBG/HEL | Cheap, but **its ToS bans crypto use including PoS and nodes, "everything crypto-related" except colocation** ([Decrypt](https://decrypt.co/108346/ethereum-nodes-cloud-services-host-hetzner-banned), [The Defiant](https://www.thedefiant.io/data-giant-shocks-ethereum-with-ban-on-mining)) | **Avoid** (LIKELY still in force) |
| Equinix Metal | Formerly popular | — | **Shut down June 30, 2026** ([DCD](https://www.datacenterdynamics.com/en/news/equinix-to-kill-off-metal-by-june-2026/)) | VERIFIED; not an option |
| AWS | 4.3% of stake; us-east-1 holds one 2.8%-slot operator | eu-central-1, eu-west-2, ap-northeast-1, us-east-1 | C8gn (Graviton4, up to 600 Gbps) is in eu-central-1 ([AWS](https://aws.amazon.com/about-aws/whats-new/2026/03/amazon-ec2-c8gn-instances-additional-regions)) | Fallback only |

**Ask any provider:**
- Is the box in the **same facility as TeraSwitch FRA (AS20326)**? Request a test IP and run `ping 64.130.50.88` (Jito FRA) and `ping 64.130.50.91` (Helius fra-sender). The goal is under 0.5 ms.
- Is there a DoubleZero port or cross-connect?

---

## 6. Recommendation

**(a) Only one box: Frankfurt, bare metal, TeraSwitch FRA.**
- If stock is unavailable, use Latitude FRA or Cherry FRA (both carry large FRA stake), and verify under 1 ms to `64.130.50.88`.
- This gives P_same 0.78, 34% of slots in-building or in-metro, and AMS (21%) at 7 ms.
- AMS is a statistical tie on P_same but worse for racing.
- If it must be AWS: **eu-central-1** (+0.5–2 ms, UNVERIFIED). Never Ohio.

**(b) Two and three boxes:**
- **2 sites: FRA + AMS (TeraSwitch AMS)** if the goal is winning races in the 56% of slots where most volume and competitors are. FRA + TYO if the goal is maximising block-0 eligibility (0.867).
- We recommend **FRA+AMS first**. pump.fun creates are spread uniformly over slots, and EU leaders are where both F-Project and the other snipers fight.
- **3 sites: FRA + AMS + TYO** (P_same ≈0.887, in-metro 62%), or FRA+AMS+NYC (0.864 / 62%).
- **4 sites: FRA+AMS+NYC+TYO** (0.928 / 64%) is our phase-2 target.
- **Then add LON and SIN** (6 sites: ≈0.949 / 81%, race score 0.56 versus F-Project's claimed set).
- The NYC box should sit in TeraSwitch NJ (Newark), which hosts Jito NY, Helius ewr, Nozomi and bloXroute. It also covers WAS/Ashburn at 7.5 ms.

**(c) Is Ohio acceptable? No, not for the hot path.**
- Expected penalty versus FRA: **+58 ms mean RTT to the leader**, and **+90 to +107 ms on the 56% of slots led from FRA/AMS**.
- P_same drops from 0.78 to 0.51, so **35% fewer creates are even block-0-eligible**.
- In-metro coverage is 0%, and the race score against a FRA/AMS/NY/TYO competitor is about 0.01.
- Paired with FRA, Ohio adds nothing that NYC wouldn't do better: FRA+CMH scores 0.856 versus FRA+NYC 0.859, but only NYC is co-located with the senders.
- **Keep the Ohio box** for the Python validation and backtest pipeline, dashboards, wallet discovery and a cold-standby signer. It is 13–16 ms from NYC, Ashburn and Chicago, so it can also probe US infra.

**(d) Instance and OS (actionable).**

*If AWS (fallback):*
- **Instance:** compute- or network-optimised current generation with ≥12xlarge or metal for full ENA queues and no CPU credit games. Use c8gn/c7gn (Graviton, highest pps) or c7i/c8i (x86, if you need x86-only crates or AVX-512 ed25519).
- Size medium (4–8 vCPU) is acceptable for the bot alone. Larger sizes give more ENA queues and lower jitter.
- **Placement and ENA:** a cluster placement group and ENA Express only help box-to-box traffic in-Region, such as the feed box to the sender box. **They do nothing for internet paths.**
- Use an Elastic IP; avoid NAT gateways.
- Keep the ENA driver ≥2.8 and enable the ENA LLQ (default on Nitro).
- Turn off `tcp_autocorking` for request/response sends ([AWS ENA docs](https://docs.aws.amazon.com/AWSEC2/latest/UserGuide/ena-express.html)).

*Everywhere (bare metal preferred):*
- **CPU:** set governor to `performance`. Disable C-states deeper than C1 (`processor.max_cstate=1 intel_idle.max_cstate=0` or `amd_pstate=active` with EPP set to performance). Disable SMT on the hot cores, or isolate siblings.
- **Core isolation:** boot with `isolcpus=2-5 nohz_full=2-5 rcu_nocbs=2-5`. Pin the feed-RX thread, the decode/sign thread and the sender thread to isolated cores on the NIC's NUMA node.
- **Interrupts:** `systemctl disable --now irqbalance`. Pin NIC IRQs to a separate core from the busy-poll thread. Set `ethtool -C <if> adaptive-rx off rx-usecs 0` (where the driver allows).
- **NIC settings:** RSS/flow steering puts shred UDP on one queue (`ethtool -N`). Enlarge rings (`ethtool -G`).
- **Busy polling:** `net.core.busy_poll=50`, `net.core.busy_read=50`, or `SO_BUSY_POLL` per socket. Better still, AF_XDP or `recvmmsg` busy-loop for shreds.
- **Buffers:** `net.core.rmem_max/rmem_default` ≥ 128 MB for shred bursts.
- **TCP and QUIC:** keep connections to Jito, Helius and Nozomi **warm** (persistent HTTP/2 or QUIC, TLS resumed). Pre-resolve DNS and pin IPs, which differ from anycast; for example, Nozomi `fra2` direct over HTTP, not Cloudflare `fra`. Use `TCP_NODELAY`.
- **Time:** use chrony against a local stratum-1 or Jito's regional NTP (`ntp.frankfurt.jito.wtf` and others, [Jito docs](https://docs.jito.wtf/lowlatencytxnsend/)) so cross-region latency logs are comparable.
- **Kernel and allocation:** a recent LTS kernel; transparent hugepages set to `madvise`; `vm.swappiness=1`; no `auditd` or noisy agents on hot cores.
- **Measure, don't assume:** run `deploy/latency_probe.sh` on every candidate box before signing a contract, and accept FRA only if Jito FRA and Helius fra-sender connect in under 1 ms.

---

## 7. Open items

1. Request trial IPs from TeraSwitch, Latitude and Cherry in FRA and measure RTT to `64.130.50.88/.91` and `64.130.47.135`. Quantify the AWS eu-central-1 → TeraSwitch FRA hop.
2. Replace the uniform-timing assumption with real data: measure where in the slot pump.fun creates land (entry index), and the per-leader-metro distribution of creates, to weight `w_m` by creates rather than slots.
3. Re-run `stake_geo.py` each epoch, because stake moves. Watch for Firedancer/Agave migrations around Alpenglow (Oct 21) changing hosting.
4. Verify that Jito's block engine forwards bundles cross-region when the leader is connected to a different region's engine (UNVERIFIED). This decides whether a FRA box can send to a TYO leader via `frankfurt.mainnet...` or must use `tokyo.mainnet...`.
5. Confirm the current Hetzner ToS text; we have only 2022–23 press coverage.
