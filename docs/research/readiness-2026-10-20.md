# Readiness for probe stage A (Oct 23, fra1): verdict and plan

Date 2026-10-20, HEAD `0aae91c`. Read-only audit of five areas (probe start path, strategy fidelity, Alpenglow
and feed, operations, research changes). Each area had an auditor and an adversarial verifier. This file merges
them. **Rule used:** a finding blocks probe A only if the verifier confirmed or upgraded it, or found it
missing from the audit. Where the auditor and verifier disagreed I opened the cited files myself; §7.0 lists
what I checked. Line numbers are at `0aae91c`.

Status words used below:
**tested** = built and unit-tested (mocks or recorded data);
**untested-live** = built, but has never run against the real chain or a real provider;
**partial** = some of it is built;
**missing** = not built;
**wrong** = built or documented, but does the wrong thing for probe A.

---

## 1. Verdict

1. **Not ready as the runbook stands, but it can be ready for a reduced probe A on Oct 23.** Followed as
   written, the probe would stop buying 6 hours after its ArmSet (the signed permission file) is signed. It
   could not fire at all, because nothing creates the 32 nonce accounts. It would trade the wrong exits. It
   would use up its 50 daily attempts before 13:00 UTC (or before 01:30 UTC at the runbook's 40 per hour).
   And its gate script would declare stage A complete on day 3. All six items in §2 are config, operator or
   small Python fixes, so they fit into Oct 21-22, with the gate fix due by Oct 26.
2. **What is proven is the paper side only.** That is the 14-hour Ohio shadow run (3,091 attempts, EV +1.32%
   per attempt, docs/research/probe-design-v2.md:6), the LEAD exit replay, the logs-websocket feed, and the risk
   caps, kill latch and deploy layer under unit tests (`bash deploy/selftest.sh`: 31 passed, 0 failed, rerun
   today with stub binaries). **What is built but has never touched the real chain** is everything that moves
   money or reads the paid feed: the five senders, the Triton gRPC adapter, the real fill lane (our own
   TradeEvent, `meta.fee`), nonce seeding and burns, guard real sells, and the systemd units running the real
   binaries.
3. **What is missing** is choosing which launches to buy before sending. The skip filter and the
   multi-wallet-bundle universe act only on paper (crates/rugsnipe/src/hot.rs:1064-1101, 1556-1607), so real
   buys go to every class-eligible create. Also missing: the index-probe lane, quarter-hour spacing, a tool to
   create nonce accounts, a dead-man alert, the direct TPU lane and a shred feed. Probe A therefore measures the
   vendor-sender stack on all launches, unfiltered. §2 item 4 makes that an explicit, written decision.

---

## 2. MUST DO BEFORE PROBE A (ranked)

Only items whose absence makes the measurement invalid, unsafe beyond the daily cap, or unable to start.
Six items. Items 1-4 and 6 must be done by Oct 22. Item 5 must be done before the first verdict read (Oct 26).

### M1. Make the probe ArmSet valid for the whole stage
- **What:** sign the stage-A ArmSet with `valid_until = created_at + 11 days`. Set
  `[risk] armset_max_age_s = 950_400` (11 days) in `/etc/rugsnipe/rugsnipe.toml`. Write both into
  docs/08 §9 step 1 as "one ArmSet per stage". On ohio1, before signing:
  ```bash
  /opt/rugsnipe/venv/bin/python -I analysis/scripts/make_paper_armset.py --kind class --ver 2 --probe-allowed --probe-stage a --out /tmp/probe-a.json
  python3 -c 'import json,sys; p=sys.argv[1]; a=json.load(open(p)); a["valid_until"]=a["created_at"]+11*86400; open(p,"w").write(json.dumps(a,indent=1,sort_keys=True)+"\n")' /tmp/probe-a.json
  sudo /opt/rugsnipe/bin/rugsnipe-ctl armset sign --key /etc/rugsnipe/keys/armset-signing.key --in /tmp/probe-a.json --out-dir /tmp/armset
  ```
- **Why:** the script hard-codes 6 hours (analysis/scripts/make_paper_armset.py:199-200). Loading refuses a
  file older than `armset_max_age_s` (default 21,600 s) or past `valid_until`
  (crates/rugsnipe-core/src/armset.rs:306-309). A running probe silently drops to paper after 6 h, with one
  journal line `armset: vN is stale` (crates/rugsnipe/src/cold.rs:2740-2748). Every later restart, unkill or
  reboot of hot is refused (crates/rugsnipe/src/app.rs:331-342), and so is the guard's `--real-sell`. The
  reload only follows the same file's mtime, but the file name carries the sha (cold.rs:2751-2757). **If
  skipped:** at most 6 h of real attempts per signing, about 40 manual re-sign/copy/edit/restart cycles in 10
  days, and silent data gaps. No SOL is at risk.
- **Effort:** 1 hour. **Who:** operator step plus a doc line. A `--valid-hours` flag in the script is
  optional. **Files:** analysis/scripts/make_paper_armset.py, docs/08-runbook.md §9 step 1, the probe config.

### M2. Create the 32 buy nonce accounts before the first real start
- **What:** install the Agave CLI for root (deploy/frankfurt-checklist.md:92). Check that the derivation
  matches the bot. Then create `rs-buy-0` … `rs-buy-31` with the probe key as base and authority:
  ```bash
  P=$(sudo solana-keygen pubkey /etc/rugsnipe/keys/probe-keypair.json)
  sudo solana create-address-with-seed --from $P rs-buy-0 11111111111111111111111111111111
  /opt/rugsnipe/bin/rugsnipe-ctl nonce init --authority $P --count 1 | grep '#0'      # same address, or stop
  for i in $(seq 0 31); do
    sudo solana -u mainnet-beta -k /etc/rugsnipe/keys/probe-keypair.json create-nonce-account \
      /etc/rugsnipe/keys/probe-keypair.json 0.00105664 --seed rs-buy-$i --nonce-authority $P; sleep 2; done
  ```
  The rent is 1,056,640 lamports each (crates/rugsnipe-tx/src/sysix.rs:12-13; about 0.034 SOL in total). If
  the CLI refuses that amount, use what `solana rent 80` prints. The public RPC keeps a keyed URL off the
  command line (review A5). Finish with the `--probe-dry` of §3, which must print
  `dry nonce burn … simulateTransaction err=null`, not
  `dry self-test: no usable nonce account` (crates/rugsnipe/src/bin/rugsnipe-hot.rs:83).
- **Why:** every real buy needs a pool nonce (app.rs:446-452), and the repo cannot create them:
  `rugsnipe-ctl nonce init` only prints unsigned transactions (crates/rugsnipe-ctl/src/lib.rs:21-23, 565-645).
  The derivation `createWithSeed(probe, "rs-buy-i", System)` is at sysix.rs:15-27 and 96-118. **If skipped:**
  every decision is `NO_NONCE` and no data is collected. **Order matters:** a real start on
  `/var/lib/rugsnipe/state.db` before the accounts exist quarantines the missing slots for good
  (app.rs:862-877; crates/rugsnipe-state/src/recovery.rs:209-235 never resets them). The slot is a hash of the
  mint modulo 32 with no fallback slot, so those launches are skipped from then on. To recover, with hot
  stopped, run `sudo -u rugsnipe sqlite3 /var/lib/rugsnipe/state.db "DELETE FROM nonces"`. Dry runs always use
  their own `--state`.
- **Effort:** 2-3 hours, including the CLI install. **Who:** operator step plus a runbook row (docs/08:399).
  **Files:** docs/08-runbook.md §9 step 3 table (`tx.nonce_pool`), deploy/frankfurt-checklist.md:92.

### M3. One probe-A config file: LEAD exits, 2 attempts per hour, CU floor 88,000
Three values whose absence corrupts the measurement. All three are config, no code. Ship them as
`config/probe-a.example.toml` and point docs/08 §9 step 3 at it. Do not edit `rugsnipe.example.toml`:
crates/rugsnipe/src/setup.rs:371-378 has a test that pins that file's exits.
- **(a) `[exit.default]` = the LEAD block from config/paper-lead.example.toml:143-157, and
  `risk.max_entry_vsol_lamports = 115_000_000_000` (paper-lead:51).**
  *Why:* the documented base (docs/08-runbook.md:382-383) is config/rugsnipe.example.toml. Its exits
  (lines 118-128) are take-profit at 2x and 3x, a -40% stop after 10 s, 150 s idle and a 30-minute hold. With
  stage A's single position slot (crates/rugsnipe-core/src/config.rs:879-886), one position blocks all attempts
  for minutes. It also measures a different policy from the shadow it is compared with. It can push realised
  loss to the 0.2 SOL cap, and hitting the cap latches the kill: a false kill caused by config. The 40 SOL
  entry cap (example:42) skips large-V0 launches that the shadow scored. `config validate` catches none of this
  (verifier ran target/debug/rugsnipe-ctl).
- **(b) `risk.max_buys_per_hour = 2`.** *Why:* the stage profile does not narrow the hourly cap
  (config.rs:869-875, 1181-1196). The cap counts first-come per UTC hour (crates/rugsnipe-core/src/risk.rs:503,
  574). The runbook sets 40 (docs/08:394), which uses up the 50 a day by about 01:15 UTC. The design's 4 per
  hour uses them up by about 12:30 UTC. Either way UTC hours 13-23, the busiest pump hours, are never sampled,
  and that cannot be fixed afterwards (probe-design-v2.md:26-33 wants every hour about 7 times). 2 per hour
  gives 48 a day across all 24 hours.
- **(c) `tx.cu_limit = 88_000`.** Keep the runbook's value (docs/08:398). Do not use the 65,000 that the
  example comment (example:91) and the validator text recommend. *Why:* a fresh probe wallet has no pump
  `user_volume_accumulator` (UVA). The buy creates it (crates/pump-codec/src/instructions.rs:869) at about +1.3
  to +2.8k CU (docs/research/gap-5.md:68). The per-mint limit assumes the UVA exists and leaves 1,050 CU to
  spare on legacy (crates/rugsnipe-tx/src/cu.rs:13-16, 46-58; docs/06-architecture.md:354). At a 65k floor,
  every landed buy can fail `ComputationalBudgetExceeded`, and the failure reverts the UVA creation every time.
  That gives 0% fills by construction, which shadow_gate counts as lost races
  (analysis/scripts/shadow_gate.py:296-301), and so a false "stop". At 88k the first fill fits up to S+W ≈ 14
  and creates the UVA. The cost is about 20% less priority per CU on BAM, Harmonic and other leaders only; no
  extra lamports are spent (cu.rs:31-34). Move to 65k for stage B, after
  `rugsnipe-ctl sim --wallet <PROBE> --legacy` passes on the probe wallet.
- **While in the file (each a loud refusal if missed, not a measurement risk):** `mode = "probe"`,
  `[probe] stage = "a"`, `grpc = { enabled = true, url = "https://<name>.rpcpool.com", x_token = "GRPC_TOKEN",
  stale_ms = 4_000, bundle_run = true }`, `logsws.enabled = false`, `rpcsend.enabled = false`, real
  `secrets.armset_pubkey` and `cold_wallet` (refused as `<base58>`, config.rs:1687-1706),
  `min_hot_balance_lamports = 100_000_000`, `max_consec_losses_per_op = 20`, `armset_max_age_s = 950_400`
  (M1), `lads_bytes = 12_124_160`, and the five senders enabled.
- **Effort:** 1-2 hours. **Who:** config file plus a doc pointer. **Files:** config/probe-a.example.toml (new),
  docs/08-runbook.md §9 step 3 (lines 382-401).

### M4. Decide, in writing, what probe A measures
- **What:** the owner amends docs/research/probe-design-v2.md §2 and docs/08 §9 before money moves:
  1. Probe A runs **unfiltered on every class-eligible create**. The build cannot select bundled launches or
     apply the skip filter before sending (hot.rs:1064-1101; e1.rs:450-455; docs/08:20). The outcome-store
     pre-warm has no effect on real buys.
  2. Keep `grpc.bundle_run = true`, so every attempt gets a `feed_bundle` row with `bundle_wallets`
     (cold.rs:2427-2476; crates/rugsnipe-state/src/schema.rs:57).
  3. The stage-B entry is read on the **in-class subset** (`bundle_wallets >= 1`), never on all attempts.
  4. Pick one of these, because the subset is only 10-20% of attempts (docs/04-strategy.md:222; verifier's
     paper runs 32/153). At 50 a day that is about 50-100 in 10 days, below the 200-attempt landing rule:
     - (i) extend A in calendar days until the subset reaches 200;
     - (ii) raise `[probe] a.max_probes_per_day` (the 0.2 SOL loss cap stays);
     - (iii) delay A for a pre-send gate, which is days of Rust.
- **Why:** unbundled launches face fewer rivals and fill easily under a bound that assumes 4 SOL of bundle
  (hot.rs:236). The all-attempts landing rate is therefore inflated, and a B decision on it would be wrong.
  The 80-90% of attempts spent off-target cannot be recovered afterwards.
- **Effort:** 1 hour. **Who:** owner decision plus a doc edit. **Files:** docs/research/probe-design-v2.md §2
  and §9, docs/08-runbook.md §0 and §9.

### M5. Bring `shadow_gate.py --probe` to the v2 rules (deadline Oct 26, before the first verdict read)
- **What:**
  - Completion at 350 attempts **and** 7 days, with a hard stop at 10 (now 300 **or** 3 days:
    analysis/scripts/shadow_gate.py:94, 380-384; analysis/rugsnipe_analysis/selftest_probe.py:81-86).
  - An in-class split by `feed_bundle.bundle_wallets >= 1` (SQL_PROBE, shadow_gate.py:76-87, has none).
  - Count as landed only create-slot fills at a premium of +2% or less, and report slot+1..+3 fills
    (`LateFill`, cold.rs:2121-2124) separately.
  - Split failures by `fills.fail_reason`, so compute or instruction errors are not read as lost races
    (:296-301).
  - B-entry bar = the larger of the pooled LEAD break-even (about 9.9%, :340-343) and the unfiltered shadow's
    break-even at 0.25 SOL (about 36%, probe-design-v2.md:83).
  - A `by_sender` block.
  - Change docs/08:314 and config/rugsnipe.example.toml:113 ("300 attempts or 3 days") to match.
- **Why:** as shipped, the gate prints `PROBE-A COMPLETE: ENTER PROBE-B` or `… stop` on day 3, with at most
  150 attempts, on the diluted population and against the wrong bar. It either sends the operator into
  0.25 SOL stage B or stops the project on an invalid read.
- **Effort:** 3-4 hours, Python plus selftest. **Who:** code. **Files:** analysis/scripts/shadow_gate.py,
  analysis/rugsnipe_analysis/selftest_probe.py, docs/08-runbook.md:314.

### M6. Check the probe wallet for stray tokens before every start (plausible; cheap)
- **What:** before each start, list the wallet's token accounts. Any non-zero balance whose mint is not an
  open position must be closed before you start (spl-token burn and close with the probe key) or escalated to
  an engineer:
  ```bash
  for prog in TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb; do
    curl -s https://api.mainnet-beta.solana.com -X POST -H 'Content-Type: application/json' \
      -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getTokenAccountsByOwner\",\"params\":[\"$P\",{\"programId\":\"$prog\"},{\"encoding\":\"jsonParsed\"}]}" \
    | python3 -c 'import json,sys; [print(a["account"]["data"]["parsed"]["info"]["mint"], a["account"]["data"]["parsed"]["info"]["tokenAmount"]["amount"]) for a in json.load(sys.stdin)["result"]["value"]]'
  done
  ```
- **Why:** at start, recovery adopts every non-zero SPL or Token-2022 balance as an open position
  (crates/rugsnipe-state/src/chain.rs:32-53, 124-135; recovery.rs:177-198), and the adoption takes the risk
  gate's position slot (app.rs:822-835; hot.rs:1432-1453). Stage A has one slot, so a single spam airdrop
  silently stops every attempt after any restart. Such a position never closes: it has no pump curve to sell
  into, and closes only on a zero balance. I confirmed this code path. How likely airdrops are is not verified.
- **Effort:** 15 minutes per start (operator). The code fix (adopt only mints with a pump curve or one of our
  intents) takes hours and belongs in §4. **Files:** docs/08-runbook.md §9 step 6 (pre-start check).

### 2b. Cheap, not blocking: do them on Oct 22 while in the same files (minutes each)
- Add `--busy-poll` to the hot drop-in line (docs/08:445 omits it; the base unit has it,
  deploy/systemd/rugsnipe-hot.service:29). Without it there are 200 µs idle sleeps (review C1).
- api-keys example: move the inline `# …` comments to their own lines (docs/08:376, 379 fail the parser at
  crates/rugsnipe-tx/src/senders/mod.rs:424-441). Add `GRPC_TOKEN`, `RPC_URL`, `TG_TOKEN` and `TG_CHAT` to that
  block (docs/08:375-381 lacks them).
- Enable `rugsnipe-guard`, `rugsnipe-hot` and `rugsnipe.target`, not `rugsnipe-keeper`: it has no binary
  (deploy/systemd/rugsnipe-keeper.service; docs/07-deploy.md:8). It would loop with 203/EXEC.
- Use `sudo` or `sudo -u rugsnipe` on the runbook's reads of `/etc/rugsnipe` and `/var/lib/rugsnipe`
  (docs/08:160, 181, 197, 230-234, 413). Make the probe key with `rugsnipe-ctl armset keygen --out …` or
  `solana-keygen` once the Agave CLI is in. Skip `hot-keypair.json` (unused).
- Use a paid RPC (Solana Vibe Station Basic or Helius Developer), not Helius Free. The real lane alone is near
  10 req/s (crates/rugsnipe/src/real.rs:47-58; infra-shopping-2026.md §3).
- Telegram (`TG_TOKEN`/`TG_CHAT`) is the only alert path. Configure it.

---

## 3. FIRST REAL RUN CHECKLIST (fra1)

Each line is a part that has never touched the real chain, and what to look at. A–D are on Oct 22 before
funding/real start, E onward in the first hour of Oct 23.

| # | Never run live | Look for (command / log line) |
|---|---|---|
| A | systemd credentials, real key load, api-keys parse | `--probe-dry` (docs/08 §9 step 5) with `--duration-s 3600`: one `dry self-test: <sender> variant 0 sig … tip … prio …` per sender (5 lines); no `credential … value must be quoted` / `group/world-readable credential` |
| B | Triton gRPC adapter after Alpenglow | `grpc feed https://…: connected true slot <N> (… ms)`. **Read it yourself: the dry run does not refuse an unhealthy feed** (rugsnipe-hot.rs:438, `!healthy && !dry`); summary `cold.grpc.convert_errors = 0`, `stale_reconnects` low, `checks.drift_slots` ≤ 4 |
| C | nonce seeding, burn tx, buy layout | `dry nonce burn …: simulateTransaction err=null`; `dry first signed buy …` fails at instruction index ≥ 3 (placeholder mint), never 0 |
| D | unit sandbox, `chronyc` inside it, watchdog, pinning | run the real unit once with a temporary drop-in adding `--probe-dry --state /var/lib/rugsnipe/probe-dry.db`: `systemctl status rugsnipe-hot` = active (running) past 3 min (READY is sent after dry self-test, rugsnipe-hot.rs:620-642); no `clock unknown`; `ps -L -o tid,psr,pcpu,comm -p $(pidof rugsnipe-hot)` shows the hot thread on an isolated core; then remove the drop-in |
| E | Alpenglow live, freeze over | `solana -u mainnet-beta feature status A1pengvuM6JEcyNuTnMqepBKhwHE3N6PmUrdATGawhJS` = active; `getAgGenesisCert` non-null (method per reports/Winning the Solana landing race.md:60,100); `solana -u mainnet-beta slot` > 459,869,000 |
| F | real start gates | journal start line `rugsnipe-hot … mode=probe probe_wallet=<P> armset=…` and `… kill file /run/rugsnipe/KILL`; `sudo -u rugsnipe /opt/rugsnipe/bin/rugsnipe-ctl kill status --state /var/lib/rugsnipe/state.db` = not killed |
| G | guard `--real-sell` with the real ArmSet | `sudo journalctl -u rugsnipe-guard`: `rugsnipe-guard: REAL sells from <P>` then `exits lease held by hot:…` |
| H | each sender accepts our bytes | `sudo -u rugsnipe sqlite3 -readonly /var/lib/rugsnipe/state.db "select sender,result,count(*) from sends group by 1,2"`: every label has `accepted`; any label all-errors → check its path (Astralane `/irisb` vs `/iris`, Jito http vs https, BlockRazor http:443; providers.rs:189-196, 418-575) |
| I | first landed buy, CU fit | the landed signature on an explorer: buy succeeded, not `ComputationalBudgetExceeded`; `select fail_reason,count(*) from fills group by 1` has no `instruction_error` cluster |
| J | own TradeEvent → position → LEAD sell | status line `probe attempts … landed …`; `select mint,state from positions` closes within seconds; `exits` rows exist; sell landed on the explorer |
| K | `meta.fee` of a failed buy | one failed attempt: `fills.fail_cost` equals the explorer fee (counted toward the 0.2 SOL cap, risk.rs:368-385) |
| L | nonce burns | `select result,count(*) from sends where kind='burn' group by 1` grows on drops and they land within a few slots; `select state,count(*) from nonces group by 1` hourly: no growing `burning`/`quarantined` (3 failed burns quarantine, nonce.rs:20) |
| M | leader schedule, BAM in maintenance | status line `| leaders epoch 1064…`; summary `cold.leaders.slots_by_class` (BAM ≈ 0 during maintenance) and `leaders_missing_from_gossip` |
| N | decision codes | `rugsnipe-hot` stats / rslog: counts of `LATE_SLOT`, `NO_NONCE`, `MaxBuysPerHour`, `MaxOpenPositions` (persistent `MaxOpenPositions` = stuck or orphan position, M6) |
| O | hour spread | after day 1: `shadow_gate.py --probe --hour-cap 2` `hours_with_attempts` ≈ 24, `max_per_hour` ≤ 2 |
| P | daily-loss accounting | end of day 1: `SUM(fills.fail_cost)` + closed P&L vs the wallet balance change on an explorer (burn fees are not counted, ≤ 0.00025 SOL/day) |
| Q | kill triggers on a live stream | status line `killed no`; if `feed_stale` or `drift` latches, read `kv.kill_trigger:*` before `unkill` (docs/08:563-568) |
| R | RPC load | no `429` in `sudo journalctl -u rugsnipe-hot`; provider dashboard ≤ plan rate |
| S | ArmSet age | `sudo /opt/rugsnipe/bin/rugsnipe-ctl armset verify /var/lib/rugsnipe/armsets/armset-2-<sha>.json --pubkey <ARMSET_PUB> --max-age 950400` = OK; no `armset: v2 is stale` in the journal |

Do not move SOL in or out of the probe wallet while hot runs: the `unexplained_drop` trigger latches on a
manual withdrawal (docs/08 §10).

---

## 4. SHOULD DO before stage B (0.25 SOL), then before full size (ranked)

**Before stage B**
1. **Pre-send entry selection** in the hot path: a create-only filter with `intx_n_buys`, and a multi-bundle
   gate or proxy visible at fire time (review B6, probe-design-v2.md §9). Without it, B faces the unfiltered
   break-even of about 36% at 0.25 SOL instead of about 7.9%. Days.
2. **ArmSet D2:** a stable `armsets/current` file that the reload follows, a signing timer on ohio1, and a
   stale-ArmSet alert in the status line and on Telegram (cold.rs:2740-2770). Hours.
3. **Alerting (D3):** require Telegram in probe, add `OnFailure=` on hot and guard, a healthchecks timer, and
   ArmSet age and nonce counts in the status line. The docs claim a dead-man that does not exist
   (docs/07-deploy.md:213-235). Hours.
4. **Feed kill hysteresis:** pause buys while the feed is stale, latch only after a sustained gap (30-60 s),
   and require 2 of 3 drift samples (cold.rs:2601-2616; triggers.rs:374-400). Hours.
5. **Real slot clock in the hot loop:** feed gRPC slot updates in, so time exits fire at slot+5 and
   `LATE_SLOT`/`STALE` mean something (hot.rs:589-591, 835-843; decide.rs:52-57). Add a real-mode test. Hours.
6. **Bundle-wallet insider sells for real positions:** forward the bundle run's wallets to the hot loop
   (cold.rs:653-668, 2427-2491; hot.rs:1259-1273). Hours.
7. **Recovery adopts only pump mints or our intents** (M6 code fix; recovery.rs:177-198). Hours.
8. **PumpSwap real sells,** and an alert on `VenueChanged` for a real position (hot.rs:2154-2162,
   2222-2231). Hours to days.
9. **Per-path calibration:** the index-probe lane (`intents.mode = "index"`, its own counters) or a
   per-sender rotation, to pick B's variant (probe-design-v2.md §3). Days.
10. **Undo the reservation on refusals after `Fire`:** add `RiskGate::release_unsent` for TOO_HIGH, LATE_SLOT,
    NO_NONCE and the others (risk.rs:337-341, 560-580; hot.rs:831-904). Hours.
11. **Quarter-hour spacing** (first qualifying launch per quarter hour) or a random per-hour schedule, so
    attempts stop clustering in the first seconds of each hour. Hours to a day.
12. **Burn with a priority fee and a tight CU limit (C7),** and two RPC providers (D4). Hours.
13. **Sell retry spacing:** `every_slots` 4-5 at 200 ms slots, and `max_sell_spend_lamports` at about 4M
    instead of the whole stake (decide.rs:63-67; example:41). Config.
14. **Telemetry:** `cu_limit`, `computeUnitsConsumed`, and leader pubkey, client_id and version per intent
    (schema.rs:29-37; real.rs:409). Hours, or a nightly offline backfill.
15. **CU floor 65,000** once the UVA exists and `ctl sim --wallet <probe> --legacy` passes. Build ctl with the
    `rpc` feature in install.sh (install.sh:410). Minutes plus a rebuild.
16. **Bid:** randomised priority arms (C3), revisit the BAM split, and a per-sender "tip buys position" flag
    (C6). A day.
17. **Operator fixes:** install.sh writes the hot drop-in itself; `config validate --creds` resolves `.json`
    and `.toml` names and parses api-keys; drop the keeper unit; update frankfurt-checklist and docs/07; add
    Nozomi, Astralane, BlockRazor and Triton rows to latency_probe.sh; fix the probe-design-v2 §2/§8 paths;
    write backups and a key-compromise runbook (D5). Hours.
18. **Price bound:** an expected bundle per launch or per operator instead of the 4 SOL constant (A1
    follow-up; hot.rs:236). A day.
19. **Alpenglow:** poll the feature and `getAgGenesisCert` in chain checks and force paper until it is
    non-null (triggers.rs:479-515). Key bundle runs by `(slot, bank_id)` and handle `UpdateParent`/`SLOT_DEAD`
    before fast leader handover (Agave 4.4, Nov 9 at the earliest). Confirm own fills before opening a
    position. Sign ArmSets with `--regime alpenglow`. A day to days.
20. **Guard:** a second sell path (Jito), not only Helius plus rpcsend (sender.rs:172-210). Hours.

**Before full size**
1. A direct TPU/QUIC lane (C5; senders/mod.rs:642-647). It carries the largest bucket of slot-0 winners in the
   research report. Days.
2. A shred feed (C4) with first-arrival across two feeds, a local slot clock, and an in-slot position and
   leader-geography fire/skip gate. Days.
3. Exits in milliseconds and aware of the leader window (B8), re-replayed on post-cutover shadow data. A day.
4. A sell nonce pool, an A/B of v1 against legacy, and an A/B of nonce against blockhash (msg.rs:66, 254). Days.
5. A bid quantile model D[leader_class][bundle_bucket] fitted from probe slots (config.rs:585 is never read).
   Days.
6. Live mode (refused in this build).

---

## 5. KNOWN AND ACCEPTED for probe A (fine at 0.02 SOL with the kill rule)

- Real buys are unfiltered and include unbundled launches. Read the in-class subset (M4, M5).
- No index probes. Paths are ranked by which one won the race (one signature per sender, sender.rs:115-170).
- Attempts cluster in the first seconds of each UTC hour (no quarter-hour spacing).
- Refusals after `Fire` use up the hour and day budget. This is rare: LATE_SLOT almost never fires on one
  ordered stream.
- `LATE_SLOT` does not catch a decision made after the slot ended in wall time. Late fills are flagged
  `LateFill` and split in the gate (M5).
- Time exits on a quiet mint wait for the next event, so this affects realised P&L, not landing or premium.
- Real positions do not see separate-transaction bundle-wallet sells. This is a P&L difference only.
- No PumpSwap real sell. A graduated position is sold by hand (docs/08:15). This is unlikely within a 1-3 s
  LEAD hold.
- No TPU lane. A result below break-even rules out the vendor-only stack, not the strategy.
- Static bid, with priority clamped below rivals' median on BAM, Harmonic and other leaders. Classes may be
  mislabelled while BAM is in maintenance. `leader_class` is recorded per send and fill.
- V_cap uses a 4 SOL bundle constant. The premium per fill is recorded, so the read can be corrected afterwards.
- `bank_id` is carried but not used as a key. That is safe until fast leader handover (Nov 9 at the earliest).
- No code check that Alpenglow is active. This is a manual check (§3 E, §6).
- Blocks without votes shift the premium and rank reconstruction. It can be recomputed offline from chain
  history.
- `feed_stale` (stale_ms 4,000 in M3) and `drift` latch the kill on one event, which costs hours, not SOL.
  Unkill and restart (docs/08:563-568).
- Dropped attempts with no accepted variant collapse into one `fills` row (store.rs:290), so they drop out of
  the denominator. This is conservative.
- Missing CU and leader telemetry can be backfilled from `fills.sig` and `intents.slot`. client_id at the time
  cannot.
- Burn fees are not counted in the daily loss: at most about 0.00025 SOL a day.
- Sell retries every 2 slots can land twice. A full-balance sell fails on balance, and the cost is bounded by
  `max_sell_spend`.
- Guard sells go only through Helius and rpcsend. Stage A has at most one 0.02 SOL position.
- Yellowstone client 15.0.0 / proto 14.0.1 rather than v16-rc. Upgrade only if the shadow day shows
  `convert_errors`.
- The `chronyc` parse does not check leap status (app.rs:202-207). Keep chrony-wait enabled.
- A newer systemd could report credential ACLs as 0440 and be refused. Stay on Ubuntu 24.04, where they are
  0400.
- The rugsnipe-keeper unit has no binary. Do not enable it (2b).
- No backups and no seed backup. The wallet holds about 1 SOL and is treated as disposable.
- The ArmSet regime tag is not enforced (armset.rs:315-319).
- Constants sized for 400 ms (`burn_after_slots` 3, `max_age_slots` 2, `max_slot_drift` 8) run at about half
  the wall time.

---

## 6. Day plan, Oct 21-23 (UTC)

**Tue Oct 20 (today, evening):** engineer starts M3 (write config/probe-a.example.toml) and M5
(shadow_gate.py); owner drafts M4. Buy Triton pay-as-you-go and create the Dragon's Mouth endpoint, so the
post-upgrade shadow can run on gRPC. Order fra1 if not already done.

**Wed Oct 21**
- **Morning (engineer):** finish M3. Write the runbook edits for M1 (one ArmSet per stage), M2 (nonce loop and
  ordering), M6 (pre-start token check) and 2b. Owner signs off M4 in probe-design-v2 §2.
- **Up to 14:10 (automatic):** the pre-cutover shadow on ohio1 stops. Confirm it stopped. Archive the
  pre-Alpenglow set with
  `sudo -u rugsnipe sqlite3 /var/lib/rugsnipe/shadow/state.db ".backup /var/lib/rugsnipe/shadow/pre-alpenglow.db"`
  and the summary, then run the shadow gate on it.
- **About 14:19:** epoch 1064 starts (slot 459,648,000). **About 14:50:** Alpenglow (Votor) activates. Do
  nothing on chain. The regime freeze forces any real buy to paper until slot 459,869,000 (about 02:40-03:40
  Oct 22).
- **After the cutover:** check that the feature is active and `getAgGenesisCert` is non-null (§3 E). If not
  by about 18:00, hold: the shadow day and probe A both slide by the delay.
- **Post-upgrade shadow restart (ohio1):** new `--state`, new ArmSet (`--ver N+1`, signed just before start),
  `--leader-schedule`, and `[feeds] grpc.enabled = true` on Triton next to logsws (credential through
  `-p LoadCredential=api-keys:…`). Watch `miss`, `reconnects`, the `grpc:` field and `first-seen`. This is the
  first live contact of the gRPC adapter after Alpenglow. The first-seen numbers from Ohio are not
  representative of Frankfurt; use the run for its health counters.

**Thu Oct 22**
- **fra1 box:**
  - Acceptance test (docs/08 §2). Add a `curl -w '%{time_connect}'` row for each of fra2.nozomi, fr.gateway.astralane
    and frankfurt.solana.blockrazor.
  - `install.sh --role hot`, then the Agave CLI for root, then the probe key.
  - `api-keys.toml` with no inline comments, including `GRPC_TOKEN`, `RPC_URL` (paid plan) and `TG_*`.
  - `sudo systemctl enable --now chrony-wait`.
  - `sudo systemd-run --uid rugsnipe -p ProtectSystem=strict -p PrivateDevices=yes --pipe --wait chronyc -c tracking`.
- **Senders:** from fra1, curl each sender with its real key and a deliberately invalid transaction. Expect a
  parse or simulation error, not 401/404/301. Settle Astralane `/irisb` against `/iris` and Jito http against
  https.
- **M1:** on ohio1, sign the stage-A ArmSet with 11-day validity. Copy the `.json` and `.sig` to
  `/var/lib/rugsnipe/armsets/`.
- **M3:** install `/etc/rugsnipe/rugsnipe.toml` from config/probe-a.example.toml
  (`sudo install -m 0640 -o root -g rugsnipe`). Run `config validate`. The `--creds` false refusals are known
  (crates/rugsnipe-ctl/src/lib.rs:239-247); trust the dry run for credentials.
- **Fund** the probe wallet: 1 SOL plus about 0.04 SOL for nonce rent. **M6:** the wallet must show no token
  balances.
- **M2:** create the 32 nonce accounts. Then run §3 A-C (one-hour `--probe-dry`, separate `--state`) and §3 D
  (the real unit, dry).
- **About 15:00-16:00:** the post-upgrade shadow reaches 24 h. Stop it and read it. The go condition is
  probe-design-v2 §7: the EV CI overlaps the pre-cutover run, and unresolved is 10% or less. Add
  `cold.grpc.convert_errors = 0`, decode `miss` below 1%, and no `would_kill` from feed_stale or drift. Record
  the measured ms per slot.
- **Evening:** the hot drop-in with `--busy-poll --armset … --rpc-url cred:RPC_URL`, then
  `install.sh --role hot --probe-armset …`. Do not start yet.

**Fri Oct 23**
- **After 03:40:** §3 E (slot past the freeze, certificate non-null).
- **Morning:** a final short `--probe-dry` (§3 A-C) on the dry state file, then the M6 token check.
- **Start:** `sudo systemctl enable rugsnipe-guard.service rugsnipe-hot.service rugsnipe.target`, then
  `sudo systemctl start rugsnipe.target`. **First hour:** §3 F-N.
- **End of day:** §3 O-P, and send the first `shadow_gate.py --probe --hour-cap 2` read to the owner,
  informational only.
- **Oct 24-25:** land M5 (shadow_gate v2). The first verdict read is Oct 26, and only with M5 in place.
  Re-check M6 before every restart.

---

## 7. Appendix

### 7.0 Disagreements resolved (files I opened)

| Item | Auditor / verifier | Checked | Decision |
|---|---|---|---|
| Hourly cap | non-blocking / upgraded | risk.rs:497-515, 560-580; config.rs:869-900, 1181-1196; docs/08:394; probe-design-v2.md:26-33 | **Blocking** (M3b): the stage profile cannot narrow it, and first-come per hour runs out the day early |
| Skip filter in probe | blocking / downgraded | hot.rs:1058-1101, 1556-1607; docs/08:20; probe-design-v2.md:29 | Not blocking on its own (it fails loudly or is documented as paper-only). The decision goes into **M4** |
| Live gRPC adapter | blocking / downgraded | rugsnipe-hot.rs:405-447; config.rs:1335-1346 | Not blocking: a real start is refused without a healthy feed. **But the dry run does not refuse** (`!healthy && !dry`), so §3 B must be read by eye |
| Genesis-cert detection | blocking / downgraded | grep for getAgGenesisCert, A1pengvuM6 and FLHoAW in crates, analysis, deploy and config: none; shadow_gate.py and prewarm_outcomes.py never read regime | Not blocking. Manual check in §3 E and §6 |
| Post-cutover shadow setup | blocking / downgraded | app.rs:325-345 (a stale ArmSet is refused at start) | Not blocking. Day-plan steps |
| api-keys inline comments | blocking / downgraded | senders/mod.rs:424-441; docs/08:376, 379 | Real, but a loud refusal at the dry run. 2b fix |
| sudo-less commands | blocking / downgraded | install.sh:290-296; the listed commands only read | Not blocking. 2b |
| solana-keygen missing | blocking / downgraded | docs/08:105-106; frankfurt-checklist.md:92; ctl `armset keygen` | Not blocking, but the Agave CLI install is part of M2 |
| CU floor vs UVA | blocking / downgraded; UVA confirmed and also found missing elsewhere | cu.rs:1-110; gap-5.md:60-75; docs/06:354, 393 | The two merge into **M3c**: keep 88,000 for stage A |
| Config sub-claims (min_hot, max_consec) | wrong / refuted | docs/08:390-395 (table sets 0.1 SOL and 20) | Refuted. M3a narrowed to exits and the entry cap |
| Stray-token adoption | found missing by verifier, "plausible" | chain.rs:32-53, 124-135; recovery.rs:170-198; app.rs:815-835; hot.rs:1432-1453 | Code path confirmed. **Blocking** under the merge rule as M6, mitigated cheaply |
| shadow_gate completion | research verifier: "not for start" / two other verifiers listed it as missed | shadow_gate.py:75-96, 280-310, 335-390 | **Blocking for the first read** (M5, deadline Oct 26), not for the Oct 23 start |
| LATE_SLOT frequency | "common" / "weak" | hot.rs:586-592, 835-843 | Rare: slot_now only advances on in-order feed events |
| Nonce rent amount | verifier 0.00105664 SOL | sysix.rs:10-13; ran `target/debug/rugsnipe-ctl nonce init --count 1` (prints 1,056,640 lamports and the `rs-buy-0` address) | Agreed |
| selftest | 31/31 | reran `bash deploy/selftest.sh`: 31 passed, 0 failed | Agreed. It uses stub binaries |

### 7.1 Every finding

Blocking column: **M1-M6** = blocking (the §2 item it maps to); "no" = not blocking; §4 / §5 = where it is
handled.

| Dimension | Item | Final status | Blocking | Evidence |
|---|---|---|---|---|
| start-path | ArmSet valid 6 h; running hot drops to paper; restarts and guard refused | wrong | **M1** | make_paper_armset.py:199-200; armset.rs:306-309; cold.rs:2740-2757; app.rs:331-342; rugsnipe-guard.rs:94-121 |
| start-path | No entry selection before a real send | missing | **M4** (decision), §4.1 | hot.rs:1058-1101, 1556-1607; e1.rs:450-455; docs/08:20 |
| start-path | Base config not LEAD (narrowed to exits and entry cap) | wrong | **M3a** | docs/08:382-383; rugsnipe.example.toml:42, 118-128; paper-lead.example.toml:51, 143-157 |
| start-path | Nonce pool creation path | missing (CLI path works) | **M2** | ctl lib.rs:21-23, 565-645; sysix.rs:12-27, 96-118; app.rs:446-452, 862-877; recovery.rs:209-235 |
| start-path | Hourly cap 4/h | partial | **M3b** (upgraded) | risk.rs:503, 574; config.rs:869-875, 1181-1196; docs/08:394 |
| start-path | Reserved caps leak on post-reservation refusals | wrong | no, §4.10 | risk.rs:337-341, 560-580; hot.rs:831-904 |
| start-path | Index-probe lane | missing | no, §4.9 | probe-design-v2.md:57-68, §9 |
| start-path | 0.02 SOL stake, 50/day, 0.2 SOL loss caps | tested | no | config.rs:879-886, 1181-1196; risk.rs:368-385, 466-474, 509-514 |
| start-path | `config validate --creds` false refusals | wrong | no, §4.17 | ctl lib.rs:228-247 |
| start-path | rugsnipe-keeper unit without a binary | missing | no, 2b | rugsnipe-keeper.service; rugsnipe-hot.service:16-17; docs/07-deploy.md:8 |
| start-path | Hot drop-in drops `--busy-poll`; other §9 mismatches | wrong | no, 2b and §4.17 | docs/08:398, 445; rugsnipe-hot.service:29; install.sh:410 |
| start-path | `--probe-dry` as pre-flight | partial | no, §3 A-D | senders/mod.rs:539-560; rugsnipe-hot.rs:78-140, 438-444; app.rs:895-925 |
| start-path | chronyc clock gate on the real box | untested-live | no, §3 D | app.rs:202-219; config.rs:1664-1671 |
| start-path | Credentials directory permission check | untested-live | no, §3 A | keys.rs:66-112 |
| start-path | gRPC health wait and Triton feed | untested-live | no, §3 B | rugsnipe-hot.rs:411-447; grpc/mod.rs:283-322, 505-527 |
| start-path | feed_stale latches after 2 s | wrong | no; stale_ms 4,000 in M3, §4.4 | cold.rs:2601-2616; triggers.rs:384-400 |
| start-path | Sender endpoints and auth vs vendor docs | untested-live | no, §3 H | providers.rs:189-196, 418-575; rugsnipe.example.toml:72 |
| start-path | Real sends, fill lane, meta.fee, nonce seeding, burns | untested-live | no, §3 C, I-L | real.rs:1-55, 280-303, 698-745; nonce.rs:15-20, 93-107 |
| start-path | Guard real sells (takeover) | untested-live | no, §3 G, §4.20 | rugsnipe-guard.rs:80-178, 297-300 |
| start-path | RPC provider load | partial | no, 2b | real.rs:47-58; rugsnipe-hot.rs:493-500, 547 |
| start-path | rpcsend on the public RPC | partial | no; disabled in M3 | rugsnipe.example.toml:68 |
| start-path | Bid split during BAM maintenance | partial | no, §4.16 | rugsnipe.example.toml:106; bid.rs:91-93, 174-183 |
| start-path | Kill file, probe_stage match, regime freeze | tested | no | config.rs:84-104, 1527-1585; tests/probe_refusals.rs; tests/real_mode_paper_forced.rs:408 |
| start-path | Unit sandbox, watchdog, start-limit with the real binary | untested-live | no, §3 D | deploy/selftest.sh:59-70; rugsnipe-hot.service:23-30 |
| start-path | Alpenglow multi-bank and own-trade positions | partial | no, §4.19 | hot.rs:637, 1046-1055 |
| start-path (missed) | Fresh probe wallet has no UVA; the 65k floor fails every buy | missing | **M3c** | cu.rs:13-16, 46-58; gap-5.md:68; instructions.rs:869; shadow_gate.py:296-301 |
| start-path (missed) | Recovery adopts any token balance and fills the single slot | wrong (plausible trigger) | **M6** | chain.rs:32-53, 124-135; recovery.rs:177-198; app.rs:822-835; hot.rs:1432-1453 |
| strategy | Probe template wrong exits, no [entry] block | wrong | **M3a** | rugsnipe.example.toml:118-128; setup.rs:371-378; install.sh:430-444 |
| strategy | multi_bundle universe never applied to real buys | missing | **M4**, M5 | hot.rs:724-916; watchlist.rs:194-205; e1.rs:450-458; shadow_gate.py:76-87 |
| strategy | Skip filter inert in probe; create-only model refused (44 vs 60 features) | missing | no (downgraded), part of M4 | hot.rs:1556-1607; filter.rs:33-97, 199-213; ctl lib.rs:285-300 |
| strategy | Live buy bound V_cap with a 4 SOL bundle | partial | no, §4.18; M5 adds the ≤+2% split | hot.rs:234-242, 906-916; e1.rs:450-466 |
| strategy | Live exit clock moves only on transaction events | wrong | no, §4.5 | hot.rs:589-591, 1936-1969; decide.rs:52-57, 86 |
| strategy | Live positions miss separate-transaction bundle wallets | missing | no, §4.6 | hot.rs:1101-1112, 1259-1273; cold.rs:653-668 |
| strategy | Shadow exit timings at 200 ms (max_hold_slots 5) | tested | no; keep 5 for A | paper-lead.example.toml:136-137, 149-151 |
| strategy | LATE_SLOT guard and slot+1 fills | wrong | no; M5 splits by landing slot | hot.rs:835-843; cold.rs:2121-2124; real.rs:575-585 |
| strategy | Sell retries at 200 ms slots | untested-live | no, §4.13 | decide.rs:63-67, 566-589; rugsnipe.example.toml:41 |
| strategy | PumpSwap real sells | missing | no, §4.8 | hot.rs:2154-2162, 2222-2231; guard.rs:400-409 |
| strategy | Outcome-store pre-warm | tested (doc paths wrong) | no; optional for A | prewarm_outcomes.py; app.rs:530-532; probe-design-v2.md:46-52 |
| strategy | Hourly thinning against the daily cap | partial | **M3b** | probe-design-v2.md:27, 33; risk.rs:503, 574 |
| strategy | Constants sized for 400 ms slots | partial | no, §5 | rugsnipe.example.toml:23-24, 54, 96; real.rs:51; tape.rs:29-60 |
| strategy | Votes leave blocks; tx_index and late_block_frac shift | untested-live | no; compare on the shadow day | e1.rs:456-457; filter.rs:54-55, 126-127 |
| strategy (missed) | ArmSet freshness stops buys after 6 h | wrong | **M1** | cold.rs:2740-2748; make_paper_armset.py:200 |
| strategy (missed) | shadow_gate v1 completion; B bar does not match the build | wrong | **M5** | shadow_gate.py:94, 340-343, 380-384; probe-design-v2.md:28, 83 |
| alpenglow | Q1 no tick or 400 ms dependence | tested | no | pump-codec alpenglow.rs:91-93; hot.rs:221 |
| alpenglow | Block-index heuristics calibrated with votes | partial | no; recompute offline | e1.rs:284-298, 456-457; bundle.rs:100, 174 |
| alpenglow | Q2 state keyed by slot, not (slot, bank_id) | partial | no, §4.19 | grpc/mod.rs:285-322, 638-644, 715, 729 |
| alpenglow | Yellowstone pins and live gRPC adapter | untested-live | no (downgraded), §3 B | Cargo.lock:5275-5299; rugsnipe-hot.rs:411-447 |
| alpenglow | Q3 leader class during BAM maintenance and FD absence | untested-live | no, §3 M | leader.rs:76-97, 208-226; app.rs:721-722 |
| alpenglow | Q4a regime_freeze_slots coverage | tested | no | config.rs:84-104; rugsnipe.example.toml:23 |
| alpenglow | Q4b no feature-gate or genesis-cert detection | missing | no (downgraded); manual §3 E | grep empty; store.py:42-43 |
| alpenglow | Q5 logs websocket on Oct 21 | untested-live (post-cutover) | no | logsws.rs:232-245, 364-385; decode.rs:20-107 |
| alpenglow | Q5 post-cutover shadow day setup | partial | no (downgraded); §6 | docs/08:185-187, 586-604; app.rs:331-342 |
| alpenglow | Regime tag not enforced on ArmSets | missing | no, §4.19 | armset.rs:315-319; rugsnipe-hot.rs:45 |
| alpenglow | Q6 clock thresholds | tested | no | triggers.rs:367-371; config.rs:178 |
| alpenglow | Q6 drift trigger | untested-live | no, §3 B, §4.4 | triggers.rs:374-380; cold.rs:2350-2368 |
| alpenglow | Q6 feed_stale under Votor skips | untested-live | no; stale_ms 4,000 in M3 | config.rs:1364-1366; grpc/mod.rs:580-585 |
| alpenglow | pump-codec BlockComponent parser | tested | no (not on the probe path) | pump-codec/src/alpenglow.rs; tests/alpenglow_vectors.rs |
| alpenglow | docs/06 Alpenglow claims vs code | wrong (docs) | no, §4.17 | docs/06-architecture.md:38, 605 |
| ops | 6-h ArmSet expiry | wrong | **M1** | as start-path |
| ops | Durable-nonce pool creation | missing | **M2** | as start-path |
| ops | api-keys example fails the parser (inline comments) | wrong | no (downgraded), 2b | docs/08:376, 379; senders/mod.rs:424-441 |
| ops | Probe config source is not the plan's probe | wrong | **M3** | docs/08:382-401; rugsnipe.example.toml:17-168 |
| ops | Plan vs build: skip, multi_bundle, thinning, index | missing | **M4** | docs/08:20 vs 301; probe-design-v2.md:26-29, 158-163 |
| ops | Keeper unit enabled, no binary | wrong | no, 2b | docs/08:448-449; install.sh:101, 421-426 |
| ops | Commands without sudo on service-owned paths | wrong | no (downgraded), 2b | install.sh:290-296; docs/08:107, 160, 181, 197, 230-234, 413 |
| ops | Pre-warm and shadow-state transfer | wrong | no; optional for A | probe-design-v2.md:46-52, 145-156; install.sh:92, 308 |
| ops | solana-keygen not installed; no seed backup | partial | no (downgraded); Agave CLI install in M2 | docs/08:105-106; install.sh:257-259; ctl lib.rs:706-745 |
| ops | Hot drop-in typed by hand, no `--busy-poll` | wrong | no, 2b | docs/08:445; cli.rs:3-23 |
| ops | `ctl sim` and cargo not runnable on fra1 | partial | no, §4.15 | docs/08:398, 603; install.sh:383-410 |
| ops | No dead-man or off-box alert | missing | no, §4.3 | alerts.rs:1-12; config.rs:697-716; docs/07:213-235 |
| ops | No backups, seed backup or compromise procedure | missing | no, §4.17 | review-2026-10-16.md:63 (D5) |
| ops | Key in the journal | tested | no | keys.rs:327-413; rugsnipe-hot.rs:212-252 |
| ops | `.sig` not installed; stale summary | tested | no | app.rs:329-342; shadow_gate.py:231-259 |
| ops | sed corrupting a line | partial | no | docs/08:133, 160, 445 |
| ops | Credential plumbing (names, paths, LoadCredential) | tested (offline) | no, §3 A | rugsnipe-hot.service:45-50; rugsnipe-guard.service:41-47; keys.rs:107-120 |
| ops | install.sh on fresh Ubuntu 24.04 | untested-live | no | install.sh:239-266, 384-405; tune-hot.sh:108-125 |
| ops | deploy/selftest.sh coverage | tested (stubs) | no | selftest.sh:32-38, 59-70, 85-97; rerun 31/31 |
| ops | Stale companion docs | wrong (docs) | no, §4.17 | frankfurt-checklist.md:107-112, 149-166; docs/07:6-8, 142; latency_probe.sh:12-37 |
| ops (missed) | shadow_gate stage A done at 300 attempts or 3 days | wrong | **M5** | shadow_gate.py:27, 94, 374, 380-384 |
| research | CU floor 88k in the runbook vs 65k in code | wrong (by itself harmless) | merged into **M3c** | docs/08:398; cu.rs:52-58, 101-105; config.rs:1108 |
| research | UVA not pre-created | missing | **M3c** | cu.rs:13-16, 46; instructions.rs:869; constants.rs:191; ctl lib.rs:86-97 |
| research | Transaction shape: buy_v3, ATA, tip in tx, LADS | tested | no | template.rs:9-20, 272-318; tests/templates.rs; tests/cu_gap5.rs |
| research | v1 format, ALT | partial | no, before full size | msg.rs:66, 254; config.rs:511-516 |
| research | Direct QUIC TPU lane | missing | no, before full size | senders/mod.rs:642-647; leader.rs:255-257 |
| research | Vendor fan-out (Helius, Jito, Nozomi, Astralane, BlockRazor) | untested-live | no, §3 H | providers.rs:1-16, 131-137, 531; sender.rs:107-160; tests/http_senders.rs |
| research | Leader-class routing of the bid | partial | no, §4.16 | bid.rs:30-33, 91-93, 174-183; leader.rs:76-90 |
| research | Bid quantile model and EV cap | missing | no, before full size | hot.rs:208, 783-786; config.rs:585, 617 |
| research | Price bound (cap over post-bundle) | partial | no, §4.18 | hot.rs:236-242, 911-914 |
| research | Probe-A telemetry | partial | no, §4.14 | schema.rs:29-37; cold.rs:1928; store.rs:290; sender.rs:286-316 |
| research | shadow_gate `--probe` read | wrong | **M5** (first read) | shadow_gate.py:94, 380, 401-403; selftest_probe.py:81-86 |
| research | Index or calibration probes per path | missing | no, §4.9 | probe-design-v2.md:66-79, 118-127 |
| research | Raw-shred ingest | missing | no, before full size | config.rs:258-264, 1655-1656 |
| research | Local slot clock and in-slot position | missing | no, before full size | clock.rs:1-23; hot.rs:94-95 |
| research | Fire/skip gate by in-slot position and geo; slot+1 re-arm | missing | no, before full size | hot.rs:835-843, 1000-1022 |
| research | Nonce vs blockhash A/B; split keys | partial | no, before full size | hot.rs:893-905; config.rs:542 |
| research | Exits in ms, leader-window aware; finalized; skip-cert | missing | no, before full size | policy.rs:189-190, 236-238, 409-411 |
| research | Alpenglow hygiene (pins, bank_id, cert gate) | partial | no; shadow-day go check §6 | Cargo.toml:36-37; grpc/mod.rs:664-772 |
| research | Metro RTT via latency_probe.sh | partial | no; §6 Oct 22 extra rows | latency_probe.sh:12-35, 70-84 |
| research (missed) | Stage-A hourly thinning not configured | partial | **M3b** | docs/08:394; rugsnipe.example.toml:39; config.rs:869-875 |
