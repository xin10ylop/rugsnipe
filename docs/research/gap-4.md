# Gap 4: CEX hot wallets and the "fresh-wallet withdrawal" trigger

**Question.** What are the top CEX hot wallets (Binance `5tzFki…`, KuCoin `BmFdpr…`, Bitget `A77HEr…`, OKX `is6MTR…`, Coinbase `GJRs4F…`/`2AQdpH…`) and the unlabelled high-rate funders in `funding_sample.json` (`F7p3dF…`, `FncazA…`, `iGdFcQ…`, `5F1seM…`)? Is each one a CEX hot wallet, an instant-swap service or a bridge relayer? For the rule "SOL withdrawal in [a, a+δ] to a wallet with no prior transactions":
- how many wallets match per day?
- what fraction create a pump.fun token within 10 and 60 minutes?
- which per-exchange withdrawal fees produce odd amounts such as 2.955?

*Measured 2026-10-14, 10:33–10:54 UTC, on public RPC (`api.mainnet-beta.solana.com` for signatures, round-robin with `solana-rpc.publicnode.com` for `getTransaction`) and the pump.fun `frontend-api-v3 /coins?creator=` endpoint. Code: `analysis/scripts/gap4/p1.py` and `p2.py`. Data: `data/reports/gap4_cex_fresh.json` (per-wallet stats and 777 recipient rows) and `data/reports/gap4_hotwallet_sample.json.gz` (3,000 decoded txs).*

## TL;DR

1. **Seven of the ten are CEX hot wallets** (behaviour VERIFIED, entity LIKELY): Binance, KuCoin, Bitget, OKX, Coinbase x2 and Bybit (`iGdFcQ…`). The other three are not:
   * `5F1seM…` is an on-ramp payout wallet (MoonPay, LIKELY).
   * `F7p3dF…` is **Relay's** cross-chain solver. The "Mayan Bridge" label in ravexutils is contradicted on-chain.
   * `FncazA…` is a **gas-sponsor / account-cleanup relayer**. It sends exactly 0.05 SOL to wallets that already have history and gets it back. It is not an exchange, swap or bridge, and 0 of 83 recipients were fresh.
2. **Volume of the raw rule.** The 7 CEX wallets send SOL to about **12,550 never-used wallets per day**. 64 % of these come from Binance `5tzFki…` (≈ 8,000/day).
3. **Precision of the raw rule (no amount filter).** **2.2 %** of fresh recipients create a pump.fun coin within 10 min (8/366, 95 % CI 1.1–4.3 %). **3.3 %** do so within 60 min (12/366, CI 1.9–5.6 %).
   * That is **~430 creates/day within 10 min and ~500/day within 60 min**. Daily creates are 46.6k (`onchain-*` docs), so the rule covers about 1 % of all creates.
   * Of the creates we saw at any point during the 1.5–6 h of observation, **50 % came within 10 min and 75 % within 60 min**.
4. **Adding an amount filter of 0.9–15 SOL** raises the 60-min precision to **9.7 %** (11/113, CI 5.5–16.6 %) and keeps 11 of the 12 creators. Fresh recipients below 0.9 SOL: 1 of 249 created a coin, and that one is a serial farm wallet.
   * A 0.9–15 SOL filter gives a watchlist of **≈ 5,000 wallets/day (~210/h)**.
   * With a 60-min TTL that is ~210 live entries. Even with a 24 h TTL it is 5k, well under Helius preconf's `signerInclude` cap of 10k.
5. **Withdrawal fees and odd amounts.** Published SOL withdrawal fees (VERIFIED, public APIs): Binance **0.001**, Bitget **0.001**, HTX **0.001**, KuCoin **0.008**.
   * Binance's fee shows up on-chain: 39 of 336 sampled outflows are a 0.1-multiple minus 0.001 (1.499, 1.999, 2.999…), against 6 exact multiples.
   * OKX and Bybit deliver round amounts exactly, so the fee is charged on top. Coinbase amounts carry 9-decimal noise.
   * **No current fee turns a round number into 2.955.** That would need a 0.045 fee (from 3.0) or a 0.005 fee (from 2.96). 2.955 is the number the rugger typed, not a fee artefact.
   * Fee-shaped values (X.999 at Binance) are the **worst** possible bands: Binance 1.499 alone is ≈ 900 fresh recipients/day with 0 creates.

## 1. Identity of each address

Method: we took signatures for 04:34–09:19 UTC (6 h back from 10:34, minus a 75-min tail). We decoded a seeded random sample of 300 successful txs per address. For each we recorded signer and fee payer, programs, SOL in/out, SPL balances, memos and the recipient's pre-balance. Balances come from `getMultipleAccounts` at 10:54 UTC.

| address | verdict | tx/h (ok) | balance SOL | on-chain evidence | external labels |
|---|---|---|---|---|---|
| `5tzFkiKscXHK5ZXCGbXZxdw7gTjjD1mBwuoFbhUvuAi9` | **CEX hot wallet, Binance** (LIKELY) | 942 | 1,163,470 | 336 distinct recipients in 368 outflows; 59 inbound sources; SPL withdrawals in 115/300 txs; fee fingerprint 0.001 = Binance's published fee | Dune `cex_solana` "Binance 1"; drix list (Solscan tag) "Binance 2" |
| `BmFdpraQhkiDQE6SnfG5omcA1VwzqfXrwtNYBwWTymy6` | **CEX hot wallet, KuCoin** (LIKELY) | 219 | 27,478 | 64 of 92 inflows from one refill wallet `EiMJefb6…`; 155 distinct recipients in 159 outflows | Dune "KuCoin 2", davirain, axiomfilter |
| `A77HErqtfN1hLLpvZ9pCtu66FEtM8BveoaKbbMoZ4RiR` | **CEX hot wallet, Bitget** (LIKELY) | 144 | 99,995 | Many fixed 0.0036 SOL top-ups to wallets with exactly 1 prior tx (gas for its own deposit addresses); fee fingerprint 0.001 | Dune "Bitget 1", mamba, ravex |
| `is6MTRHEgyFLNTfYcuV4QBWLjrZBfmhVNYR6ccgr8KV` | **CEX hot wallet, OKX** (LIKELY) | 255 | 251,770 | SOL-only (0 SPL); 118 distinct inbound sources in 135 inflows (deposit sweeps); round net amounts | mamba, drix, ravex |
| `GJRs4FwHtemZ5ZE9x3FNvJ8TMwitKTh21yxdRPqn7npE` | **CEX withdrawal wallet, Coinbase** (LIKELY) | 150 | 27,034 | Signs 300/300; **0 inflows** in sample (pure payout wallet); 165/204 amounts have 9 decimals; USDC payouts | Dune "Coinbase 3", davirain. **drix calls it "Binance Hot 2", which is wrong.** |
| `2AQdpHJ2JpcEgPiATUXjQxA8QmafFegfQwSLWSprPicm` | **CEX withdrawal wallet, Coinbase** (LIKELY) | 149 | 22,057 | Same shape as GJRs4F: 2 inflows/300, 9-decimal amounts, USDC | Dune "Coinbase 2", mamba. **drix calls it "OKX Hot", which is wrong.** |
| `iGdFcQoyR2MwbXMHQskhmNsqddZ6rinsipHc4TNSdwu` | **CEX hot wallet, Bybit** (LIKELY) | 257 | 194,161 | SOL-only; 206 distinct recipients in 214 outflows; 80 inbound sources; round net amounts | ravex, axiomfilter, DefiLlama Bybit adapter fork |
| `5F1seMKUqSNhv45f6FhB2cFmgJbk8U1avJw7M6TexUq1` | **On-ramp payout wallet, MoonPay** (LIKELY) | 145 | 2,875 | 300/300 txs carry a 32-hex memo (order id); 0 inflows; 135/300 are USDC/USDT payouts | ravex only |
| `F7p3dFrjRTbtRp8FRF6qHLomXbKRBzpvBLjtQcfcgmNe` | **Bridge/intent solver, Relay** (VERIFIED by on-chain program and counterparty match) | 5,027 | 6,769 | Fee payer 296/300; 287/300 carry a memo with a `0x…` 32-byte request id; 243/300 move USDC; 13 call Relay's depository program `99vQwt…`; inflows from Relay vault authority `7uTT8Xi5…` | Chainstack FOMO docs: "Relay's payout solver" ([raw](https://raw.githubusercontent.com/chainstacklabs/fomo-solana-rh-listeners/main/docs/07-addresses.md)). ravex's "Mayan Bridge" is contradicted. |
| `FncazAs6omJJjtLVzquzT9KoyXn6tFixr9kGjr42ktLj` | **Gas-sponsor / account-cleanup relayer**, unknown operator (behaviour VERIFIED, identity UNVERIFIED) | 1,155 | 77 | Every outflow is exactly **0.05 SOL** (143/143). Recipients are existing wallets (median ~50 prior sigs; **0/83 fresh**). They then sign a tx to program `Send9wszHjEiS3hwKcPeSLsPRu5Gb62iCrJEcG4Mq3b` (log `TransferWithWindow`), which closes their token accounts, moves tokens and returns ~0.05 SOL to FncazA. | none found (web search empty; Solscan and Arkham return 403) |

Notes:
* The tx/h figures replace the earlier samples: the 4,752 / 1,095 / 224 / 155 tx/h in `funding_sample.json` are from 1,000-sig heads.
* FncazA counts as "exchange-like" in `funding.py` only because it has more than 2000 txs. It should get a `sponsor` label so chains do not stop at it.
* Public label lists disagree on the Coinbase wallets: the drix repo maps both to Binance and OKX. Dune (hildobby) is right on both counts, judging by behaviour (no inflows, 9-decimal payouts).

## 2. The rule, measured forward from the funder side

Definitions:
* **Fresh recipient:** a SOL `transfer` from the hot wallet to a destination whose pre-balance is 0. We confirmed with `getSignaturesForAddress(dest)` that the funding tx is the destination's **oldest** signature; wallets with 1000 or more sigs are never fresh.
* **Creates:** `frontend-api-v3 /coins?creator=dest`, lag = `created_timestamp − funding blockTime`.
* **Observation window:** funding at 04:34–09:19 UTC, checked at 10:47–10:51 UTC. Every recipient had at least 88 min to create.
* **Per day:** hits / 300 × (successful tx/h × 24). This is a 4.75 h Asia/EU-hours window, so diurnal bias is possible.

| funder | pre-balance 0 | verified fresh | had prior txs | create ≤10 min | ≤60 min | any (≤1.5–6 h) | fresh/day | creates ≤60 min/day |
|---|---|---|---|---|---|---|---|---|
| Binance 5tzF | 147 | 106 | 41 | 5 | 5 | 6 | **7,991** | **377** |
| KuCoin BmFd | 113 | 85 | 28 | 3 | 4 | 6 | 1,489 | 70 |
| Bitget A77H | 83 | 35 | 48 | 0 | 0 | 1 | 404 | 0 |
| OKX is6M | 49 | 39 | 10 | 0 | 0 | 0 | 796 | 0 |
| Coinbase GJRs | 72 | 12 | 60 | 0 | 1 | 1 | 144 | 12 |
| Coinbase 2AQd | 86 | 12 | 74 | 0 | 0 | 0 | 143 | 0 |
| Bybit iGdF | 107 | 77 | 30 | 0 | 2 | 2 | 1,583 | 41 |
| **7 CEX total** | 657 | **366** | 291 | **8** | **12** | 16 | **≈12,550** | **≈500** (≤10 min: ≈430) |
| MoonPay 5F1s | 33 | 15 | 18 | 0 | 0 | 1 | 174 | 0 |
| Relay F7p3 | 4 | 2 | 2 | 0 | 0 | 0 | ~800 (n=2) | 0 |
| FncazA | 83 | 0 | 83 | 0 | 0 | 0 | 0 | 0 |

**Pre-balance 0 alone is not "fresh."** 44 % of the CEX pre-balance-0 recipients had earlier txs:
* Coinbase users re-funding drained wallets (median >100 prior sigs);
* Bitget's 0.0036 SOL gas top-ups to deposit addresses that already received a token (1 prior sig).

Pre-balance is free to read in the transfer itself. The signature check costs one call, but it is needed. A live bot can replace that call with its own account-first-seen index.

**Precision by amount** (CEX fresh recipients, create within 60 min):

| amount band (SOL) | n | fresh/day | ≤10 min | ≤60 min | precision ≤60 min (95 % CI) |
|---|---|---|---|---|---|
| < 0.05 | 38 | 1,428 | 0 | 0 | 0 % (0–9.2) |
| 0.05–0.5 | 177 | 4,944 | 0 | 1 | 0.6 % (0.1–3.1), and that one is serial farm `HBBGaJ…` |
| 0.5–0.9 | 34 | 1,120 | 0 | 0 | 0 % (0–10.2) |
| **0.9–3.1** | 85 | 4,183 | 7 | 8 | **9.4 % (4.8–17.5)** |
| 3.1–15 | 28 | 814 | 1 | 3 | 10.7 % (3.7–27.2) |
| ≥ 15 | 4 | 61 | 0 | 0 | n/a |

**Creators caught.** Funded amount and create lag:
* Binance: 1.974 (403 s), 12.901 (295 s), 2.999 (279 s), 1.194 (211 s), 0.915 (121 s).
* KuCoin: 1.116 (70 s), 1.199 (67 s; 3 coins), 5.098 (837 s), 1.172 (42 s).
* Coinbase: 1.5 (787 s).
* Bybit: 5.105 (3,040 s).

These lags agree with the backward sample in `onchain-funding-patterns.md` §4.3 (fresh exchange-funded creators 24–798 s, median 159 s).

**Outcomes** (pump.fun API `ath_market_cap` in USD at 10:55 UTC, UNVERIFIED as a peak):
* The 11 creators other than the serial farm wallet made 14 coins. 11 of them topped out at **$3.3–4.5k**, which is at or near the curve floor, and one reached $8.9k.
* **2 graduated:** KuCoin-funded `47EHtT…pump` (ATH $84.5k) and Bybit-funded `F7T8WJ…pump` (ATH $103.8k, created 51 min after funding).
* So the funding rule finds launches. It does not pick winners. Selection must come from cluster history and slot-0 shape (Gaps 1–3).

**Cross-check against the backward sample.** In the near-random "recent" arm of `funding_creators.jsonl`, 3 of 100 creators were fresh, directly funded by one of these 7 wallets, and created within 1 h (lags 29, 153 and 798 s). That is 3 % (CI 1–8.5 %), against the forward estimate of 500 / 46.6k ≈ 1.1 % of creates. Both are small n, but consistent.

## 3. Amount bands [a, a+δ]

* **Average density.** Across the 7 CEX wallets, the 0.9–3.1 SOL range carries **≈ 19 fresh recipients/day per 0.01 SOL band**, or ≈ 1.9/day per 0.001 band (the guru's 3-decimal match). Binance contributes about 64 % of that.
* **Fee-shaped values are hot spots, not quiet bands.** From the Binance sample:
  * 1.499: 12 hits, ≈ 900/day. These arrive in pairs at identical timestamps roughly hourly (…383 s), i.e. scripted payouts, then idle or a single forward. **0 creates.**
  * 1.999: 4 hits; the earlier siblings scan found 4 in 17 min.
  * 2.999: 3 hits, ≈ 226/day, of which 1 creator.
  * 0.999: 3 hits.
* **The guru's 2.955 band.** One hit, Binance → `33hx2o…` with **2.955041 SOL**. The wallet is fresh (9 txs) and created no coin. A 3-decimal band also catches 6-decimal noise.
* **Practical rule.** Put a band only on a value proven by ≥ 2 creates from the same funder (a "cluster amount"). Never put one on a `round − fee` value. For CEX-funded launches the amount mostly does not repeat (6-decimal noise, §4.4 of the funding doc). The robust key is **fresh + CEX + 0.9–15 SOL**, then decide at the create.

## 4. Withdrawal fees and odd amounts

| exchange | SOL fee | source | on-chain fingerprint |
|---|---|---|---|
| Binance | **0.001** (min 0.01) | VERIFIED: `https://www.binance.com/bapi/capital/v1/public/capital/getNetworkCoinAll` (network SOL, `withdrawFee` 0.001) | 39/336 outflows = k·0.1 − 0.001 (1.499, 1.999, 2.999, 0.999, 4.999); 6 exact multiples |
| Bitget | **0.001** (min 0.094) | VERIFIED: `https://api.bitget.com/api/v2/spot/public/coins?coin=SOL` | 7/155 = k·0.1 − 0.001 |
| HTX | **0.001** fixed | VERIFIED: `https://api.huobi.pro/v2/reference/currencies?currency=sol` | not sampled |
| KuCoin | **0.008** (min 0.016) | VERIFIED: `https://api.kucoin.com/api/v3/currencies/SOL` (`withdrawalMinFee`) | weak: 3/155 = k·0.1 − 0.008 |
| OKX | unknown (API needs key) | UNVERIFIED | Round net amounts arrive exactly (1.0, 2.0, 5.5, 15.0), so the fee is charged on top (LIKELY) |
| Bybit | unknown (CloudFront geo-block) | UNVERIFIED | 15/214 exact 0.1-multiples; fee on top (LIKELY) |
| Coinbase | not published in API (`/currencies/SOL`: min withdrawal 0.005) | VERIFIED min, fee unknown | 165/204 amounts have 9 decimals (fiat-denominated sends); no fingerprint |
| Gate | API shows min 0.1, no fee | partial | not sampled |

**2.955 by inversion.** Adding today's fees gives 2.956 (0.001) or 2.963 (0.008), so 2.955 is not round-minus-fee anywhere. A "3 − fee" reading needs 0.045, and "2.96 − fee" needs 0.005; neither fee exists in 2026. The guru's other value, 2.995 = 3 − 0.005, would fit an older 0.005 fee (UNVERIFIED). Either way, these numbers were one operator's chosen constant, and the operator can change it at will. Today the fee-shaped residues mark **retail**: X.999 at Binance/Bitget/HTX, X.992 at KuCoin, exact round numbers at OKX/Bybit.

## 5. Reconciling the conflicting files

| source | claim | why it differs |
|---|---|---|
| skeptic #26 | 0/11 creators with a labelled CEX within 3 hops | Early 11-creator run with the first, small label set (no Bybit `iGdF`, no ravex) |
| rugger-ecosystem | 10/60 direct CEX | Different 60-creator sample (rugger-candidate selection) |
| critique C6 | 24 direct / 29 within 3 hops (n=126) | Older version of `funding_sample.json` |
| `funding_sample.json` now | **27 direct / 40 within 3 hops**; bundled arm 14/31 vs recent 9/68 | Re-trace after adding Bybit, MoonPay, FixedFloat and Relay labels |
| this gap (forward) | 3.3 % of fresh CEX recipients create ≤60 min; ≈ 1 % of all creates | Funder-side measure; independent of creator sampling |

**The 2,127 s median lag.** The funding-to-create median of 2,127 s (and 50/126 > 24 h) is a mix. It includes non-fresh serial wallets whose *main* funding is days old. Among fresh creators the median is 160 s. In the forward data, 12 of the 16 creates that happened within the observation window came within 60 min. Wallets funded and then idle for more than 24 h exist: 37 of 106 Binance fresh recipients had no further tx after 1.5–6 h. They cannot be caught with a short TTL, but they are cheap to keep on a 24 h, amount-filtered list.

## 6. What to build

1. **Watch outflows from the 7 CEX wallets, MoonPay and FixedFloat over Geyser.** Relay `F7p3dF…` is optional: 4/300 txs pay SOL to new wallets.
   * Read `dst_pre == 0` from the transfer. Confirm first-seen from our own account index rather than RPC.
   * Drop amounts < 0.9 SOL. Drop `round − fee` amounts unless the operator is known.
   * Expected load: ≈ 5k entries/day, ~210/h.
2. **Two TTLs:**
   * 60 min "hot", pre-built buy tx and `signerInclude` subscription. This catches about 75 % of the CEX-funded creates seen.
   * 24 h "warm", subscription only, for the slow tail.
3. **The funding rule is a pre-arm, not a buy signal.** A fresh wallet that never creates costs nothing. Precision only sizes the watchlist (90 % of entries never create). False snipes are decided at the create by cluster history and slot-0 bundle shape: 11 of the 14 caught coins died at the floor.
4. **Label hygiene:**
   * Add FncazA (`sponsor`) and Relay (`bridge`, fix the ravex "Mayan").
   * Fix drix's Coinbase→Binance/OKX mislabels.
   * Treat the Bitget 0.0036 SOL top-ups as internal deposit-address gas.

## 7. Limitations

* One 4.75 h window, 300 txs per wallet. The fresh-recipient per-day numbers carry about ±20 % sampling error. The create counts (8 and 12) carry wide CIs, shown above.
* No weekend or US-hours coverage.
* Recipients were observed for 88 min to 6.3 h, so "any" undercounts long-lag creates.
* Only these 10 funders were measured. There are 24 hot CEX wallets in `labels_verified.json`; Binance has others besides `5tzFki…`.
* The coins API only sees pump.fun creates. Creates on other launchpads are missed.
* Public RPC returned a transient `413 data allowance` once; failed decodes were retried, and all 3,000 txs decoded.
