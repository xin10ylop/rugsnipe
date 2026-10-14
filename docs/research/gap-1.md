# Gap 1: do pump.fun operators with an edge exist?

**Question.** Take at least 14 days of pump.fun history. Cluster creators into operators using five keys: shared funder within 3 hops, CEX withdrawal-amount band, bundle shape, launcher program and metadata host. Then answer:

* How many operators launch at least 5 tokens?
* For each operator, what share of its launches reach +100%, +150% and +200% from a realistic entry? The entry is placed after the creator's bundle and the median third-party buyer in slot 0.
* Does this hold walk-forward? Discover operators on days 1..t, then score them on days t+1..t+7. Give Wilson confidence intervals and the number of qualifying launches per day.

*Measured 2026-10-14, 08:58 to 11:05 UTC. Code is in `analysis/scripts/gap1/` (`s1`..`s7`, `hits.py`). The machine-readable result is `data/reports/gap1_operators.json`. The full console output is `data/cache/gap1/s7_final.txt`.*

## TL;DR

* **No operator found clears break-even. VERIFIED, on our sample and model.**
  * The skeptic's break-even is 43-48% hit rate at TP +100%.
  * We scored 129 operator units with at least 15 launches each. None has a +100% hit rate of 43% or more, even as a point estimate.
  * The best *lower* Wilson bound is 17.3%. The best point estimate, 35.3% (6/17), is in-sample and is about what the maximum of 129 noisy estimates would give anyway.
* **Walk-forward, selection helps but tops out around 15-25%.**
  * Discovering every operator with no hit-rate filter gives 6-10% at +100%. That is the base rate.
  * The guru's rule ("at least 3 of the last 10 launches hit +100%") gives **18.6-21.8% out of sample**. Wilson upper bounds are 32.6-35.3%. It qualifies only about 50-90 launches/day among the wallets we expanded.
  * Ranking wallets by their past hit rate does persist: the top third scored **15-16%** next week and the bottom third about 1%. That is a 2.2× lift over base, still a factor of about 3 short of break-even.
* **Operators are plentiful, but most are spam farms.**
  * About 49.4k creates/day (VERIFIED, full 16-day index).
  * An estimated **8,900 creator wallets launched at least 5 tokens** in 16 days, **1,390 launched at least 100**, and **52 launched at least 1,000**.
  * Wallets with at least 5 launches made **68%** of all launches.
  * Funder-linked groups of wallets (rotating operators) and CEX amount-band groups hit +100% at **0-5%**. They are dominated by dust-funded coin farms.
* **Bundled launches are the best population class, at about 2.4× the base rate, and still far short.**
  * Launches with at least 1 wallet bundled next to the create hit +100% **17.0% [14.9, 19.3]** of the time (n=1,165).
  * Fresh, bundled, exchange-funded launches (the guru's archetype) hit **18.1% [13.1, 24.6]** (n=171).
* **Conclusion: on this data the edge does not exist at TP +100% with a natural-floor stop.**
  * Even our optimistic hit definition, which sells the whole fill at the post-entry peak, falls short of the 43-48% break-even by 2-3×.
  * Speed cannot close this gap. Selection must improve 2-3× more, or the exit model must change.

## 1. Data and method

| step | what | source | n |
|---|---|---|---|
| s1 index | every signature of the pump mint authority `TSLvdd1…eokM`, 2026-09-28 08:59 → 10-14 08:59 UTC (16 d) | `getSignaturesForAddress`, api.mainnet-beta.solana.com, 849 pages | 848,242 sigs; 781,123 successful and ≥6 h old |
| s2 decode | uniform seeded random sample of those sigs → `getTransaction` → CreateEvent, signer, dev buy, uri host, launcher programs, CU, tips | mainnet-beta, api.mainnet.solana.com, public.rpc.solanavibestation.com, leorpc | 9,947 sigs (f = **1.273%**); **9,905 creates (99.6%)** |
| s3 outcome | coin snapshot (`POST /coins/mints`); slot-0..+2 s trades with `slotIndexId` (slot + in-block tx index); post-entry max | frontend-api-v3, swap-api `/v2/coins/{mint}/trades` | 9,905 |
| s3b fill | slot-0 structure for launches skipped by s3 (low ATH) | swap-api | full-structure prefix: 7,462 ranks (75% of sample) |
| s4 funding | signer's pre-create history (1 page); main incoming SOL among its 3 oldest txs; follow ≤3 hops; stop at labelled CEX, bridge or swap (`labels.py`), pump curve, or a wallet with ≥1000 earlier sigs ("busy") | mainnet-beta (only full-history `getSignaturesForAddress`; SVS/leorpc truncate: 56-90 vs 277-1000 sigs, tested) | 9,905 |
| s6 counts | exact 16-day launch counts for 400 random sampled singleton wallets | `/coins?creator=` (limit 50; `limit>50` silently returns 70) | 400 |
| s6 expand | 200 random wallets with 2-9 sampled launches; listing + 40 random coins each, scored like s3 | same | 7,270 coins, 196 wallets with ≥5 launches |

**Entry model** (`s3_outcomes.py`, `hits.py`). Launches are ordered by (slot, tx index).

* **Bundle**: the create tx plus following slot-0 buys whose tx index is ≤ create+5 with gaps ≤2, plus any trade by the signer, creator or fee payer.
* **Third-party buys**: the other slot-0 buy txs.
* **Entry spot**: after the bundle and after the *median* third-party buy (the ⌈k/2⌉-th). With no slot-0 activity, entry is right after the dev buy.
* **Fill**: a 0.5 SOL buy on the constant-product curve, k = 30 × 1.073e9, with 1.25% fee each side (0.95% protocol + 0.30% creator).
* **Hit at +X**: selling the whole fill at the **maximum spot after the entry position** nets ≥ (1+X) × 0.5 SOL. That requires about 2.1× / 2.65× / 3.2× spot for +100 / +150 / +200%.
* **The definition is deliberately optimistic.** There is no time limit, and the exit is at the exact peak.
* **Post-entry max**:
  * from the trades in the first 2 s,
  * plus the trades just before the API's `ath_market_cap_timestamp` (gives the ATH in SOL),
  * or the latest 4 pages when the ATH fell inside slot 0.
* **Skipped coins**: coins with `ath_market_cap` < $5,500 are not hits by construction. SOL was ≥ $102.2 throughout (min of `priceUsd/priceSol` over 4,199 traded launches), so the max is < 58 SOL < 2.09 × floor.
* **Population**: only **non-mayhem, SOL-quoted** launches are scored. That is 6,168 of 9,905. Mayhem is 25.1% of creates and is handled separately (§6). Non-SOL quotes are 13.4%.

**Decoder verification (VERIFIED).** On launch `6bLzLig…o3U2` (slot 451740411) we fetched each slot-0 trade with `getTransaction` and decoded its TradeEvent.

* All six swap-api mcaps matched the on-chain virtual reserves exactly: 40.13 / 44.32 / 48.30 / 51.00 / 56.72 / 58.53 SOL.
* The bundle sits at tx index 88-91. The two independent snipers sit at 382 and 451.
* Our entry is after the 1st sniper, at 56.72 SOL.

**Clustering keys.**

* **Wallet**: the create signer. We checked for service signers (one signer passing ≥3 different `creator` args) and found none.
* **Funder component**: union-find over non-terminal, non-busy hops (≤3).
* **Exchange band**: fresh signers (≤10 prior txs) keyed by (terminal CEX or busy funder, first-hop amount to 0.01 SOL, dev buy to 0.01).
* **Bundle shape**: (number of bundle wallets, capped at 5; dev buy to 0.01; bundle SOL to 1).
* **Metadata host and launcher program are descriptive only.** They identify *tools*, not operators:
  * `ipfs.io` is on 66.4% of creates, `metadata.j7tracker.io` 16.9%, `meta.uxento.io` 3.7%, `pump.mypinata.cloud` 1.7%, `m.rapidlaunch.io` 1.7%.
  * Launcher programs (`6Vo3245…`, `FAdo9N…`, `J7pour…`, `pfeeUx…`) each cover hundreds of unrelated signers.
  * Unioning on them merges thousands of signers into a few "operators" (tested; not reported).

## 2. How many operators launch ≥ 5 tokens?

**Horvitz-Thompson estimate.** Each sampled wallet is weighted by 1/P(selected and measured). Wallets sampled ≥2 times use N̂ = c/f. Sampled singletons use their exact listing count, with a second-stage fraction of 400/5,295.

| launches in 16 d | creator wallets (estimate) |
|---|---|
| ≥ 2 | ~30,900 |
| **≥ 5** | **~8,900** |
| ≥ 20 | ~2,050 |
| ≥ 100 | ~1,390 |
| ≥ 300 | ~350 |
| ≥ 1,000 | ~52 |

* **Distribution**: of the random singleton wallets, 174/400 really launched one token and 162/400 launched ≥5.
* **Share of all launches**: wallets with ≥5 launches made 68.4% of launches, ≥100 made 48.9%, and ≥1,000 made 13.8%. VERIFIED (sample + listings), LIKELY ±15% on the counts.
* **Spot check.** `HfLpidy8…` has 400 listed coins in the last 3 days, and 5 of 5 sampled launches in that span appear in the listing, which matches f. It made 134 sampled launches, or ~10.5k in 16 d. They have no dev buy, are 0/68 at +100%, and the wallet also funds 0.01-SOL "fresh" farm wallets.
* **Rotating operators (funder components).**
  * We found 99 multi-wallet components in the sample (55 of size 2, 28 of size 3, up to 10).
  * Their 478 sampled launches are mostly 0-dev-buy farms with median ATH = floor.
  * Exchange-band keys gave 145 multi-wallet clusters. The largest are 0.01-0.03 SOL dust bands from busy hubs (`HfLpidy8…`, `49CfXAr5…`, `6qYwbJ6q…`) plus a "Binance 1, 0.01 SOL" band. These are **farms, not ruggers**.
* **The guru's archetype is rarer than his framing suggests.** Fresh signer + exchange funding + bundle is **3.7% of structured launches** (171/4,635 non-mayhem SOL-quoted). That is about 1,100/day at 49.4k creates/day, and it is a *class* of many operators.

## 3. Base rates (non-mayhem, SOL-quoted; Wilson 95%)

| class | n | +100% | +150% | +200% |
|---|---|---|---|---|
| all | 6,168 | **7.0%** [6.4, 7.7] | 4.9% [4.4, 5.5] | 3.8% [3.4, 4.4] |
| before Oct 9 (400 ms slots) | 4,412 | 7.2% [6.4, 8.0] | 5.1% | 4.0% |
| from Oct 9 (200 ms slots) | 1,756 | 6.6% [5.5, 7.9] | 4.4% | 3.4% |
| fresh signer (≤10 prior txs) | 2,390 | 9.3% [8.2, 10.5] | 6.8% | 5.4% |
| serial signer (≥1000 prior txs) | 1,944 | 8.1% [7.0, 9.4] | 5.0% | 3.9% |
| not bundled | 3,470 | 3.7% [3.1, 4.4] | 2.5% | 1.9% |
| **bundled (≥1 adjacent wallet)** | 1,165 | **17.0%** [14.9, 19.3] | 12.1% [10.4, 14.1] | 9.4% [7.9, 11.3] |
| bundle ≥3 wallets | 624 | 16.0% [13.4, 19.1] | 11.2% | 9.5% |
| **fresh + bundled + exchange-funded** | 171 | **18.1%** [13.1, 24.6] | 14.0% [9.6, 20.0] | 11.7% [7.7, 17.4] |
| ≥1 third-party slot-0 buyer | 1,504 | 12.2% [10.6, 13.9] | 8.7% | 6.8% |
| no third-party slot-0 buyer | 3,131 | 4.6% [3.9, 5.4] | 3.1% | 2.4% |

**Entry facts** (n=5,226 structured):

* Median entry spot is **29.83 SOL**, 6.7% above the 27.96 floor.
* 31.7% of launches have any third-party slot-0 buyer; p90 is 2 buyers.
* 25.7% are bundled.
* The base rate (7.0%) reproduces the skeptic's 6.8% from an independent, larger sample. The 200 ms slot change did not move it.

## 4. Per-operator hit rates (full window, in-sample)

* We scored **129 operator units with ≥15 launches**: 196 expanded wallets, sampled wallets, bundle-shape, funder and exchange-band clusters.
* **0 have a point estimate ≥ 43%**, and the best lower bound is 17.3%.
* The top of the list is consistent with selection noise among 129 units:

| unit | type | est. launches / 16 d | n scored | +100% | +150% | +200% |
|---|---|---|---|---|---|---|
| `DWHEZ8Jn…` | wallet (expanded) | 294 | 17 | 35.3% [17.3, 58.7] | 29.4% | 11.8% |
| `DRXPafDu…` | wallet (expanded) | 492 | 23 | 30.4% [15.6, 50.9] | 13.0% | 8.7% |
| `bwamJzzt…` | wallet (sample) | ~3,900 | 34 | 29.4% [16.8, 46.2] | 17.6% | 14.7% |
| `8HeDT75s…` | wallet (expanded) | 241 | 26 | 26.9% [13.7, 46.1] | 11.5% | 11.5% |
| `8emnM6nf…` | wallet (expanded) | 121 | 39 | 25.6% [14.6, 41.1] | 15.4% | 12.8% |
| bundle shape (1 wallet, dev 0.99, 2 SOL) | cluster | ~1,780 | 17 | 23.5% [9.6, 47.3] | 17.6% | 11.8% |
| `HfLpidy8…` | wallet (sample) | ~10,500 | 68 | 0.0% [0, 5.3] | 0% | 0% |

**`bwamJzzt…`** is the most "rugger-like" high-volume operator.

* It dev-buys a fixed **11.85 / 4.94 / 9.88 SOL** and uses the custom host `md.sdfgsdfsdf.uk`.
* It sets a different `creator` on 33 of 51 sampled launches, which is a fee-recipient rotation.
* About 300 listed coins appeared in the last 11 h alone.
* Entry after its dev buy is ~54 SOL mcap. The hit rate is 29% [17, 46], well below break-even.

## 5. Walk-forward (discover days 0..t-1, score days t..t+6; non-mayhem SOL)

Rules:

* **R0**: every operator with ≥5 estimated launches before t (and ≥2 observed).
* **R1**: the guru's rule, ≥3 of the last 10 pre-t launches hit +100% (≥5 scored).
* **R2**: ≥43% of the last ≥8.

"Launches/day" is the operators' estimated test-window launches per day, scaled from the sample or listing.

| unit / rule | t | operators | scored | launches/day | +100% | +150% | +200% |
|---|---|---|---|---|---|---|---|
| wallet (sample) R0 | 5 / 7 / 9 | 313 / 445 / 575 | 311 / 327 / 319 | ~7,700-8,200 | 9.3 / 8.6 / 9.7% (CI ≤ 13.5) | 5.5 / 4.9 / 5.0% | 3.9 / 3.4 / 3.4% |
| wallet (expanded) R0 | 5 / 7 / 9 | 119 / 131 / 152 | 998 / 970 / 912 | ~1,450-1,570 | 6.1 / 6.3 / 6.4% (CI ≤ 8.1) | 3.9 / 3.7 / 4.1% | 3.1 / 2.8 / 2.7% |
| **wallet (expanded) R1** | 5 / 7 / 9 | 3 / 3 / 6 | 43 / 32 / 55 | **57 / 50 / 92** | **18.6 [9.7, 32.6] / 18.8 [8.9, 35.3] / 21.8 [12.9, 34.4]%** | 14.0 / 9.4 / 10.9% | 11.6 / 9.4 / 7.3% |
| wallet (sample) R1 | 7 / 9 | 2 / 5 | 22 / 32 | 393 / 538 | 22.7 [10.1, 43.4] / 18.8 [8.9, 35.3]% | 13.6 / 6.2% | 13.6 / 6.2% |
| wallet (expanded) R2 | 9 | 2 | 21 | 41 | 14.3 [5.0, 34.6]% | 0% | 0% |
| bundle-shape R0 | 5 / 7 / 9 | 55 / 72 / 90 | 122 / 151 / 158 | ~2,300-3,400 | 10.7 / 12.6 / 15.8% (CI ≤ 22.3) | 5.7 / 7.9 / 11.4% | 3.3 / 6.6 / 10.1% |
| bundled wallet/component R0 | 5 / 7 / 9 | 48-86 | 55 / 72 / 87 | ~1,100-1,850 | 10.9 / 11.1 / 17.2% (CI ≤ 26.5) | 7.3 / 6.9 / 12.6% | 3.6 / 2.8 / 9.2% |
| funder component R0 | 5 / 7 / 9 | 26 / 42 / 62 | 78 / 38 / 14 | ~260-890 | 0 / 0 / 0% (CI ≤ 21.5) | 0% | 0% |
| exchange band R0 | 5 / 7 / 9 | 35 / 55 / 73 | 43 / 65 / 161 | ~680-1,930 | 2.3 / 4.6 / 1.2% | ≤ 3.1% | ≤ 1.5% |

**Persistence test** (expanded wallets with ≥5 pre-t and ≥3 test launches, split into terciles by pre-t +100% rate):

| t | top third (pre-t mean) → test | middle → test | bottom → test |
|---|---|---|---|
| 6 | 20.6% → **15.1%** [11.2, 19.9] (n=259) | 2.1% → 2.7% | 0.0% → 1.1% |
| 8 | 21.7% → **16.2%** [12.2, 21.1] (n=266) | 2.3% → 5.4% | 0.0% → 1.0% |

* **Selection skill is real.** Past hit rate ranks operators: a ~15× spread between terciles, holding out of sample.
* **The level is wrong.** The best tercile regresses from ~21% to ~16%.
* **Capacity and statistics.**
  * Rule R1 picked only 3-6 wallets per split, at about 50-90 qualifying launches/day. Those are among the 200 expanded wallets, a random 22% of the 920 mid-size wallets, so about 230-420/day pool-wide.
  * At 18-22% and break-even 43%, the expected PnL is negative.
  * To *prove* 43% vs 22% at 80% power would need about 30 snipes. To prove an operator *above* 43% needs the 100-170 snipes the skeptic computed. No operator unit reached such a rate.

## 6. Side finding: mayhem coins (UNVERIFIED economics)

* Mayhem coins (25% of creates) hit "+100%" **28.0% [26.2, 29.8]** of the time (n=2,413).
* This comes from the protocol's AI agent trading the extra 1B supply. It is *not* the rugger mechanism, and the floor is not guaranteed, because the agent sells tokens that were never bought from the curve.
* Our loss side (floor) does not apply, so the PnL is unknown.
* It is worth a separate study: the agent schedule, the post-pump low, and the drawdown.

## 7. Answer to the question and implications

1. **Operators with ≥5 launches**: about 8,900 wallets (16 d), making 68% of launches, plus ~100 visible multi-wallet funder groups in a 1.27% sample. Most volume comes from farms and serial bots with 0-8% hit rates.
2. **Per-operator hit rates from a realistic entry**:
   * the best in-sample is about 25-35% at +100%, 13-29% at +150% and 9-19% at +200%, with lower bounds of 13-17%;
   * out of sample, after selection, it is about 15-22% at +100%, 9-14% at +150% and 7-12% at +200%.
3. **Versus break-even** (43-48% at +100%, ~33% at +200%): the gap is **2-3×**. Our hit definition already sells at the exact peak, so real numbers are lower.
4. **Volume**:
   * qualifying launches are ~50-90/day among the expanded wallets, ~230-420/day pool-wide under the guru rule;
   * an unselected bundled class is ~7,700/day (25.1% of the 62% of creates that are non-mayhem SOL-quoted, 17% hit rate);
   * the fresh + bundled + exchange class is ~1,100/day (18%).
5. **What could still create an edge** (untested here):
   * a better exit than "hold to +X";
   * a front-run of the consolidation sell;
   * size-scaled stops;
   * a feature model across classes, aiming to push the top tercile from ~16% to ≥43%.

   Speed only moves the entry from the median third-party buyer to the first. In §3, launches with *no* third-party buyer, where entry is cleanest, hit only 4.6%.

## 8. Limitations

* **Hit definition is optimistic**: max spot after entry over the coin's life (ATH fetched 6 h to 16 d after creation), with full size sold at the peak and no slippage from other sellers. This biases hit rates *up*, so it strengthens the negative result.
* **Sampling depth**:
  * f = 1.27%, so operators with <~80 launches are individually invisible except through the 400 singleton listings and 200 expanded wallets.
  * Funder and exchange-band clusters are sample-only, with small n per operator.
* **Funding trace**:
  * only 1 page of pre-create history and the 3 oldest txs;
  * wallets with ≥1000 earlier txs (44% of launches) are treated as their own operator;
  * linking through busy hubs only happens via the exchange-band key;
  * the CEX label set is from `labels.py`.
* **Bundle detection**: index adjacency only (≤5 after create, gaps ≤2). Delayed bundles (slot+1..3) and shared-tip or shared-funder attribution are not used. Structure covers a 75% rank prefix (7,462/9,947); within it, coverage is complete.
* **Listings**: `/coins?creator=` counts only coins whose `creator` field is the wallet. Launcher-signed coins with another `creator` are missed (3/400 singletons listed 0).
* **Regime**: 200 ms slots since Oct 9 (days 11-16). Alpenglow (~Oct 21) may change slot-0 structure again.
* **Shared limits**: about 25% of swap-api calls were retried after 429s. Some funder hops failed after 10 retries; those launches were traced to depth <3.

## 9. Reproduce

From `analysis/scripts/gap1/`, run `s1_index.py 16`, `s2_decode.py 10000`, `s3_outcomes.py 0`, `s3b_fill.py 0`, `s4_funding.py 0`, `s6_expand.py counts 400`, `s6_expand.py outcomes 40 200`, then `s7_walkforward.py 9947`. All responses are cached under `data/cache/{rpc,pumpapi,gap1}`.
