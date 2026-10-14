# rugsnipe

An automated, validated alternative to the manual "rugger sniping" method promoted by FOMO / F-Project
on Solana pump.fun: discover serial token ruggers from on-chain data, measure whether sniping them is
profitable, and execute at the lowest possible latency.

Status: research and validation first, then execution. See `docs/` in reading order:

| Doc | Content |
|---|---|
| `docs/00-transcript-digest.md` | What the three videos actually say: method, rules, bot features, 27 claims |
| `docs/01-claims-verification.md` | Each claim checked against primary sources and on-chain data |
| `docs/02-pumpfun-mechanics.md` | Pump.fun protocol in 2026 (CreateV2, Token-2022, fee program, curve, PumpSwap) |
| `docs/03-speed-infra.md` | How to be first: feeds, senders, geography, region recommendation vs AWS Ohio |
| `docs/04-strategy.md` | Our strategy, derived from the validation numbers |
| `docs/05-validation.md` | The validation system: discovery, backtest, results |
| `docs/06-architecture.md` | System design |
| `docs/07-deploy.md` | Deployment on AWS (or better) |
| `docs/research/` | Raw research notes with sources |

Layout:

```
analysis/   Python: data pull (public RPC + pump.fun API), rugger discovery, backtest, reports
crates/     Rust: hot path (feed, decoders, executor, senders)
deploy/     server setup, latency probes, systemd units
config/     example configuration
data/       committed small datasets and reports (raw caches are gitignored)
docs/       documentation
```
