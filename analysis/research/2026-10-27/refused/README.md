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
