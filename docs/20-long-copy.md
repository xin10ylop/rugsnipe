# 20. The long copy: wallet 4's big buys of coins priced in other tokens (Oct 27)

`rugsnipe-long`, its own process and wallet beside the hot unit. Research in
`analysis/research/2026-10-27/h9long/`, code in `crates/rugsnipe-long/` and `crates/pump-codec/src/multihop.rs`.

## 1. How H9kuqGKN makes its money

- **+355 SOL in 8 days** (Oct 19 to Oct 27; its SOL rose +412 with +11.9 deposited, 467 SOL now). Every day positive.
- **Where:** pump coins priced in other tokens (WBTC, PUMP, TTWO, USDC, xStocks, graduated pump coins): +278 on 3,056
  coins. SOL-priced pump coins +29, PumpSwap-only tokens +17, the quote tokens it holds +29.
- **Its rule:** one buy (tiers of about 0.9, 1.13 and 1.5 SOL), one sell of everything at **2.5x** the price its buy
  left, at **0.7x**, or after **30 minutes** (77% of its trips). About 10% of trips reach 2.5x; they carry all the
  profit. Its 1.5-SOL buys reach it about twice as often as the small ones.
- **The check view's per-coin numbers for this wallet are wrong coin by coin** (a coin sold into its quote token shows
  its profit on the quote token's row); only its total is right.

## 2. The copy, and the test it passed

Copy its 1.5-SOL buys of coins priced in other tokens, 1 SOL each, landing in its slot or the next; sell at **2.0x**
its price, **0.8x**, or **29:50**. Picked on Oct 19-22, scored on Oct 23-27, worst-case fees (0.0095 SOL a
transaction), the replay's 0.0155 SOL bias off every fill, **its own sale placed ahead of ours whenever the two are
within a slot** (`exits_pess.py`):

| | Oct 19-22 | Oct 23-27 (out of sample) |
|---|---|---|
| all its 1.5-SOL other-quote buys | +39.5 (+35.9 without the best 2) | +15.3 (+12.2) |
| **the ones pump's own programs can route (what is built)** | +11.0 (+8.5) | **+12.2 (+9.3); at most 3 open: +11.0 (+8.2)** |
| SOL-priced coins | -11.1 | -2.3 |

- **2.0x and not its 2.5x:** its sale is as fast as ours and big enough to move the price. At 2.45x its sale came first
  on 83 of 126 take-profits; at 2.0x on 8.
- **We don't take its profit:** our buy lifts the price by our own tokens (about 6%, 8.5% at 2x) and our sell takes
  exactly that back out; once we are out the curve is where it would be without us.
- **Risks:** the edge is shrinking (its big buys hit 2.5x 29% of the time on Oct 19, about 10% now; about +4 SOL a
  day on Oct 23-27 against +10 before); a few take-profits carry everything; Oct 23-27 rests on WBTC-routed coins
  (+10.0 of +12.2).

## 3. How it trades (`crates/rugsnipe-long`)

- **Feed:** a Yellowstone stream of its own (the hot config's `[feeds] grpc`): wallet 4's landed transactions, every
  transaction of ours (failed too), every transaction on a coin we hold, slots.
- **Trigger:** its buy of a pump coin whose curve quote is not SOL, valued at **1.45 SOL or more** (its 1.5 tier):
  wrapped SOL into its `multi_hop_swap`, or the share of what it sent Jupiter that the buy used (it sends 5/3 SOL and
  keeps about a tenth), or a held token at the oracle price. Its second buy of a coin, a coin already ours, a complete
  curve, `max_open`, the daily stop and a `KILL` file in the state dir are skips.
- **Route:** PumpSwap's `multi_hop_swap` (16 accounts + 5 a hop), byte-equal to wallet 4's own (`tests/multihop_fixtures.rs`).
  Its own route is copied when it starts from wrapped SOL or a float we hold (WBTC); a direct curve buy goes behind its
  quote's canonical pool against wSOL or WBTC. Wrapped SOL is wrapped and unwrapped inside the transaction. At most 2
  hops (a 3-hop transaction is over 1,232 bytes); no Jupiter leg (that part made +0.4 out of sample).
- **Buy:** one transaction, one sender (two landing would double the position) plus the same bytes by
  `sendTransaction`; `min_out` from its own fill at `max_premium` (25%).
  A paper buy fills on the curve at the end of its landing slot and is refused by the same `min_out`
  (journal `long paper-refused #N`, state failed, the fee a landed failure pays): 2026-10-27, coins it buys can
  double within 2 seconds (6y9aDagF: 1.2x after 1 s, 2.0x after 2 s), and a paper fill past the limit would
  book a trade the real buy never gets.
- **Exits:** on every trade of the coin, the curve price against the price its buy left, **without our own tokens**:
  take profit 2.0x, stop 0.8x, 1790 s after its buy, the curve completing, or **its own sell** when it leaves first
  (2026-10-27, CkFH2smc: it sold after 14 minutes and the copy held to 30; the backtest's price path ends at its sale,
  so its copies sell right behind it: `grid.py mix`, 1 of 135 copies on P1 and 8 of 136 on P2; journal
  `long its-sell #N <mint>: out behind it`, exit `its_sell`). The sell goes back along the route,
  closes the wSOL and coin accounts (their rent comes back), and is retried with a lower floor if it fails or drops.
- **When things fail** (2026-10-27, after the hot bot's 2RR6sfbi): a sell that lands and fails goes again at once 3
  times (the floor drops each time), then waits 5 s, 30 s, 2 min, and past `max_sell_fail_sol` (0.01) of failed fees
  on the position every 10 min; never given up (journal `long CRITICAL sells of #N ... keep failing`). A coin whose
  curve completes in any state (a sell already out included) sells on its canonical PumpSwap pool, as after 3 failed
  sells when the pool exists (simulated on mainnet: C5xph2yL and EJAxERvK into WBTC, `examples/sim_pool_sell.rs`). A
  buy not seen in 90 s is given up as dropped; if it lands after all (`long late-fill`), the position reopens on its
  tokens, also when it landed while the bot was down. Statuses search the whole history, so a sell that landed while
  the bot was down closes the position instead of being sold again.
- **Accounting is chain-true:** a position's result is the sum of our wallet's lamport changes over its transactions,
  plus a float's token change at the oracle price. Paper (`mode = "log"`) prices the same decisions on the live curve.
- **Measured on mainnet** (simulation from our wallet, a 2-hop wSOL route): buy 1,088 bytes, 166k compute units; buy
  and sell back in one transaction 1,158 bytes; the round trip costs about 3% at 1 SOL (the backtest charged 5.1%).

## 4. Runbook (fra1)

```
cd ~/rugsnipe && git pull && sudo bash deploy/install.sh --role hot
sudo /opt/rugsnipe/bin/rugsnipe-long keygen --out /etc/rugsnipe/keys/long-wallet.json   # prints the new address
```

1. **Paper first.** `/etc/rugsnipe/long.toml` is created with `mode = "log"`. Set `rpc_url = "cred:RPC_URL"` to a line of
   the api-keys file that holds your RPC URL, then `sudo systemctl enable --now rugsnipe-long`. The journal shows each
   of its buys, our paper buy and close; `bash ~/check.sh` shows the book at the end.
2. **Fund the wallet** (the address keygen printed): `max_open` x `stake_sol` + about 0.1 SOL for fees, split between
   SOL (routes from wrapped SOL) and the WBTC float: `sudo /opt/rugsnipe/bin/rugsnipe-long float buy
   3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh 3.0 --config /etc/rugsnipe/long.toml` (through Jupiter).
3. **Real:** `mode = "on"` in long.toml, `sudo systemctl restart rugsnipe-long`.
- **Stop new buys:** `sudo touch /var/lib/rugsnipe-long/KILL` (exits continue); remove it to resume.
- **The book:** `sudo /opt/rugsnipe/bin/rugsnipe-long status --config /etc/rugsnipe/long.toml --chain`.
- **Floats back to SOL:** `... float sell 3NZ9JMVBmGAqocybic2c7LQCJScmgsAZ6vQqTDzcqmJh all --config ...`.
