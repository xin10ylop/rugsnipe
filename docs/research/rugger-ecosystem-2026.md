# Serial ruggers and bundlers on pump.fun (2025-2026): how they operate and how to detect them

Research date: 2026-10-14. SOL is about $107. Tags: **VERIFIED** means a primary source (docs, program behaviour, or an on-chain query we ran ourselves). **LIKELY** means a reputable secondary source. **UNVERIFIED** means a single vendor or anecdotal claim.
Our own on-chain queries (methods in section 7) used the project cache `data/cache/create_index.json`. It holds every signature of the pump mint authority `TSLvdd1pWpHVjahSpsvCXUbgwsL3JAcvokwaKt1eokM` over the 48 h up to 2026-10-14 08:05 UTC: 101,457 signatures, 93,191 of them successful. From these we drew random samples of 299 create transactions, 60 dev wallets and 120 tokens.

## TL;DR
1. **"Dev + 4 bundle wallets" matches Jito's 5-tx bundle limit (LIKELY explanation).** Bundles are atomic, so nobody gets inside one. We enter after it, in slot 0 via shreds or in slot+1, and the bundle's size becomes our entry signal.
2. **About 46.6k creates per day** (guru: 15-20k). 77.6% carry an initial buy in the create tx (median 0.198 SOL, p90 3.0 SOL). 16.1% pay a Jito tip. VERIFIED.
3. **CEX-funded devs are common.** 10/60 dev wallets were funded directly by a labelled CEX, most often Binance `5tzFki…`, which the drix list (citing Solscan) calls "Binance 2", probably the guru's "Binance 2". 22/60 dev wallets have more than 5k signatures, so many creators reuse wallets.
4. **Consolidate-then-dump is real but uncommon in random tokens.** Only 2 of 14 big single-slot dumps involved transferred-in tokens, while 10 of 14 were many wallets selling in the same slot. Validate it per rugger.
5. **Bundler, insider and sniper labels are commodity** (Axiom, GMGN, Codex, Mobula, Bubblemaps, RugCheck). Our edge is an entry in the same slot, triggered by a known rugger's funding graph.

## 1. Bundler tooling and funding

### 1.1 Launch and bundle tooling
- **Jito bundles** ([docs](https://docs.jito.wtf/lowlatencytxnsend/), VERIFIED). At most 5 txs, executed sequentially and atomically; minimum tip 1,000 lamports. Live landed-tip floor at 2026-10-14 08:12 UTC: p50 0.000016, p95 0.00219, p99 0.00227 SOL ([tip_floor](https://bundles.jito.wtf/api/v1/bundles/tip_floor)).
- **PumpPortal** ([creation](https://pumpportal.fun/creation/), [fees](https://pumpportal.fun/fees/), VERIFIED). Create + dev buy plus buys from up to 5 signers in one Jito bundle. Fees: 0.5% (local) or 1% (lightning), nothing extra for creation.
- **Smithii bundler** ([page](https://smithii.io/en/pump-fun-bundler-bot/), VERIFIED vendor page). Dev wallet plus up to 16 wallets. 0.1 SOL per launch bundle and 0.1 SOL per same-block "Bundle Sell". Claims to "shut out… sniper bots".
- **Open-source bundlers.** 20-21 wallets "on the same block" ([gittrend](https://gittrend.io/repo/PUMPFUNSCRIPT/pumpfun-bundler), [gitlab](https://gitlab.com/explore/projects/topics/pumpfun-bundler), LIKELY). More than 5 wallets requires several signers per tx.
- **Wash and comment bots.** In a stratified pump.fun sample, wash bots appear in about 4% of projects and comment bots in about 26%. A comment bot lifts participation by +116% ([SBFC 2025](https://sbfc.sydney.edu.au/2025/papers/SBFC2025_4G3_P154.pdf), LIKELY).

**Our check of the create transaction itself (VERIFIED, n = 299).** Only 1 of 299 create transactions contained a buy by a *second independent wallet*. 45 of 299 look like "creator ≠ buyer", but in 44 of them the buyer was the transaction's own fee payer, who passed a different `creator` to `create_v2`. **Implementation note: key "dev buy" on the signer or fee payer, not on `CreateEvent.creator`.** Bundle wallets therefore sit in the *adjacent* transactions of the bundle, not inside the create transaction. Slot-0 adjacency is measured in `docs/research/onchain-firstslot-stats.md` (workflow C). Compute units of the create transaction: p10 115k, p50 211k, p90 312k.

### 1.2 Typical sizes (guru vs. data)
- Guru: dev buy of about 0.1 SOL plus 4 wallets of about 10 SOL each (45-60% of supply). On the curve, 40 SOL of net slot-0 buys gives a market cap about 5.4× the initial one, roughly 152 SOL or $16.3k (`pumpfun-mechanics-2026.md`, VERIFIED math).
- In-transaction initial buy across all creates (VERIFIED, n = 232 with a buy): p25 0.01, p50 0.198, p75 0.99, p90 3.0, p95 4.94, p99 12.8 SOL. 58 were at least 1 SOL, 28 at least 3 SOL, and 6 at least 10 SOL.
- Deployer-funded same-block snipers split their buys across 2-4 wallets ([Pine Analytics, Apr 2025](https://pineanalytics.substack.com/p/exit-liquidity-machines), LIKELY).

### 1.3 How dev and bundle wallets are funded
- **Direct CEX withdrawal (VERIFIED, our query).** We traced 60 random dev wallets (signer of the create transaction) back up to 3 hops. Results: 10 were funded *directly* by a labelled CEX wallet and 12 had a CEX at the origin. 14 ended at an unlabelled "exchange-like" wallet (more than 2,000 signatures, so an unknown CEX or a mother wallet). 12 ended at a normal wallet (mother/simple chain). 23 could not be resolved, 22 of those because the dev wallet itself has more than 5,000 signatures.
  - CEX sources seen: Binance `5tzFki…` ×4, KuCoin `BmFdpr…` ×3, Bitget `A77HEr…` ×2, Coinbase `GJRs4F…` and `2AQdpH…`, and Crypto.com `AobVSw…`.
  - Example first-funding amounts: 1.994, 0.949, 1.06723, 2.057058, 3.585518 SOL. These are odd and non-round, like the guru's "2.955". The sample is too small to show repeats.
- **Wallet reuse vs. rotation (VERIFIED).** 22 of 60 dev wallets were older than our 5-page cap (more than 5,000 signatures). Within the 300-create sample, top creators appear 6, 6, 5, 4 and 4 times, out of about 93k creates. RugCheck shows one live creator, `HgFN8J…8bDG`, with **48** tokens (`creatorTokens`) and a "Creator history of rugged tokens" danger flag. So there are two populations, reusers and rotators. The guru's "≤5-10 tokens per creator" filter targets the rotators.
- **Multi-hop chains and mixers.** Pine says deployer-funding chains are "often 5 to 7 hops deep" (LIKELY). The guru puts mixer-funded ruggers at about 2% (UNVERIFIED).

## 2. Detection heuristics and quantified rates

### 2.1 Definitions used by tools (for calibration), all VERIFIED from docs
- **Codex** ([docs](https://docs.codex.io/recipes/discover-tokens)). Sniper: buys within 5 s of the first trade. Bundler: ≥4 wallets in the same block, each ≥$5, together ≥0.05% of supply. Insider: in the create tx, received tokens from the creator, or first funded by the creator (Solana: also early plain-transfer recipients).
- **Mobula** ([docs](https://docs.mobula.io/almanac/detecting-snipers-bundlers)). Sniper: buys in blocks 0-3 from a fresh, just-funded wallet. Bundler: ≥3 wallets with the same tx or block, a common funder, similar amounts, and tokens **later consolidated into one wallet**.
- **GMGN** ([docs](https://docs.gmgn.ai/index/insider-traders-snipers-first-70-buyers.md)). Tracks the first 70 buyers. Insider: holds tokens without buying them. Has a "Scammer wallets" tag for repeat rug buyers.
- **Bubblemaps** ([blog](https://blog.bubblemaps.io/whats-the-difference-between-bundle-cluster-2/)). Bundle: same block or within seconds, no funding link needed; ≥10% of supply = red flag. Cluster: shared funder or token transfers over days to months.
- **RugCheck free API** (`api.rugcheck.xyz/v1/tokens/{mint}/report`, queried 2026-10-14). Returns `creatorTokens[]`, `graphInsidersDetected`, `insiderNetworks` and a "Creator history of rugged tokens" risk flag; new mints are listed at `/v1/stats/new_tokens`.

Caveat: SBFC 2025 treats any non-creator trade in the creation block as the creator's own. Shred snipers break that assumption: Pine reports that over 50% of tokens are sniped in the creation block. **A slot-0 buy is not proof of an insider.** Attribution needs adjacent tx indexes, a shared tip account, or a shared funder.

### 2.2 Rates and outcomes (with sources)
| Metric | Value | Source / status |
|---|---|---|
| Fraud share | 98.6% of more than 7M pump.fun tokens (Jan 2024 to Mar 2025) tied to rug or pump-and-dump; fewer than 100k kept ≥$1k liquidity | Solidus Labs via [The Defiant](https://thedefiant.io/news/defi/alarming-99-of-memecoin-launches-on-pumpfun-are-pump-and-dumps-or-rug-pulls-report) LIKELY |
| Graduation | 1.7% before $TRUMP and 0.9% after (Jan 2025, stratified) | SBFC 2025 LIKELY |
| Graduation 2026 | daily rate passed 1% in late Jan 2026 (Dune via news); about 0.8% in June 2026; 6.7% spike, with a 4.7% four-day average, after the **BOOST** launch change (late Jul 2026) | [The Block](https://theblock.co/post/409815/pump-fun-token-graduation-rate-jumps-boost-changes-launch-incentives) LIKELY; current Oct 2026 rate UNVERIFIED |
| Sniper prevalence | 84% of projects have buys in the first 5 blocks; 24% have creator "rat bots" in the creation block; creators dump about 50% of projects; snipers cut creator profit by about $54 per project | SBFC 2025 LIKELY |
| Deployer-funded same-block snipes | more than 15,000 tokens per month, 4,600+ sniper wallets, 10,400+ deployers; more than 15,000 SOL realized per month; **87% of snipes profitable**; 55% fully exit in under 1 min, 85% within 5 min; activity 14:00-23:00 UTC | Pine Analytics (Mar-Apr 2025, Flipside) LIKELY |
| Bundled-token outcomes | 38 bundled tokens: minimum drawdown 78%, more than half down 99%+, average life under 24 h before rug | Bubblemaps via [CoinGabbar](https://www.coingabbar.com/en/crypto-currency-news/bubblemaps-bundled-token-report-us-leads-memecoin-scam) LIKELY |
| Trader PnL (Apr 2026) | 3.14M wallets: 5.37% made more than $1k, 0.78% lost more than $1k; profitable share 73.3% (Apr 2026) vs 30.1% (Jun 2025) | [CoinGecko/Dune](https://www.coingecko.com/research/publications/pump-fun-traders-are-making-a-comeback) LIKELY |
| Average token lifetime / hold time (guru: 56 s) | no primary source found; Pine's exit times are the closest proxy | UNVERIFIED; measure it ourselves |

### 2.3 Detector features for our system (synthesis)
1. **Creator funding.** Funder label (CEX, mother, fresh), first-funding amount, time from funding to create, and wallet age or signature count.
2. **Slot-0 structure.** Transactions adjacent to the create (tx index +1..+4), shared Jito tip account, signer count, total net SOL, and the post-bundle market cap.
3. **History.** Prior tokens of the creator *and of the funding source*. RugCheck `creatorTokens` and the pump API supply these for free, but we should build our own index.
4. **Fingerprints.** Instruction variant (`buy_v2`/`v3`), CU limit and price, tip size, metadata URI host and name pattern, and launcher program. Unlabelled top-level programs (for example `6Vo3245…`, `J7pourV…`) show up in about 3% of creates and are candidate launcher or bundler programs to label. (VERIFIED: counted in our sample; identity unknown.)
5. **Post-launch.** Token-2022 transfers between tracked wallets, and the first sell by any bundle wallet.

## 3. CEX hot-wallet labels (Solana)
Sources: Dune spellbook `cex_solana_addresses.sql` (hildobby); FLOCK4H/Mamba `cex.rs`; drixindustries `known-addresses.ts` (entries tagged "Solscan labeled"); DaviRain-Su `labels.rs`; ChainBench `anchors.go`. All are cached in `data/cache/labels/` and merged in `analysis/rugsnipe_analysis/labels.py`. Solscan and Arkham were not reachable directly: Solscan returns Cloudflare 403 and Arkham requires a login. On-chain columns come from `getAccountInfo` and `getSignaturesForAddress(limit 1000)` on api.mainnet-beta, 2026-10-14 08:09 UTC. All are system-owned accounts.

| Entity | Address | Sources | Balance (SOL) | Time for last 1000 txs | Confidence |
|---|---|---|---|---|---|
| Binance | `5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9` | 5 (Dune "Binance 1", drix/Solscan "Binance 2") | 1.74M | 64 min | HIGH, main hot wallet; most frequent dev funder |
| Binance | `9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM` | 3 | 10.57M | 68 days | HIGH entity, but a cold wallet (irrelevant for funding) |
| Binance | `2ojv9BAiHUrvsm9gxDe7fJSzbNZSJcxZvf8dqmWGHG8S` | 3 | 4 | 65 days | MEDIUM (low activity) |
| Coinbase | `H8sMJSCQxfKiFTCfDR3DUMLPwcRbM61LGFJ8N4dK3WjS` | 2 | 20.7k | 5.6 h | HIGH |
| Coinbase | `2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm` | 3 | 26.0k | 5.5 h | HIGH |
| Coinbase | `GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE` | 3 | 28.0k | 5.4 h | HIGH |
| Kraken | `krakeNd6ednDPEXxHAmoBs1qKVM8kLg79PvWF2mhXV1` | 1 (Dune) + vanity prefix | 14 | **217 s** | MEDIUM-HIGH (very hot relay) |
| Kraken | `FWznbcNXWQuHTawe9RxvQ2LdCENssh12dsznf4RiouN5` | 3 | 12 | 240 days (last tx 10-01) | MEDIUM (dormant) |
| KuCoin | `BmFdpraQhkiDQE6SnfG5omcA1VwzqfXrwtNYBwWTymy6` | 2 | 29.0k | 4.7 h | HIGH; 3 of 60 dev funders |
| KuCoin | `57vSaRTqN9iXaemgh4AoDsZ63mcaoshfMK8NP3Z5QNbs` | 1 | 0 | last tx 09-06 | LOW (inactive) |
| OKX | `is6MTRHEgyFLNTfYcuV4QBWLjrZBfmhVNYR6ccgr8KV` | 2 (Mamba, drix; not in Dune) | 257.7k | 3.8 h | HIGH |
| OKX | `5VCwKtCXgCJ6kit5FybXjvriW3xELsFDhYrPSqtJNmcD` | 2 | 0 | last tx 10-04 | LOW-MEDIUM |
| Bybit | `AC5RDfQFmDS1deWZos921JfqscXdByf8BKHs5ACWjtW2` | 4 | 308.8k | 45 h | HIGH |
| Bybit | `42brAgAVNzMBP7aaktPvAmBSPEkehnFQejiZc53EpJFd` | 1 | 20.0k | last tx 09-30 | LOW-MEDIUM |
| Gate | `u6PJ8DtQuPFnfmwHbGFULQ4u4EgjDiyYKjVEsynXq2w` | 3 | 309.1k | 7.4 h | HIGH |
| MEXC | `ASTyfSima4LLAdDgoFGkgqoKowG1LZFDr9fAQrg7iaJZ` | 4 | 34.4k | 4.1 h | HIGH |
| Bitget | `A77HErqtfN1hLLpvZ9pCtu66FEtM8BveoaKbbMoZ4RiR` | 2 | 118.6k | 8.3 h | HIGH; 2 of 60 dev funders |
| HTX | `BY4StcU9Y2BpgH8quZzorg31EGE4L1rjomN8FNsCBEcx` | 2 | 1 | 15.5 days | MEDIUM (low activity now) |
| Crypto.com | `AobVSwdW9BbpMdJvTqeCN4hPAmh4rHm7vwLnQ5ATSyrS` | 2 | 128.3k | 4.3 h | HIGH |
| Crypto.com | `6FEVkH17P9y8Q9aCkDdPcMDjvj7SVxrTETaYEm8f51Jy` | 1 | 453.1k | 37 h | MEDIUM |

Caveats:
- Numbering differs between lists ("Binance 1" vs "2").
- The Dune list has 36 invalid non-Solana strings, and some Dune "Backpack" entries are token mints (see the `labels.py` header).
- CEXs rotate hot wallets, so reverify monthly.
- Unlabelled "exchange-like" funders were 14 of 60 in our trace. Auto-label any funder with more than 1,000 tx/h that funds many fresh wallets.

## 4. How ruggers evade snipers, and what it means for our entry rules
- **Atomic bundle (create + up to 4 buy txs).** Nobody can get inside it. Use it as *information*: decode it from shreds, compute the post-bundle reserves, and enter only if **post-bundle mcap ≤ M_max**.
  - At $107/SOL, $15k ≈ 140 SOL mcap ≈ 37 SOL of net slot-0 buys.
  - The floor loss is 1 − 28/140 ≈ −80% before fees, which matches the guru's "−80/−85%".
- **Farming (big first candle, then a dump on snipers in slot 0 or 1).**
  - Learn each rugger's bundle-size distribution and skip when the observed size is above its p90.
  - Stop trading a rugger after N consecutive losses on it.
  - Sell immediately when *any* bundle wallet sells. A creator-only "dev sell" trigger misses this.
- **Delayed bundle (bundle lands in slot+1..+3).** Slot-0 snipers become the exit liquidity. For a tracked rugger, also trigger on the first buy by a wallet funded from its source within T minutes.
- **Randomized funding amounts.** These break the amount window. Score instead: a fresh wallet whose first tx is funding from a tracked source within Δt before the create, plus fingerprints (CU, tip, ix variant, metadata host).
- **Multi-hop, mixer or anti-bubblemaps funding.** Index every creator's funding offline. A reverse lookup from our creator index is cheaper than scanning CEX outflows. Same-slot adjacency plus a shared tip account still links wallets with no shared funder.

Tip economics: the guru says TradeWiz maxes the tip at 0.05 SOL. That is about 22× the current p99 landed Jito tip (0.00227 SOL, VERIFIED). Ordering inside a slot depends on the leader's scheduler and the bundle auction. Tune the tip per leader instead of maxing it (see the speed-infra doc).

## 5. Consolidation before dump ("front-run sell")
- **It exists as a known pattern.** Mobula lists "tokens later consolidated into one wallet" as a bundler signal. GMGN's "Insider" metric is "wallets that hold tokens but did not buy" (transfer recipients). Codex counts plain-transfer recipients as insiders on Solana. F-Project advertises a "front-run sell" when at least 50% of supply is transferred into one wallet (guru, UNVERIFIED).
- **Our measurement (VERIFIED method, small sample).** Of 120 random tokens with complete pump.fun trade history and at least 20 trades, 42 (35%) had a wallet that sold at least 1% of supply more than it bought on the curve, so the tokens arrived by transfer. 14 tokens had a single-slot dump of at least 30% of supply. Of those, **only 2** had such a transfer-in seller in the dump slot, 10 were multi-wallet same-slot dumps, and 4 were single-wallet dumps.
  - Conclusion: in the *general* population, the "all wallets sell in one block" dump is more common than consolidate-then-dump. Consolidation must be confirmed per rugger before we rely on it. Use it as a secondary exit trigger, not the primary one.
- **How to detect it.** The pump program emits no event for token transfers. Subscribe to Token-2022 transfer instructions on the mint (or to token-account updates for the tracked wallets) via Geyser or shreds. Trigger an exit when Σ transferred into one owner ≥ X% of supply within 2 slots.
- Data quality: one slot showed more than 100% of supply "sold" in the trade API. Deduplicate by signature and instruction first.

## 6. Competitor bots and fee models
| Product | Relevant features | Fees | Status |
|---|---|---|---|
| F-Project (Fomo Project LLC, Whop) | "Private block-0 sniping infrastructure", ShredStream, multi-nonce, multi-region. 1,731 joined; 4.9/5 from 242 reviews ([whop.com/fpro](https://whop.com/fpro)). Per the videos: funding-source watch with amount ranges, track-buy range (anti-farm), max-loss stop, front-run sell, "hardcore" fee mode. | Search snippet: €1,000 lifetime (not shown on the fetched page) | listing VERIFIED; price UNVERIFIED; the guru's "25k/month" members is inconsistent with 1,731 joined |
| GMGN | **Dev Sell**: auto-sell your position when the creator sells ≥ a set ratio in one tx. Insider, sniper and first-70-buyer tracking; "Scammer wallets" tag; migrated snipe/sell; copy trade; SnipeX (X-post sniping) | 1% per trade | [docs](https://docs.gmgn.ai/index/gmgn-fees-settings.md) VERIFIED |
| Axiom | Migration sniper buy/sell; wallet tracker; token-info Sniper %, Insider %, Bundler %, Dev holding % | Net 0.95% (Wood) down to 0.75% (Champion), via cashback tiers | fees [docs](https://docs.axiom.trade/getting-started/fees/axiom-fees.md) VERIFIED; panel metrics LIKELY |
| Trojan | Copy trading, auto-sell, wallet tracker, limit/DCA | 0.9% gross, about 0.72% effective with 20% cashback | features VERIFIED ([llms.txt](https://docs.trojan.com/llms.txt)); fee LIKELY ([madeonsol](https://madeonsol.com/blog/best-axiom-alternatives)) |
| TradeWiz | Telegram-first copy trading; snipe; auto-sell; Anti-MEV via Jito | 1% flat plus 0.002 SOL per new token account | LIKELY ([madeonsol review, 2026-09-30](https://madeonsol.com/blog/tradewiz-review-solana-copy-trading-bot)); tradewiz.ai now markets "Tradewiz PVP" copy trading on **Robinhood Chain** (VERIFIED homepage meta, 2026-10-14) |
| Photon / BullX | Generic sniping and copy | about 1% | LIKELY; one source says BullX shut down in June 2026 (UNVERIFIED) |
| PumpPortal | Bundle creation API; data WebSocket (paid trade streams) | 0.5% local, 1% lightning | VERIFIED |

None of the public terminals advertise **rugger-funding-graph triggered slot-0 entry**. That is F-Project's niche, and ours. Public "dev sell" features key on the creator wallet only. Ruggers dump from bundle wallets, so a dev-sell trigger misses the guru's model.

## 7. Methods
- **Create sample.** 300 sigs (seed 7) decoded with `dataset.creation_record`. The Jito tip is the SOL sent to the `getTipAccounts` set.
- **Funding.** `funding.trace_funding(max_depth=3)` on 60 fee payers (seed 3).
- **Consolidation.** `PumpApi.trades_latest(mint, 30)`, complete histories only. A transfer-in seller has Σsell − Σbuy ≥ 1% of supply.
- **Limits.** Samples are small (299, 60, 120), so error bars are wide.

## 8. Open questions
1. What share of slot-0 buys come from the creator's own bundle (adjacent transactions, same tip account) versus independent shred snipers? Workflow C's first-slot report should split these.
2. The CEX withdrawal-fee arithmetic: does the guru's "2.955" equal a round request minus a fixed fee? Check Binance's current SOL withdrawal fee against observed amounts in a larger sample.
3. What are the unlabelled top-level programs in creates (`6Vo3245…`, `J7pourV…`, `FAdo9NC…`, `AKVwzx…`)? Launcher, bundler or bot programs? Label them.
4. What is the current (Oct 2026) graduation rate after BOOST, and does BOOST change rugger economics (more snipers bidding pre-bond)?
5. Measure per-rugger consolidation frequency before building the front-run-sell path.
