# Landing a buy in the creation slot ("block 0") on Solana, Oct 2026

Researcher notes, written 2026-10-14. Tags: **VERIFIED** means a primary source (code or official docs) or our own on-chain measurement backs it. **LIKELY** means it is inferred from data or secondary sources. **UNVERIFIED** means it is a vendor claim or rumour.
The container that ran these checks sits behind an HTTPS proxy, and every curl `time_connect` came back as 127.0.0.1. **None of our latency timings are usable**, so all geography conclusions below rest on stake and leader data, not on RTT.

## 0. Key numbers that drive the design

| Fact | Value | Status / source |
|---|---|---|
| Slot time **now** | **~213 ms** (60 s samples: 277–283 slots) | VERIFIED, `getRecentPerformanceSamples` 2026‑10‑14. Agave 4.2 shipped 200 ms slots behind a feature gate ([Solana Compass](https://solanacompass.com/news/agave-42-ships-90-rent-reduction-33x-larger-transactions-200ms-slots-and-a-50000-sol-bug-bounty)). Mainnet runs `solana-core 4.3.0` |
| Leader window | 4 consecutive slots (~850 ms) | VERIFIED, `getSlotLeaders` over 1,600 slots: 396 runs of 4 |
| Stake by metro | Frankfurt 32.2% (+Rüsselsheim/Limburg ≈34%), Amsterdam 19.0% (+Diemen/Rotterdam ≈21%), London 8.4%, Tokyo 5.6%, Šiauliai LT 4.6%, Singapore 4.3%, US‑East (Ashburn 3.9 + NYC 2.5 + Newark 1.6) ≈8%, LA 2.1%, **Ohio ≈0%** | VERIFIED, stakewiz API, 691 non‑delinquent validators, 438.6M SOL |
| Leader slots in our 1,600‑slot sample | Frankfurt 36%, Amsterdam 19%, London 7.5%, Tokyo 6%, Šiauliai 5.8% | VERIFIED (our measurement) |
| Client mix by stake | Agave/Jito‑Agave 4.x ≈84%, Firedancer v26.x ≈15%, Frankendancer 0.3% | VERIFIED via version strings (Firedancer tags `v26.09.0`, [newreleases](https://newreleases.io/project/github/firedancer-io/firedancer/release/v26.09.0)) |
| Jito‑enabled stake | 98.7% | VERIFIED (stakewiz `is_jito`) |
| BAM (Jito Block Assembly Marketplace) | 386 validators, **37.6% of stake**. BAM nodes by stake: ams 10.2%, fra 5.3%, sqq (Šiauliai) 4.5%, tyo 4.2%, sin 3.7%, iad 3.5% | VERIFIED, `explorer.bam.dev/api/v1/validators` |
| Leader slots by class (sample) | non‑BAM Agave 50%, BAM Agave 33%, Firedancer 16% | VERIFIED (our join) |
| Alpenglow | BLS keys registered on 99.9% of stake; activation expected with 4.3 but not confirmed | LIKELY ([solana.com](https://solana.com/upgrades/alpenglow)) |

**Implication:** Ohio is the wrong place to run this. About 75% of stake, and about 70% of leader slots, sit in Frankfurt, Amsterdam, London and the Baltics. Put the primary box in **Frankfurt** and the secondary in **Amsterdam**. Optional extras are NY/Ashburn (~8%) and Tokyo (~6%). Of F‑Project's 6 sites, the LA site covers only 2.1% of stake, and they leave out London (8.4%) and Šiauliai (4.6%).

## 1. How a leader orders transactions within a block

**Agave (and Jito‑Agave) scheduler.** VERIFIED from [`core/src/transaction_priority.rs`](https://raw.githubusercontent.com/anza-xyz/agave/master/core/src/transaction_priority.rs):
`priority = reward × 1,000,000 / (cost + 1)`
- `reward` is the validator's share of fees: all of the priority fee plus half of the 5,000‑lamport base fee.
- `cost` is the cost model's total: signatures × 720, write locks × 300, instruction‑data bytes / 4, the **requested** `compute_unit_limit`, and a loaded‑accounts‑data‑size charge of **8 CU per 32 KiB of the requested `loaded_accounts_data_size_limit`** ([cost_model.rs](https://raw.githubusercontent.com/anza-xyz/agave/master/cost-model/src/cost_model.rs), [block_cost_limits.rs](https://raw.githubusercontent.com/anza-xyz/agave/master/cost-model/src/block_cost_limits.rs)).
- The default data limit is 64 MiB, which adds **16,384 CU of phantom cost**. Snipers that set it tightly observably use 12–16 MiB (≈3–4k CU), so they save about 12–13k CU of cost. On a ~62k CU buy that is **~20% more priority per lamport, free**. In our scan at least 56% of pump buys never set it, and most of those that did still sent the 64 MiB default (VERIFIED).
- Jito tips are invisible to this formula unless the transaction arrives through the block engine.
- The scheduler drains a priority queue while the slot runs and the bank is built continuously, so a high‑fee transaction that **arrives late is still placed late**.

**Jito block engine.** VERIFIED ([docs.jito.wtf](https://docs.jito.wtf/lowlatencytxnsend/)):
- Auctions run in parallel on **50 ms ticks**. Bundles that touch the same writable accounts compete in a single auction, ranked by **tip ÷ requested CU**.
- Bundles hold at most 5 transactions. The minimum tip is 1,000 lamports, paid to one of 8 tip accounts. Do not put the tip account in an ALT.
- The default rate limit is 1 request/s per IP per region, raised by ticket. Requests authenticate with a UUID passed as `x-jito-auth` or `?uuid=`.
- `sendTransaction` always runs with skip_preflight and forwards as a bundle; `bundleOnly=true` adds revert protection. Jito's guidance for `sendTransaction` is a 70/30 split between priority fee and tip.
- Regions: amsterdam, dublin, frankfurt, london, ny, slc, singapore, tokyo (`<region>.mainnet.block-engine.jito.wtf`), plus the global `mainnet.block-engine.jito.wtf`.
- The live tip floor at 07:56 UTC was p50 6.9e‑6, p75 1e‑5, p95 3.6e‑4, p99 8.7e‑4 SOL (`bundles.jito.wtf/api/v1/bundles/tip_floor`). That covers all bundles, not contested launches.
- **The 50 ms tick is about 25% of a 213 ms slot.** Block‑engine paths are structurally slower for reactive sniping.

**Firedancer `pack`.** VERIFIED from [fd_pack.h](https://raw.githubusercontent.com/firedancer-io/firedancer/main/src/disco/pack/fd_pack.h) and the [forum overview](https://forum.solana.com/t/a-practical-overview-of-scheduler-implementations-in-solana-validators/4803):
- Ordering is by reward per compute, with penalties for hot writable accounts and large allocations.
- Bundles run in **FIFO** order. A one‑transaction bundle is "a transaction with extremely high priority".
- Durable‑nonce conflicts are resolved by priority (`NONCE_PRIORITY`).

**BAM.** VERIFIED in outline ([bam.dev](https://bam.dev/docs), [Helius](https://www.helius.dev/blog/block-assembly-marketplace-bam)):
- A TEE (SEV‑SNP) node sequences the transactions and bundles, then streams them to the validator, which executes FIFO with respect to account locks.
- BAM validators advertise the BAM node as their TPU, so a "direct to leader" send lands on the BAM node. Sit close to the **BAM node region** (ams/fra/sqq), not only the validator.
- The batch interval is not published (UNVERIFIED).

**Harmonic.** LIKELY/UNVERIFIED: an aggregator that auctions blocks from several builders. Reports say it releases shreds only after an auction of about 300 ms ([Blockworks Research](https://blockworks-research.beehiiv.com/p/solanas-block-building-wars)). If that is true, ShredStream‑based block‑0 sniping cannot work against Harmonic leaders.

**Does paying more land you earlier? Measured: only weakly.** We took every pair of pump trades on the same mint in the same block over 1,820 slots and checked whether the earlier transaction had the higher fee per cost unit:

| Leader class | pairs | earlier tx had higher fee/CU | per‑(slot,mint) mean | contended groups (≥6 trades) |
|---|---|---|---|---|
| non‑BAM Agave | 9,702 | 54.0% | 53.3% | 54.5% |
| BAM Agave | 5,938 | 58.3% | 57.2% | 59.9% |
| non‑BAM Firedancer | 10,308 | 87.1% (dominated by a few hot mints) | 56.6% | 59.3% |

Pure FIFO would score 50% and strict priority 100%. **Arrival time dominates.** Fee only re‑orders transactions that sit in the same scheduling batch. LIKELY; the method is limited by the fact that fee/cost is the only priority signal we can observe.

## 2. Block‑0 study: 87 pump.fun creates, slots 456,764,000–456,765,819

Method: we pulled every block via `getBlock` (v1 transactions need `maxSupportedTransactionVersion: 1`) and decoded compute‑budget instructions and v1 `transactionConfig`, recognised tips by provider tip account, and joined leaders to stakewiz and BAM data. Scripts are in the session scratchpad (`scan.py`, `analyze.py`, `order.py`).

- **Create rate:** 87 in about 6.5 minutes, roughly 13 per minute (`create_v2`, plus `CreateCoinAndBuyBondingCurveV3` from launchers).
- **Only 23 of 87 creates (26%) drew any non‑creator buy in the same slot.** We saw 85 block‑0 buys in total:
  - 24 sat at index gap 1–3 right after the create. All succeeded and their median total bid was 0.0003 SOL. We take these to be dev or launcher bundles; one was confirmed as the same Jito bundle as its create through `bundles.jito.wtf/api/v1/bundles/transaction/<sig>`.
  - **61 were reactive buys** (gap ≥4). **33 succeeded and 28 failed.**
  - 27 of the 28 failures were slippage errors (`BuySlippageBelowMinTokensOut` 6042, or `TooMuchSolRequired` 6002). They landed, but behind other buyers. VERIFIED via `getTransaction`.
- **Reactive reaction time.** Converting index gap to time (gap ÷ txs in block × 213 ms), the fastest third‑party buys landed **about 9–30 ms after the create executed**. Typical values were 10–45 ms, with some as late as about 195 ms (LIKELY, since we assume uniform packing).
- **Create position in its block:** p25 24%, median 48%, p75 81%. Creates in the last ~15% of a block almost never drew a block‑0 buy. The leader stays the same for up to 3 more slots, so slot +1 is the fallback (we saw 70 buys at slot+1).
- **Compute (VERIFIED, our data):**

| Instruction (successful buys, n=5,689) | CU consumed p10/p50/p90 |
|---|---|
| legacy `buy` | 74k / **117k** / 133k |
| `buy_exact_quote_in_v2` | 110k / 135k / 190k |
| `buy_exact_sol_in` | 71k / 89k / 107k |
| `buy_exact_quote_in_v3` | 55k / 78k / 152k |
| **`buy_v3`** (17 accounts) | **49.5k / 51.2k** / 283k (multi‑ix outliers) |
| Reactive block‑0 winners (whole tx incl. ATA create, compute budget, tip) | p50 **59k** consumed; requested limit p50 75k |

  The guru's "115k vs 60–70k" lines up with legacy `buy` against `buy_v3` plus ATA. `buy_v3` needs the user's token ATA to exist already ([TRADE_V3.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/instructions/TRADE_V3.md)). That cannot happen before the create, so add `createAssociatedTokenAccountIdempotent`. Create the `user_volume_accumulator` (0.0018 SOL of rent) **once, ahead of time**, so the hot path never pays for it.
- **What reactive block‑0 buyers paid** (priority fee plus tips, as bid):
  - Winners: p25 0.0015, **p50 0.004**, p75 0.0138, p90 0.049 SOL.
  - Losers: p50 0.0012.
  - Largest seen: a 0.15 SOL Helius tip and a 0.256 SOL priority fee.
- **Paths seen on block‑0 buys:** Helius 15, Nozomi 12, Astralane 9, Jito 7, NextBlock 1, plus 46 with no tip to a known provider. Those 46 are direct, staked or unknown senders; unknown recipients included `rapidX…`, `LandX…` and `moon…`. 41% of all buys used v1 transactions and 15 of 85 used ALTs. (Prefix matching for Nozomi and Astralane is LIKELY.)

## 3. Third‑party senders (2026)

| Service | Regions (host pattern) | Min tip | Notes | Status |
|---|---|---|---|---|
| **Helius Sender** | `{slc,ewr,lon,fra,ams,sg,tyo}-sender.helius-rpc.com/fast` (http) + global https | **0.001 SOL** (Sender Max, all paths); **5,000 lamports** with `?swqos_only=true` | Requires both a tip and a priority fee. Routes to "Helius, Jito, Harmonic, Rakurai". No API credits used. 1 tx/s keyless, 50 tx/s with a key per region. `/ping` warm‑up. Claims "≤1.5 slots" | VERIFIED docs ([1](https://www.helius.dev/docs/sending-transactions/sender), [2](https://www.helius.dev/docs/faqs/sender.md)) |
| **Nozomi (Temporal)** | pit, ewr, ash, lax, fra, ams, lon, tyo, sgp: `<x>1.nozomi.temporal.xyz` direct http | 0.001 SOL; below that, dropped silently | Fastest path is **Batch Send over a warm plain‑http connection**. Orders by tip, then arrival. Bids into Jito and Harmonic. Auto‑retries. Per‑region rate limits, so multi‑region sends are free. Says durable nonces "may be deprioritised" by builders. Over‑requesting CU dilutes the fee | VERIFIED ([endpoints](https://use.temporal.xyz/nozomi/endpoints.md), [tipping](https://use.temporal.xyz/nozomi/tipping-and-faq.md)) |
| **Astralane Iris** | fr, fr2, ams, ams2, lim, lit, lon, ny, la, jp, sg; IPv4 + QUIC | 0.001 free tier; 0.0001 VIP1–2; **0.00001 VIP3** (8 SOL of tips over 7 days) | Pure FIFO routers. Calls: `sendIdeal`, `sendPaladin`, `getNonce`. Also sells **Postpacks**: notice from Harmonic leaders *before* execution, earlier than shreds | VERIFIED ([endpoints](https://astralane.gitbook.io/docs/low-latency/endpoints-and-configs.md), [tiers](https://astralane.gitbook.io/docs/low-latency/send-txn-fee-tiers.md), [postpacks](https://astralane.gitbook.io/docs/low-latency/postpacks.md)) |
| **NextBlock** | fra, ams, lon, sg, tokyo, ny, slc, dublin, vilnius; QUIC on :11100 (ALPN `nb-tx/1`) | not stated (docs example uses 0.0001) | $249–1,749 a month for 5–50 TPS. Has a `snipeTransaction` flag on HTTP/gRPC. QUIC is raw bytes and fire‑and‑forget. Also a TX stream at 5 SOL per month | VERIFIED ([quickstart](https://docs.nextblock.io/getting-started/quickstart.md), [QUIC](https://docs.nextblock.io/api/quic-transaction-submission.md), [pricing](https://docs.nextblock.io/pricing-and-rate-limits.md)) |
| **bloXroute Trader API** | uk, ny, germany, amsterdam, tokyo `.solana.dex.blxrbdn.com` + global | 0.001 SOL | **`submit-snipe`** takes 2 transactions: one to Jito, one over staked connections. Make them mutually exclusive with a **shared durable nonce** | VERIFIED ([snipe](https://docs.bloxroute.com/solana/trader-api/api-endpoints/transaction-submisson/submit-snipe.md), [tips](https://docs.bloxroute.com/solana/trader-api/introduction/tip-and-tipping-addresses.md)) |
| **0slot** | Frankfurt, Amsterdam, NY, Tokyo, LA | 0.001 (0.0001 on the 50‑TPS plan) | "Dedicated SWQoS"; benchmarks are their own | UNVERIFIED (0slot.trade homepage only) |
| **Solana Vibe Station Lightspeed** | basic/ultra/elite tiers | 0.001; `skipPreflight` required | Static or per‑holder "stealth" tip addresses. `?only=swqos,jito` picks upstreams. Staked RPC at $300–1,000 a month | VERIFIED ([docs](https://docs.solanavibestation.com/api-reference/lightspeed/lightspeed.md)) |
| Paladin | n/a | n/a | Small stake share; Astralane exposes `sendPaladin` | UNVERIFIED, low priority |

Practical note: most sender minimums are 0.001 SOL, which is 2.5× the median winning bid. Helius `swqos_only` (5,000 lamports) and Astralane VIP3 (10,000 lamports) are the only cheap ways to fan out.

## 4. SWQoS and sending straight to the leader

- 80% of TPU capacity is shared out by stake. A staked validator can lend "virtual stake" to a trusted peer with `--staked-nodes-overrides`, and an RPC can pin `--rpc-send-transaction-tpu-peer` ([solana.com](https://solana.com/developers/guides/advanced/stake-weighted-qos)). VERIFIED.
- Agave 3.1 raised unstaked QUIC connection slots from 500 to 2,000, with a separate pool of 2,000 for staked. Peers are capped at 8 connections per IP, and a connection needs about **1/50,000 of stake (~8.8k SOL today)** to count as staked ([Triton](https://blog.triton.one/evolution-of-solanas-stake-weighted-quality-of-service-from-the-agave-side/)). LIKELY.
- In the Agave master stream throttle, an unstaked peer may send **400 TPS** while load is under 70%, falling to 100 TPS above it. Staked peers are unthrottled until 76% load ([stream_throttle.rs](https://raw.githubusercontent.com/anza-xyz/agave/master/streamer/src/nonblocking/stream_throttle.rs)). VERIFIED for master; deployed 4.3 may differ.
- What that means for us: a sniper sends a few transactions a second, so **throughput is not the problem; arrival time is**. Unstaked direct QUIC to the leader works most of the time. The risks are eviction and handshake churn at the very moment of a hot launch. A staked identity, or peering with a validator in Frankfurt or Amsterdam, removes those risks. It does not raise your priority: SVS states plainly that SWQoS "doesn't put you in the front of the line".
- Mechanics: refresh the schedule with `getLeaderSchedule` and `getClusterNodes.tpuQuic`. Keep **warm QUIC connections** to the current leader and the next ~8 leaders (ALPN `solana-tpu`). For BAM leaders the advertised TPU is the BAM node.

## 5. Building the transaction for speed

1. **Use `buy_v3` / `buy_exact_quote_in_v3`** (17 accounts) and create the ATA idempotently in the same tx.
2. Set the CU limit to measured usage plus ~5%, about 62–65k. Cost in the formula is the **requested** CU, so over‑asking dilutes both the Agave priority and Jito's tip/CU ranking.
3. Set `SetLoadedAccountsDataSizeLimit` to a tight measured value (~12–16 MiB as observed; calibrate). That saves about 13k cost units.
4. Use **v1 transactions** (SIMD‑0296; config lives in the header, so there are no compute‑budget instructions). They are live and 41% of buys already use them.
5. Use an ALT only for the static pump accounts (global, fee_config, buyback recipient, programs). **Never put tip accounts in an ALT**: Jito and SVS do not detect them there.
6. Use **skipPreflight**, and never simulate on the hot path.
7. Blockhash: use the most recent one from your own shred or Geyser feed; any blockhash from the last 150 slots is valid. With 213 ms slots that window is about 32 s, so refresh every slot.
8. Pre‑sign everything except the mint‑derived accounts. An ed25519 signature costs tens of microseconds, so sign after detection.
9. **Fan out with mutually exclusive variants.** Each sender needs its *own* tip instruction, so the variants have different signatures. Put every variant on **one durable nonce**: whichever lands first advances the nonce and kills the rest. You pay only one tip, and Agave and Firedancer drop the lower‑priority nonce duplicates (VERIFIED in `receive_and_buffer.rs` and `fd_pack.h`). Advancing the nonce costs an extra writable account and a small amount of CU. Nozomi warns some builders deprioritise nonce transactions, so A/B test it per path.
10. Fire on sight; do not wait for confirmation. Only 26% of creates attract any reactive block‑0 buyer, and every reactive buy that failed in our data lost on slippage, not inclusion.

## 6. Failure modes and selling

- **Landing behind other buyers.** 46% of reactive block‑0 buys failed this way in our sample. Set `max_sol_cost` (or `min_tokens_out`) from the curve state **after the expected k buyers ahead of you**, and cap it at what the trade can bear. Put the tip *inside* the buy transaction: on revert you pay only the base and priority fee, never the tip ([Jito](https://docs.jito.wtf/lowlatencytxnsend/), [Nozomi](https://use.temporal.xyz/nozomi/tipping-and-faq.md)). That makes **tips safer than priority fees** for the share of the bid that is at risk.
- **Same‑slot or near‑slot dumps.** In our window the creator sold within 3 slots only once (slot +3). Watch the current slot's shreds and fire the sell as soon as a dev sell or bundle exit appears.
- **Selling.** On the bonding curve use `sell_v3` with `min_sol_output`. After completion, **no buys or sells are possible on the curve until `migrate` runs**, which is permissionless; `buy_v3` can be a "synthetic migration" that buys through into the future pool ([SYNTHETIC_MIGRATION.md](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/docs/SYNTHETIC_MIGRATION.md)). Then sell on PumpSwap `pAMMBay6oceH9fJKBRHGP5D4bD4sWpmSwMn52FMfXEA` with `sell_v2`. Use effective quote reserves = vault + `virtual_quote_reserves`, which **can be negative** since Sept 30 ([README](https://raw.githubusercontent.com/pump-fun/pump-public-docs/main/README.md)). Pre‑build both sell paths.
- **Revert protection** (Jito `bundleOnly`, NextBlock `revertOnFail`) costs latency. Use it only on the slower variant.

## 7. Recommended landing stack, fastest first

1. **Our own Frankfurt bare‑metal box** (plus Amsterdam) sending **direct QUIC to the leader TPU or BAM node**, ideally through a staked identity or validator peering. In‑metro cost is about 0.1–1 ms. The cost is stake capital or a partner fee, and there is no per‑tx tip. LIKELY fastest.
2. **Colocated senders over plain http or QUIC in the same metro, sent in parallel:** Nozomi `fra1`/`ams1` Batch Send, Astralane `fr`/`ams` by IPv4 and QUIC, NextBlock `frankfurt:11100`, Helius `fra-sender`/`ams-sender`. Each adds one hop but brings its own staked or builder relationships. 0.001 SOL per landed tx, less at Astralane VIP or with Helius swqos_only.
3. **Jito block engine** in the leader's region (`frankfurt`/`amsterdam`), using `sendTransaction?bundleOnly=true` or a one‑tx bundle. This variant carries the revert‑protected, tip‑heavy bid. It adds up to a 50 ms tick, works only on Jito or BAM leaders, and cannot be used in a create+buy bundle unless we hold the creator's signed transaction.
4. Plain staked RPCs (SVS, Triton). Useful as backups and for selling.

Pick the routing per slot from the leader's class and city. Firedancer leaders: fee per CU matters more, and bundles are FIFO. BAM: sit near the BAM node. Harmonic: shreds can arrive late, so prefer a postpack/preconf feed or skip.

## 8. "Hardcore mode", made concrete

You cannot see a rival's fee before both transactions are in flight, so outbidding is a **forecast built from the history of past creation slots**, followed by in‑slot reaction.

```
offline/continuous (from our own shred+block feed):
  for each create C (watched devs and look-alikes):
     record every buy in slots s..s+3 on C.mint: signer, gap, fee/cost, tip(provider,amount),
            ok/err, leader class+city
  competitor table K[wallet] = {bid distribution, path, gap distribution, hit rate}
  winning-bid model D[leader_class][dev_bucket] = quantiles of the min bid that landed ahead
                                                  of all failed/later reactive buys
pre-slot (every slot):
  warm QUIC to the next 8 leaders' TPU/BAM nodes; refresh blockhash/nonce; keep the template signed except the mint
on create seen (shred entry / postpack) at in-block position f (0..1):
  if f > 0.85 and leader has no next slot left: skip (block-0 unreachable)
  N_hat = expected rival buyers for this dev from K;  bid_needed = Q_q(D[class][dev]) * (1+eps), q=0.9
  bid = min(bid_needed, EV_cap = P(win) * E[profit] * kelly_frac)
  split: Firedancer/Agave TPU path -> mostly priority fee (cup = bid_fee*1e6/CU_LIMIT);
         Jito/BAM/Nozomi path -> mostly tip (tip only paid if the tx succeeds)
  slippage: max_sol_cost = price_after(N_hat rival buys of typical size) * (1+s)
  build K variants on ONE durable nonce, fire all concurrently from the leader metro
in-slot feedback (same slot's shreds):
  if rivals' buys already landed past our slippage bound -> do nothing (our tx fails cheaply)
  if create was late in the block and the leader owns slot+1 -> re-arm a slot+1 variant on the same nonce
after the slot: log landed gap/position vs paid; update D with online quantiles (decay ~1 day);
  if we win by large gaps -> lower q; if we lose on slippage behind higher bidders -> raise q
```

Because order is mostly arrival time (§1), spend the budget in this order: **colocation > fan‑out > CU trimming (free priority) > fees**.

## 9. Cost per snipe (landed), Oct 2026

| Level | Recipe (62k CU limit, LADS trimmed) | Priority fee | Tip | **Total SOL** (+0.000005 base) |
|---|---|---|---|---|
| Floor / no rivals | Helius `swqos_only`, cup 100k µL/CU | 0.0000062 | 0.000005 | **~0.000016** |
| Cheap fan‑out | Astralane VIP3 + Helius swqos, cup 1M | 0.000062 | 0.00001 | **~0.00008** |
| Sender minimum | any 0.001‑tip sender, cup 1M | 0.000062 | 0.001 | **~0.0011** |
| Typical block‑0 winner (observed p50) | cup ~16M + 0.003 tip | 0.001 | 0.003 | **~0.004** |
| Competitive (observed p75) | cup ~60M + 0.01 tip | 0.0037 | 0.01 | **~0.014** |
| Hardcore (observed p90) | cup ~160M + 0.039 tip | 0.0099 | 0.039 | **~0.05** |
| Extremes seen | n/a | up to 0.256 | up to 0.15 | 0.15–0.26 |

Failed attempts cost only base fee plus priority fee; the in‑tx tip reverts. With durable‑nonce fan‑out only one variant can land, so you pay at most one tip.

## 10. Open questions and next measurements

- Rerun this scan over 24 hours or more, on our own RPC, to cover thousands of creates. Measure true arrival‑to‑inclusion times with shred timestamps instead of index estimates.
- Run ping and QUIC handshake tests from AWS eu‑central‑1 against bare metal in Frankfurt (Equinix FR, Latitude, Cherry, Teraswitch). Measure RTT to the TPUs of the top 50 leaders and to the BAM nodes. None of these could be measured from here.
- Confirm Harmonic's shred timing and stake share. Price out Astralane postpacks and Harmonic preconfs as a detection feed that beats ShredStream.
- Check the nonce‑deprioritisation claims for each builder, and the exact tight LADS for a `buy_v3`+ATA transaction (simulate and read loaded bytes).
- Identify the unknown tip recipients (`rapidX…`, `LandX…`), which may be private sender services.
