# Infrastructure shopping list (researched 2026-10-15)

Two research passes on 2026-10-15 (feeds; senders and RPC), each price taken from the provider's own
page on that day unless dated otherwise. The orchestrator re-fetched the load-bearing ones: Triton
pay-as-you-go ($125 prepaid, non-refundable, 12 months; $0.08/GB; shreds $0.18 per 15 min), Alchemy
gRPC ($75/TB, no minimum, spend caps do not cover gRPC), Helius Sender (50 req/s per key per region
with a free key, 1 req/s keyless per IP), Solana Vibe Station Basic RPC ($15/day, $25/week, $75/mo,
100 req/s). Everything else is as the researchers found it; the "could not verify" lists are kept.

## 0. The answer

**Principle.** Subscriptions are 2-5% of what live trading costs; per-transaction fees are the rest,
and the sending side costs nothing to join. So the money question is the bid policy, not the vendor.

| Stage | Feed (see launches) | Sending | RPC (cold lane + create feed) | Cash before you know |
|---|---|---|---|---|
| Shadow (now, Ohio) | free `logsSubscribe` | none | public RPC; add a free Helius key if `unresolved` stays high | $0 |
| **Probe-A (1-3 days, real sends at 0.02 SOL)** | **Triton pay-as-you-go**: $125 prepaid (lasts a year), ~$4 used; endpoint resolves into the TeraSwitch FRA building. Runner-up Alchemy pay-as-you-go ~$4 | all free: Helius Sender (free key), Jito `frankfurt` bundleOnly, Nozomi `fra2`, Astralane free tier, BlockRazor; direct TPU once built | Helius free + SVS Basic one week $25 | **about $55-100 all-in** incl. ~0.3-0.7 SOL of fees for 300 attempts at floor-to-median tips. Cap the probe tip near 0.005 SOL or p75 bids push it to $250 |
| **Month 1 (0.25-0.5 SOL)** | Triton pay-as-you-go ~$40/mo of bandwidth for the whole pump program (~$1 creates-only), or the provider that wins the probe's first-seen test. Upgrade path: Helius Business $499 (LaserStream `fra` in the building, then Pro $999 + shreds $800) | same free set; Helius Developer $49 adds a tip-free staked `sendTransaction` lane (5/s); NextBlock $249 only if an A/B shows it wins | Helius Developer $49 + SVS Basic $75 | subscriptions ~$124/mo; fees $32-245/day depending on bids and volume |
| Later (after the gate) | DoubleZero Edge $1,500/mo (no trial, monthly invoice), Helius raw shreds $800-1,000/IP; short-term shred A/B: Triton $17/day, BlockRazor $50/day, SVS $25/day (AMS) | Astralane VIP tiers come from organic tips (4 SOL/7 days -> 0.0001 minimum) | | |

**Market changes since docs/03 was written.** Jito ShredStream closed on Sep 5 2026; on Sep 9 DoubleZero
cut unauthorised retransmitters of its leader-shred feed, so third-party "leader shred" claims dated
before that are stale. Everstake ShredStream is $500/mo (1 region) or $1,100/mo (6 regions incl. FRA);
its $4,000/mo product is "Direct Shreds". `instantnodes.io` now redirects to an unrelated domain: avoid.

**Corrections to docs/08 §13.** Helius Sender: 50 req/s per region with a free API key (keyless 1/s
per IP). Helius Sender Max accepts `sendBundle` (up to 4 tx). NextBlock has a free trial (1 tx per
10 s). BlockRazor is free (3 TPS) with a 0.0001 SOL minimum tip. Jito tip floor on Oct 15: p50 3.1e-6
SOL. Nozomi ranks senders partly on historical success rate and Astralane's free tier penalises a high
failure rate, so a 50% revert rate costs priority on those legs: track landing per sender.

**Data volume (drives bandwidth-billed feeds).** One 20-second sample: ~21 pump transactions per slot,
59% failed, ~4.6M/day; 3.5-4.5 KB per gRPC update; ~16 GB/day for the whole program, ~8 GB/day with
`failed=false`, ~0.3 GB/day creates-only (filter on the mint authority `TSLvdd1…eokM`). Re-measure on
the box.

## 1. Feeds: Yellowstone gRPC with a FRA/AMS presence (ranked for the probe)

Location method: DNS answers with EDNS client-subnet 64.130.50.0/24, owner from ipinfo. "TeraSwitch
FRA" is very probably our building; ping from the box to confirm.

| # | Provider / plan | Price | Short-term | Streams / caps | Frankfurt host (resolved) |
|---|---|---|---|---|---|
| 1 | Triton One Dragon's Mouth, pay-as-you-go | $125 prepaid (12 mo), $0.08/GB | yes (usage) | "flexible"; 1,000 streams on preconf gRPC | `api.rpcpool.com` -> 64.130.50.84 TeraSwitch FRA; own `*.rpcpool.com` from the portal; token only in `x-token` |
| 2 | Alchemy gRPC pay-as-you-go | $75/TB, no minimum, not on Free | yes | no caps stated; spend caps do not cover gRPC | `solana-mainnet.streaming.alchemy.com` -> 64.130.50.164, 198.13.138.x TeraSwitch FRA |
| 3 | ERPC shared Geyser | €198/mo or €0.34/h (page contradicts itself on hourly; confirm) | hourly listed | 1 IP, 10 streams | `grpc-fra1-1.erpc.global` -> 151.241.178.x TeraSwitch FRA |
| 4 | Solana Vibe Station Basic | $15/day, $40/week, $150/mo | daily | 5 connections | AMS and ATL only (FRA-AMS 7.4 ms) |
| 5 | constant-k Blade Runner | $99/mo, 1-hour trial | trial | 75 gRPC + 15 SubscribeDeshred streams | `prism-frankfurt.constant-k.com` -> Latitude.sh FRA |
| 6 | NoLimitNodes Pro / Ultra | $49 (2 streams) / $199 (10) | no | | velia.net Frankfurt |
| 7 | Chainstack Growth + add-on | $49 + $49 (2) / $149 (7) / $449 (25) | no | 20 tx filters | region not stated (Limestone, Frankfurt per ipinfo, unverified) |
| 8 | Corvus Labs Starter | $160/mo | no | | `fra.corvus-labs.io:10101` synlinq.de Frankfurt |
| 9 | Shyft Build (incl. RabbitStream pre-execution) | $199/mo | no | 10 connections | `grpc.fra.shyft.to` -> Cherry FRA |
| 10 | Solana Tracker | €200/mo | no | 25 | TeraSwitch AMS (not FRA) |
| 11 | Subglow Sniper | $99 (buffered: avoid for sniping) | trial by review | 2 | origin unknown (Vercel front) |
| 12 | Helius LaserStream | $499 Business / $999 Pro | no | 10 / 100 connections | `laserstream-mainnet-fra.helius-rpc.com` -> 64.130.32.x TeraSwitch FRA |
| 13 | QuickNode | $499 Scale or $49 + $499 add-on | trial has no gRPC | 10 streams | region not documented |
| 14 | OrbitFlare shared | $500/mo | no | | FRA not named |

Shred-level and pre-execution: Triton shreds $0.18/15 min or from $900/mo (SubscribeDeshred paid beta,
price unpublished); BlockRazor UDP $50/day or $500/mo (FRA); SVS raw shreds $25/day AMS; ERPC UDP
€0.69/h or €398/mo; Everstake $500/$1,100 (1-hour preview); OrbitFlare $1,000/mo FRA (1-hour trial);
constant-k ShredQuasar $299; Shyft RabbitStream in Build; NoLimitNodes HyperShreds +$250; Astralane
ShredPay: no seat price, tier by trailing-24h tips (0.2 SOL tier 1), relay in TeraSwitch FRA and
Cherry; Helius raw shreds $1,000/IP ($800 on Pro); DoubleZero Edge $1,500/machine, no trial.

Ping list for the box: `laserstream-mainnet-fra.helius-rpc.com`, `solana-mainnet.streaming.alchemy.com`,
`api.rpcpool.com`, `grpc-fra1-1.erpc.global`, `grpc.fra.shyft.to`, `fra.corvus-labs.io`,
`prism-frankfurt.constant-k.com`, `streams.mainnet.solana.nolimitnodes.com`,
`yellowstone-solana-mainnet.core.chainstack.com`, `basic.grpc.solanavibestation.com` (AMS reference).

## 2. Senders with Frankfurt endpoints

For every service the tip is an instruction inside our transaction, so a reverted buy pays no tip
(verified on 105,849 failed pump buys, docs/03 §6.1); base and priority fee are paid on a landed
failure. With one durable nonce across variants at most one tip is paid per attempt.

| Service | Frankfurt endpoint | Min tip | Cost to join | Rate limit | v1 / nonce |
|---|---|---|---|---|---|
| Helius Sender Max | `http://fra-sender.helius-rpc.com/fast?api-key=KEY` (warm `/ping`) | 0.001 SOL + priority >= 5,000 lamports | free, 0 credits | 50/s per key per region; keyless 1/s per IP | landed / landed |
| Helius Sender swqos_only | same `&swqos_only=true` | 5,000 lamports | free | same | landed / landed |
| Jito block engine | `frankfurt.mainnet.block-engine.jito.wtf` (`amsterdam.` too) `?bundleOnly=true` | 1,000 lamports; floor p50 3.1e-6 SOL | free, no key | 1/s per IP per region (ticket to raise) | yes / yes; failed bundle costs nothing |
| Nozomi | `http://fra2.nozomi.temporal.xyz` (direct) | 0.001 SOL (below: silently dropped) | free key | 5/s per key per region | explicit v1; nonce "may deprioritize" |
| Astralane Iris | `http://fr.gateway.astralane.io/iris` (185.191.117.97), QUIC :7000 | free 0.001; VIP1-2 0.0001; VIP3 0.00001 | free; VIP by 7-day tips 4/5/8 SOL | 5 TPS free, 0 bundles; VIP 20/40/80 | landed / nonce-native |
| NextBlock | `frankfurt.nextblock.io`, QUIC :11100 | ~0.001 (SDK) | trial 1 tx/10 s; $249 / $749 / $1,749 | per plan | landed (n=2) |
| bloXroute | `germany.solana.dex.blxrbdn.com` | 0.001 | modular pricing, unpublished | ~1 tx/s intro | nonce not observed |
| 0slot | `de2.0slot.trade` | 0.001 (0.0001 at 50 TPS) | free first week | 5/20/50 TPS | nonce not observed: keep off the shared nonce |
| Stellium | `fra1.flashrpc.com` (Telegram for access) | 0.001 | unpublished | unpublished | v1 supported; nonce landed |
| BlockRazor | `http://frankfurt.solana.blockrazor.xyz:443/sendTransaction` | 0.0001 | free | 3 TPS | landed / landed (no nonce in sandwichMitigation mode) |
| Glaive (new) | `http://fra.glaive.trade`, QUIC 4000 | 0.0001 | free key | 10 TPS | v1 explicit; unproven |
| Lunar Lander (new) | `http://fra.lunar-lander.hellomoon.io/send` | 0.001 | tip only | rate-limits repeated failures | unknown |
| Speedlanding | | | **do not use**: its API key is a Solana keypair secret | | |

**Direct TPU from an unstaked box.** Agave 3.1+ keeps an unstaked pool of 2,000 connections (8 per
IP); "staked" needs about 1/50,000 of stake (~8.5-8.8k SOL ≈ $0.9M). Agave PR #10666 would give
unstaked senders 0 TPS when a leader is saturated (approved June, stale Sep 21, merge unverified).
BAM/Harmonic proxy-TPU admission for unstaked senders is undocumented. Keep direct QUIC as a $0 leg,
never the only leg, and do not buy stake. Cheapest staked lane: Helius Developer $49 (5 tx/s). ERPC
sells a shared SWQoS endpoint (€42/mo plus holding 4.2 elSOL per TPS) and a +€100 dedicated add-on
with a 1,000 SOL allocation (below the threshold; unverified).

## 3. RPC for the cold lane and the websocket create feed

Load: ~49k create notifications/day (~200 MB at an assumed 4 KB each); cold lane 3 req/s = 259k
calls/day, 10 req/s = 864k/day.

| Plan | Price | Fit |
|---|---|---|
| Helius Free | $0 | 1M credits/mo, 10 req/s: ~3.8 days of the cold lane at 3 req/s per month; fine for the websocket feed and the Sender key |
| Helius Developer | $49 | 10M credits, 50 req/s, staked sends 5/s: a full month at 3 req/s |
| Helius Business | $499 | 100M credits, 200 req/s, LaserStream |
| Chainstack Developer / Growth | $0 / $49 | free capped at 5 req/s on Solana; Growth 20M RU (~27M needed at 10 req/s) |
| QuickNode trial / Build | $0 (1 mo) / $49 | 30 credits per Solana call: the trial lasts ~1.3 days at 3 req/s |
| dRPC free | $0 | 210M CU per 30 days, 20 CU/call: a full month at 3 req/s on public nodes |
| Triton pay-as-you-go | $125 prepaid | $10 per M calls: ~$78/mo at 3 req/s |
| **Solana Vibe Station Basic** | **$15/day, $25/week, $75/mo** | 100 req/s unmetered, websockets included; AMS/ATL |
| ERPC Developer (FRA) | €42/mo | 50 req/s; "tokens" undefined |

## 4. Cost model (SOL = $101.93 on Oct 15; 50% of attempts fail and pay the modelled 0.00073 SOL)

Probe at 0.02 SOL per attempt, 300 attempts: fees 0.27 SOL ($28) at floor tips, 0.71 SOL ($72) at the
median 0.004 SOL bid, $225 at p75; plus $25 (SVS week) and the Triton deposit ($125 cash, ~$4 used).
Month 1 at 300 attempts/day: ~$76/day at median bids ($2,300/mo), $32 at floor tips, $229 at p75;
at 1,000 attempts/day: ~$245/day median. Subscriptions (~$124/mo) are 2-5% of that.

## 5. Signup steps

1. Helius: dashboard.helius.dev, free key; Sender URL as above; month 1 upgrade to Developer.
2. Jito: nothing to sign; ticket only for a rate increase.
3. Nozomi: dashboard.nozomi.temporal.xyz, one key, Batch Send, never retry client-side.
4. Astralane: portal.astralane.io, Discord ticket for the key.
5. BlockRazor: account + auth token, mode `fast`.
6. Triton: onboarding link on triton.one/pricing, $125 in stablecoins, endpoints in the portal; if gRPC
   is not shown open a ticket (ask for SubscribeDeshred beta and FRA shreds at the same time); check
   `getent hosts <you>.rpcpool.com` lands on 64.130.50.x and ping it.
7. Alchemy: app + pay-as-you-go, `solana-mainnet.streaming.alchemy.com` with `X-Token`; tight filter.
8. SVS: solanavibestation.com, Basic weekly for the probe, monthly for month 1.
9. Optional A/B: 0slot free week, Glaive key, NextBlock trial, Stellium via Telegram, ERPC hourly.

Repo wiring needed before the probe (task #12): gRPC feed and real senders in the hot binary; key
slots for Helius, BlockRazor and Glaive in the config (`NOZOMI_KEY` and `ASTRALANE_KEY` exist).

## 6. Not verified

Real latency of any provider to the leader (only a first-seen test from the box settles it); whether a
"TeraSwitch FRA" IP is our building; Triton gRPC enablement on pay-as-you-go and its FRA shred price;
ERPC hourly billing; Alchemy compressed vs uncompressed billing; Chainstack/QuickNode/NoLimitNodes/
Subglow node placement; Helius refund rules; NextBlock minimum tip; 0slot docs (unreachable); bloXroute
tier prices; Agave PR #10666 merge status; BAM/Harmonic unstaked admission; failure-penalty thresholds at
Nozomi and Astralane; the 4 KB notification size; and every bid input, which was measured before
Alpenglow (Oct 21).
