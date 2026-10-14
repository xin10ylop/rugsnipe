# Skeptic fact-check: the "rugger sniping" pitch (FOMO / F-Project)

Date: 2026-10-14. SOL = $107.80 (`GET https://frontend-api-v3.pump.fun/sol-price`).
Tags: **VERIFIED** = primary source or our own on-chain/API measurement. **LIKELY** = secondary source. **UNVERIFIED** = no evidence either way.
Transcript refs are `L<n>` lines of `data/transcripts_flat.txt`.

Our own datasets:
- **D1**: `data/tokens_light.jsonl`. 1,496 pump.fun creates over 45.8 h (slots ~456.2M-456.8M), decoded from the create transactions, joined to the pump.fun API.
- **D2**: per-trade histories (`data/cache/pumpapi/*/trades_*`, pump.fun swap API). 349 SOL-quoted coins with complete history and an entry below 300 SOL market cap. 117 of them are a random sample. The other 232 are prior coins of sampled creators, which biases them toward repeat creators.
- **D3**: 59 `getTransaction` calls on `solana-rpc.publicnode.com` for buyers in slot 0 and slots 1-3 of 14 random bundled launches.
- Scripts are in the session scratchpad (not committed). Re-run before relying on any number.

## 1. Claim-by-claim verdicts

| # | Claim (ref) | Verdict | Evidence / correct number |
|---|---|---|---|
| 1 | Max supply "1,07 milliard" (L65) | **FALSE** | VERIFIED: the mint `Dqu5uh…pump` has supply 1e15 base units at 6 decimals = **1.000B**, with mint and freeze authority both null. 1.073B is the curve's *virtual* token reserve. 793.1M is sellable on the curve. Mayhem coins have 2B. So his "615M of 1.07B" (L211) is really 61.5%. |
| 2 | Liquidity is protocol-provided; you can always sell (L77-84) | **TRUE on the curve only** | VERIFIED (curve invariant, `pumpfun-mechanics-2026.md` §4). After migration to PumpSwap the price can go below the curve floor (`launchpad-landscape-2026.md`). |
| 3 | Floor ≈ "$2,500" market cap (L80) | **PARTLY** | VERIFIED: the floor is 30 SOL virtual → **27.959 SOL ≈ $3,014**. $2,500 implies SOL ≈ $89. |
| 4 | You can never lose 100%; max ≈ -80/-90% (L84, L176) | **PARTLY** | The floor loss is set by your entry: L = 1 − (30/V_entry)², plus 2.5% fees. **-63% ↔ entry market cap 75 SOL ($8.1k). -80% ↔ 140 SOL ($15k).** His "first candle ≤ $15k" rule is exactly the -80% line. The loss is unbounded after migration. |
| 5 | "1% fee" on buy and sell (L146) | **FALSE** | VERIFIED: FeeConfig `8Wf5Ti…` charges **1.25% per side** (95 bps protocol + 30 bps creator). D1: all 1,084 SOL-quoted dev buys carry 95/30 bps. 57 non-SOL-quoted creates carry creator fees of 50-300 bps, i.e. up to **3.95% per side**. |
| 6 | Migration ≈ "$35,000" to "Raydium" (L89) | **FALSE (stale)** | VERIFIED: migration happens at 85.005 SOL real / 115.005 virtual = **410.88 SOL ≈ $44.3k**. Coins go to **PumpSwap** (`pAMMBay6…`), which pump.fun has used since 2025-03-20. |
| 7 | 15-20k tokens/day (L13) | **PARTLY (stale)** | VERIFIED: **~21.7k-31.6k/day** (on-chain mint-authority signatures over 4.2 h, and API paging; `pumpfun-mechanics-2026.md`, `launchpad-landscape-2026.md`). His figure fits late 2025. |
| 8 | 90% of Solana memecoins are born on pump.fun (L63) | **PARTLY** | VERIFIED (our counts): about **81% of launchpad creations** and 72% of 30-day launchpad volume. The second venue is Meteora DBC (~4.8k/day). |
| 9 | Pump.fun made > $1B revenue in 2 years (L86) | **TRUE** | LIKELY: $1.08B by Mar 2026 per DefiLlama ([The Block](https://www.theblock.co/post/393358/pump-fun-becomes-solanas-first-1b-revenue-platform)). |
| 10 | Creating a token costs ~$2 (L63) | **PARTLY** | VERIFIED: a create-only transaction costs **0.0057-0.0096 SOL ≈ $0.61-1.03**, excluding priority fee (12 transactions). So a rug's fixed cost is trivial. |
| 11 | Average hold time is 56 s, "verifiable" (L23-24) | **PARTLY / no source** | LIKELY: Galaxy put the median Solana memecoin hold at ~100 s in Oct 2025, and a Dune dashboard showed a median of ~62 s (low of 44 s) in Mar 2026 ([Cointelegraph](https://cointelegraph.com/news/memecoins-attract-users-platforms-reap-profits-report)). The order of magnitude is right. We found no source for "56 s average". |
| 12 | A block is 400 ms (L46-48) | **FALSE (now)** | VERIFIED: slots are **200 ms** since epoch 1053 (2026-10-09) ([solana.com](https://solana.com/upgrades/reduced-slot-times)). `getRecentPerformanceSamples` today gave **211-221 ms/slot**. "Block 0" is now a 200 ms target. A leader still holds 4 consecutive slots. |
| 13 | Buying in block 0 is feasible | **TRUE, but narrower** | VERIFIED (D2): 30% of launches have ≥3 distinct buyers in the creation slot (103/349), so same-slot entry happens. The window halved on Oct 9. Alpenglow (~2026-10-21) changes bank semantics (`speed-data-feed.md`). |
| 14 | "Nobody can buy before the dev" (L149) | **PARTLY** | VERIFIED (D1): only **83.1% (1,243/1,496)** of creates include a dev buy in the create tx. When they don't, a sniper can be first. In bundled launches you are structurally behind the **whole Jito bundle** (dev + 4 wallets), not just the dev. |
| 15 | ShredStream shows the create "before it happens" (L397) | **PARTLY / outdated** | Shreds appear *after* the leader executes the transaction. You see it before confirmation, but never before execution. You can still land in the same slot or leader window. **Jito ShredStream was shut down on 2026-09-05** ([docs.jito.wtf](https://docs.jito.wtf/lowlatencytxnfeed/), VERIFIED). Its successor is DoubleZero Edge. |
| 16 | 6 servers (NY, LA, AMS, FRA, Tokyo + HQ), €10-15k/month (L319, L434) | **UNVERIFIABLE cost; topology half-right** | VERIFIED leader-slot shares for the current epoch: **Frankfurt 32.5%, Amsterdam 18.9%, London 8.7%, Tokyo 5.7%**, NY 2.5%, LA 2.1%. LA and NY are low-value sites, and London is missing. Two EU boxes cover over 50% of leader slots. |
| 17 | France → Brazil ≈ 600 ms, so you miss by a block (L318-319) | **FALSE magnitude** | LIKELY: Paris–São Paulo RTT is ~190-200 ms (one-way ~100 ms). The conclusion (you can miss a 200 ms slot) still holds, but Brazil hosts a negligible share of leader slots. |
| 18 | "Hardcore mode" outbids other snipers' fees (L302) | **PARTLY** | Ordering inside a block comes from Jito's sealed-bid bundle auction (98.7% of stake runs Jito) plus the scheduler's priority-fee-per-CU ordering. You cannot see a rival's bid before it lands, so "outbidding" means guessing from past snipes. The 0.025 SOL cap is plausible. |
| 19 | Optimised buy uses 60-70k CU vs 115k unoptimised (L308) | **TRUE-ish** | VERIFIED (D3, n=48 slot-0 buys): CU **p10 53.5k, p25 60.9k, median 92k, p75 156k**. Lean transactions do hit ~60k. |
| 20 | TradeWiz maxes the sender tip at 0.05 SOL (L307) | **UNVERIFIED** | His own comparison video is the only source. |
| 21 | "A Solana tx will never cost more than $1" (L43) | **FALSE** | His own hardcore cap (0.025 SOL ≈ $2.7) and the TradeWiz tip (0.05 SOL ≈ $5.4) contradict it. D3: slot-0 buyer overhead (fee + tips + rent) is **median 0.0036, p75 0.0195, p90 0.035 SOL ($3.8)**. Uncontested transactions are cheap: the Jito landed-tip median is 6e-6 SOL ([tip_floor](https://bundles.jito.wtf/api/v1/bundles/tip_floor)). |
| 22 | Public bots are always slower than private infra; his bot was first 5/5 (L295-300) | **UNVERIFIABLE** | It was a self-run marketing test with n=5. Nothing structural stops a public bot from colocating. It is true that one shared tx pipeline cannot be first for every user at once. |
| 23 | Bots built with Claude are slower than Maestro (L310) | **UNVERIFIABLE, self-serving** | Speed comes from the data source, region and sender path, not from the language model used to write the code. |
| 24 | The rugger owns ~45% of supply via 4×10 SOL bundles (L114-115, L211) | **PARTLY** | VERIFIED (D2, 103 launches with ≥3 slot-0 buyers): net slot-0 supply is **median 21.4%, p75 27.8%, p90 37.2%** of 1B, and slot-0 SOL is median 7.5 SOL (p90 15.9). 45% may be reached with later buys. The 4×10 SOL pattern is a tail case. |
| 25 | Rugging is "very, very expensive" (L110, L235) | **PARTLY / overstated** | The create costs < $1. Wash volume costs 2.5% per round trip, but the creator gets back 0.30% of *all* volume (creator vault). His claim of "1% CEX deposit + 1% withdrawal" is wrong: deposits are free and SOL withdrawals carry a flat small fee (LIKELY). The real cost is the pump capital put at risk. |
| 26 | Exchange-funded wallets are untraceable except by the fixed amount (L235-237) | **PARTLY** | Timing, fee payer, fresh-wallet behaviour (create within minutes of funding) and the bundle signature are also usable. In our small funding sample, **0/11 creators had a labelled CEX within 3 hops** (`data/reports/funding_sample.json`). The "CEX 10/10" pattern may be rarer than he implies. Needs n ≫ 11. |
| 27 | A rugger lasts ~1 week (L271) | **UNVERIFIABLE; self-contradictory** | He also says members have kept the same rugger for 3-12 months (L274). Measure it: operator half-life is a key validation metric (§4). |
| 28 | Good ruggers hit +100% from 3/4 of the first candle on ≥3/10 tokens (L217, L234) | **Unproven, look-ahead** | VERIFIED base rate (D2, entry at the end of slot 0, target ≥2.05× spot): **random coins 8/117 = 6.8% (95% CI 3.5-12.9%)**, all coins 9.5%, bundled coins 12.6%. His "volume ≥ $20k" filter lifts this to 73% (8/11), but volume is only known *after* the pump, so that is **outcome leakage**. For 3/10 the Wilson CI is **10.8-60.3%**, so 10 tokens cannot certify anything. |
| 29 | Natural SL ≈ -80% (L176) and "-63%" in his Claude simulation (L512) | **TRUE as arithmetic** | See #4. But it contradicts #30. |
| 30 | Win rate ≥ 33% with TP:SL ≥ 3:1 is profitable; his win rate is 25-40% (L227, L245) | **FALSE as stated** | His 3:1 example assumes a **-50% SL** ("trois fois moins 50", L227). TP +100% against the natural -63/-80% is **1.6:1 / 1.25:1**. The break-even hit rate is **43-48%** (§2). |
| 31 | "Farming": ruggers make a big first candle and dump on snipers (L453, L467) | **PLAUSIBLE, observed** | VERIFIED (D2): **8/103 bundled launches had sells inside slot 0** and **28/103 had a ≥3 SOL sell within ~5 s**, against 2/246 and 10/246 for unbundled launches. |
| 32 | He made ~$2M in 3 years and ~$60k/month (L425, L433) | **UNVERIFIABLE** | No wallets disclosed. His income comes from selling the bot, which is the incentive behind the pitch. |
| 33 | Member results: Pacino $25k best month, Noah $10k in 3 months (L340, L347) | **UNVERIFIABLE; survivorship** | These are selected interviews. He admits many members quit (~50% within 30 days, per the digest). No denominator is given. |
| 34 | Pump.fun is "safe": no honeypot, Token-2022 tricks impossible | **TRUE on pump.fun** | VERIFIED: a pump.fun Token-2022 mint has only the `metadataPointer` and `tokenMetadata` extensions, with mint and freeze authority null. This does **not** hold on Meteora DBC or Raydium LaunchLab (transfer hooks and transfer fees are allowed; `launchpad-landscape-2026.md`). |

## 2. Economics: is the stated edge positive?

**Model** (`ev.py`). Snipe size X = 0.5 SOL. Pump fee 1.25% per side.
- *Win*: exit at (1+TP)× spot × (1 − exit slippage) × (1 − fee) − sell overhead.
- *Miss*: exit at (1 − L) × (1 − fee) − sell overhead.
- Buy cost = X(1+fee) + buy overhead.
- Failed attempts pay half the buy overhead (priority fee without a tip).

| Cost scenario | Buy priority + tip | Sell overhead | Exit slippage | Failed attempts |
|---|---|---|---|---|
| low | 0.002 | 0.0005 | 1% | 0% |
| base (≈ D3 p75) | 0.010 | 0.002 | 2% | 10% |
| high (hardcore cap) | 0.030 | 0.005 | 4% | 20% |

**Break-even hit rate**

| | TP +100% | TP +150% | TP +200% |
|---|---|---|---|
| L = 63%: low / base / high | 41.0 / 42.8 / 47.3% | 31.4 / 32.7 / 36.1% | 25.4 / 26.5 / 29.2% |
| L = 80%: low / base / high | 46.7 / 48.3 / 52.5% | 36.5 / 37.8 / 41.1% | 30.0 / 31.0 / 33.7% |

**EV per snipe, base costs** (SOL per 0.5 SOL snipe; % of stake; per day at 10 snipes, in USD at $107)

| p | L | TP +100% | TP +150% | TP +200% |
|---|---|---|---|---|
| 25% | 63% | -0.140 (-28%), -$150/d | -0.079, -$85/d | -0.019, -$20/d |
| 33% | 63% | -0.077 (-15%), -$82/d | +0.003, +$3/d | +0.083 (+17%), +$88/d |
| 40% | 63% | -0.022, -$24/d | +0.075, +$80/d | +0.172 (+34%), +$183/d |
| 33% | 80% | **-0.133 (-27%), -$143/d** | -0.053, -$57/d | +0.026, +$28/d |
| 40% | 80% | -0.073, -$78/d | +0.024, +$26/d | +0.121, +$130/d |

The bolded cell is his headline rule: 33% hits, TP +100%, natural SL -80%. Under base costs it **loses about 27% of stake per snipe**.

What this means:
- His headline rule (3/10 hits, TP +100%, natural SL) loses money under every cost scenario we modelled. It only works if the true hit rate at +100% is ≥ 43-48%, or if wins are larger (TP 150-200%) at the same hit rate. His own Claude simulation (4/10 at TP +200%, L = 63%, +38% on 3 SOL) agrees with our model (+34%). But it is in-sample on n=10, with a CI of 17-69%.
- **Arrival order matters a lot.** Entering at a price 5% / 15% / 30% above the reference (being the 2nd-3rd sniper, or landing after the bundle) pushes the TP-100 / L-80 break-even to **51% / 57% / 66%**. His own estimate is a 15% impact for being 3rd (L298).
- **Fixed costs scale badly at small size.** At 0.5 SOL, base overhead (~0.012 SOL plus 2.5% fees) eats ~5% per round trip. At 0.1 SOL it is ~14%.
- One positive finding (D2): when the price first trades ≥ 2.05× entry, it is still ≥ 2× in the next slot **94%** of the time (31/33), and **85%** three slots later. A TP sell that fires within one slot usually fills, so TP fill risk is smaller than the entry risk.
- **The upside is small in absolute terms.** In the good cases it is +$80-180/day per 10 snipes, which cannot pay for €10-15k/month of infrastructure without dozens of profitable operators. Either members subsidise his infrastructure, or the real edge is different from what he shows.

## 3. Hidden risks

1. **Farming the sniper.** The rugger's bot sees our buy in its own slot-0/1 data and can sell into us in the same or next slot. 7.8% of bundled launches had a slot-0 sell. Our snipe is a visible, repeated pattern: same wallet, same size, same timing.
2. **Latency competition.** Every other private sniper on the same operator raises our entry price and lowers both the TP probability and the floor buffer (see the drift numbers in §2). Measured slot-0 overhead reaches p90 0.035 SOL, which shows a fee war already exists.
3. **Supply-share detection.** He says rugger bots are binary: if the bundle didn't get its target share, they dump at once (digest §3.8). Our buy directly lowers the bundle's share.
4. **Dev or bundle dumping in the same slot as our buy.** Order inside a slot is decided at the leader. A sell that lands right after our buy inside slot 0 leaves no time to react. Only pre-trade filters and position size help.
5. **Exiting into a thin curve.** On a miss we sell after the dump, near the floor, where V ≈ 30-35 SOL. Our own 0.5 SOL sell then moves the price ~3%. With several snipers exiting together, the floor is shared and slippage stacks. A failed sell during the dump costs the full loss.
6. **Fee traps.** Non-SOL-quoted coins carry creator fees up to 300 bps (3.95% per side, D1). Mayhem coins (~22% of D1, i.e. 334/1,496) have agent flow that distorts candles. Filter both out.
7. **Token-2022 tricks.** Not possible on pump.fun mints (verified extensions). They become live risks if the system expands to Meteora DBC or LaunchLab: transfer hooks, transfer fees up to 5%, mint authority.
8. **Regime shifts.** The data is going stale fast: 200 ms slots (Oct 9), ShredStream gone (Sep 5), Alpenglow (~Oct 21), tx v1 (Sep 15), fee changes. Any backtest older than a few weeks is suspect.
9. **Legal and regulatory exposure.** Knowingly trading alongside a fraud scheme is a grey area. Profits may count as proceeds of fraud. CEX compliance may flag deposits that touch rug clusters. Pump.fun itself faces a RICO class action in SDNY, 25-cv-880 ([The Block](https://www.theblock.co/post/363996/burwick-law-names-solana-and-jito-executives-in-expanded-rico-case-against-pump), LIKELY). Taxes apply to every trade. This is not legal advice: get counsel before scaling.
10. **Survivorship and incentives.** The testimonials are selected. The P&L is undisclosed. The seller earns from subscriptions whether members win or not.
11. **Small samples.** "Test on 10 tokens", "stop after 5 losses" and "3/10 hits" are all statistically meaningless (CI 11-60%). Separating p = 0.40 from a break-even of 0.31 needs **~170 snipes** (one-sided α = 0.05, power 0.8). Separating 0.55 from 0.43 needs ~106. Given the ~1-week operator lifetime, one operator may not live long enough to be validated.

## 4. The 10 things our validation must measure before risking capital

1. **Realistic entry price.** Measure the spot price at our achievable in-slot position: after the dev, the bundle and the median third-party slot-0 buyer. Do not use the "3/4 of the first 1 s candle" heuristic. Report the drift against the end-of-slot-0 price.
2. **Hit rate per operator at TP +100/150/200%, out of sample.** Discover operators on days 1..t and test on days t+1..t+k. Never use features computed after entry (final volume, ATH). Report Wilson CIs.
3. **The loss distribution on misses.** Measure the actual exit price under our exit rules (floor, no-activity timer, dump trigger), including sell slippage when other holders exit at the same time. Do not assume -63% or -80%.
4. **All-in cost per round trip.** Priority fee, tips, failed-tx fees, rent left unrecovered, 1.25% × 2 fees, and quote-mint or creator-fee variants. Measure it from our own testnet-sized live fills, not from a model.
5. **Land rate and position in the slot against competitors.** Count rival slot-0 buyers per operator, their tips and CU, and whether we land in slot 0, slot 1 or later, per region.
6. **Operator persistence.** Measure how many tokens and days an operator signature (funding source + amount band + bundle shape) stays stable, and how fast the hit rate decays after we start sniping.
7. **The adversarial response.** Measure how often slot-0 or next-slot sells appear *conditional on our presence*. Compare against matched launches we skipped to detect farming and supply-share triggers.
8. **Dump-signal lead times.** Measure precision, recall and lead time in ms for consolidation transfers, dev sell, big-buy triggers and no-activity, and the P&L improvement over a fixed TP.
9. **Capacity and sizing.** Measure EV as a function of buy size (curve impact and detection risk), and the total number of qualifying snipes per day across all operators, to get the daily EV ceiling.
10. **Statistical stopping rules and regime checks.** Predefine the n needed per operator (~100-170 snipes to resolve a ~10-point edge), apply a multiple-testing correction across many candidate operators, and re-baseline after each protocol or network change (Alpenglow, fee changes).

Bottom line: the pump.fun mechanics he describes are mostly right in shape but stale in numbers. His selection statistics are in-sample, use look-ahead filters and rest on n=10. His profitability rule is arithmetically wrong for the natural stop-loss. An edge can exist only if we, out of sample, find operators whose hit rate at our *real* entry exceeds ~45% at TP +100% (or ~33% at TP +200%) after all costs. That is what the validation system must prove.
