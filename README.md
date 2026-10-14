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
crates/     Rust workspace (docs/06 section 3)
  pump-codec       constants, PDAs, curve math, events, ix parse/build, TxView, CreateHit (wire + logs), launchers, transfers, Alpenglow
  rugsnipe-core    config + validation, Mode (paper default), signed ArmSet, Watchlist, RiskGate, kill file, rslog, IntentId
  rugsnipe-feed    Feed trait; LogsWs (free), PumpPortal (optional, off), collector relay, replay, recorder; dedup, FiredSet, health
  rugsnipe-tx      templates + PatchTable, NoncePool (no-double-buy rule), HotSigner, senders (Paper always; real ones feature-gated)
  rugsnipe-exit    exit engine: position machine, triggers, sell bounds, retries (pure decide())
  rugsnipe-state   state.db (SQLite WAL, one writer thread), Ledger, Recovery/Reconcile, exits Lease, RpcBudget
  rugsnipe-sim     template simulation against live curves (feature rpc)
  rugsnipe-ctl     operator CLI: config validate, template dump, nonce init/close (dry run), armset verify/keygen/sign
  rugsnipe         binaries: rugsnipe-hot, rugsnipe-replay, rugsnipe-guard
deploy/     server setup, latency probes, systemd units
config/     example configuration (rugsnipe.example.toml = production template, paper.example.toml = local paper runs)
data/       committed small datasets and reports (raw caches, data/raw/ and state are gitignored)
docs/       documentation
```

## Running (paper mode)

Paper is the default and the only mode that runs without keys: `rugsnipe-hot` generates a throwaway
wallet at start, the only sender is the paper recorder (it never opens a socket), every create gets a
`CreateHit` and a `Decision` record in rslog, intents/sends/fills/positions/exits go to `state.db`, and
paper positions open at the E2 state (end of slot+1) and are exited by the exit engine with simulated
sell fills. Probe and live are refused unless every check of docs/06 section 6 passes (caps, cu_limit,
credentials, clock, post-exec feed, signed `gate_passed` ArmSet, `--live` + arm-live token), and real
senders are only compiled with the `real-senders` feature.

```sh
cargo build --release -j 2
# 1. a dev ArmSet signing key OUTSIDE git (keygen refuses paths inside the repo except keys/)
target/release/rugsnipe-ctl armset keygen --out /secure/armset-dev.key      # prints the pubkey
#    put the pubkey into config/paper.example.toml [secrets].armset_pubkey (or a copy of it)
# 2. an ArmSet from the analysis data, signed
python -I analysis/scripts/make_paper_armset.py --kind operators --ver 1 --out /tmp/ops.json
target/release/rugsnipe-ctl armset sign --key /secure/armset-dev.key --in /tmp/ops.json --out-dir data/raw/paper
target/release/rugsnipe-ctl armset verify data/raw/paper/armset-1-<sha>.json --pubkey <pubkey>
# 3. paper run on the free logsSubscribe feed (Ctrl-C or --duration-s to stop; prints a JSON summary)
target/release/rugsnipe-hot --config config/paper.example.toml \
    --armset data/raw/paper/armset-1-<sha>.json \
    --state data/raw/paper/state.db --rslog-dir data/raw/paper/rslog \
    --record data/raw/feed --duration-s 1200 --summary data/raw/paper/summary.json
#    metrics: curl -s 127.0.0.1:9464   kill (buys stop, exits continue): touch data/raw/paper/KILL or kill -USR1 <pid>
# 4. replay a recording through the same hot loop (counting allocator + latency histograms)
target/release/rugsnipe-replay --frames data/raw/feed/logsws-<ts>-0000.jsonl \
    --state none --rslog-dir /tmp/replay-rslog --assert-zero-alloc --p99-max-us 100
# 5. guard skeleton: watches the exits lease, dry-run sells when the hot box stops renewing it
target/release/rugsnipe-guard --config config/paper.example.toml --state data/raw/paper/state.db
# operator CLI
target/release/rugsnipe-ctl config validate config/paper.example.toml
target/release/rugsnipe-ctl template dump --mint <MINT> --wallet <WALLET> --format v1
target/release/rugsnipe-ctl nonce init --authority <WALLET> --count 32          # unsigned, dry run
```

`--paper-unsigned-armset` (paper only) loads an ArmSet whose signature does not verify; without it an
unsigned, stale or wrong-regime ArmSet means exits only. `feeds.pumpportal` stays off: PumpPortal allows
one connection per IP and the collector normally holds it.

Checks: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check`.
