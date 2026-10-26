# Wallets 4 (H9kuqGKN) and 6 (D5BDDkBt) live, Oct 26 night (docs/19 section 13)

These run against the session scratchpad: its Triton client (`month/tri.py`) and the pump parsers (`rivalpnl/`).

- `pull.py WALLET T0`: the wallet's landed transactions since T0 (compact rows, as `oos/pull.py`).
- `coins.py WALLET`: each coin whose first trade of the wallet's is a buy. Its create and class, as the bot reads them:
  `create_v2`, not mayhem, not cashback, priced in SOL. For the in-class launch buys at create + 0/1, every landed trade
  up to the wallet's last sell + 3 slots (at least create + 40, at most create + 160).
- `land.py`: our live copies on chain (our wallet's transactions since 13:00 UTC). Our landing slot and block index
  against the wallet's buy, our sell, and our net.
- `replay.py WALLET [PREM ...]`: our copy replayed on the tapes, with our real landings (the 10 nearest by the wallet's
  block index), each bound, and exits at create + h, its first sell + 1 and its last sell + 1. Three periods.
- `calib.py`: the replay's price model on our real fills, against our real net. It reads 0.0155 SOL high per fill.
- `why.py`: why the bot did not see some launch buys. Each coin's create on chain: token program, mayhem flag, quote
  mint.
