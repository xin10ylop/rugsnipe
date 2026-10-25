# 18. 6anjqMe as a copy target (2026-10-26)

Wallet `6anjqMeWigQFo5wpciARAunVQUXTqoat8w8fFWarCkpc` (docs/12 section 8), put through the process that gave the
AHwJSnFN copy (docs/17 section 9). Verdict: **no slice of his trades is worth copying at our speed.** The bot keeps
copying AHwJSnFN.

## The data

His transactions of Oct 1 to Oct 25 23:33 UTC: 44,968 for Oct 1-20 (the month pull) and 11,559 for Oct 21-25 (the
last day and a half over Triton, the Helius key being at its usage limit). His P&L per coin comes from the pump trade
events (SOL in and out, pump fees, transaction fees, tips): he pays through a wrapped-SOL account, so his wallet's
balance change does not show the trades. Scripts: `analysis/research/2026-10-26/six/`.

## What he trades (Oct 21-25)

- **1,660 coins priced in SOL** (what the bot can trade): +8.01 SOL for him, +0.005 a buy, 39% wins. Median buy
  0.51 SOL, median hold 10 slots to his first sell. He buys after the launch: the curve holds a median 40 SOL at his
  buy (a fresh curve: 30).
- **1,982 coins priced in another token** (bought through a swap; the bot cannot trade them).

His own result on the SOL coins by slice, 5 days (`screen.py`):

| slice | buys | his net | a buy | note |
|---|---|---|---|---|
| all | 1,634 | +8.01 SOL | +0.005 | |
| tip >= 8M | 59 | +4.11 | +0.070 | Oct 1-20: +0.007 a buy, 9 of 16 days |
| size 1-2 SOL | 143 | +4.19 | +0.029 | Oct 1-20: +0.030 a buy, 15 of 20 days |
| creator he bought before (24 h) | 290 | +3.47 | +0.012 | |
| held 0-5 slots | 188 | +16.89 | +0.090 | known only after the fact |
| held 10 slots or more | 795 | -23.12 | -0.029 | known only after the fact |

He is a scalper: his winners are the trades he leaves within a few slots, his losers the ones he holds. Which is which
shows only after his buy.

## The copy (`tapes.py`, `copy.py`, `copy2.py`)

The curve's state at the end of each slot after his buy, from the chain. Our buy lands at the end of his slot (63%) or
the next (37%), bound at his post-trade price + 5%, 0.5 SOL, 1.25% fee each way, 3M buy tip, 0.5M sell tip:

| copied (Oct 21-25) | coins | his net | copy, best exit | days positive |
|---|---|---|---|---|
| all (a random 200 and the slices: 546) | 546 | +10.09 | +0.25 (sold 12 slots after his buy); -6.02 at 1 slot | 0-3 of 5 |
| the same, sold one slot before his first sell (no bot can know it) | 546 | | -4.49 | 0 of 5 |
| creator he bought before | 290 | +3.47 | -1.06 | 0-2 of 5 |
| tip >= 8M | 59 | +4.11 | +1.28 (8 slots) | 4 of 5 |
| size 1-2 SOL | 143 | +4.19 | +2.26 (12 slots) | 5 of 5 |

The size slice was chosen on these days, so it was tested on Oct 1-20 (254 coins, his net +7.62): **every exit loses**
(best -0.48 SOL at 3 slots, -1.73 at 12, 5 to 8 of 20 days positive; with our buy always a slot later, -2.26). The
tip slice does not hold for him on Oct 1-20 either.

## Why it fails where AHwJSnFN works

AHwJSnFN buys at the launch with a conviction that shows in his tip and size, and sells at a create-relative slot per
operator that the table knows, so a copy a pipeline behind him still rides the move. 6anjqMe buys into a curve that is
already moving and leaves on the price within a few slots. His edge is his entry price and his exit, and a copy a slot
behind him pays more and is still holding when he is out. Even an exit one slot before his own sell, which no bot can
know, loses on his coins.

His coins priced in another token are not covered: copying them needs the swap route into the quote token and a new
buy path in the bot.
