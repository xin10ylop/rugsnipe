# Speed infrastructure and server placement (October 2026)

Short version: **your AWS box in Ohio (us-east-2) is the wrong place for the hot path.** Put the
execution box in **Frankfurt**, ideally bare metal in the TeraSwitch facility (or Latitude / Cherry in
Frankfurt), with **Amsterdam** as the second box. Keep Ohio for analytics, backtests and discovery,
which are not latency-sensitive. If it must stay on AWS, use **eu-central-1 (Frankfurt)**, never Ohio.

Everything below was checked today against primary data (Stakewiz validator data, the current epoch's
leader schedule, Jito's and Solana Foundation's own pages, and real on-chain transactions). Details and
sources are in `docs/research/speed-*.md`.

## 1. Four facts that changed since the videos were recorded

| Fact | Status | Why it matters |
|---|---|---|
| Slots are **200 ms** (measured 214-221 ms) since epoch 1053, Oct 9 2026 | Verified | "Block 0" is now a 200 ms window, half of what the videos describe. Distance to the leader matters twice as much. |
| **Jito ShredStream was shut down on Sep 5 2026** | Verified on docs.jito.wtf | The feed F-Project advertises no longer exists. Replacements: DoubleZero Edge multicast, Helius shred delivery, Solana Vibe Station raw shreds, Triton deshred gRPC, Helius pre-confirmations. |
| **Transaction v1** (SIMD-0385) is live; in a sampled block all 13 pump.fun transactions were v1 | Verified | Decoders must handle legacy, v0 and v1. v1 has no address-lookup tables and puts the compute budget in the header (smaller, cheaper transactions). |
| **Alpenglow activates at epoch 1064, Oct 21 2026 ~15:00 UTC** | Verified on solana.com | Confirmation drops to ~150 ms; a slot may contain several banks; Geyser streams gain `bank_id`. Any feed code must be re-tested that week. |

## 2. Where the leaders are

A Solana leader produces 4 consecutive slots (~850 ms) and the schedule is public for the whole epoch.
Share of leader slots by metro, computed from Stakewiz geolocation joined to the epoch 1057 schedule
(438.6M SOL of active stake, 98.7% of it Jito-enabled):

| Metro | Leader slots | | Metro | Leader slots |
|---|---|---|---|---|
| **Frankfurt** | **32-35%** | | Singapore | 4.2% |
| **Amsterdam** | **19-21%** | | Washington/Ashburn | 3.9% |
| London | 8.4-9.2% | | New York / Newark | 2.5% + 1.6% |
| Tokyo | 5.6-6.1% | | Los Angeles | 2.1% |
| Šiauliai (LT) | 4.5% | | Dublin, Stockholm | 2% each |

Europe produces about **74%** of all slots. **Ohio produces 0%**; the nearest hub is Chicago at 1.3%.

Almost every low-latency vendor sits in the same provider, TeraSwitch (AS20326): all Jito block
engines, all Helius Sender and LaserStream regions, Nozomi, NextBlock, bloXroute, Triton. TeraSwitch
also hosts about 28% of leader slots (10.8% in its Frankfurt building alone).

## 3. What Ohio costs you

Round-trip times from published matrices (WonderNetwork, DoubleZero, CloudPing):

| From | to Frankfurt | to Amsterdam | to London | to Tokyo | to NYC |
|---|---|---|---|---|---|
| **Ohio (us-east-2)** | 104-108 ms | 97 ms | 90 ms | 137-146 ms | 16 ms |
| Virginia (us-east-1) | 88-93 ms | 87 ms | 75 ms | 149 ms | 7.5 ms |
| **Frankfurt** | 0 | 7 ms | 17 ms | 230 ms | 86 ms |
| Amsterdam | 7 ms | 0 | 6 ms | 236 ms | 76 ms |

Model: a create executes at a uniformly random point inside a 217 ms slot; we see it via shreds
(leader → us), react, and send (us → leader); we land in the same slot if the round trip plus ~5 ms of
processing fits in the remaining slot time. Weighted over the real leader schedule:

The model below assumes 5 ms of processing. Adding the 20-50 ms batch windows on the two thirds
of leaders that run BAM or Harmonic lowers every P(same slot) by roughly 0.07-0.14 (Frankfurt 0.78 →
0.65-0.72, Ohio 0.51 → 0.38-0.45) but does not change the ranking.

| Box set | Mean RTT to leader | P(same slot) | Slots within 2 ms | Slots within 10 ms |
|---|---|---|---|---|
| **Ohio alone (today)** | 102 ms | **0.51** | 0% | 0% |
| Virginia alone | 89 ms | 0.57 | 4% | 8% |
| **Frankfurt alone** | 44 ms | **0.78** | **34%** | 55% |
| Amsterdam alone | 44 ms | 0.78 | 20% | 64% |
| Frankfurt + Amsterdam | ~38 ms | ~0.80 | **56%** | 64% |
| Frankfurt + Tokyo | 24 ms | 0.87 | 40% | 61% |
| Frankfurt + Amsterdam + Tokyo | | ~0.89 | 62% | |
| Frankfurt + Amsterdam + NYC + Tokyo | 11 ms | 0.93 | 64% | 78% |
| + London + Singapore (6 boxes) | 6 ms | 0.95 | 81% | |

Two numbers matter. **P(same slot)** says how often block 0 is even reachable: Ohio reaches it half
the time, Frankfurt 78%. **"Within 2 ms"** says how often you can win a race against a competitor who
is also fast: a bot in the leader's own building beats one 7 ms away almost every time. Ohio is in
nobody's building. F-Project claims NY, LA, Amsterdam, Frankfurt and Tokyo; LA covers 2% of slots and
they skip London (9%), so a Frankfurt + Amsterdam + NYC + Tokyo + London + Singapore set beats their
claimed footprint.

**Bottom line:** moving from Ohio to Frankfurt raises block-0 eligibility by about half (0.51 → 0.78)
and takes race-winning coverage from 0% to 34%. Adding Amsterdam takes it to 56%.

## 4. Recommended build-out

1. **Now, one box: Frankfurt bare metal.** TeraSwitch FRA (Ryzen 9950X, ~$425/mo list) is the best
   building; Latitude.sh FRA (~$400/mo) or Cherry Servers FRA are the alternatives. Ask for a test IP
   and run `deploy/latency_probe.sh`: accept the box only if Jito Frankfurt (64.130.50.88), Helius
   `fra-sender` (64.130.50.91), the Harmonic Frankfurt TPU (198.13.140.105) and the BAM Frankfurt node
   (198.13.140.227) answer in under 1 ms. Hetzner is excluded (crypto clause in its terms); Equinix Metal closed in
   June 2026.
2. **If AWS is mandatory: eu-central-1**, a current-generation compute instance (c7i / c8g, at least
   large enough for full ENA queues), Elastic IP, no NAT gateway. Expect an extra 0.5-2 ms versus the
   TeraSwitch building. ENA Express and placement groups do nothing for traffic to leaders.
3. **Second box: Amsterdam** (TeraSwitch AMS). Third: **Tokyo** or **New York/Newark**.
4. **Keep Ohio** for the Python discovery/backtest pipeline, dashboards and as a cold-standby signer.
   It is 13-16 ms from NYC, Ashburn and Chicago, so it can also probe US infrastructure.
5. **Multi-box operation:** each box loads the epoch leader schedule and the identity → metro map
   (`deploy/stake_geo.py`); for every 4-slot leader window the closest box is primary. Snipes are signed
   with durable nonces from a shared pool so two boxes can never buy twice.

## 5. Feeds (observe the create as early as possible)

Nobody sees a transaction before the leader executes it. Earliest to latest:

| Feed | What it is | Cost | Notes |
|---|---|---|---|
| Helius `preconfSubscribe` | Leader execution notices (Helius-forwarding and BAM validators) | Pro plan $999/mo + credits (~$250/day at full coverage) | Earliest signal where covered; coverage is partial |
| **DoubleZero Edge** | Leader shreds over multicast, one hop | **$1,500/mo per machine** in FRA/AMS, $900 LON/NY/SGP/TYO, $450 elsewhere (verified on the DZ subscribe page; the old per-epoch on-chain seats were decommissioned Aug 30 2026) | Successor to Jito ShredStream. Covers leaders that publish: **53% of all slots**, 55.6% of Frankfurt-led slots, 56% of Amsterdam, 85% of London, 4% of Šiauliai. GRE multicast, needs a public IP without NAT (bare metal); has devices inside the TeraSwitch FRA/AMS buildings |
| **Helius shred delivery** | Raw UDP shreds from Helius's #2-stake Frankfurt validator (Turbine root or layer-1 for ~every shred) | $1,000/mo per IP ($800 on Pro) | Self-serve, works on EC2; the natural complement for the ~47% of slots whose leaders do not publish to DZ |
| Solana Vibe Station raw shreds | Raw shreds | $250/mo (AMS) | Cheapest way to start A/B testing shreds |
| Triton Yellowstone `SubscribeDeshred` | Pre-execution transactions with ALTs resolved | Usage-based | Simplest API that is still pre-execution |
| **Yellowstone gRPC `processed`** (Helius LaserStream fra/ams $499-999/mo, SVS $150/mo, QuickNode) | Post-execution stream, 10-80 ms after shreds | | Needed anyway for state, fills, blockhash; the realistic free-ish starting point |
| PumpPortal websocket | Third-party relay | Free | ~1 slot late; fallback and cross-check only |

Plan: start on Yellowstone gRPC (paper trading measures exactly how late we are per leader). Once the
strategy is validated, the recommended feed stack is **one DZ Edge Frankfurt seat ($1,500) plus Helius
raw shreds ($1,000) on the same box, ≈ $2,500/mo**, deduplicated per shred with eager Reed-Solomon
recovery (any 32 of a 64-shred FEC set rebuilds it, so the "86% of slots needed repair" statistic on
DZ costs microseconds, not a Turbine hop). Run `geyserbench`-style first-seen tests per leader class
for a week before adding a third source (Triton raw shreds, Everstake at $4,000/mo is poor value).

**Alpenglow (Oct 21)**: the leader window stays 4 slots; blocks gain a header and footer marker and
an "abort" batch (8 zero bytes); ticks collapse to one per slot, so any "position in slot" heuristic
based on ticks breaks; the `UpdateParent` (bank-cleared) marker only arrives with fast leader handover,
whose feature gate is **not activated** on any cluster today (expected with Agave 4.4, Nov 9 at the
earliest). Off-the-shelf shred decoders such as `sol-shred-sdk` silently drop the new markers, so the
shred decoder must be ours. Yellowstone gRPC 14+/15+ exposes `bank_id` and `include_update_parent`.

## 6. Sending (land in the same slot)

Who orders the block depends on the leader's software (DoubleZero validator metadata, verified
today): **BAM (Jito's block assembly marketplace) 37.9%**, **Jito-Agave 27.2%**, **Harmonic 25.1%**,
Firedancer 7.2%, other 2.6%. Two thirds of stake therefore sequence transactions in **batch auctions**
(BAM rounds of 20-50 ms; Harmonic's default is a 50 ms frequent batch auction ordered by priority fee
and tips, with shreds still streaming continuously), and **67.9% of stake advertises a proxy TPU**
(BAM node or Harmonic remote TPU, almost all at TeraSwitch) rather than the validator itself. On
Jito-Agave leaders arrival order dominates; on BAM and Harmonic leaders the fee decides the
position inside the batch. Measured over 15,978 blocks in 24 h (write-conflicting pump-trade pairs,
leaders classified by client): inside a BAM round the earlier transaction had the higher compute-unit
price 86-89% of the time, and the fee-ordered span is ~5% of the block, which fits **20 ms rounds**
(Jito's changelog), not the 50 ms on the docs page; Harmonic's fee-ordered span is just as narrow and
only ~14% of stake behaves FBA-like; beyond ~5% of block distance every class falls back to arrival
order. For **block-0 snipers reacting to the same create**, the higher bid landed first 60% of the
time on Jito-Agave, 69-76% on Harmonic and 73% on BAM, and snipers that paid through a **Jito bundle
landed later than direct senders on every class** (the bundle path adds the 50 ms auction tick). So
the bid policy must be **per leader class**: BAM → compute-unit price with a trimmed limit, Harmonic
→ priority fee (Jito-account tips buy nothing there), Jito-Agave and Rakurai → arrival first with a
modest fee; and the block-0 variant should go **direct to the leader's TPU or a same-building sender
with the bid in the priority fee**, keeping the Jito bundle variant as the revert-protected backup.
Classification is free: `getClusterNodes` returns the client id (AgaveBam, JitoLabs, HarmonicAgave,
...) matching DoubleZero's metadata for 99.8% of stake, plus the advertised `tpuQuic` address; refresh
every epoch and every few minutes during the Alpenglow cutover, when BAM validators fall back to plain
Jito-Agave mode (client id flips to JitoLabs). The spending priority remains **co-location > fan-out >
compute trimming > fees** across windows; inside the window the fee is the tie-breaker.

| Path | Min tip | Notes |
|---|---|---|
| Direct QUIC to the leader TPU (or BAM node) from the same metro | none | Fastest; unstaked works most of the time, a staked identity or validator peering removes eviction risk |
| Helius Sender `http://fra-sender.helius-rpc.com/fast` | 0.001 SOL, or 5,000 lamports with `swqos_only=true` | Routes to Helius, Jito, Harmonic, Rakurai |
| Nozomi `fra2`/`ams1` (Temporal) | 0.001 SOL | Plain-http batch send over a warm connection |
| Astralane `fr`/`ams` | 0.001 SOL (down to 0.00001 at VIP3) | FIFO routers, QUIC available |
| NextBlock `frankfurt:11100` (QUIC) | subscription $249+/mo | `snipeTransaction` flag |
| Jito block engine `frankfurt.mainnet.block-engine.jito.wtf` | tip floor p50 ~0.00001 SOL, contested p90 ~0.04 SOL | 50 ms auction tick; use `bundleOnly` for the revert-protected, tip-heavy variant |

Transaction recipe (from decoding real block-0 winners): `buy_v3` (17 accounts) + idempotent Token-2022
ATA create + tip transfer, ~55-62k CU with a tight compute-unit limit and a tight loaded-accounts-data
limit (saves ~13k cost units of "phantom" cost, i.e. ~20% more priority per lamport for free), tip
inside the transaction (a reverted buy pays no tip), `max_sol_cost` set from the expected bundle,
pre-created `user_volume_accumulator`, warm connections, fire on sight, and **several variants on one
durable nonce** so only one can land and only one tip is paid. Measured on 3,600 creates over 24 h:
the first independent slot-0 winner bids a median **0.0034 SOL** (p75 0.0165, p90 0.068) and lands a
median 149 transactions (~28 ms) after the create; the fastest repeat snipers land 5-9 transactions
after it; among contested launches the earlier winner had the higher bid 73% of the time. Creates in
the last ~5% of a block (6.8% of creates) are unreachable in slot 0, and slot+1 has the same leader
75% of the time. Fill premium over the post-bundle price: 1st reactive buyer +1.1% median (+3.6% on
CEX-funded bundled launches), 3rd +10.8%, 5th +15.8%, so **rank 1 is the whole edge** and slippage
must be capped near +10%. Use **transaction v1** where the path
accepts it: no compute-budget instructions, the compute-unit limit and loaded-accounts size **must**
be set in the header (unset means zero and the transaction pays its fee for nothing), the priority fee
is a total in lamports, no address-lookup tables. Simulated on mainnet against fresh curves: the
snipe `ATA create + buy_v3 + tip` is **805 bytes** (880 with a nonce advance), consumes **61.5-71k CU**
depending on PDA bumps (set the limit to **72,000**, or 72,150 with a nonce), and loads
**12,089,448 bytes** of accounts (set 12,124,160 = 370 pages; recompute from live program sizes),
which cuts the scheduler's "phantom" cost by ~13,400 units versus the 64 MiB default. Keep every
variant ≤ 1,232 bytes: Astralane and NextBlock QUIC and Nozomi bundles cap there even for v1. In a
160-block scan, v1 landed through Jito (`sendBundle` included), Helius (both tiers), Nozomi,
Astralane, Stellium, 0slot, BlockRazor, NextBlock and bloXroute; **v1 plus a durable nonce** landed
through all of those except 0slot (unproven) and bloXroute. Neither the Agave nor the Firedancer
scheduler penalises nonce transactions (they only de-duplicate per nonce); only Nozomi warns that
builders "may deprioritise" them, so A/B test per path. The next nonce value is
`sha256("DURABLE_NONCE" ‖ previousBlockhash of the slot where the advance landed)`, so every box can
re-arm without an RPC read; a reverted snipe still advances the nonce and cancels the other variants.
Fan-out is bounded by rate limits: Jito and keyless Helius allow 1 request/s per IP per region, so
more than one Jito variant per region needs more IPs or boxes. Observed cost of a landed block-0 buy:
median 0.004 SOL, p75 0.014 SOL, p90 0.05 SOL. "Hardcore mode" is a forecast from the history of past
creation slots for that operator (bids that landed ahead of the failures), not a reaction to a rival's
bid, which cannot be seen in time.

## 7. OS tuning on the hot box

`performance` governor, C-states capped at C1, `isolcpus`/`nohz_full` for the feed, decode and sender
threads, `irqbalance` off with NIC IRQs pinned away from the busy-poll core, `busy_poll`/`busy_read`,
large socket buffers for shred bursts, warm HTTP/2 and QUIC connections with pinned IPs, chrony against
Jito's regional NTP so cross-box latency logs are comparable. Full checklist in `deploy/`.
