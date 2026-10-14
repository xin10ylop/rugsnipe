# rugsnipe

**What this is.** A research-first replacement for the manual "rugger sniping" method promoted by FOMO /
F-Project for Solana pump.fun launches. It checks every claim of that method against on-chain data,
searches for a policy that survives honest out-of-sample tests after all costs, and carries a Rust hot
path (feed, decode, decide, sign, send, exits) and the deployment to trade the one policy that survives,
in paper/shadow mode first. No mode that moves money runs until a pre-registered go/no-go rule passes.

**The verdict in five lines** (docs/04):

1. His rule as stated (block-0 entry behind the rugger's bundle, hold for +100%) loses: −10.5% per trade out of sample, −21% on an independent holdout; copy-trading, per-rugger arming and mayhem coins lose too.
2. One family survives: the **rank-1 micro-scalp**: buy 0.5 SOL right behind a multi-wallet bundle in the creation slot (fill capped at +2% over the post-bundle price) and sell within 1-3 s. +2.5% to +3.7% per trade on holdouts no search touched (+3.2% on the 480 pre-registered fresh launches); +4.6% when skipping the bottom tertile of a frozen P(take-profit) score.
3. It holds only at rank 1: one sniper ahead of us takes it to about 0; the critic's realistic-fill stack gives −0.4% [−2.0, +1.3] on the fresh holdout (`data/reports/strategy/critic_realistic.json`, reproduced by `analysis/scripts/strategy/critic_realistic.py`). Our real landing rank is unmeasured.
4. The business is decided by the landing rate and by what a lost race costs: 0.003 SOL per failed attempt needs a 12% landing rate to break even at 0.5 SOL, the modelled 0.00036-0.001 SOL needs 1.6-4.4%. The bid goes into a tip paid only on success where the leader orders by tip (Jito-Agave), and into a capped priority fee where it orders by fee (BAM, Harmonic, other: 0.00073 SOL per failed attempt at the defaults; docs/03 §6.1).
5. Next: shadow mode on a Frankfurt box against a fixed go/no-go rule, then a two-stage probe with a kill rule: probe-A measures landing, premium and failed-attempt cost at 0.02 SOL; probe-B (about 200 landed trades at 0.25 SOL) only if A clears the break-even. The runbook is docs/08.

| Doc | Content |
|---|---|
| [`docs/00-transcript-digest.md`](docs/00-transcript-digest.md) | What the three videos actually say: method, rules, bot features, 27 claims |
| [`docs/01-claims-verification.md`](docs/01-claims-verification.md) | Each claim checked against primary sources and on-chain data |
| [`docs/02-pumpfun-mechanics.md`](docs/02-pumpfun-mechanics.md) | Pump.fun protocol in 2026 (CreateV2, Token-2022, fee program, curve, PumpSwap) |
| [`docs/03-speed-infra.md`](docs/03-speed-infra.md) | How to be first: leaders by metro, feeds, senders, the per-class bid and pay on success (§6.1), Frankfurt vs AWS Ohio |
| [`docs/04-strategy.md`](docs/04-strategy.md) | **The strategy verdict**: what works, what does not, the policy, the go/no-go rule (§7) |
| [`docs/05-validation.md`](docs/05-validation.md) | The validation system: discovery, backtest, holdouts, results |
| [`docs/06-architecture.md`](docs/06-architecture.md) | System design: crates, data model, configuration (§6), hot path, exits, modes |
| [`docs/07-deploy.md`](docs/07-deploy.md) | Deployment: topology, install, phases, costs, kill and alert procedures |
| [`docs/08-runbook.md`](docs/08-runbook.md) | **Operator runbook**: rent and accept the box, install, shadow mode, reading the go/no-go, probe, kill switch, Alpenglow, weekly checks, costs, keys |
| [`docs/research/`](docs/research/) | Raw research notes with sources |

Layout:

```
analysis/   Python: data pull (public RPC + pump.fun API), rugger discovery, backtest, reports
crates/     Rust workspace (docs/06 section 3)
  pump-codec       constants, PDAs, curve math, events, ix parse/build, TxView, CreateHit (wire + logs), launchers, transfers, Alpenglow
  rugsnipe-core    config + validation, per-class bid policy (pay on success), Mode (paper default), signed ArmSet, Watchlist, RiskGate, kill file, rslog, IntentId
  rugsnipe-feed    Feed trait; LogsWs (free), PumpPortal (optional, off), collector relay, replay, recorder; dedup, FiredSet, health
  rugsnipe-tx      templates + PatchTable, NoncePool (no-double-buy rule), HotSigner, senders (Paper always; real ones feature-gated)
  rugsnipe-exit    exit engine: position machine, triggers, sell bounds, retries (pure decide())
  rugsnipe-state   state.db (SQLite WAL, one writer thread), Ledger, Recovery/Reconcile, exits Lease, RpcBudget
  rugsnipe-sim     template simulation against live curves (feature rpc)
  rugsnipe-ctl     operator CLI: config validate, template dump, nonce init/close (dry run), armset verify/keygen/sign
  rugsnipe         binaries: rugsnipe-hot, rugsnipe-replay, rugsnipe-guard
deploy/     server setup, latency probes, systemd units
config/     example configuration (rugsnipe.example.toml = production template, paper.example.toml = local paper runs,
            paper-lead.example.toml = the docs/04 lead policy in shadow mode: E1 fills, lead exits, failed-attempt costs)
data/       committed small datasets and reports (raw caches, data/raw/ and state are gitignored)
docs/       documentation
```

## Running (paper mode)

Paper is the default and the only mode that runs without keys: `rugsnipe-hot` generates a throwaway
wallet at start, the only sender is the paper recorder (it never opens a socket), every create gets a
`CreateHit` and a `Decision` record in rslog, intents/sends/fills/positions/exits go to `state.db`, and
paper positions open at the E2 state (end of slot+1) and are exited by the exit engine with simulated
sell fills. Probe and live are refused unless every check of docs/06 section 6 passes (caps, cu_limit,
credentials, clock, post-exec feed, a signed fresh ArmSet carrying `probe_allowed` for probe or
`gate_passed` for live, `--live` + arm-live token for live), and real senders are only compiled with the
`real-senders` feature.

```sh
git config core.hooksPath .githooks   # once per clone: the pre-commit hook refuses key material
                                      # (keys/, keypair JSON arrays, PEM private keys); install.sh does it too
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
#    the lead policy (docs/04) in shadow mode: a class ArmSet (every create) + config/paper-lead.example.toml;
#    the summary's cold.paper block has landing rate, premium, exits by reason, net P&L and EV per attempt
python -I analysis/scripts/make_paper_armset.py --kind class --ver 3 --out /tmp/class.json
target/release/rugsnipe-ctl armset sign --key /secure/armset-dev.key --in /tmp/class.json --out-dir data/raw/paper-lead
target/release/rugsnipe-hot --config config/paper-lead.example.toml --armset data/raw/paper-lead/armset-3-<sha>.json \
    --state data/raw/paper-lead/state.db --rslog-dir data/raw/paper-lead/rslog --duration-s 1200
python -I analysis/scripts/paper_vs_sim.py --run data/raw/paper-lead --out data/reports/paper_lead/vs_sim.json
# 4. replay a recording through the same hot loop (counting allocator + latency histograms)
target/release/rugsnipe-replay --frames data/raw/feed/logsws-<ts>-0000.jsonl \
    --state none --rslog-dir /tmp/replay-rslog --assert-zero-alloc --p99-max-us 100
# 5. guard skeleton: watches the exits lease, dry-run sells when the hot box stops renewing it
target/release/rugsnipe-guard --config config/paper.example.toml --state data/raw/paper/state.db
# operator CLI
target/release/rugsnipe-ctl config validate config/paper.example.toml
target/release/rugsnipe-ctl template dump --mint <MINT> --wallet <WALLET> --format v1   # per-mint CU limit
cargo run --release -p rugsnipe-ctl --features rpc -- sim --n 3 [--nonce <ACCT>]       # read-only CU fit
target/release/rugsnipe-ctl nonce init --authority <WALLET> --count 32          # unsigned, dry run
```

`--paper-unsigned-armset` (paper only) loads an ArmSet whose signature does not verify; without it an
unsigned, stale or wrong-regime ArmSet means exits only. `feeds.pumpportal` stays off: PumpPortal allows
one connection per IP and the collector normally holds it.

Checks: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all --check`.
