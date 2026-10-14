"""rugsnipe_analysis: validation tooling for the pump.fun 'rugger sniping' hypothesis.

Modules:
    rpc       -- polite, cached, round-robin Solana JSON-RPC client
    pumpapi   -- pump.fun frontend-api-v3 (coins) and swap-api (trades) clients
    events    -- base58 + pump.fun Anchor event decoding (CreateEvent, TradeEvent)
    curve     -- bonding-curve math (prices, market caps, quotes with fees)
    dataset   -- sampling + pulling of the token dataset (resumable)
    firstslot -- per-token first-slot metrics and aggregate statistics
    verify    -- RPC cross-check of the swap-api trade feed
    selftest  -- plain-assert unit tests (python3 -I -m rugsnipe_analysis.selftest)
"""

__version__ = "0.1.0"
