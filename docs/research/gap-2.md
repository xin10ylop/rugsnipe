# Gap 2: first-slot competition on tracked operators vs random launches

**Question.** Over at least 24 h and 3,000 creates, compare launches by tracked repeat operators (≥3 prior launches, or a fresh wallet funded by a CEX) with random launches. After removing the creator's own bundle (adjacent tx index, same Jito bundle, shared signer, shared funder), how many independent reactive buyers land in slot 0 and in slot+1? Which bids and index gaps win? What share of attempts fails on slippage? How far above the post-bundle price do the 1st, 3rd and 5th reactive buyers fill? What share of creates lands on leaders, or at positions in the block, where a block-0 reaction is physically impossible?

All numbers below come from our own on-chain pull (VERIFIED, primary data) unless marked otherwise. The ± values are 95% normal CIs.

## TL;DR

1. **Independent block-0 competition is lower than the >50% that Pine reports, but it is concentrated on the coins we want.** Across SOL-quoted creates, 22.1% (±1.5) drew at least one independent reactive buyer that succeeded in slot 0, 6.4% drew 3 or more, and 2.1% drew 5 or more. Counting slot 0 and slot+1 together: 34.6%, 13.6% and 5.6%.
2. **"Tracked" is the wrong axis; bundle shape is the right one.** Our tracked definition (≥3 prior launches, or CEX-fresh) catches 75% of SOL-quoted creates, and 62% of those come from coin farms with ≥50 prior launches that snipers mostly ignore. Bundled launches (≥1 SOL of creator bundle, or ≥2 bundle wallets) draw about 2.3× the competition: 50.9% have an independent slot-0 winner, 18.9% have 3 or more, and **74.7% have one by slot+1**. **CEX-fresh + bundled** launches are the most contested group: **62.2%** have a slot-0 winner and **85.4%** have one by slot+1 (n=82).
3. **Entry premium over the post-bundle price, by reactive rank (median, p75):** 1st **+1.1%, +3.9%**; 3rd **+10.8%, +21.2%**; 5th **+15.8%, +35.1%**. On CEX-fresh + bundled launches the 3rd buyer pays a median **+15.9%** and the 5th **+32.6%** (n=17). The skeptic's break-even mapping (5%, 15%, 30% premium gives 51%, 57%, 66% break-even) applies directly. **Being first is worth roughly 6-15 points of break-even hit rate.**
4. **Failures.** Of the wallets that attempted a slot-0 reactive buy, **46% won, 22% failed only on pump slippage** (6042/6002), **29% were on-chain bot-guard aborts** from wrapper programs, and 3% failed for other reasons. On bundled launches with repeat creators, slippage reaches **38%**. In slot+1, slippage is 12.6% and guard aborts 54.8%.
5. **What wins.** For the first slot-0 winner, the median total bid (priority fee + tips) is **0.0034 SOL** (p75 0.0165, p90 0.068). The median index gap is **149 txs (13% of the block, ~28 ms)**. The fastest repeat snipers land 5-9 txs after the create. In 73% of same-slot winner pairs, the earlier winner bid more.
6. **Where reaction is impossible.** **6.8%** of creates sit in the last 5% of their block, where only 1.2% got a reactive buy. The last 15% holds **16.7%** of creates, with a 6.8% reaction rate against 23% elsewhere. **No leader class blocks reaction**: BAM 19.1%, non-BAM Agave 21.0%, Firedancer 22.5%. **Leader geography does matter**: Europe 23.8%, North America 13.8%, APAC 9.0%. We cannot identify Harmonic leaders (no public list), and Harmonic's own docs contradict the claim of a "300 ms auction before shreds".

## 1. Data and method

- **Population.** Every successful signature of the pump.fun mint authority `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM` from 2026-10-13 07:56 to 2026-10-14 07:55 UTC (slots 456,367,374-456,762,917). That is **44,389 creates in 24 h**, from the cached `getSignaturesForAddress` index in `data/cache/create_index.json`. We drew a uniform random sample of **3,600**, seed 20261014. **3,581 decoded** (19 had no CreateEvent). **2,925 are SOL-quoted**, which is the base for every price statistic. 32% of those are mayhem coins.
- **Blocks.** `getBlock(slot0)` and `getBlock(slot0+1)` with `transactionDetails:full`, `encoding:json`, `maxSupportedTransactionVersion:1` and rewards (the leader comes from the Fee reward). We fetched 7,073 blocks, from publicnode and from `api.mainnet-beta.solana.com` for slots publicnode had pruned. Only 1 of the next slots was skipped. Every transaction that references the pump program was kept with its block index, signers, fee, CU, CU price and limit (from compute-budget instructions or the v1 `transactionConfig`), tips (positive balance deltas on 150 tip accounts of 17 senders, taken from `sol-trade-sdk/src/constants/swqos.rs`), and the decoded Create/Trade events from emit_cpi inner instructions. For failed transactions we also kept the error code, the failing program and the error log line.
- **Creator bundle (removed).** It consists of the create tx; members of the same **Jito bundle** (`bundles.jito.wtf/api/v1/bundles/transaction/<create sig>`, then `/bundle/<id>`; 35.2% of creates were in a Jito bundle); the **contiguous run** of txs on this mint at index create+1..+4; and any tx whose signer or trade `user` is the creator, the create signer, or an already-identified bundle wallet.
  - Validation: of 351 adjacency-only rows, only 10 came from wallets that also snipe other launches, so the rule is clean.
  - **Shared funder:** for each winning reactive wallet that appeared in ≤2 launches (415 wallets), we checked its first and main funder against the creator, the creator's funder and the bundle wallets. **20 wallets (21 buys) were linked and excluded.** 54 buys came from fresh (≤24 h, ≤50 sigs) but unlinked wallets. They are possible hidden insiders and make up at most ~3.5% of reactive winning buys.
- **Reactive buyer.** A distinct signer, outside the bundle, that attempted a buy on the mint in slot 0 (index > create) or in slot+1. A wallet **wins** if any of its txs succeeded. A wallet fails on **slippage** if it never won and at least one tx hit pump `6042 BuySlippageBelowMinTokensOut`, `6002 TooMuchSolRequired` or `6003` (including wrapper programs that re-throw 0x179a or 0x1772). A **bot-guard** failure is any other custom error from a non-pump wrapper program, plus duplicate copies (System `0x0`).
- **Labels.**
  - **repeat**: `prior ≥ 3`, where prior is the number of coins by the creator (or the create signer) with `created_timestamp` before this create. Source: `frontend-api-v3.pump.fun/coins?creator=<w>&sort=created_timestamp&order=ASC&limit=50`. Prior counts above 50 are capped and called **farm**.
  - **CEX-fresh**: the first or main funder of the create signer is a labeled CEX (`rugsnipe_analysis.labels`, category `cex`), and the signer's first tx is ≤72 h before the create (`funding.first_funding`, archive RPC). Funders: Binance 136, KuCoin 46, Coinbase 27, Bybit 19, Revolut 17, MEXC 9.
- **Premium.** `prem_avg = (sol_amount/token_amount) / P_postbundle − 1`, where `P_postbundle` is the curve price `vsol/vtok` after the last create or bundle trade. `prem_start` uses the spot price just before the buyer's own trade. Ranks run over distinct winning wallets, ordered by (slot, index), across slot 0 and slot+1.
- **Reproducibility.** Scripts are in `analysis/scripts/gap2_firstslot/` (`sample → fetch → jito → label → aggr → indep → stats/stats2/stats3`), and the raw output is in `stats_final.txt`. They are scratch-quality and path-relative to the session scratchpad.

## 2. How many independent reactive winners (SOL-quoted, funder-linked wallets excluded)

| Group | n | slot0 ≥1 | slot0 ≥3 | slot0 ≥5 | slot+1 ≥1 | slot0+1 ≥1 | ≥3 | ≥5 | wallets attempting slot0 (mean) |
|---|---|---|---|---|---|---|---|---|---|
| All SOL-quoted | 2,925 | **22.1%** | 6.4% | 2.1% | 21.3% | **34.6%** | 13.6% | 5.6% | 1.05 |
| Tracked (repeat or CEX-fresh) | 2,186 | 20.1% | 6.2% | 2.2% | 19.4% | 31.3% | 12.9% | 5.8% | 0.96 |
| ↳ farm (≥50 prior) | 1,346 | 18.9% | 6.6% | 2.4% | 18.9% | 30.2% | 13.7% | 5.9% | 0.93 |
| ↳ repeat 3-49 | 642 | 12.8% | 2.8% | 1.4% | 14.6% | 22.4% | 6.2% | 3.4% | 0.59 |
| ↳ CEX-fresh | 271 | **41.0%** | 10.7% | 3.0% | 29.9% | **55.0%** | 22.5% | 9.6% | 1.82 |
| Tracked excl. farms | 868 | 21.4% | 5.3% | 2.0% | 19.6% | 32.4% | 11.4% | 5.4% | 0.98 |
| Untracked | 739 | 27.9% | 7.0% | 1.6% | 26.9% | 44.5% | 15.7% | 5.1% | 1.30 |
| **Bundled** (all) | 570 | **50.9%** | 18.9% | 6.5% | 49.5% | **74.7%** | 38.8% | 16.8% | 2.95 |
| Bundled & tracked | 402 | 51.5% | 18.2% | 7.0% | 51.2% | 75.4% | 40.5% | 18.4% | 2.99 |
| **Bundled & CEX-fresh** | 82 | **62.2%** | 18.3% | 9.8% | 53.7% | **85.4%** | 43.9% | 20.7% | 3.43 |
| Bundled & repeat 3-49 | 73 | 45.2% | 12.3% | 4.1% | 56.2% | 72.6% | 31.5% | 16.4% | 2.52 |
| Bundled & untracked | 168 | 49.4% | 20.8% | 5.4% | 45.2% | 73.2% | 34.5% | 13.1% | 2.86 |

CI half-widths: ±1.5 points for n=2,925; ±4 for n=570; ±10-11 for n≈80.

Reading the table:
- **Farms dominate the "repeat" label.** 1,122 SOL-quoted creates (38%) came from a creator wallet with ≥50 earlier pump coins. On 81% of them, no reactive buyer won in slot 0. "≥3 prior launches" therefore does not identify the guru's rugger. His ruggers rotate the creator wallet every launch, so they show up as **new wallets that are CEX- or mother-funded and launch with a bundle**.
- **Competition follows visible bundle size, not our tracking.** Rival bots already key on the slot-0 bundle and the funding source. On CEX-fresh + bundled launches, a median 3.4 independent wallets attempt slot 0, about 1.6 win, and 85% are bought by someone by slot+1. Any tracked-rugger list F-Project members share would add to this. These numbers are a **lower bound** for "known rugger" launches.
- **Professional snipers do the reacting.** We found 620 distinct winning wallets. Wallets that won in ≥3 different launches in our 8% sample made **86% of slot-0 winning buys** (1,320/1,540). The top wallet won in 70 launches across 64 creators; 78 wallets won in ≥10. Fresh, unlinked one-off wallets made ≤3.5%.

## 3. Outcomes of attempts (share of attempting wallets)

| Group | slot | attempts | won | failed: slippage | failed: bot-guard / duplicate only | other |
|---|---|---|---|---|---|---|
| All SOL-quoted | 0 | 3,063 | 46.0% | **22.1%** | 28.9% | 3.0% |
| All SOL-quoted | +1 | 4,349 | 30.0% | 12.6% | 54.8% | 2.5% |
| Tracked | 0 | 2,104 | 47.0% | 24.3% | 25.7% | 3.0% |
| Bundled | 0 | 1,681 | 44.4% | 26.9% | 24.9% | 3.9% |
| Bundled & repeat 3-49 | 0 | 184 | 38.0% | **38.0%** | 19.6% | 4.3% |
| Bundled & CEX-fresh | 0 | 281 | 47.0% | 22.4% | 23.8% | 6.8% |

- **Slippage losers are not simply late.** A winner sat at a lower index than a slippage loser in only 61.1% of same-launch pairs, and its priority fee was higher in only 55.1% (n=3,537 pairs). Median index gap: winners 197, slippage losers 213. Most losers set `min_tokens_out` (or `max_sol_cost`) too tight against a price that the bundle or earlier buyers had already moved. This is VERIFIED (codes 6042 ≫ 6002 in slot 0: 732 vs 121 txs).
- **Bot-guard aborts are cheap, speculative sprays.** Examples are `ESQHZej9…` (0x7d2, 344 slot-0 txs), `BevFQ2LK…` (0x3, 280), `5kZmvKba…` (0x6f, 272), `7JwTi9am…` (0x7) and `rNFuTz3V…` (0x9). Their signers never won the same mint, and their median priority fee is 0.000005 SOL. They are LIKELY on-chain conditional snipers that fire at many creates and abort inside their own program; the abort reason is not decodable without their IDLs. Counting them as slippage-like gives a slot-0 upper bound of **51%** of attempts lost.
- **Duplicate copies.** All 360 System `0x0` failures came from wallets that had already won the same mint. They are multi-sender duplicates killed by a guard account. A "failure rate" counted per transaction overstates the losses; count per wallet.

## 4. What wins: bids, index gaps, transaction shape

| Metric (slot 0, independent winners, SOL-quoted) | p10 | p25 | p50 | p75 | p90 |
|---|---|---|---|---|---|
| Total bid of all winners (priority + tips, SOL) | 0.00002 | 0.0002 | **0.0018** | 0.0089 | 0.039 |
| Total bid of the **first** winner per launch | 0.00002 | 0.001 | **0.0034** | 0.0165 | 0.068 |
| Priority fee only, winners | 0.000001 | 0.000045 | 0.00079 | 0.004 | 0.0177 |
| Priority fee only, slippage losers (tips revert on failure, so this is not comparable to total bid) | 0.000004 | 0.000031 | 0.00015 | 0.0028 | 0.0125 |
| Slot+1 winners, total bid | 0.00002 | 0.0001 | 0.00034 | 0.0026 | 0.0086 |
| First winner: index gap from the create (txs) | 44 | 82 | **149** | 271 | 499 |
| First winner: gap from the end of the bundle | 36 | 72 | 138 | 261 | 485 |
| First winner: gap as share of the block | 4.2% | 7.2% | 13.1% | 23.3% | 41.8% |
| ≈ ms at 215 ms/slot (uniform packing assumed, LIKELY) | 9 | 15 | 28 | 50 | 90 |
| First winner CU consumed | 50.5k | 54.6k | 70.4k | 87.3k | 95.9k |
| First winner CU limit | 52k | 58k | 82k | 115k | 170k |
| Winner buy size (SOL) | 0.01 | 0.08 | 0.30 | 1.08 | 1.98 |

- **Ordering does respond to the bid inside a slot.** Among launches with ≥2 slot-0 winners, the earlier winner had the higher total bid in **73.4%** of pairs (n=1,892). The first winner's median bid is 0.006 SOL, against 0.0011 for later winners (n=327 contested launches). Prior work found ordering "arrival-dominated" (54-58%) for all pump trades. For contested block-0 snipes, the bid matters more. LIKELY causal; tips route through senders that re-order.
- **The fastest repeat snipers land 5-9 txs after the create.** That gap is the practical floor for a shred-reactive bot.
- **Senders of slot-0 winners (by tip account):** none/unknown 437, Helius 376, Astralane 139, Jito 126, Nozomi 123, Flashblock 55, Helius+Jito 41. First-winner tx versions: v0 231, legacy 217, v1 199.
- **Implication for our bot.** About 0.003-0.02 SOL of total bid wins most uncontested-to-moderate launches. Against bundled CEX-fresh launches, plan for the p90 (0.04-0.07 SOL). The 0.05 SOL TradeWiz-style flat tip sits between the p75 (0.0165) and the p90 (0.068) of first-winner bids, so it overpays on most launches. Bid adaptively by bundle size.

## 5. Fill premium over the post-bundle price

`prem_avg` is the buyer's average fill price over the post-bundle spot, so it includes the price impact of the buyer's own size. Values are p25 / p50 / p75 / p90.

| Rank of reactive winner | All SOL-quoted | Tracked excl. farms | Bundled | Bundled & CEX-fresh |
|---|---|---|---|---|
| 1st | 0.2 / **1.1** / 3.9 / 7.8% (n=1,013) | 0.3 / 1.5 / 3.7 / 7.8% (n=281) | 0.4 / 1.6 / 4.6 / 8.4% (n=426) | 1.2 / **3.6** / 6.5 / 9.2% (n=70) |
| 3rd | 4.2 / **10.8** / 21.2 / 35.3% (n=399) | 4.9 / 11.7 / 24.5 / 37.8% (n=99) | 3.7 / 10.3 / 22.2 / 37.3% (n=221) | 11.1 / **15.9** / 28.5 / 37.4% (n=36) |
| 5th | 6.2 / **15.8** / 35.1 / 48.0% (n=164) | 9.5 / 18.6 / 39.1 / 58.7% (n=47) | 6.0 / 18.0 / 37.5 / 54.3% (n=95) | 18.6 / **32.6** / 39.7 / 52.0% (n=17) |

- **Order matters.** About half of 3rd and 5th fills happen in slot+1.
- **`prem_start`** (spot just before the buyer's own trade) is 0% for the 1st by construction, a median 9.5% for the 3rd, and 15.3% for the 5th.
- **Mapping to the skeptic's break-even** (TP +100%, natural stop, base costs: 5%→51%, 15%→57%, 30%→66%):
  - Being 1st on a bundled CEX-fresh launch costs about **51-52%**.
  - Being 3rd costs about **57%**.
  - Being 5th costs **~66%** or more.
  - These hit rates are far above the 6.8-12.6% base rates measured elsewhere. **A per-operator edge has to beat the 1st-rank cost, and we only get that rank when we out-bid and out-locate the snipers measured here.**

## 6. Where block-0 reaction is physically impossible

**Create position in the block** (index / txs in block). p10/25/50/75/90 = 8/21/51/77/92%.

| Position | share of creates | slot-0 independent winner | slot-0 attempt |
|---|---|---|---|
| 0-25% | 29.4% | 24.9% | 37.8% |
| 25-50% | 19.9% | 20.8% | 29.2% |
| 50-75% | 23.8% | 25.1% | 35.0% |
| 75-85% | 10.2% | 18.3% | 28.6% |
| 85-95% | 9.9% | 11.2% | 16.0% |
| **95-100%** | **6.8%** | **1.2%** | 2.1% |

- The last ~15% of the block holds **16.7%** of creates, and only 6.8% of them draw a slot-0 reaction (23.2% before that point). The truly infeasible zone is the **last ~5% (~10 ms)**, which holds **6.8% of creates**.
- **Slot+1 has the same leader in 75.0% of cases** (4-slot rotation), so slot+1 is the natural fallback. It is also where farming dumps land.

**Leader classes.** Leaders were joined to `explorer.bam.dev/api/v1/validators` and to the Stakewiz version field (26.x = Firedancer, LIKELY).

| Leader class | creates | slot-0 winner |
|---|---|---|
| non-BAM Agave | 1,639 | 21.0% |
| BAM Agave | 1,332 | 19.1% |
| non-BAM Firedancer | 551 | 22.5% |
| BAM Firedancer | 59 | 20.3% |

- **No class blocks reaction.** BAM's 20 ms rounds do not prevent same-slot snipes.
- **Geography does.** By leader IP region (Stakewiz): Europe 23.8% (n=2,594), North America 13.8% (n=538), APAC 9.0% (n=410). Restricted to positions <85%, Europe reaches 27.2%. This is consistent with competitors concentrated in AMS and FRA.
- **Per-leader heterogeneity is real but small:** χ²=129.7, df=87, p≈0.002.
  - 5 of 88 leaders with ≥10 creates had zero slot-0 winners, against 2.2 expected (75 creates, 2.1%). They were in Stockholm, Amsterdam, Tokyo, Singapore and Washington.
  - The lowest rates among large samples: Upbit (Boardman, AWS) 6.8% of 59; OKX (Singapore) 6.9% of 29.

**Harmonic.**
- **Harmonic leaders cannot be identified with public data.** The Harmonic docs FAQ ([docs.harmonic.gg](https://docs.harmonic.gg/reference/faq.md)) says validators are whitelisted, and no public identity list exists. VERIFIED.
- **Shreds stream continuously.** The docs say "Across every Harmonic strategy, the validator broadcasts shreds continuously throughout each leader slot" ([scheduling-strategies](https://docs.harmonic.gg/concepts/scheduling-strategies.md), [streaming](https://docs.harmonic.gg/concepts/streaming-mode.md)). The default strategy is a **50 ms frequent batch auction** sorted by priority fee and tip, with no tip accounts. This contradicts the "~300 ms auction before shreds" claim in `speed-tx-landing.md`.
- **What FBA implies for us.** On FBA leaders, a reaction cannot land in the same 50 ms batch as the create. It needs about 1-2 batch windows, so roughly the **last 25-50% of a Harmonic slot** is infeasible instead of the last 5%. LIKELY, inferred from the docs.
- **Leaders named "Coinbase"** (Coinbase reports 76% of its fleet on Harmonic, [Coinbase Q2 2026](https://coinbase.com/blog/q2-2026-solana-validator-performance-report)) showed 18.9% (5 leaders, 74 creates), close to the pooled 20.5%. Harmonic stake share: UNVERIFIED.

**Other.** 1.1% of launches had a tx touching the mint *before* the create (blind or pre-positioned attempts).

## 7. Reconciling the conflicting figures

| Source | Their figure | Our equivalent (24 h, n=3,581) |
|---|---|---|
| Pine (Apr 2025), via launchpad/rugger docs (LIKELY) | >50% sniped in the creation block | Any non-creator slot-0 buy, bundle included: **31.5%**. Independent reactive winner: **20.5%** (all quotes), 22.1% (SOL-quoted). Pine's figure is stale or includes bundles. |
| speed-tx-landing (87 creates, 6.5 min) | 26% drew a non-creator same-slot buy; 28/61 reactive failed, 27 on slippage | Consistent: 22-31% depending on the definition. Per wallet, 22% slippage plus 29% guard aborts. |
| skeptic D2 (n=349, biased) | 30% with ≥3 slot-0 buyers | ≥3 slot-0 buyers, creator included: **23.3%**. Excluding the creator: 18.0%. ≥3 **independent** winners: **6.4%**. D2's figure mostly counted bundle wallets. |
| rugger doc → `onchain-firstslot-stats.md` | file missing | Replaced by this file. Bundle share: 35.2% of creates in a Jito bundle; 12.0% have bundle wallets but no independent slot-0 winner. |

## 8. Consequences for the build

1. **Target class = new creator + CEX or mother funding + slot-0 bundle.** "Repeat creator" mostly finds farms. Our tracked-rugger feed will compete with the same professional wallets that are already present on 62-85% of these launches.
2. **The edge has to come from rank 1.** Budget about 0.005-0.02 SOL of bid, a lean transaction (~55-70k CU consumed, the limit set close to it), and EU presence (AMS/FRA). Skip, or switch to slot+1 logic, when the create sits in the last ~10% of the block or the leader is in APAC/NA. Expect a fill about 1-4% above the post-bundle price when we win rank 1, and set slippage at ≥10% to avoid joining the 22-38% slippage-loss group.
3. **Size the slippage tolerance by expected rank.** Tolerance below the observed rank-3 premium (median 10-16%) means we lose most contested launches. Tolerance above it means paying a rank-3/5 price, which costs 57-66% break-even. **Hard cap: do not fill above about +10% over the post-bundle price.**
4. **Backtests must use `prem_avg` for our rank, not the end-of-slot-0 price.** For contested launches, the end-of-slot-0 price already embeds rank-3+ fills.

## 9. Limitations and open items

- Failed-tx tips revert, so failure bids are priority-only. Bot-guard abort reasons need the wrapper IDLs. "Fresh unlinked" wallets (≤3.5% of winning buys) may still be CEX-funded insiders.
- The ms conversion assumes uniform packing (LIKELY); exact timing needs a shred feed with per-region arrival timestamps.
- Harmonic leaders are unlabeled. Labeling them needs a disclosure or an inference from 50 ms fee-sorted batches (not done).
- Single 24 h window at 200 ms slots, before Alpenglow (~Oct 21). Rerun weekly.
