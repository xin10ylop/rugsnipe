# Refused copies: deciding at execution time, on chain (Oct 27, night)

Question: the wallets made +7.43 SOL on the 69 coins whose copies our price limit refused. Can a buy that decides when
it executes, from what a program can read on chain at that moment, fill the right ones? Rules: pick on early days,
score on later ones; sniper sets and classes come only from earlier coins; 0.0155 SOL model bias comes off every fill
a mechanism adds; each period also scored without its best 2 coins.

All scripts run against the session scratchpad: the Triton client, the 5-day dataset `dec/five`, and `refused/` for
the tapes and outputs.

## Data
- `pull_live.py`: every buy we sent since the copy went live (Oct 25 22:08 UTC), landed or refused (6042), from our
  wallet's own transactions (332 coins, 70 refused).
- `tapes.py`, `tapes2.py`: 60-slot coin tapes. `tapes2.py` anchors the curve's signature search at a block past
  create + 60. It reads every trade of the create slot and the next 3, then each later slot's closing trades, plus
  every trade of his ATA. That covers the live 332 coins and his 575 keyed 5-day copies, plus 300 of his 1,109 other
  launch buys at random.
- `eng.py`: the 5-day copy model (`dec/copy/scratch.py`) on the long tapes, and launch-buyer classes (sniper /
  reactor / new) learned only from coins created at least 150 slots before.

## The refused population itself: nothing survives
- `eval5.py` + `ana5.py`: the 5-day copy of him, 1,323 refused landings on 152 coins. Tested:
  - premium bands, one-sided or not (including 25-45%);
  - who bought between (sniper / reactor / new shares, buy counts, sells, his slot or the next);
  - exits at create + 3 .. + 20 and right after his sale;
  - a 0.2 SOL slice;
  - re-entry when a slot closes within 0 / 3 / 5% of his price before he sells.

  3,664 settings: 18 are positive on Oct 21-23 without the best 2 coins, and none of those is also positive on Oct 24-25.
- Every premium band loses on its own: 5-10% P1 -1.56 / P2 -0.91, 10-15% -0.84 / -0.42. Bands of 20% and wider
  hold 2-4 fills in 5 days, and those lose too.
- `evlive.py` + `analive.py`: live, at our real positions, every wallet. Filling the refused copies adds:

  | wallet | adds (SOL) | without its best 2 |
  |---|---|---|
  | him (28) | -0.69 | |
  | 3yrJE1A8 (7) | -0.68 | |
  | JBokzLFR (15) | -0.37 | |
  | H9kuqGKN (15, at its 20%) | +0.74 | -0.00 |

  Of 1,294 wallet x band x exit x size x re-entry settings, none is positive in the early half without its best 2.
- **Why:** what moves the price between the wallet's buy and ours is mostly launch snipers' buys that were already in
  flight. At our price, those coins lose as a group. The wallets' +7.43 was made at their own entry.

## The one mechanism with an edge: a ghost copy, sent before the wallet buys
At each in-class launch, send the buy when the bundle completes, before the wallet's buy exists. Lighthouse checks
at execution:
- the wallet's token account holds the coin;
- ours does not;
- slot <= create + 1;
- min_out fixed from the post-bundle price.

Ghosts that execute before the wallet fail for about 0.000015 SOL. The first one after it fills. The reactive copy
asserts our ATA too, so the two never both fill.

- **Landing, measured, not assumed** (`presend.py`): the old rule bot sent at exactly that moment. On the 83 coins he
  bought too, all 83 of its buys landed after his, with a median of 1 trade between (0 trades on 41). Our reactive
  copy live sits a median of 2 trades behind him (0 on 22 of 103).
- **Value on his keyed copies** (`ghost4.py`, 5 days, all 575, bound post-bundle + 10%, picked on P1):

  | P_AFTER | P1 Oct 21-23 | P2 Oct 24-25 |
  |---|---|---|
  | 0.8 | +1.99 [+1.72] | +0.69 [+0.33] |
  | 0.5 | | +0.41 [+0.19] |
  | 1.0 | | +0.88 [+0.43] |

  Live (`ghostlive.py`, 0.83 days): him +0.49 [+0.27]. The other wallets total about +0.06 without wallet 4.
- **It fails as a whole, because a pre-sent buy can't see his tip.** The copy key is his tip (>= 2M). The ghost also
  fills his other launch buys, which are about 2 in 3 of them: -19.3 SOL over 5 days at post-bundle + 10% (scaled
  from 296 sampled). Every attempt to tell them apart on chain fails:
  - **His size, via his ATA amount** (`ghost4.py`, inline variants): the best size and bound on P1 is >= 1.1 SOL at
    +5% (+0.57). On P2 it loses (-0.07).
  - **His total spend, tip + priority fee** (`urgency.py`): this is what his lamport drop shows. His high-priority,
    low-tip buys lose (-3.3 at U >= 5M), so every threshold nets -0.6 to -1.3.
  - **The launch signer's keyed share over the 24 hours before** (`bysigner.py`): negative at every threshold.
  - **Fill everything, then sell unkeyed fills at once** (`abort.py`): -0.0155 a fill (about 0 before the bias),
    -12.7 SOL over 5 days.
  - **His tip route**: Helius carries both his keyed and unkeyed tips, and its tip accounts are shared, so a
    lamport check on them can't tell.
- **The copy_more wallets need no tip key, and their accounts are predictable** (`accounts.py`). Each opens its token
  account with `createAccountWithSeed(base = wallet, seed = mint prefix)`:
  - JBokzLFR and D5BDDkBt: the first 32 characters of the mint;
  - CxyKYmQo: the first 16;
  - 3yrJE1A8: the first 16 on 8 of 10 buys, the ATA on the other 2;
  - H9kuqGKN: an 11-character random seed, so it can't be targeted.

  Live, the ghost adds about nothing for them (pb + 10%: 3yr +0.06, JBo +0.08, Cxy +0.03, D5 -0.11). Their landing
  against a ghost hasn't been measured.

## Mainnet checks
- **Lighthouse is real and cheap** (`lighthouse_users.py`, and the decodes in the report):
  - program `L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95`, upgradeable;
  - instruction tags: 2 AssertAccountData, 5 AssertAccountInfo, 9 AssertTokenAccount, 15 AssertSysvarClock;
  - IntegerOperator: 4 is `>=`, 5 is `<=`;
  - 496-716 CU per assertion, up to 9,452;
  - the repo's token-account tests use Token-2022 accounts;
  - error 6001 means the assertion failed.
- **Pump bots already use it this way.** One sample of about 1,900 pump transactions came from 17 signers, and their
  assertions include:
  - a price cap (the curve's lamports `<=`);
  - their own ATA must not exist;
  - slot `<=`;
  - another wallet's token amount `>=`. That is ghost copying. 3Xtu99Js does it on HH2oPpyB 680 times and FN71Sic4
    147 times, and 870 of 874 such transactions failed.

  None of the sampled assertions target our six wallets.
- **He buys into his Token-2022 ATA** (9 of 9 checked; `ata.py`). The copy_more wallets don't (see above).

## What would settle it
A way to see his tip at execution: there is none on chain. Short of that, a live probe of one ghost per in-class
launch for about a day (about 0.03 SOL) would measure the real landing (P_AFTER, j) per wallet. It would only pay if
it showed the copy_more wallets gaining much more than this model gives.

## Round 2: making the ghost pay (selection)

**The economics, checked** (`ghost4.py`, P_AFTER 0.8, before the spray):

| bound | keyed gain over today | his other buys | break-even keyed share | ceiling |
|---|---|---|---|---|
| post-bundle + 10% | +0.0049 a buy | -0.0174 a buy (-0.0237 a fill, fill rate 0.73) | 78% | +0.56 SOL a day (P2 +0.37) |
| post-bundle + 5% | +0.0040 a buy | -0.0104 a buy | 72% | +0.47 a day (P2 +0.28) |

**The spray is far bigger than assumed.** The 1,861 launches a day were only his and our coins.
- Every pump create signs with the mint authority `TSLvdd1p...`. Counting its signatures gives about 55,800 creates a
  day on Oct 26-27, and 45,600 a day on Oct 23-24.
- In class: about 48% (create_v2, not mayhem, SOL quote), so about 26,600 in-class launches a day (`launches.py`).
- A failed ghost costs 15,000 lamports (all 70 of our refused buys paid exactly that). One ghost per in-class launch
  is about 0.40 SOL a day per target wallet.

**1. Ghost targets** (`gtargets.py`, `gtrank.py`, `gtcheck.py`):
- Every launch buyer on 14,880 tapes (5-day 8,349, oos 5,323, 60-slot 1,208), at the ghost position, sold at create + 3
  to + 12.
- Of the 95 wallets with 15+ buys in both halves, 7 are positive early without their best 2. 4 of those stay positive
  late, all near zero per buy:

  | wallet | late (SOL) | buys | per buy | token account |
  |---|---|---|---|---|
  | 8ki679dZ | +0.36 | 56 | +0.0064 | random seed: can't be targeted |
  | 8ptvshQR | +0.06 | 79 | | mixed ATA / random |
  | 9qz7cWBq | +0.03 | 505 | | random |
  | 4LTaX4wo | +0.02 | 30 | | mint[:32], predictable but worthless |

- None covers the 0.40 SOL/day spray of its own ghost.
- The universe is a limit: tapes exist only for coins he, we or the earlier finalists touched. A search free of that
  bias needs tapes of every in-class launch (about 26,600 a day; roughly 800k RPC calls for one day).
- **Our six, every launch buy at the ghost position, no key** (late, without best 2):
  - him -1.1 to -3.3;
  - 3yr -0.3 to -0.5 (the tapes stop before its create + 29);
  - JBo -0.7 to -1.8;
  - Cxy -0.8 to -2.0;
  - D5 -0.2 to +0.1;
  - H9 +0.0 to +1.0 (random seed, so no ghost).
- **Why the copy_more wallets came out at about zero:** our reactive copy already sits right behind them. The live
  median is 0 trades between for JBo (63 of 116 at zero) and Cxy (10 of 16), and 1 for 3yr. The fills we skip
  (their other buys) lose at the ghost position.

**2. Keyed prediction for him** (`keyed.py`, `guardkey.py`, `guardlive.py`; trained Oct 21-23, tested Oct 24-25):
- **Create-time features only:**
  - the signer's, host's and tool's past keyed share and his past net there;
  - his 24 h buys of the signer, creator = signer, dev buy, bundle SOL, create fee, name and symbol length, the hour.

  Test precision is 51-56% at 22-26% recall, with no threshold above 79%. The ghost money is negative.
- **Adding what his buy shows on chain at execution** (size, and U = tip + priority fee):
  - trees reach 72-76% precision at 100% recall;
  - boosted trees reach 94-99%, but only by reading the 1-lamport remainder his priority fees carry inside U. A guard
    can't read that directly; it only sees his wallet's drop.
- **The guard version, using the lamport drop.**
  - Rule: drop - base fee - rent (three exact values, told apart by the low digits), modulo 0.1 SOL = tip (+ priority,
    which ends in 1 lamport) (- the lamport his tier sometimes loses). Key = x >= 2M with no priority remainder, or
    size >= 2 SOL.
  - Oct 21-25: precision 75.5%, recall 85.8%.
  - Money at post-bundle + 5% (P_AFTER 0.8), before the spray: P1 +0.76, P2 +1.06. Without each period's best 2:
    +0.07 and +0.25 a day.
  - **His last 24 hours** (376 buys): precision 65.3%, recall 88.5%. 45 buys now carry an extra 1M transfer
    (2,513,840 beyond the rent). A drop can't tell that from a 1M-higher tip.
- **Pre-filter** (`prefilter.py`): see the next section.

**3. Delivery:**
- **Staggered volley:** P_AFTER was 83 of 83 for a single ghost, so there is nothing to raise. Each extra ghost lands
  later (a larger D) and costs another 15,000 lamports a launch. The real effect needs a live probe.
- **OR across wallets:** Lighthouse assertions are AND only, so it's one transaction per target wallet (the spray
  times the number of targets). One transaction for several wallets needs our own guard program, which can also run
  the lamport key.
- **A cheap pre-filter** (`prefilter.py`: a random 1,000 of the 91,115 creates on Oct 23-24):
  - the volume then: 45,600 creates a day, 37% in class, so 16,900 in-class launches a day;
  - "the signer was bought by him in the 72 h before": keeps 11.3% of launches (about 1,900 a day, 0.029 SOL of spray
    a day) and 71% of his keyed buys;
  - the 24 h version: 8.4% of launches and 63% of his keyed buys;
  - metadata hosts don't filter: 88-95% of launches kept.
- **The best combination: lamport key + 72 h signer pre-filter, post-bundle + 5%, after the spray:**

  | period | per day | without best 2 |
  |---|---|---|
  | P1 | +0.050 | -0.090 |
  | P2 | +0.166 | +0.076 |

  At post-bundle + 10% it loses in both periods. With the key at its last-24 h precision (65%), it loses outright.

**Verdict, round 2:** none passes.
- No target wallet pays its own spray.
- No create-time model reaches the precision bar.
- The one on-chain key that comes close loses its precision as soon as his payment pattern shifts.

What's left is about +0.1 SOL a day at best, and it rests on a 1-lamport artifact of his fee settings.
