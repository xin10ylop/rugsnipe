# On-chain funding patterns of pump.fun creators (Task B)

*Measured 2026-10-14 between 07:26 and 08:50 UTC. Code: `analysis/rugsnipe_analysis/{labels,funding}.py` and `analysis/scripts/{verify_labels,trace_funding}.py`.
Data: `data/funding_creators.jsonl` (180 rows, one per creator) and `data/reports/funding_sample.json` (aggregates and siblings scans).*

## TL;DR

* **The guru's "exchange" scheme is real and easy to detect.** A fresh wallet is funded by a CEX hot wallet a few minutes before it creates a token, and the token launches with a bundle.
  * We found 13 creators of this kind: fresh, funded by a labeled exchange, and created at most 1 hour after funding.
  * In all 13, funding came **24 to 798 s** before the create (median **159 s**). 11 of the 13 (85 %) launched with at least 2 other wallets buying in the creation slot.
  * The dominant source is **one wallet: Binance `5tzFki…uAi9`**. It was the direct funder of 18 of the 180 sampled creators. Dune calls it "Binance 1"; repos that copy Solscan's tag call it "Binance 2", the wallet the guru uses. It moves about 940 tx/h and holds 1.7 M SOL.
* **The "mother with a fixed amount" scheme is also real.** We caught one live:
  * `ET69Tz…NrD` was itself funded through **FixedFloat**, a no-KYC instant exchange. In 3 days it sent **exactly 0.9 SOL** to 57 wallets, 41 of them brand-new.
  * 15 of those 41 created pump.fun coins (20 coins). The two in our sample both dev-bought exactly **0.8395 SOL** and bundled 2-3 wallets in slot 0.
  * This is the fingerprint the guru describes: same amount, same dev buy, same bundle.
  * The coins' results were mediocre: median ATH $4.0k, p90 $6.9k, best seen $12.9k, none graduated.
* **The specific "2.955 / 2.995 SOL" amounts never appeared** (0 of 126 traced creators), and **fixed amounts are not the norm** for CEX-funded ruggers. Binance-funded fresh creators received 0.996, 1.07, 1.13, 1.19, 1.21, 1.27, 1.30, 2.50, 2.99 SOL and so on. That is 6-decimal noise, not a reusable constant. Amount matching works for some mother wallets, not as a universal key.
* **The population is not what the guru implies:**
  * **37 of the 100** most recent launches came from wallets that had already created **50 or more coins** (and have more than 3000 txs). These are launch farms that do not rotate wallets.
  * 28 of the 100 were fresh wallets.
  * 4 of the 100 were mass-farm wallets funded with dust, e.g. `94dDvx…`, which seeded 387 fresh wallets with 0.0104 SOL each. Their coins never left the floor: median ATH $2,990, the curve floor.
* **For the bot, the practical edge is speed of attribution, not manual Solscan work.**
  * Exchange-funded ruggers create about 2.5 minutes (median) after funding. So the bot must watch CEX hot-wallet and mother **outgoing** transfers live (Geyser/Yellowstone) and pre-arm fresh recipients *before* they create.
  * Scanning Binance after the fact over public RPC is infeasible: 300 txs cover only about 17 minutes of its activity.
  * The reverse index (every new creator's funding, captured at creation) is cheap and is what `siblings_from_index()` implements.

## 1. Address labels (`labels.py`)

**Sources** (every entry cites at least one; see `labels.SOURCES`):

* The Dune spellbook `cex_solana.addresses` list.
* Community repos: FLOCK4H/Mamba, drixindustries/Rug-Killer, DaviRain-Su/tokens-analysis, christianfm10/ravexutils (~200 entries), christianfm10/axiomfilter, wallets-alpha.
* ChainBench's platform fee table (derived from DefiLlama adapters) and a DefiLlama Bybit adapter fork.
* Mayan's swap-sdk, jup-ag/platform-list and RedBoardDev/binsight for bridges.
* Chainstack's FOMO docs for the Relay solver.
* **Jito's live block engine** (`getTipAccounts`, authoritative).

Solscan itself returns HTTP 403 (Cloudflare) to scripts, so Solscan tags are only cited second-hand.

**Verification** (`scripts/verify_labels.py`, results in `analysis/rugsnipe_analysis/labels_verified.json`):

* For every address we fetched owner program, balance, executable flag, and the last 1000 signatures (CEX-like) or 200 (others). From those we derived tx/h and last activity.
* Verdicts over 392 labels: **175 confirmed, 185 inconclusive, 32 rejected**.

| category | high | medium | low | rejected |
|---|---|---|---|---|
| cex | 31 | 55 | 140 | 32 |
| swap_service (FixedFloat, ChangeNOW, Changelly, ChangeHero) | 1 | 2 | 2 | 0 |
| onramp (MoonPay) | 0 | 3 | 0 | 0 |
| gambling | 1 | 6 | 4 | 0 |
| institution (MM/custody) | 0 | 4 | 16 | 0 |
| bridge | 12 | 11 | 2 | 0 |
| dex / launchpad / mev | 6 / 3 / 8 | 1 / 9 / 0 | 0 | 0 |
| bot_fee (Axiom, GMGN, Photon, BullX, Trojan, Banana, Maestro) | 0 | 36 | 7 | 0 |

**Data-quality findings.** These are worth knowing before trusting any public label list.

* **Dune's list:**
  * 36 of its 166 Solana entries are not Solana pubkeys at all: BTC `bc1…` addresses, MultiversX `erd1…`, Cardano `addr1…`/`DdzFF…`, and lower-cased strings.
  * 7 of its "Backpack" entries are **token mints**: JUP, HNT, JTO, mSOL, RNDR and others.
* **drixindustries:** "Binance 1" `5tzL3D…` and "Binance 3" `H8sMJSCQ…DUYexta…` have **no on-chain history at all**. They look fabricated: each shares a long prefix with a real address.
* **Other hobby lists:** some contain addresses with the character `0`, which is invalid base58.
* **ChainBench:** has one Jito tip account with a typo. The live list was used instead.
* **Many "CEX" entries are dead:**
  * Upbit and CoinDCX deposit wallets are drained to 0 lamports and closed. They are kept as `inconclusive` because they have history.
  * OKX 3-22 have been dormant since 2024.
  * Only 24 CEX wallets are hot right now (>= 60 tx/h). Another 34 are warm (1-60 tx/h) and 125 are cold.
* **Wallets our own traces surfaced** as unlabeled high-activity funders, then identified on GitHub and verified on-chain:
  * Bybit `iGdFcQ…` (185k SOL, 247 tx/h)
  * FixedFloat `5ndLnE…` (24k SOL)
  * MoonPay `5F1seM…`
  * Relay's payout solver `F7p3dF…` (5,180 tx/h)
* Final confidence combines source agreement with the on-chain verdict. `get_label()` never returns a `rejected` label.

## 2. Method (`funding.py`)

* **First / main funding.**
  * We paginate the creator's signatures to the oldest page. The cap is 3 pages (3000 sigs); above it the status is `older_than_cap`.
  * We then decode the earliest successful txs before the create time. Decoding uses parsed System transfers (incl. CPI), with a balance-delta fallback.
  * `first` is the first incoming SOL, i.e. what Solscan shows as "funded by". `main` is the largest incoming SOL before the create.
  * They are the same transfer in 98 of 126 cases. The chain follows `main`.
* **Chain.**
  * We walk up to 3 hops: funder, then the funder's funder (as of the time it sent downstream).
  * We stop at a labeled terminal: CEX, swap service, on-ramp, casino, bridge, DEX, or pump.fun.
  * We also stop at an unlabeled wallet with more than 2000 txs (`exchange_like`).
  * When the money arrived from a PDA or relayer inside a program call, the hop is attributed to that program (`via_bridge`, `via_dex`, `via_launchpad`). `via_launchpad` means the SOL came out of a pump.fun bonding curve, i.e. **proceeds of selling a previous coin**.
* **Hop classes.**
  * `fresh_intermediary`: at most 10 txs and born at most 48 h before it funded downstream.
  * `wallet`: any other unlabeled wallet.
  * `exchange_like`: more than 2000 txs.
* **Fresh creator:** history complete, at most 5 txs before the create, and first tx at most 24 h before the create.
* **Bundle proxy:** creation-slot trades from swap-api. A create counts as bundled if at least 2 distinct non-creator buyers bought in slot 0.
* **Siblings scan:** given a funder and an amount, we decode its last N txs (N = 300) and keep transfers within ±0.0005 SOL. For each recipient we check whether the transfer is its first-ever tx (fresh), then use `/coins?creator=` to see what it launched.

## 3. Sample

* **Pool:** 1000 newest coins from `/coins` (07:26 to 08:16 UTC, 53 minutes). The API offset is capped at about 1000 rows.
* **180 creators in three arms:**
  * **recent:** the 100 most recent unique creators, near-random.
  * **notable:** 40 creators of coins with ATH >= $8k.
  * **bundled:** 40 creators whose creation slot had at least 2 non-creator buyers putting in at least 1 SOL. 58 of 400 screened coins (14.5 %) qualified.
* **Only "recent" is representative.** The other two arms are deliberately enriched.
* **126 of 180** creators had a complete history (3000 sigs or fewer) and a decoded funding. 54 are `older_than_cap` serial wallets.
* **RPC cost:** ~4,500 calls for the first run (19 min), and ~1,300 calls (4.5 min, mostly cached) for the re-trace with the final label set.

## 4. Results

### 4.1 Who funds creators (direct funder of the `main` transfer, traced creators only)

| direct funder | all (n=126) | recent (n=68) | notable (n=27) | bundled arm (n=31) |
|---|---|---|---|---|
| labeled CEX | 27 | 9 | 4 | **14** |
| labeled on-ramp (MoonPay) | 4 | 4 | 0 | 0 |
| fresh intermediary (<=10 txs, born <=48 h before) | 26 | 14 | 6 | 6 |
| regular unlabeled wallet ("mother" candidate) | 33 | 22 | 9 | 2 |
| unlabeled >2000-tx wallet | 24 | 11 | 6 | 7 |
| pump.fun curve (sell proceeds) | 8 | 5 | 1 | 2 |
| DEX swap / bridge | 4 | 3 | 1 | 0 |

**Chain pattern** (after up to 3 hops):

| pattern | all | recent | bundled arm |
|---|---|---|---|
| exchange (labeled CEX / swap / on-ramp, direct or via fresh hops) | 35 | 15 | 15 |
| unlabeled >2000-tx wallet (unknown exchange or busy mother) | 28 | 13 | 9 |
| mother_or_simple (ends at a regular wallet) | 39 | 27 | 2 |
| pump.fun / DEX proceeds | 13 | 7 | 4 |
| bridge | 3 | 2 | 1 |
| fresh chain still unresolved after 3 hops | 8 | 4 | 0 |

**Labeled exchanges within 3 hops:** Binance 21, KuCoin 4, Bybit 4, MoonPay 4, FixedFloat 2, and Gate.io, Coinbase, Bitstamp, Kraken, LBank 1 each.

* **One Binance hot wallet (`5tzFki…`) dominates.** It was the direct funder of 18 creators.
* **"Binance 2" in the guru's video** is consistent with this wallet: drix's Solscan-tag copy calls it Binance 2, and it shows transfers every few seconds and a huge balance.
* **The CEX share depends on the arm:**
  * Among bundled creates it is about 45 % (14 of 31 traced).
  * Among random recent creates it is only about 13 % (9 of 68).
  * Among high-ATH coins it is 15 % (4 of 27).

### 4.2 Fresh wallets vs serial wallets

| | recent arm (n=100) | all (n=180) |
|---|---|---|
| fresh creator (<=5 txs before create, born <=24 h before) | 28 | 60 |
| wallet already created >=50 coins (and >3000 txs) | 37 | 64 |
| wallet created exactly 1 coin | 36 | 71 |

The guru wants rotating, fresh-wallet ruggers and avoids same-address serial creators. On today's pump.fun, about a third of launches come from serial launch wallets with 50 or more coins. **47 % of launches from wallets with more than 3000 txs were bundled** (15 of 32 in the recent arm), versus 14-24 % for the fresh and not-fresh traced groups. "Bundled" alone is therefore not a rugger-quality signal; the wallet-rotation filter matters.

### 4.3 Funding to create delay

* **All traced creators (n=126):** p10 23 s, p25 159 s, median 35 min, p75 6 days.
  * 23 created within 60 s of funding, 50 within 10 min, 65 within 1 h, and 50 more than 24 h later.
* **Fresh creators (n=60):** p25 39 s, median **160 s**, p75 571 s. 46 of 60 (77 %) created within 10 minutes of funding.
* **Fresh, exchange-funded creators (n=13):** 24, 29, 39, 75, 86, 88, 153, 159, 164, 227, 308, 398, 798 s.
* **Consequence for the bot:** a funding watcher that only reacts after the create is too late to pre-classify. One that sees the funding transfer has a median **~2.5 minute head start** to:
  * score the recipient,
  * load the cluster's stats,
  * pre-build the buy transaction,
  * subscribe to the recipient's account.

### 4.4 Amounts: fixed odd amounts?

* **Main funding (n=126):** median 1.01 SOL, p75 2.0, p90 17.7.
  * Bins: < 0.05: 17, 0.05-0.5: 25, 0.5-1: 17, **1-2: 32**, 2-3: 11, 3-10: 10, >= 10: 14.
* **Only 16 of 126 (13 %) are multiples of 0.1 SOL;** 110 are "odd". But odd mostly means 6-decimal noise rather than a deliberate constant:
  * Binance withdrawals to fresh creators: 0.9965, 1.0697, 1.1317, 1.1749, 1.1888, 1.2090, 1.2660, 1.2968, 2.0041, 2.4990, 2.9866, 2.9896, 4.9181.
  * 1.999 and 2.499 are round amounts minus a 0.001 fee.
* **Repeated amounts across distinct creators:**

| amount | creators |
|---|---|
| 0.01041188 | 4, all from mother `94dDvx…` |
| 0.9 | 2, from mother `ET69Tz…` |
| 1.999 | 2, Binance |
| 1.0 / 2.0 / 0.5 | various |

* **The guru's 2.955 / 2.995: 0 occurrences.** It was one rugger's constant at the time of his video.
* **The siblings scan on Binance 1.999** found 4 matches in 17 minutes, 3 to fresh wallets, and **none of those created a coin.**
  * This is exactly the guru's warning that round-ish CEX amounts mix with retail withdrawals.
  * An amount alone does not identify a rugger; amount + fresh recipient + a create within minutes does.

### 4.5 Mother wallets (siblings scans)

| funder | amount | scanned window | matches | fresh recipients | recipients that created coins | their coins: ATH median / p90, graduated |
|---|---|---|---|---|---|---|
| `ET69TzBLppVXzQAzMiXPxYKzDybFrxg759zuVGAQJNrD` (funded via FixedFloat) | 0.9 SOL | 275 txs, 78 h | 57 | 41 | 15 (20 coins) | $4.0k / $6.9k, 0 graduated |
| `94dDvxXtGsdVVUmBDFkMx1eL68Zy8qRmPSpNEGDKvCh1` (funded by an old >2000-tx wallet) | 0.01041188 SOL | 169 txs, 18 days | 387 (batched transfers) | 387 | 60 of 60 checked | $2,990 / $3,017 (floor), 0 graduated |
| `EPLFBYEj…NxY7` | 11.309 SOL | 291 txs, 12 days | 1 | 1 | 1 (44 coins, serial) | $4.8k / $14.6k, 1 graduated |
| Binance `5tzFki…` | 1.999 SOL | 300 txs, **0.3 h** | 4 | 3 | 0 | n/a |
| Binance `5tzFki…` | 1.297 SOL | 300 txs, 0.3 h | 0 | 0 | 0 | n/a |
| 3 fresh intermediaries | 1.08-1.58 SOL | <=9 txs | 1 each | 1 | 1 | $3.9-4.7k |

Example chains, verbatim from `data/funding_creators.jsonl`:

* **Exchange scheme:**
  * FixedFloat → `ET69Tz…` (2.67 SOL; mother, 276 txs) → `6FA22F…`
  * `6FA22F…` is a fresh creator: 0.9 SOL, created 245 s later, dev buy 0.8395, 2 bundle wallets with 1.62 SOL. Its token reached ATH **$12.9k (~4.3x the floor)**.
  * A second recipient, `6uEqLH…`, followed the same pattern: 0.9 SOL, created 1,321 s later, dev buy **0.8395**, 3 bundle wallets with 2.61 SOL. ATH $3.8k.
* **Binance direct:** Binance `5tzFki…` sends 2.98656 SOL to `H57KAw…` (fresh, 8 txs). `H57KAw…` creates 227 s later with dev buy 0.75 SOL and 5 bundle wallets with 5.35 SOL. ATH $4.4k.
* **Profit recycling:** a pump.fun curve pays out sell proceeds to fresh intermediary `7cAm5L…` (1.48 SOL). 1.58 SOL then goes to fresh creator `8TMpPa…`, which creates 273 s later with a 0.99 SOL dev buy and 2 bundle wallets with 3.2 SOL.
* **Coin farm:** `BjpL2b…` (>2000 txs) sends 0.5 SOL to `94dDvx…`, which seeds hundreds of wallets with 0.0104 SOL each. Each creates a coin 2 s after funding with no buys. ATH = floor.

### 4.6 Guru claims vs evidence

| claim | verdict | evidence |
|---|---|---|
| Good ruggers rotate wallets and use fresh wallets | **partly** | 28 % of recent launches are fresh wallets, 37 % come from 50+-coin serial wallets. Rotation exists but is a minority behaviour, which makes it a useful filter. |
| Funded by CEX hot wallets (Binance 2, Coinbase, KuCoin, OKX...) | **yes, Binance-dominated** | 21 of 40 exchange-funded chains are Binance, mostly one hot wallet. KuCoin 4, Bybit 4. Coinbase 1, OKX 0 in this hour. |
| ...shortly before creating | **yes** | Exchange-funded fresh creators: 24-798 s, median 159 s. |
| Fixed odd amount (e.g. 2.955) per rugger | **weak / rugger-specific** | 0 hits of 2.955/2.995. CEX amounts are mostly non-repeating 6-decimal values. Mothers *can* use fixed amounts (0.9, 0.0104), and siblings scans find them instantly. |
| "Mother" bank wallet with fixed amounts | **yes** | `ET69Tz…`: 0.9 SOL to 41 fresh wallets in 3 days, with an identical dev buy (0.8395 SOL) across its coins. It was itself funded via FixedFloat, i.e. a mother behind an instant exchange. |
| Bundle 2-6 wallets in slot 0 | **common** | 14.5 % of screened launches had at least 2 non-creator slot-0 buyers with at least 1 SOL. 85 % of fresh exchange-funded creates were bundled. |
| Amount filter alone finds the rugger (Solscan amount range) | **no** | Binance 1.999: 3 fresh recipients in 17 min, 0 created coins. You need amount + fresh + create, i.e. a live join. |

## 5. What this means for our system (better than manual)

1. **Live funding watcher (core feature).**
   * Subscribe over Geyser/Yellowstone gRPC to the hot exchange wallets and to every mother discovered so far.
   * Exchange wallets: the 24 hot and 34 warm CEX wallets (`labels_verified.json`, `activity_class`), plus FixedFloat, ChangeNOW, Changelly, ChangeHero and MoonPay.
   * Emit `(funder, recipient, lamports, slot)` for every outgoing transfer to a **new** account. Freshness check: the account did not exist before, so it is free to verify from pre-balances = 0.
   * Keep a TTL'd watchlist of these recipients (median create comes ~160 s later; keep entries for 30 min).
   * When a watchlisted wallet creates a coin, its funding attribution is already known **before** the creation slot is even confirmed.
2. **Cluster identity = (funder, amount bucket, dev-buy amount, bundle shape).**
   * The ET69 example shows dev-buy amount and bundle size are at least as stable as the funding amount.
   * Score each cluster with Task A's outcome metrics (TP hit rate from end-of-slot-0 price), not with the funding source alone.
3. **Reverse index instead of scanning exchanges.**
   * Store every creator's `first`/`main` funding at creation time (cost: 1-3 RPC calls, or zero with a Geyser feed).
   * `siblings_from_index()` answers "who else did this funder fund at this amount" in O(1).
   * Scanning an exchange after the fact over public RPC is hopeless: Binance `5tzFki…` does about 940 tx/h, so 300 decoded txs cover about 17 minutes.
4. **Filters suggested by the data** (to be validated against outcomes in Task A/C):
   * Drop dust-funded coin farms: main funding < 0.05 SOL, mother with hundreds of 0.0104-SOL children, no slot-0 buys.
   * Separate serial 50+-coin wallets: the guru's filter, but they are 37 % of launches and very often bundled.
   * Treat profit recycling (`via_launchpad`: funds from a pump.fun curve, often through a fresh intermediary) as evidence of an active rugger.
5. **Labels need upkeep.** Public lists are partly wrong (36 invalid, 7 token mints and 3 fabricated entries found). Re-run `verify_labels.py` weekly. Promote any unlabeled `exchange_like` funder that recurs into a label after identification; `funding_sample.json → unlabeled_exchange_like_funders` lists candidates.

## 6. Limitations

* **One 53-minute window** of launches on a single day; 180 creators, 126 with decoded funding. The arms are enriched, so cross-arm percentages are descriptive, not population rates. Only the "recent" arm approximates a random draw.
* **History cap:** `older_than_cap` (> 3000 sigs) creators are not traced. They are almost all serial launch wallets.
* **`exchange_like`** (> 2000 txs, unlabeled) mixes unknown exchange wallets, busy mothers and old personal wallets.
* **"Main funding"** only scans the 12 oldest pre-create txs, so a wallet with many small early transfers can miss a larger later one.
* **Outcome fields:**
  * ATH is the pump.fun API value in USD at fetch time, ~10-80 minutes after creation, so later pumps are missed.
  * TP-hit rates from the end-of-slot-0 price belong to Task A's first-slot metrics and are not computed here.
* **Siblings scans** look only at the last 300 txs of each funder, and the recipient coin lookup is capped at 60 recipients.
* **Public RPC behaviour:**
  * publicnode silently truncates `getSignaturesForAddress`. Fixed in `rpc.py` by pinning history calls to mainnet-beta.
  * Signature pages fetched via publicnode with a `before` cursor *before* that fix may be cached truncated. Clear `data/cache/rpc` if in doubt.

## 7. Reproduce

```bash
cd /home/user/rugsnipe
.venv/bin/python -I -m rugsnipe_analysis.selftest           # from analysis/ (offline tests)
.venv/bin/python -I analysis/scripts/verify_labels.py        # ~5 min, rewrites labels_verified.json
.venv/bin/python -I analysis/scripts/trace_funding.py --n-recent 100 --n-notable 40 --n-bundled 40   # ~20 min cold
```
