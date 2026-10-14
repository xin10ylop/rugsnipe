# Speed infrastructure and server placement (October 2026)

Short version: **your AWS box in Ohio (us-east-2) is the wrong place for the hot path.** Put the
execution box in **Frankfurt**, ideally bare metal in the TeraSwitch facility (or Latitude / Cherry in
Frankfurt), with **Tokyo** as the second box and Amsterdam (AMS3) as the third (§4). Keep Ohio for analytics, backtests and discovery,
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

1. **Now, one box: Frankfurt bare metal in the TeraSwitch FRA2 building.** TeraSwitch's own looking
   glass measures 0.08-0.12 ms from the FRA2 router to Jito FRA, Helius `fra-sender`, the Harmonic
   Frankfurt remote TPU, the BAM Frankfurt node, Nozomi, NextBlock, Helius LaserStream FRA and the
   DoubleZero `fra2-tsw` device: everything is in one building. List price ~$425/mo for a Ryzen 9950X
   (hourly billing, no minimum), but stock must be requested from sales. **Cherry Servers Frankfurt
   is on the same campus (0.09 ms) and has stock today**: EPYC 9375F 32-core at €989/mo or €1.69/h
   (verified via their public API; their Ryzen 9950X is out of stock), so it is the immediate
   fallback. Latitude.sh FRA is a different building (0.7 ms away, ~$400/mo). Ask for a test IP and
   run `deploy/latency_probe.sh`: accept the box only if Jito Frankfurt (64.130.50.88), Helius
   `fra-sender` (64.130.50.91), the Harmonic Frankfurt TPU (198.13.140.105), the BAM Frankfurt node
   (198.13.140.227) and the DZ device (198.13.140.16) answer in under 0.2 ms. Hetzner is excluded
   (crypto clause in its terms); Equinix Metal closed in June 2026; ERPC's Frankfurt metal has been
   sold out since March.
2. **If AWS is mandatory: eu-central-1**, a current-generation compute instance (c7i / c8g, at least
   large enough for full ENA queues), Elastic IP, no NAT gateway. Expect an extra 0.5-2 ms versus the
   TeraSwitch building. ENA Express and placement groups do nothing for traffic to leaders.
3. **Second box: Tokyo (TeraSwitch TYO2), not Amsterdam.** With the corrected model (direct sends,
   BAM 20 ms rounds, leaders keyed on their advertised ingress, creates weighted by where they
   actually land), Frankfurt alone reaches 0.74 of creates in slot 0; adding Tokyo lifts that to
   0.82 (+7.8 points, on creates that Europe-only snipers cannot reach at all), while adding
   Amsterdam adds only +1.3 points of eligibility but turns ~8% of creates into in-building races
   won on fee. Third box: Amsterdam **AMS3** (the 64.130.42.0/24 site, 0.07-0.09 ms from the largest
   single ingress in the network, the BAM Amsterdam node with 10% of slots; never AMS1, which is
   0.7-0.9 ms away) or Newark/Ashburn, depending on where paper trading shows the profitable contested
   launches. Frankfurt + Tokyo + Newark scores 0.87; F-Project's claimed footprint minus LA scores
   0.88.
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
| PumpPortal websocket | Third-party relay | Free | ~1 slot late; fallback and cross-check only. **Carries no v1 transactions** (0 of 519 v1 creates in a 40-min collector run, while `logsSubscribe` saw all 519; v1 is ~45% of creates) and on non-SOL quotes its `mint` field is the quote mint. Never a sole create feed. |

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
modest fee; and the block-0 variant should go **direct to the leader's TPU or a same-building sender**, keeping
the Jito bundle variant as the revert-protected backup. Where the bid sits (tip or priority fee)
now follows §6.1: the priority fee is paid on failure, so it carries the bid only where the leader
orders by it, and never above `bid.failed_attempt_lamports_max`.
Classification is free: `getClusterNodes` returns the client id (AgaveBam, JitoLabs, HarmonicAgave,
...) matching DoubleZero's metadata for 99.8% of stake, plus the advertised `tpuQuic` address; refresh
every epoch and every few minutes during the Alpenglow cutover, when BAM validators fall back to plain
Jito-Agave mode (client id flips to JitoLabs). The spending priority remains **co-location > fan-out >
compute trimming > fees** across windows; inside the window the fee is the tie-breaker.

| Path | Min tip | Notes |
|---|---|---|
| Direct QUIC to the leader's advertised TPU (validator, BAM node or Harmonic remote TPU) from the same building | none | Fastest. Admission at those proxies is undocumented, but on-chain the untipped path wins first place in block 0 as often on BAM/Harmonic leaders (30-49%) as on plain Jito-Agave ones (26-32%), hot launches included. Agave-lineage servers never refuse a new unstaked connection (they evict the least-recently-used one), so keep 1-2 warm connections per proxy and re-handshake on eviction; never reconnect more than 8 times/minute per IP. **Do not buy stake**: a staked identity needs ≥8,762 SOL (~$0.9M) and buys admission, not priority |
| Helius Sender `http://fra-sender.helius-rpc.com/fast` | 0.001 SOL, or 5,000 lamports with `swqos_only=true` | Routes to Helius, Jito, Harmonic, Rakurai. The 5,000-lamport `swqos_only` leg is the cheapest staked fan-out and rides on the same durable nonce as the direct leg |
| Astralane `fr`/`ams` VIP3 | 10,000 lamports after 8 SOL of cumulative tips (prepayable) | Second cheap fan-out leg; same priority for every tier |
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
snipe `ATA create + buy_v3 + tip` is **805 bytes** (880 with a nonce advance) and loads
**12,089,448-12,095,893 bytes** of accounts (set 12,124,160 = 370 pages; recompute from live program
sizes). Its compute cost is **deterministic per mint**: `62,310 + 1,500·S` with a nonce (62,160
without), where S is the sum of `255 − bump` over four PDAs the decoder derives anyway (bonding
curve, the curve's base ATA, the curve's WSOL ATA, our base ATA). S is roughly negative-binomial
with mean 4, so the median mint costs ~68k but the tail reaches 83k (S = 14 in 361 mainnet
simulations; research/gap-5, re-derived here on every record). **A static 72,150 limit fails 16%
of mints**; 88,000 fails 0.075%. Set the limit **per mint at `62,500 + 1,500·S` plus a 1,500 margin**
(the simulation residual is up to +38 CU above the envelope; our `buy_exact_quote_in_v3` template
costs 1,185 more than gap-5's `buy_v3`, so the hot path patches `65,000 + 1,500·S`, which `ctl sim`
re-measured exactly on live mints), with 88,000 as the static floor if
per-mint patching is unavailable; in v1 the fee is a fixed total, so the tighter per-mint limit buys
~25% better priority rank per lamport at the median S. Mayhem coins cost ~400 CU less; a first trade
without a user volume accumulator costs ~2.8k more (pre-create it, and grind the wallet key so its
UVA and WSOL-ATA bumps are 255). `partial_fill` never rescues a slippage failure (6002/6042), so omit
it on SOL non-mayhem snipes; a failing slippage buy still burns ~49k CU,
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

### 6.1 Pay on success: what a lost race costs (added 2026-10-14)

When we lose the race, the slippage-capped buy (`max_sol_cost` at post-bundle +2%) still lands and
fails, or is dropped. What it costs depends only on where the bid sits, and that decides the
business: the P&L model (`docs/research/strategy-pnl-and-screens.md` §1.4) moves the break-even
rank-1 landing rate of the 0.5 SOL LEAD from **12.1% at 0.003 SOL per failed attempt to 2.2% at
0.0005 SOL** (+9.0 → +18.2 SOL/day at a 20% landing rate).

| Fact | Source | Check on our data |
|---|---|---|
| The base fee (5,000 lamports per signature) and the priority fee are charged on a failed transaction | solana.com/docs/core/transactions: "Fees are still charged on failure" | gap-2 blocks (7,074 blocks, `analysis/scripts/gap2_firstslot/fee_on_failure.py`): on **105,849 of 105,849** failed pump buys the fee payer lost exactly `meta.fee`; legacy `meta.fee = 5,000·sigs + ceil(price·limit/1e6)` on 40,548 of 40,548 (the priority fee is charged on the requested CU limit, not on the units used) |
| A transaction is atomic: a failed instruction reverts every instruction, so a tip transfer in the same transaction moves only when the buy succeeds | solana.com/docs/core/transactions: "If any instruction fails, the entire transaction fails and all state changes are reverted" | **0** tip transfers moved in those 105,849 failures, although **68,598** of them carried a tip; gap-5 counts a failed transaction as "untipped" for this reason |
| A failed Jito bundle (or a `bundleOnly` send) is not committed at all: it costs nothing | docs.jito.wtf: "If any transaction in a bundle fails, none of the transactions in the bundle will be committed"; tips belong inside the main transaction, a stand-alone tip transaction risks being unbundled | |
| What losers actually paid | | slot-0 slippage failures (n = 10,238): priority fee p10 1,020, **p50 15,000**, p75 115,000, p90 1,200,000 lamports per transaction |

**Policy** (`[bid]`, docs/06 §6; `rugsnipe-core::bid`):

- **Tip after the buy, in the same transaction.** Every buy and sell template ends with the tip
  transfer (`rugsnipe-tx/tests/templates.rs`, `tip_is_the_last_instruction_after_the_buy_or_sell`).
  Atomicity, not the position, is what makes the tip revert; "last" is the invariant that keeps the
  buy as the instruction that can fail first and matches the decoded winners.
- **Tip-ordered class carries the bid in the tip** (Jito-Agave, through the block engine and the
  Jito tip accounts): `split = [0.0, 1.0]`, header priority fee at the scheduling floor
  `min_priority_lamports = 10,000` (slot-0 winners: p10 1,000, p25 45,000 lamports). A failure
  there costs 15,000 lamports (one signature) or nothing if the bundle drops.
- **Priority-ordered classes** (BAM, Harmonic, Firedancer, other) bid with the priority fee, which
  is at risk: `split = [1.0, 0.0]`, clamped so that base fee + priority never exceeds
  **`failed_attempt_lamports_max` (default 1,000,000 lamports = 0.001 SOL)**. The cap is validated
  (at least 2 × 5,000 + the floor) and the hot path enforces it on every variant
  (`templates.rs`, `bid_plans_keep_a_failed_attempt_within_the_cap`). **BAM is in this group by
  default** (added 2026-10-14, final review): BAM's own rule for tips versus priority fee is not
  documented, and our measurement (§6) saw the earlier transaction inside a BAM round carry the
  higher compute-unit price 86-89% of the time, i.e. CU-price ordering like Harmonic. It stays
  priority-ordered until the probe measures which variant lands per leader class.
- **Leader unknown** (today: the hot path has no leader schedule yet): the primary variant is the
  pay-on-success one and the secondary the clamped priority one, on one durable nonce, so at most
  one lands. Paper charges the expected cost over `paper.leader_mix` (the §6 stake shares):
  0.272 × 15,000 + 0.728 × 1,000,000 ≈ **732,000 lamports (0.00073 SOL) per failed attempt** at
  the default bid, against docs/04's 0.003. **Modelled range: 0.00036 SOL** (if BAM honoured a
  pay-on-success tip: 0.651 × 15,000 + 0.349 × 1,000,000) **to 0.001 SOL** (every leader
  priority-ordered, the cap). In the P&L model (docs/04 §4) the break-even landing rate of the
  0.5 SOL LEAD is **1.6% / 3.3% / 4.4%** over 0.00036 / 0.00073 / 0.001 SOL (+18.7 / +17.4 / +16.4
  SOL/day at 20%), and of the 0.25 SOL LEAD 4.1% / 8.1% / 10.7%.
- **Sells use the same split** (`BidPolicy::plan_sell`): the exit engine's sell bid goes into the
  tip with the priority fee at the floor on a tip-ordered leader, and into the priority fee with no
  tip elsewhere (an unknown leader is planned as priority-ordered). It is paid once, never as both
  fee and tip, and it is not clamped by the buy cap: a sell that does not land leaves the position
  open, and `risk.max_sell_spend_lamports` bounds the spend (`rugsnipe/tests/pipeline.rs`,
  `bid.rs` `sells_pay_the_bid_once_through_the_class_split`).
- **Recorded:** paper writes the modelled cost to `fills.fail_cost` and the summary's
  `paper.failed_cost_lamports`; real mode must write `meta.fee` of our own failed transaction
  (`rugsnipe::e1::failed_tx_fee`), which the real fill lane (task #12) has to call.

**The catch to measure, not assume.** On Jito-Agave leaders a direct-TPU send is ordered by arrival
and priority fee; a tip there buys position only through the block engine, and bundle senders
landed *later* than direct senders on every class (the 50 ms auction tick, above). BAM's own
ordering rule for tips versus priority fee is not publicly documented; our measurement saw CU price
order inside BAM rounds (86-89%), which is why BAM bids with the priority fee by default.
Pay-on-success can therefore cost landing rate, and a priority bid costs more per failure. Probe-A
(docs/04 §7) logs the landing rate per variant and leader class; if the floor-priority variant loses
rank 1 where a priority bid would have won, raise `min_priority_lamports` for that class rather than
drop the cap, and if BAM turns out to honour the tip, set `bid.split.bam = [0.0, 1.0]` again.

## 7. OS tuning on the hot box

`performance` governor, C-states capped at C1, `isolcpus`/`nohz_full` for the feed, decode and sender
threads, `irqbalance` off with NIC IRQs pinned away from the busy-poll core, `busy_poll`/`busy_read`,
large socket buffers for shred bursts, warm HTTP/2 and QUIC connections with pinned IPs, chrony against
Jito's regional NTP so cross-box latency logs are comparable. Full checklist in `deploy/`.
