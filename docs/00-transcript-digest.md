# Transcript digest: FOMO / F-Project "rugger sniping" (3 videos)

Source: three French YouTube videos by "FOMO" (expat in Thailand, sells a private Telegram bot
called **F-Project**, access by sales call only). Transcripts in `data/transcripts_flat.txt`.

* Video 1 (2h56): full "formation" with a beginner (Arthur). 9 acts: reset, playground (Solana/pump.fun),
  the business, tools (Axiom, Solscan), analysis, security, first trade, exit strategy, 30-day plan.
* Video 2 (19m): "the memecoin food chain" (retail < KOL < insiders < snipers < dev), with real examples.
* Video 3 (42m): full tour of the F-Project bot features and its Claude MCP integration.

Everything below is **what he claims**. Verification lives in `docs/01-claims-verification.md`
and `docs/research/`.

---

## 1. The thesis in one paragraph

On pump.fun, some developers ("ruggers") run bots that create memecoins all day, buy their own token with
their own wallets to paint a pump, attract retail, and dump everything in one block. Because it is a bot,
the behaviour is repetitive and therefore predictable. Instead of avoiding rugs, you **buy the rugger's
token in the same block it is created (block 0) and sell at a fixed take-profit before he dumps**. You
grind many small +100% wins rather than hunting 100x. "Steal from thieves."

## 2. Solana / pump.fun mechanics as he describes them

| Item | His statement |
|---|---|
| Block | 400 ms; transactions are grouped per block; "block 0" = same block as the event you target |
| Pump.fun supply | max 1.07 billion tokens per token (fixed by the platform) |
| Liquidity | emitted by the protocol (virtual), so no remove-liquidity / honeypot possible on pump.fun; you can always sell |
| Price floor | a token can never go to 0; floor ≈ $2,500 market cap (varies 2,300-3,500 with SOL price); you can lose up to ~-90% but never -100% |
| Fees | 1% on every buy and every sell, taken by pump.fun |
| Bonding curve | price rises with each buy along a mathematical curve, specific to pump.fun |
| Migration | at ≈ $35,000 market cap (depends on SOL price) the token migrates to "Raydium" and becomes tradable everywhere (Jupiter etc.); before that only on pump.fun |
| Creation | anyone, < 30 s, ≈ $2 |
| Volume | 15,000-20,000 tokens created per day on pump.fun in 2026; 90% of Solana memecoins are born there; pump.fun > $1B revenue in 2 years |
| Hold time | average memecoin hold time 56 seconds |
| Other venues | Meteora, Raydium, Jupiter launchpads exist but are "less safe" than pump.fun; he trades pump.fun only, Solana only |
| Tx cost | on Solana you "never pay more than $1" per transaction, even under contention |

Vocabulary he uses: CA (contract = mint address), market cap, bonding curve, migration, blocks,
**fresh wallet** (new address with no history), DB/DS (dev buy / dev sell), bundle, KOL, retail, TP/SL.

## 3. Anatomy of a rug (his model)

1. Dev creates the token (cost ≈ $2). The **dev buy** with the creator wallet is always the very first
   buy; nobody can buy before the creator.
2. **Bundle**: in the first millisecond, 4-ish fresh wallets each buy ~10 SOL (example seen on video:
   dev buy 0.1 SOL, then 4 × 10 SOL). Total rugger share of supply typically **45%**, sometimes > 50%
   (example: 105M + 274M + 161M + 75M = 615M of 1.07B).
3. The rugger keeps buying with hundreds of own wallets (clusters of 200-300) to paint candles and
   volume; may add fake socials / AI-generated sites.
4. Retail and KOLs enter.
5. **Dump**: all wallets sell in a single block (< 400 ms) → one big red candle. Price returns near the
   floor. Variant: wallets first **transfer all tokens to one main wallet** (1 block), then that wallet
   sells (next block) → ~800 ms warning ("front-run sell").
6. The rugger rotates the **creator wallet every token** (otherwise trivially trackable), but often
   reuses pump wallets (changing 100s of wallets is costly). Funding patterns:
   * **Simple**: A creates, A sends all SOL to B, B creates… (possibly via intermediates). Rated 1/10
     (too lazy, probably not rich).
   * **Mother wallet**: a bank address (4k-400k USD) sends a fixed amount to intermediates / creators and
     receives the proceeds back. Rated 5/10.
   * **Exchange**: funds leave a CEX hot wallet (Binance 2, Coinbase, KuCoin…) to a fresh wallet; the
     CEX's internal account is invisible on-chain, but the **amount is fixed and precise** (e.g.
     2.955 SOL, 2.97 SOL, 3.989 SOL) because it is a bot, so you filter the hot wallet's outgoing
     transfers by that amount range and by "fresh wallet only". Rated 10/10 (most opaque, most used by
     good ruggers). Mixers are similar.
7. Rugging is expensive (1% pump.fun on every buy/sell with every wallet, CEX deposit/withdraw fees,
   the SOL used to pump). So they optimise and repeat → patterns are stable for days/weeks. A good
   rugger lasts ~1 week before changing; some members have followed the same rugger 3-12 months.
8. Why he dumps when he dumps: ran out of pump budget; a big retail buy appeared (e.g. 3 SOL); too many
   snipers took supply (his bot is binary: if the bundle did not get ≥ its target share, e.g. 600M, dump
   immediately); not enough buyers.
9. **Farming**: a rugger who detects a sniper makes a large first candle and dumps on the next block,
   repeatedly, to drain the sniper. Defences: max consecutive loss counter, snipe delay, min market
   cap, buy-amount ranges, cooldowns.

## 4. Method 1: dev sniping (his main method)

Find a token that pumped and rugged → analyse its creator → if the creator is trackable and profitable,
wait for his next creation and snipe it at block 0.

Selection checklist (Axiom "Pulse" with filters: protocol = pump only, quote = SOL, audit "dev pair
created" ≤ 5-10, volume ≥ $20k (he raises to 30-40k when volume is high); display columns: market cap,
volume, fees paid, dev creations, funding time):

1. **Gain potential**: from the price at ~3/4 of the first 1-second candle (the bundle occupies the
   first 3/4; you buy at the end of the candle) to the ATH there must be ≥ +100%. Axiom hides the
   dev's own candle, so use the first candle after the dev buy.
2. **First candle ≤ $15k market cap**. A big bundle means you enter high and the "natural SL" costs
   -80% instead of -15/-20%.
3. **Funding is trackable** on Solscan (first incoming transfer, "Funded by", token-creator tag with
   the ★ star, Transfers tab filtered: SOL only, outgoing only, amount range).
4. Analyse **≥ 10 previous tokens** of the same operator (not more than ~50; ignore tokens older than
   10 days). Build a table: funding amount, dev buy, bundle size, ATH, +100% hit or not.
5. **Token win rate ≥ 33%** (≥ 3 of 10 hit +100%) and **TP:SL ≥ 3:1**. If hit rate is ~50%, lower the
   TP (e.g. +150% hit 5/10 beats +500% hit 2/10). Prefer many small wins; big TPs annoy the rugger and
   get you farmed.
6. Confirm identity across tokens by the **bundle signature** (dev-buy size + number and size of
   bundle wallets) and chart shape, not only by amount.
7. Then configure the bot: monitor the funding source (CEX hot wallet or mother) with up to 3-5 amount
   ranges, fresh-wallet-only, keep-address; auto buy 0.25-0.5 SOL (5% of capital, max 10%); TP +100%;
   no SL (floor) or sell-on-no-activity; max-loss counter.
8. Test on ≥ 10 tokens (or 4 if hit rate ~50%); if 5 consecutive losses, turn off and rethink.

Good vs bad rugger summary:
* Good: +100% on ≥ 3/10, first candle < 15k, trackable funding, consistent scheme, positive P&L on 10.
* Bad: +100% on < 3/10, first candle > 15k, mixer/untraceable funding (≈ 2% of ruggers), incoherent
  scheme from token to token.

## 5. Method 2: copy-trading the rugger's pump wallets

When the dev cannot be traced, find wallets that belong to him and copy their buys (worse entry, but no
tracking needed):

* Identify them from the **bundle** (nobody sane buys 4 × 10 SOL in the first ms), from the inside of
  the **big red dump candle** (Axiom "click candle to filter transactions"), or big green candles.
* Qualify a wallet: 7-day buy count consistent with ~8 tokens/day (e.g. 57 buys / 7 d; 1,000 is a
  normal trader, 187 too many), win rate > 50% (Axiom bar / GMGN), entry market caps consistently low
  (2k-9k), balance not zero, Activity shows the same tokens as the rugger.
* Entry when copying = end of the candle in which the wallet bought, not his price. Re-check +100% from
  that entry on ≥ 10-15 tokens. Also use the copied wallets to recover the dev's funding scheme
  (mother wallets send e.g. 9 SOL to bundle wallets, which come from 200-SOL sub-mothers that rotate
  every 3-4 hours).

Correlations he looks for: bundle size vs outcome (e.g. 7.6 SOL bundles pump, 10 SOL ones do not),
funding amount vs outcome, fresh-wallet bundles for the tokens he intends to send high, dev-sell
patterns, big-buy → dump.

## 6. Economics and discipline rules

* 5% of capital per snipe (max 10%). Scale only after a positive test.
* His own win rate is 25-40%; profit comes from TP:SL ≥ 3:1.
* Max 5-10 tokens per day per rugger; he runs 2-3 core ruggers.
* Stop after 5 consecutive losses; no revenge trading; do not mix with trend trading.
* Expect ~50% of bot buyers to quit within 30 days; members' results: Noah $10k in 3 months with 3
  ruggers; Fred -5 SOL month 1 then +$2,006 month 2; Pacino $25k best month after 6 months (manual);
  Gumeti $5-6k/month manual. He claims ~$2M over 3 years and ~$60k/month.
* Live test in video 1: 45 min to find a rugger, 3 snipes: -$11, +$50 → ≈ +$39.

## 7. Speed and infrastructure claims

* Being first after the bundle matters: every later sniper pushes the price up for you. He estimates
  being 3rd instead of 1st costs ≈ 0.115 SOL per 0.5 SOL snipe (15% price impact assumption) → 11 SOL
  per 100 snipes.
* **ShredStream**: his infra sees the create request before confirmation and places the buy so it lands
  in the same block. Leaders rotate every block and can be anywhere (Brazil, Japan…): a bot in France
  needs ~600 ms to reach a Brazilian leader → block +1. Hence **6 servers**: NY, LA, Amsterdam,
  Frankfurt, Tokyo (+ a "HQ"), costing €10-15k/month.
* Fees: base fee + priority fee always; **TX senders** (Jito-type services, 0.001-0.05 SOL) are extra.
  TradeWiz maxes the sender tip (0.05 SOL) with near-zero priority fee; F-Project balances them per
  transaction and trims **compute units to 60-70k** (vs 115k unoptimised). "Hardcore mode" inspects
  other snipers' fees and outbids them, capped at 0.025 SOL (~$3).
* His comparison video: F-Project first in 5/5 snipes vs TradeWiz, Bloom, Maestro, Trojan. He mocks
  "Claude-written bots" as slower than Maestro.
* His MCP/Claude integration: 71 tools; Claude analyses a rugger from a Solscan CSV export and
  simulates P&L (0.3 SOL, TP 200%, natural SL -63% → 4/10 hits, +38% net on 3 SOL deployed).

## 8. F-Project bot feature inventory (video 3)

Wallet: create/import, positions (sell all), multibuy (several wallets), presets, groups, 30 ruggers
per wallet, notifications, FR/EN, referrals, Discord, Chrome extension for manual trading on Axiom with
their speed.

Per-rugger settings:
* Auto buy / auto sell toggles; **max loss** (N consecutive losses → auto-buy off; anti-farm, anti-sleep).
* **Tracking mode**: new token creation (snipe), track buy (copy), track sell, **buy on dev sell**
  (min/max % sold) — buy when the dev wallet sells, because the pump continues with other wallets; can
  combine modes and wallets to look like two different buyers.
* **Buy mode**: simple vs **hardcore** (fee auction + lighter tx to be first sniper).
* Buy config: amount (default 0.01; 0.05 minimum sensible, 0.25 typical), **snipe delay** (wait N s,
  e.g. 5 s, for ruggers who fail/farm: buy the bottom of the failed launch), **max token age** (copy
  only young tokens), **track-buy amount range** (copy only buys between X and Y SOL: anti-farm),
  **follow cooldown** (min seconds between buys), **wait for their buy** (confirmed vs fire on sight),
  **buy only once** (per token, optionally across wallets), **min/max market cap** (works with delay for
  copy; meaningless for creation sniping since the creation price is always ≈ 2,300), **buy the dip**
  (buy X SOL when price dips Y% after entry — for ruggers whose first red candle is always the same).
* Sell config: up to **5 TP/SL levels** with partial sells; **trailing stop** (% from ATH, or by market
  cap); **sell on no activity** (e.g. 150 s without a trade); sell delay (ms); **phantom SL** (SL only
  active after 10 s to survive early sniper dumps); **auto-sell on big buy** (sell 100% when a single buy
  of 2.8-3.2 SOL appears, because that rugger dumps when a whale enters); **front-run sell** (sell when
  ≥ 50% of supply is transferred into one wallet — the consolidation that precedes the dump).
* **Rugger protection** (automatic wallet tracking): **last transfer** mode (when the tracked wallet
  empties itself into another wallet, replace it), **exchange pattern** mode with **keep address**,
  **fresh wallet only**, up to **5 transfer ranges**, **remove if no launch** (drop added wallets that
  did not create within N hours), **inherit settings** (child wallets get their own preset).
* Gas settings, presets.

## 9. Enumerated claims to verify

C1 1.07B max supply · C2 protocol-emitted virtual liquidity, no honeypot possible on pump.fun ·
C3 floor ≈ $2,500 MC, loss capped ≈ -80/-90% · C4 1% fee buy and sell · C5 migration ≈ $35k MC to
Raydium · C6 400 ms blocks, "block 0" semantics · C7 dev buy always first, nothing can precede it ·
C8 15-20k tokens/day, 90% of Solana memes, > $1B revenue, ≈ $2 to create · C9 average hold 56 s ·
C10 ruggers hold ~45% of supply via 4-wallet 10-SOL bundles · C11 creator rotates wallet each token;
funding from CEX hot wallets with fixed odd amounts to fresh wallets · C12 consolidation-before-dump
exists (front-run sell) · C13 good ruggers: ≥ 3/10 tokens give +100% from end of first candle ·
C14 first candle ≤ $15k keeps natural SL ≈ -15/-20% (contradiction: he also says -63/-80% on misses) ·
C15 ShredStream lets you buy in the creation block · C16 leaders rotate globally; multi-region
servers needed; France→Brazil 600 ms · C17 optimised pump.fun buy ≈ 60-70k CU vs 115k · C18 priority
fee vs tip balancing and fee outbidding ("hardcore") decide intra-block order · C19 tx never costs
> $1 · C20 free public bots are structurally slower than private infra · C21 a rugger lasts ~1 week ·
C22 his P&L $2M / 3 y, 6 servers €10-15k/month · C23 rugging is very expensive for the rugger ·
C24 win rate 25-40% is enough with TP:SL 3:1 · C25 only pump.fun is "safe"; Meteora/Raydium
launchpads are not · C26 farming (ruggers hunting snipers) is common · C27 AI cannot automate rugger
discovery (he says it is too complex) — this is exactly what we intend to automate.

## 10. Where we can beat him (first thoughts, to be confirmed by validation)

* He discovers ruggers **manually** in Axiom/Solscan (10 min per rugger, 3-5 h/day). The whole
  discovery pipeline is mechanical: funding graph, fixed amounts, fresh wallets, bundle signatures,
  outcome statistics. We can run it continuously over **every** pump.fun creation and rank operators
  by backtested expectancy, not by eyeballing 10 charts.
* His entry is "end of first candle after the bundle" with a fixed TP. We can condition entry on the
  **observed bundle** (size, number of wallets, dev share) in the creation slot, size the position by
  curve impact, and pick TP/exit per operator from the backtest (hit rate × payoff), including
  dump-signal exits (consolidation transfers, big-buy triggers, dev sell).
* His stats are eyeballed on 10 tokens. We can require statistical significance and recompute daily.
* Speed: same ingredients are available to us (ShredStream, Yellowstone gRPC, Jito/Helius/Nozomi
  senders, CU-trimmed prebuilt transactions, multi-region). Nothing in his infra is proprietary.
