# Completeness critique of the claims research (2026-10-14)

Scope: the four claims files (`pumpfun-mechanics-2026.md`, `launchpad-landscape-2026.md`, `rugger-ecosystem-2026.md`, `skeptic-factcheck.md`). I also read the four `speed-*.md` files, because several hot-path facts conflict across the two groups. I recounted a few numbers from local data:

- `data/tokens_light.jsonl` (D1): 1,496 decoded creates over 45.8 h.
- `data/reports/funding_sample.json`: generated 08:34 UTC, 180 creators.
- `data/cache/create_index.json`.

Recounts are marked **[critic recount]**. Scripts were inline `python3 -I` one-offs and are not committed.

Bottom line: the protocol mechanics are well verified, so the remaining builder risk is a handful of hot-path details. The **edge itself is still unmeasured**. No file measures a hit rate for a specific operator, the competition on tracked operators, or the loss on misses under real exit rules. The skeptic's arithmetic shows the strategy needs all three.

---

## 1. Contradictions between files

| # | Topic | What the files say | Local data / resolution |
|---|---|---|---|
| C1 | **Creates per day** | launchpad ~21.7k (API, 07-08 UTC) and "matches the guru's 15-20k". mechanics ~31.6k (mint-authority signatures, 4.2 h overnight) and "above the guru". rugger ~46.6k (mint-authority signatures, 48 h). speed-data-feed ~24k (0.28/s). speed-tx-landing ~18.7k (87 in 6.5 min). | A 2.5× spread. `create_index.json` stores **raw mint-authority signatures, not decoded creates**, and speed-data-feed found a PumpSwap tx among 19 mint-authority signatures. So 31.6k and 46.6k are upper bounds. API counts may drop `hidden` or `is_banned` coins, since those fields exist. **Fix:** count decoded `create`/`create_v2` instructions per hour for 7 days. |
| C2 | **Non-SOL quote share** | mechanics ~13% (n=74). launchpad ~14%, but it counts 4.3% `null` as non-SOL (probably SOL). skeptic gives only "57 non-SOL creates with creator fee ≠ 30 bps". | **[critic recount]** D1: **281/1,496 = 18.8% non-SOL**. Top quotes: `XsoCS1…` 52, PUMP 33, `SPCX…` 18, `Xsc9q…` 15, USDC 14. 14 non-SOL dev buys show **0 bps protocol fee (0/30)**, so the fee model differs by quote in more ways than "creator fee up to 3%". |
| C3 | **Mayhem share** | mechanics ~36% (n=74). skeptic 22%. | **[critic recount]** D1 334/1,496 = **22.3%**. mechanics' small sample is off. |
| C4 | **Token program / create path** | launchpad: "all 770 mints Token-2022; `create_v2` effectively the only path". speed-data-feed: 100% `create_v2` (n=19). | **[critic recount]** D1 has **11/1,496 (0.7%) SPL-Token (`Tokenkeg…`) mints created Oct 12-14**, so legacy `create` is still in use. Its layout differs (user #7, token_program #9). Parse it or skip it explicitly. |
| C5 | **Dev-sell detection** | mechanics: dev sell = `TradeEvent.user == creator` (VERIFIED field). rugger: 45/299 creates have creator ≠ buyer, so key on the signer. | **[critic recount]** D1: **282/1,496 = 18.9%** have `CreateEvent.creator` ≠ create signer/fee payer. 179 of them are SOL-quoted; 43 go through launcher `J7pour…` and 6 through `rapidV…`. The mechanics rule misses about 1 in 5 dev sells. Track the signer, the `creator` arg and the bundle wallets. |
| C6 | **CEX funding share** | skeptic #26: "0/11 creators had a labelled CEX within 3 hops", so the CEX pattern "may be rarer". rugger: 10/60 direct CEX, 12 CEX at origin. | `funding_sample.json` (now n=180, 126 resolved) has **24 direct CEX and 29 within 3 hops**. The **bundled arm has 14 of 31 resolved direct from a CEX, against 6 of 68 for random recent creates**. The skeptic number is stale. CEX funding is concentrated in bundled launches, which supports the guru's model. 0/126 used 2.955/2.995. |
| C7 | **Slot-0 sniper prevalence** | Pine (cited by launchpad, rugger, skeptic): >50% of tokens sniped in the creation block. speed-tx-landing: 23/87 = **26%** drew any non-creator same-slot buy. skeptic D2: 103/349 = 30% have ≥3 slot-0 buyers. | D2 is biased: 232/349 are prior coins of sampled creators, and it counts bundle wallets as buyers. rugger cites `docs/research/onchain-firstslot-stats.md` (workflow C) for the bundle-vs-sniper split, but **that file does not exist**. Unresolved. |
| C8 | **PumpSwap fee on exit** | launchpad: "0.25% (0.20 LP + 0.05 protocol) per README, plus creator fee". | mechanics decoded FeeConfig `5PHirr…`. Canonical pools charge **1.25% below 420 SOL mcap and 1.20% at or above it**, stepping down to 0.30% only at ≥98k SOL. Non-canonical pools are flat 0.30%. Graduates exit at ~1.2-1.25%, not 0.25%. |
| C9 | Curve fee tiers | launchpad: curve fee "set by market-cap tier, not decoded [U]" (open item). | mechanics decoded it: flat 95+30 bps, no curve tiers. The launchpad open item is stale. Read FeeConfig at runtime anyway. |
| C10 | **Graduation / migrate atomicity** | launchpad: "graduation is atomic with the last buy" and "no migration fee". speed-tx-landing: after completion, "no buys or sells … until `migrate` runs (permissionless)". mechanics and launchpad's own table: `pool_migration_fee` = 0.015 SOL. | This depends on the variant: v3 synthetic migration is atomic, while legacy/v2 completions are not. Nobody measured how many slots pass between `CompleteEvent` and pool creation, i.e. how long a held position cannot be sold. |
| C11 | Create-tx CU | mechanics: `create_v2` 98-166k, create+buy 151-166k (≈12 samples). rugger: p50 211k, p90 312k (n=299). speed-data-feed: 113-330k. | The mechanics range is too narrow. A related builder conflict: pump docs recommend a **~100k static CU limit** (bump search varies), but speed-tx-landing recommends **62-65k**. Too tight a limit fails the tx. |
| C12 | **Tx v1** | speed-data-feed: SIMD-0385, "no address lookup tables" in v1. speed-tx-landing: SIMD-**0296**, "use v1" **and** "use an ALT for static accounts". speed-reference-impls: Helius README says v1 is "not activated on any cluster". | On-chain, 41-53% of creates and buys are v1, so v1 is live. If v1 really has no ALTs, the speed-tx-landing recipe is self-contradictory. |
| C13 | Dev buy inside create tx | mechanics 84% (63/75), skeptic 83.1% (1,243/1,496), rugger 77.6% (n=299). | Minor. Use the D1 number. |
| C14 | Holder-reward share | mechanics 18% (n=74), launchpad 12% (n=770). | D1 coin records lack the field. Minor. |
| C15 | Jito tip floor | rugger p99 0.00227 SOL (08:12). speed-tx-landing p99 0.00087 (07:56). skeptic median 6e-6. | The floor changes minute to minute and covers all bundles. The rugger "TradeWiz tip = 22× p99" comparison depends on the snapshot. For contested launches, use the observed block-0 winner bids: p50 0.004 SOL, p90 0.049 SOL. |
| C16 | Alpenglow date | speed-data-feed: epoch 1064, ~Oct 21 (VERIFIED). speed-tx-landing: "expected with 4.3, not confirmed". | Use the speed-data-feed date. It lands one week from now and changes processed-commitment and bank semantics. |
| C17 | `complete=true` with ATH < $6k | launchpad: 6 anomalous coins [U]; exclude them from backtests. | **[critic recount] Resolved: all 6 in D1 are mayhem-mode, SOL-quoted coins.** The real SOL-quoted graduation rate in D1 is 31/1,215 = **2.6%** (37/1,215 = 3.0% with mayhem pseudo-completions). This answers rugger's "Oct 2026 graduation rate UNVERIFIED", with a 45.8 h caveat. |
| C18 | Floor and migration in USD | $2,992 / $43,964 at $107 (mechanics). $3,014 / $44.3k at $107.80 (skeptic). | Consistent. Store thresholds in SOL (27.959 / 410.88). |

## 2. UNVERIFIED (or weak) claims that matter for the bot or the edge

| Claim | Where | Why it matters | How to settle |
|---|---|---|---|
| Harmonic leaders release shreds only after a ~300 ms block auction | speed-tx-landing (LIKELY/UNVERIFIED) | If true, block-0 reaction is impossible on those leaders. Their slot share is unknown. | Per-leader shred-arrival offsets from a shred feed; Harmonic validator list |
| Operator lifetime ~1 week (guru), with 3-12 month members contradicting it | skeptic #27 | Sets whether ~100-170 snipes per operator can ever accumulate | Operator clustering over ≥30 days (Gap 1) |
| "87% of snipes profitable" (Pine, Apr 2025) | rugger 2.2 | These are **deployer-funded self-snipes**, not third-party snipers. It is easy to misread as evidence for our edge. | Do not use it as an edge prior |
| F-Project "front-run sell" at ≥50% supply transferred; "track-buy range" anti-farm | rugger 5-6 | Exit and entry design copies them | Gap 3 measures whether the signals have lead time |
| Unlabelled launcher programs `6Vo3245…`, `J7pour…`, `FAdo9NC…`, `AKVwzx…`, `rapidV…` | rugger 2.3 | `J7pour…` is in 43 of the 282 creator≠signer creates, so launchers change who "the dev" is | Program IDL/website lookup, Solscan labels |
| Unknown tip recipients `rapidX…`, `LandX…`, `moon…` | speed-tx-landing | They may be the private senders that rival snipers use | Program/account lookup |
| Jito block engine forwards bundles cross-region | speed-geography | Decides whether one FRA box can bid on TYO/NY leaders through Jito | Test sends |
| Legacy `buy` remaining accounts are mandatory | mechanics | Only if we use the legacy `buy` (v3 is preferred) | Simulate |
| Block-0 graduation frequency | launchpad | Exit routing for winners | Count `CompleteEvent` slot = create slot over 7 days |
| Mixer-funded ruggers ~2%; F-Project €1,000 price; TradeWiz 0.05 SOL tip; 56 s hold | rugger, skeptic | Low impact on the build | Skip |

## 3. Missing items

**For the bot builder**
- **Worst-case CU** for our exact tx: `buy_v3` + Token-2022 ATA idempotent create + durable-nonce advance + tip, across many mints. Nobody has simulated this combination. See C11.
- **`partial_fill` / `track_volume` semantics** on `buy_v3`. Does `partial_fill=true` turn a slippage failure into a partial fill at `max_sol_cost`? 46% of reactive block-0 buys failed on slippage, so this matters.
- **Shred-level classifiers** (no logs available):
  - The SOL vs non-SOL rule from the `create_v2` remaining-account count. mechanics lists 3 tail accounts; sol-shred-sdk lists 4 (adds `quote_control`).
  - The byte offset of `is_mayhem_mode` after the variable-length strings.
  - The legacy `create` layout (C4).
- **Mayhem coins on v3**: which buyback-recipient set they need (normal vs "reserved"). mechanics says mayhem uses the reserved set but does not say so for v3.
- **Sell path details**:
  - `sell_v3` accounts are given only as "same 17".
  - PumpSwap `sell_v2` (17 accounts) is not enumerated.
  - The post-`CompleteEvent` gap before the pool exists is unmeasured (C10).
- **Rent in the cost model**: Token-2022 ATA rent (~0.002 SOL) and reclaiming it on close. The skeptic lists "rent unrecovered" with no number. At 0.5 SOL that is ~0.4% per trade.
- **Exit-trigger plumbing**: how to watch Token-2022 transfers on a mint at shred or processed level when the bundle wallets are not known before the create.
- **CEX labels**: only 20 addresses are labelled. In `funding_sample.json`, unlabelled "exchange-like" funders are the largest origin class (55/126). The top unlabelled funders are `F7p3dF…` (4,752 tx/h), `FncazA…` (1,095), `iGdFcQ…` (224) and `5F1seM…` (155). No doc gives SOL withdrawal fees per exchange, which are needed to read "odd" amounts.
- **Historical data source**: pump `/coins` paging dies past ~1,000 rows, `/trades` is per mint at 60 req/min, and the create index covers 48 h. No file evaluates a 30-day+ source.

**For the strategist**
- **Hit rate per operator, out of sample.** None exists. Base rates from a realistic end-of-slot-0 entry are 6.8% for random coins and 12.6% for bundled ones, against a break-even of **43-48% at TP +100%** with the natural stop.
- **Competition on tracked operators.** None exists. F-Project's 1,731 members probably track the same operators, so random-coin competition (26%) underestimates it.
- **Loss on misses under real exit rules.** None exists. The EV model assumes L = 63% or 80%.
- **Bundle-shape distribution from an unbiased sample.** Only D2 exists (biased, n=103: median 7.5 SOL, 21.4% supply). The guru's 4×10 SOL is a tail case.
- **Qualifying launches per day.** Needed for capacity and the daily EV ceiling.
- **Funding-to-create lag is measured but not reported.** `funding_sample.json` has a median of 2,127 s; 23/126 within 60 s, 50/126 within 10 min, 50/126 later than 24 h. The watchlist design depends on it.

## 4. Ranked gaps (one researcher each)

1. **Do operators with an edge exist?** Over ≥14 days, cluster creators into operators by shared funder (≤3 hops), CEX amount band, bundle shape, launcher and metadata host. How many operators launch ≥5 tokens? For each, what share of launches reach +100%, +150% and +200% from a realistic entry (after the creator's bundle and the median third-party slot-0 buyer)? Score walk-forward (discover on days 1..t, test on t+1..t+7), with Wilson CIs and qualifying launches per day.
2. **What does entry really cost on tracked operators' launches?** Compare tracked-operator launches with random ones over ≥24 h and ≥3,000 creates. Count independent reactive buyers in slot 0 and slot+1, after removing the creator's bundle (adjacent tx index, shared Jito bundle or tip, shared funder). Report their winning bids and index gaps, the slippage-failure share, and the fill drift of the 1st, 3rd and 5th reactive buyer above the post-bundle price. Also report the share of creates on leaders or slot positions where block-0 reaction is impossible (Harmonic, BAM, last ~15% of the block). This also delivers the missing `onchain-firstslot-stats.md`.
3. **How do rugs end, and can we exit first?** Measure the bundle-shape distribution and the dump mechanism: dev/signer sell, multi-wallet same-slot sell, or consolidate-then-sell, with timing from the create. For each exit signal, report precision, recall, lead time before a 50% drop, and the realized exit price for a 0.5 SOL sell while other holders exit in the same slot.
4. **How precise is the CEX-funding trigger?** Do this for the top CEX hot wallets and the unlabelled high-rate funders. First identify what each funder is: a CEX hot wallet, an instant-swap service, or a bridge. Then, for the rule "withdrawal in [a, a+δ] to a wallet with no prior tx", report how many wallets match per day, what share create a pump.fun token within 10 and 60 minutes, and which per-exchange withdrawal fees produce odd amounts.
5. **What is the exact minimal snipe and exit tx set, proven by simulation on mainnet state?** Cover:
   - CU worst case
   - v1 + ALT legality and sender acceptance
   - `partial_fill`
   - mayhem buyback recipient on v3
   - shred-level SOL/mayhem/legacy-create classifiers
   - the gap between completion and migrate, and the PumpSwap `sell_v2` layout and fee at ~411 SOL
6. **Which data source gives ≥30 days of history** with every create plus slot s..s+3 trades, in-slot tx index, signer, fee payer, compute budget, tips and funding edges? Report cost, freshness and whether it decodes `create_v2`, `buy_v3` and v1 transactions. Candidates: Dune, BigQuery, Old Faithful, Helius archival, Flipside/Allium, or our own `getBlock` crawler. Pre-Oct-9 data (400 ms slots) and pre-Sep-15 data (no v1) must be flagged as a different regime.
