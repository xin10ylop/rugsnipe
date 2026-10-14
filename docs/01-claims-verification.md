# Claims verification: what the videos get right, wrong and stale

Each claim is numbered as in `docs/00-transcript-digest.md` §9. Verdicts: **TRUE**, **PARTLY**,
**STALE** (was true, numbers moved), **FALSE**, **UNVERIFIABLE**. "Verified" means I re-checked it
myself today (2026-10-14) against a primary source: the official pump.fun IDL/docs, decoded on-chain
accounts and transactions via public RPC, the pump.fun API, Stakewiz / DoubleZero validator data, or
the Solana Foundation and Jito pages. Research agents' findings were only accepted after such a check
or when two independent agents reached the same number from different data. Sources and detail:
`docs/research/`.

## A. Protocol mechanics

| # | Claim | Verdict | What is actually true |
|---|---|---|---|
| C1 | Max supply 1.07B tokens | **FALSE** | Total supply is **1.000B** (1e15 base units, 6 decimals). 1.073B is the *virtual* token reserve of the curve; 793.1M are sellable on the curve. Mayhem-mode coins mint 2B. His "615M of 1.07B = 57%" is really 61.5%. Verified from Global and a mint's supply. |
| C2 | Liquidity is protocol-provided; no remove-liquidity or honeypot on pump.fun | **TRUE on the curve** | Verified: the mint is Token-2022 with only metadata extensions, mint/freeze authority null; the curve always buys back. After migration the PumpSwap LP is burned (no rug pull), but there is **no price floor** any more. |
| C3 | Floor ≈ $2,500 market cap; max loss ≈ -80/-90% | **STALE / PARTLY** | Floor = 30 SOL virtual → **27.959 SOL ≈ $3,000 at $107**. $2,500 assumed SOL ≈ $89. The loss at the floor depends on the entry: -63% from a $8k entry, **-80% from a $15k entry** (his "first candle ≤ $15k" rule is exactly the -80% line), plus 2.5% round-trip fees. Unbounded after migration. |
| C4 | 1% fee on buy and sell | **FALSE** | **1.25% per side**: 95 bps protocol (half of it a buyback) + 30 bps creator. Verified in a TradeEvent decoded today (`fee_basis_points=95`, `creator_fee_basis_points=30`). Non-SOL-quoted coins can carry up to 3.95% per side. |
| C5 | Migration at ≈ $35k to Raydium | **STALE / FALSE** | Completion at **85.005 SOL raised = 410.88 SOL market cap ≈ $44,000** (verified on completed curves: virtual SOL 115.005). Graduates go to **PumpSwap** since 2025-03-20, not Raydium. Raydium only hosts pre-2025 graduates and LaunchLab (bonk.fun) tokens. |
| C6 | Blocks are 400 ms | **STALE / FALSE** | **200 ms slots** since epoch 1053 (Oct 9 2026); measured 214-221 ms today. Even before Oct 9 the chain ran about 268 ms per slot (epochs 1040-1048, from `getBlockTime` at epoch starts; research/gap-6), so 400 ms was only ever the nominal figure. A leader still produces 4 consecutive slots. |
| C7 | The dev buy is always first; nothing can precede it | **PARTLY** | True when the dev buy is inside the create transaction (**83%** of creates, n=1,496). In the other 17% any sniper can be first. In bundled launches you are behind the whole atomic Jito bundle (create + up to 4 buy transactions), not just the dev. |
| C8 | 15-20k tokens/day; 90% of Solana memes; > $1B revenue; ≈ $2 to create | **STALE / PARTLY** | **46,600 creates/day** average over the last 48 h (93,191 successful creates; a 50-signature decode sample was 100% `create_v2`), ranging from a 25-30k/day pace at 06-10 UTC to **60-75k/day at 14-21 UTC**. pump.fun has ~81% of launchpad creations and 72-87% of launchpad volume (Meteora DBC is #2 at ~4.8k/day). Revenue passed $1B around March 2026 (DefiLlama, likely). Creating costs the creator 0.006-0.010 SOL ≈ **$0.6-1.0** plus fees. |
| C9 | Average hold time 56 s | **UNVERIFIABLE** | No source for "56 s". Secondary sources put the median Solana memecoin hold at ~60-100 s in 2025-26, so the order of magnitude is right. |
| C19 | A Solana transaction never costs more than $1 | **FALSE** | Uncontested transactions are cheap (Jito tip floor p50 ≈ 0.00001 SOL), but **landed block-0 buys cost a median 0.004 SOL, p75 0.014, p90 0.05 SOL (≈ $5)**; outliers paid 0.15-0.26 SOL. His own "hardcore cap" (0.025 SOL ≈ $2.7) contradicts the claim. |
| C25 | Only pump.fun is "safe"; Meteora/Raydium launchpads are not | **TRUE for Meteora DBC, mostly for LaunchLab** | Verified in the docs: DBC allows transfer-hook tokens (sells can be blocked), creator mint authority in some configs, fee cliffs up to 99%, and up to 90% of post-migration liquidity unlocked. LaunchLab rejects transfer hooks but allows Token-2022 transfer fees up to 5% and platform fees up to 5%. |

## B. The rugger model

| # | Claim | Verdict | What is actually true |
|---|---|---|---|
| C10 | Ruggers own ~45% of supply via 4 wallets × 10 SOL in the first ms | **PARTLY** | Bundling is real: in creation slots with ≥ 3 buyers, net slot-0 supply is **median 21%, p75 28%, p90 37%** of 1B, and slot-0 SOL is median 7.5 (p90 16). 40 SOL bundles are a tail case. "Dev + 4 wallets" matches Jito's 5-transaction bundle limit. 14.5% of launches had ≥ 2 non-creator buyers with ≥ 1 SOL in slot 0. |
| C11 | Creator rotates the wallet each token; funding from CEX hot wallets with fixed odd amounts to fresh wallets | **PARTLY, and the amount rule is weak** | Verified live: fresh wallets funded by a CEX hot wallet (dominantly Binance `5tzFki…`, which Solscan labels "Binance 2") create a token **24-798 s later (median 159 s)** and 85% of those launch bundled. But **37% of launches come from serial wallets with ≥ 50 coins** that never rotate (he filters them out, correctly). Funding amounts are mostly 6-decimal noise (0.9965, 1.0697, 1.2090…); his 2.955/2.995 appeared 0 times in 126 traces. Fixed amounts do exist on mother wallets (one sent exactly 0.9 SOL to 41 fresh wallets, each then dev-buying exactly 0.8395 SOL). Round CEX amounts (1.999) also match retail withdrawals that never create. **The usable key is amount + fresh recipient + a create within minutes, plus the bundle fingerprint**, which is what a live funding watcher can do and a manual Solscan scan cannot. |
| C12 | Tokens are consolidated into one wallet one block before the dump ("front-run sell") | **EXISTS, UNCOMMON** | Of 14 single-slot dumps (≥ 30% of supply) in a random sample, **2** were sold by a wallet that received tokens by transfer, **10** were many wallets selling in the same slot, 2 single-wallet. Treat consolidation as a secondary exit signal; "any bundle wallet sells" is the primary one. Note the program emits nothing for transfers, so this needs Token-2022 transfer monitoring. |
| C13 | Good ruggers give +100% from the end of the first candle on ≥ 3/10 tokens | **UNPROVEN; his evidence has look-ahead** | Base rate for +100% from the end-of-slot-0 price: **6.8% of random coins** (95% CI 3.5-12.9%), 12.6% of bundled coins. His "volume ≥ $20k" filter raises it to ~73%, but volume is only known *after* the pump: **outcome leakage**. 10 tokens cannot certify 3/10 (Wilson CI 11-60%). Whether specific operators exceed the break-even rate out of sample is exactly what `docs/05-validation.md` measures. |
| C14 | First candle ≤ $15k keeps the natural SL at -15/-20% | **FALSE (self-contradictory)** | At a $15k entry the floor loss is **-80%**; he himself quotes -63/-80% elsewhere. -15/-20% only holds if you enter near the floor, i.e. with no bundle ahead of you. |
| C21 | A rugger lasts ~1 week | **UNVERIFIABLE / contradictory** | He also says members kept the same rugger for 3-12 months. Operator persistence is a key validation metric. |
| C23 | Rugging is very expensive for the rugger | **OVERSTATED** | Creation < $1; wash volume costs 2.5% per round trip but the creator earns back 0.30% of all volume; CEX deposits are free. The real cost is the pump capital at risk. |
| C26 | Ruggers "farm" snipers (big first candle, dump next block) | **TRUE** | 8 of 103 bundled launches had sells **inside slot 0**, and 28 of 103 had a ≥ 3 SOL sell within ~5 s (vs 2/246 and 10/246 for unbundled launches). |
| C27 | AI cannot automate rugger discovery | **FALSE** | Discovery is a mechanical join (funding graph, amount band, fresh wallet, bundle signature, outcome statistics). The on-chain validation package already clusters creators automatically; the hard part is the out-of-sample statistics, which humans eyeballing 10 charts do worse. |

## C. Economics

| # | Claim | Verdict | What is actually true |
|---|---|---|---|
| C24 | Win rate 25-40% is enough with TP:SL ≥ 3:1 | **FALSE as stated** | His 3:1 example assumes a **-50% stop**. With the natural stop (-63% to -80%) and TP +100% the ratio is 1.6:1 to 1.25:1 and the **break-even hit rate is 43-48%** after 1.25% × 2 fees, tips and slippage. At his headline numbers (33% hits, TP +100%, floor -80%) the expected value is **about -27% of stake per snipe** (≈ -$140/day at 10 × 0.5 SOL). TP +200% at 33-40% hits is positive (+17-34%). His own Claude simulation (4/10 at +200%, +38% on 3 SOL) agrees with this model but is in-sample on n=10. |
| C22 | $2M in 3 years, €10-15k/month of servers | **UNVERIFIABLE** | No wallets disclosed; income also comes from selling the bot (Whop lists ~1,700 members, ~€1,000 lifetime). Member testimonials are selected; he admits ~50% quit within 30 days. |

## D. Speed and infrastructure

| # | Claim | Verdict | What is actually true |
|---|---|---|---|
| C15 | ShredStream lets you buy in the creation block | **STALE** | **Jito ShredStream was shut down on 2026-09-05** (verified on docs.jito.wtf). Shreds never show a transaction before the leader executes it; they show it before the block completes, which with 200 ms slots and 4-slot leader windows still allows same-slot or same-leader landing. Replacements: DoubleZero Edge (~$3k/mo in Frankfurt, covers ~53% of stake), Helius shred delivery, Helius pre-confirmations, Triton deshred, SVS. |
| C16 | Leaders rotate worldwide; France→Brazil 600 ms; need servers in NY, LA, AMS, FRA, Tokyo | **PARTLY** | The leader schedule is public per epoch, so it is not random at decision time. Leader slots: **Frankfurt 32-35%, Amsterdam 19-21%, London 9%, Tokyo 6%, NYC/Newark 4%, LA 2%**; Europe 74%, Brazil negligible. Paris→São Paulo is ~190 ms RTT, not 600. LA is a weak site and London is missing from his list. |
| C17 | Optimised buy ≈ 60-70k CU vs 115k | **TRUE** | Measured: `buy_v3` ≈ 50-51k CU; whole block-0 winner transactions ≈ 55-60k (I decoded one at 54,877 CU); legacy `buy` ≈ 117k. |
| C18 | Priority fee vs tip balancing and "hardcore" outbidding decide intra-block order | **PARTLY** | Depends on the leader's software (verified mix: BAM 37.9%, Jito-Agave 27.2%, Harmonic 25.1%, Firedancer 7.2%). On Jito-Agave leaders arrival order dominates (fee re-orders only 54-58% of same-block pairs); on BAM and Harmonic leaders the fee decides position inside 20-50 ms batch windows. You cannot see a rival's bid in time, so "outbidding" is a forecast from past creation slots. |
| C20 | Public bots are structurally slower than private infra; his bot was first 5/5 | **UNVERIFIABLE** | A self-run marketing test with n=5. Nothing stops a public bot from co-locating, but one shared pipeline cannot be first for every user at once. Transaction v1 (live since Sep 2026; all 13 pump transactions in a block I sampled were v1), durable nonces and v3 instructions are already used by rival snipers. |

## E. What this means

1. **Mechanics: right in shape, stale in numbers.** The curve, floor, migration and "always sellable"
   story is correct for the bonding curve. Fees are 1.25% not 1%, migration is PumpSwap at ~$44k,
   slots are 200 ms, supply is 1B, and the create rate is 2-3× what he says.
2. **The rugger model is real.** CEX-funded fresh wallets that create within minutes and launch
   bundled, mother wallets with fixed amounts and identical dev buys, same-slot dumps and sniper
   farming all show up in today's data. His **manual** detection (fixed amount on Solscan) is the weak
   part; a live funding watcher with a ~2.5-minute head start is the strong version of his idea.
3. **His profitability rule is wrong.** Against the natural stop, +100% needs ≥ 43-48% hits, not 33%.
   The edge can only exist for specific operators at higher take-profits or with dump-signal exits, and
   it has to be proven out of sample. That is the job of the validation system.
4. **His infrastructure story is outdated.** ShredStream is gone, slots halved, two thirds of leaders
   batch-auction, and the geography favours Frankfurt/Amsterdam far more than his NY/LA sites.
