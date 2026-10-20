# 07 Deployment

Date 2026-10-14. This guide covers what runs where, how to install it, what it costs, how to move from paper to live, and how to stop it.
It implements docs/06-architecture.md sections 2, 6, 10, 12 and 13, and docs/03-speed-infra.md sections 4-7.

**Status.** The deployment layer is ready. The binaries are not.
- `deploy/install.sh`, the systemd units and `deploy/tune-hot.sh` are linted and verified in this container (see "Validation").
- `rugsnipe-hot` and `rugsnipe-guard` exist and run paper and probe (live is refused, docs/08 section 0). The nonce burns, leader schedule and recovery that `rugsnipe-keeper` was planned for run inside `rugsnipe-hot`; the `rugsnipe-keeper` binary does not exist. **Never enable `rugsnipe-keeper.service`**: it would restart-loop with `203/EXEC`. Enable `rugsnipe-guard`, `rugsnipe-hot` and `rugsnipe.target` only (docs/08 section 9 step 6). The hot unit still names the keeper in `Wants=`, so a failed keeper unit next to a running hot unit is expected and harmless.
- The collector unit runs the existing `analysis/scripts/collector.py`. The discover timer is a placeholder that runs the selftest until M2.
- Python units exit cleanly with code 78 if their module or script is missing, instead of restart-looping. install.sh does not enable a Rust unit whose binary is missing.

## 1. Topology

```
                         you (laptop) ── ssh ──┬───────────────────────────────┐
                                               ▼                               ▼
 public RPC, PumpPortal(x1), ┌──── ohio1  AWS us-east-2  18.188.14.68 ────┐   ┌──── fra1  Frankfurt bare metal (TeraSwitch) ─────────┐
 pump.fun API ──────────────►│ role analytics (exists)                     │   │ role hot (to rent)                                   │
                             │  rugsnipe-collector  → analytics.db         │   │  (no keeper unit: recovery, nonces, leaders in hot)  │
                             │  rugsnipe-discover.timer (6 h) → ArmSet+sig │   │  rugsnipe-guard   exit-lease takeover                │
                             │  reports, backups, ArmSet signing key ONLY  │   │  rugsnipe-hot     feed→decide→sign→send, exits       │
                             │  chrony → Amazon Time Sync                  │   │  chrony → ntp.frankfurt.jito.wtf, isolcpus, tuning   │
                             │  10.77.0.1 ◄══════ wg0 (UDP 51820) ═══════════════► 10.77.0.2   metrics :9464 on wg0 only            │
                             └─────────────────────────────────────────────┘   └──────────▲────────────────────────┬──────────────────┘
                                ArmSet push ohio1→fra1; rslog + state.db                  │ feeds                  │ sends (same building)
                                snapshots fra1→ohio1 every 5 min                          │ logsSubscribe (free)   ▼ Helius fra-sender, Nozomi fra2,
                                                                                         │ SVS gRPC → LaserStream   Astralane fr, Jito FRA, NextBlock,
                                                                                         │ DZ Edge + Helius shreds  leader TPU / BAM / Harmonic
```

## 2. What runs where

| Box | Role / command | Units | Keys under `/etc/rugsnipe/keys` |
|---|---|---|---|
| this container | tests, paper runs, reports | none (run by hand) | none |
| **ohio1** (exists) | `sudo deploy/install.sh --role analytics` | `rugsnipe-collector.service`, `rugsnipe-discover.timer`/`.service`, `rugsnipe.target` | `armset-signing.key`, `api-keys.toml`. **Never** wallets (install.sh refuses) |
| **fra1** (to rent) | `sudo deploy/install.sh --role hot`, then `deploy/tune-hot.sh` | `rugsnipe-guard`, `rugsnipe-hot`, optional `rugsnipe-tune`, `rugsnipe.target` (not `rugsnipe-keeper`: no binary) | `probe-keypair.json`, `api-keys.toml` (`hot-keypair.json` only for live, which this build refuses). **Never** the ArmSet signing key (install.sh refuses) |
| second box | phase 2, after the gate: tyo1 (TeraSwitch TYO2), then ams1 (AMS3), per docs/03 section 4 and docs/06 section 11 | same as fra1 | same as fra1 |

Step-by-step guides:
- `deploy/aws-ohio-analytics.md` for ohio1;
- `deploy/frankfurt-checklist.md` for renting and accepting fra1, onboarding the feeds, and the paid-sender key list;
- `deploy/wireguard-example.conf` for wg0.

Reference tools: `deploy/latency_probe.sh` (RTT from a candidate box) and `deploy/stake_geo.py` (stake and leader-slot geography, region model).

### Files and layout (both roles)

| Path | Owner / mode | Content |
|---|---|---|
| `/opt/rugsnipe/src` | root 0755 | copy of the checkout. `data/` is a symlink to `/var/lib/rugsnipe/data` |
| `/opt/rugsnipe/venv` | root | Python venv with the analysis package installed editable |
| `/opt/rugsnipe/bin` | root 0755 | `rugsnipe-*` binaries, `rugsnipe-pyrun`, `tune-hot.sh` (hot) |
| `/opt/rugsnipe/{rustup,cargo,target}` | root | pinned toolchain (`rust-toolchain.toml`, 1.97.1) and the build tree |
| `/opt/rugsnipe/REVISION` | root 0644 | deployed commit, dirty-file count, time, role |
| `/etc/rugsnipe/rugsnipe.toml` | root:rugsnipe 0640 | config from `config/rugsnipe.example.toml`. Never overwritten |
| `/etc/rugsnipe/keys/` | root 0700, files 0400 | credentials. Loaded by PID 1 with `LoadCredential=`. The service user cannot read this directory |
| `/var/lib/rugsnipe/{state,rslog,armsets,data}` | rugsnipe 0750 | `state.db`, rslog, ArmSets, caches and reports, `analytics.db` (Ohio) |
| `/run/rugsnipe` | root:rugsnipe 1770 (tmpfiles.d) | `KILL` file and `pp.sock`. The sticky bit stops services from deleting root's `KILL` |

### Contracts the code must honour

- **Credentials.** Units read their credentials from `$CREDENTIALS_DIRECTORY/<name>`: hot loads `hot-keypair`, `probe-keypair` and `api-keys`; guard and keeper load `hot-keypair` and `api-keys`; the collector loads `api-keys`; discover loads `armset-signing-key` and `api-keys`.
  - A missing file yields the literal content `ABSENT` (`SetCredential=` fallback). The binary must treat that as "no key", and config validation must refuse probe/live.
  - `api-keys` is a TOML table whose keys are the names in the config (`NOZOMI_KEY`, `GRPC_TOKEN`, `TG_TOKEN`, `HC_URL`, ...).
- **Rust services.** Each is started as `<bin> --config /etc/rugsnipe/rugsnipe.toml` with `RUGSNIPE_CONFIG` set.
  - `Type=notify`: send `READY=1` only after chain recovery.
  - Send `WATCHDOG=1` only while the feed and exit loops advance (`WatchdogSec`: hot 3 s, guard 5 s, keeper 30 s).
- **Python services.** `rugsnipe-pyrun <module>` runs `python -I -m rugsnipe_analysis.<module>`, and `rugsnipe-pyrun --script <name>` runs `analysis/scripts/<name>.py`. Both get `RUGSNIPE_CONFIG`, `RUGSNIPE_STATE_DIR=/var/lib/rugsnipe`, `RUGSNIPE_RUN_DIR=/run/rugsnipe` and `RUGSNIPE_DB=/var/lib/rugsnipe/analytics.db`.
  - Exit code 78 means "not deployed / misconfigured", and the unit does not restart.
- **Writable paths.** Only `/var/lib/rugsnipe` and `/run/rugsnipe` (`ProtectSystem=strict`).
- **Hardening.**
  - Every service runs with `NoNewPrivileges`, an empty `CapabilityBoundingSet` and a syscall filter, except `rugsnipe-tune` (a root one-shot that applies the OS tuning). `systemd-analyze security` scores 1.3-1.8 ("OK").
  - `rugsnipe-hot` alone keeps `CAP_SYS_NICE` (for SCHED_FIFO and affinity). Hot, guard and keeper all hold the hot key and have `LimitMEMLOCK=infinity`, so they can `mlockall` it.
  - When the `Shreds` feed strips DZ Edge GRE in XDP, add `AF_XDP`/`AF_PACKET` and `CAP_BPF`/`CAP_NET_ADMIN` through a drop-in, never in the base unit.
  - `rugsnipe-hot` is pinned with `CPUAffinity=` to the isolated cores. Guard and keeper are pinned to the housekeeping cores. install.sh writes the `20-cpus.conf` drop-ins from `/sys/devices/system/cpu/isolated`.

## 3. Phases and the gate

The system stays **paper-only unless the go/no-go gate passes** (docs/06 sections 0 and 9.5). Each phase has an entry condition.

| Phase | Where / feed | Proves | Entry condition | Monthly cost |
|---|---|---|---|---|
| **paper** | ohio1 or this container; `logsSubscribe` + PumpPortal (free) | decode coverage, decisions, internal latency, E2 P&L. Nothing about block 0 | now | $0 extra (Ohio already paid) |
| **shadow** | fra1 + SVS Yellowstone gRPC; still `mode = "paper"` | detection lead vs rivals' landing slots per leader class | M4 paper run clean (decode-miss < 1%) | box ~$400-425 + SVS $150 |
| **probe-A** | fra1 + paid post-exec feed (Triton Dragon's Mouth, LaserStream fra or SVS); `config/probe-a.example.toml`, `[probe] stage = "a"`: 0.02 SOL buys on every class-eligible create (unfiltered: the build cannot select before it sends, readiness audit M4), ≤ 150 attempts/day and 6 per UTC hour, 1 open position, 0.6 SOL daily loss cap (decided 2026-10-20, docs/research/probe-design-v2.md §2 item 4, option ii), until ≥ 350 attempts and ≥ 7 days with ≥ 200 in-class attempts (hard stop at 14 days); one ArmSet for the stage (`--valid-hours 360`, `armset_max_age_s = 1_296_000`) | landing rank, premium, failed-attempt cost per leader class, which bid variant lands; true `buyers_ahead` replaces the guessed k̂. EV is negative at this size (fixed costs) and is not a kill reason | shadow `GO on (a)`; `cu_limit` set by `ctl sim`, keys 0400, all caps non-zero, clock offset ≤ 5 ms, healthy post-exec feed, a signed ArmSet with `probe_allowed` (and `probe_stage = "a"` if it names one) | + the feed; ≤ 0.6 SOL a day at risk under the cap, expected 0.19-0.31 SOL a day, about 1.3-4.3 SOL over the 7-14 days |
| **probe-B** | same box and feed, `[probe] stage = "b"`: 0.25 SOL buys, ≤ 300 attempts/day, 2 open positions, 1 SOL daily loss cap, about 200 landed trades | EV per attempt with failures at our real rank, premium and cost; the inputs of gate (b) | probe-A's **in-class** landing rate (`feed_bundle.bundle_wallets >= 1`) above max(the pooled LEAD break-even at its measured failed-attempt cost, the unfiltered shadow break-even at 0.25 SOL) and in-class median premium ≤ +2% (`shadow_gate.py --probe`: `ENTER PROBE-B`); a signed ArmSet with `probe_stage = "b"` | ≤ 1 SOL/day at risk under the kill rule (docs/04 §7) |
| **live** | fra1; after the gate add DZ Edge + Helius shreds | money | **gate (below)** + `mode = "live"` in TOML + CLI flag + local `rugsnipe-ctl arm-live` token for that gate run (valid 7 days) | + DZ $1,500 + Helius shreds $800-1,000 + tips |

**Settled (2026-10-14):** the probe runs in two stages, A (measurement at 0.02 SOL) then B (0.25 SOL), as above; docs/04 §7, docs/06 §9.5/§10, docs/08 §9 and the `[probe]` section of the example configs say the same. Shadow runs on `logsSubscribe`; the hot binary has the Yellowstone gRPC feed (docs/08 §9 step 0), which probe requires and shadow may run for the first-seen comparison.

**Probe kill rule** (docs/04 §7, `[probe] kill`, `shadow_gate.py --probe`): kill when the EV per attempt including failed attempts has a 90% upper bound below 0 after 100 landed trades or 400 attempts, whichever comes first (probe-B); or the median realised premium exceeds +3%; or the landing rate stays below the break-even for the measured failed-attempt cost over 200 attempts; or the stage's daily loss cap is hit.

**The gate** (docs/06 section 9.5) for the class-arm LEAD, signed into the ArmSet as `gate_passed`. All of these must hold:
- (a) shadow: ≥ 1,000 qualifying launches over at least one full day, shadow EV per attempt at the measured rank, premium and cost with a 90% lower bound above 0 (bootstrap by hour clusters);
- (b) probe-B complete: ≥ 200 landed trades, EV per attempt including failures with a 90% lower bound above 0, no single hour above 40% of the P&L, ahead of the first follower on at least one hot launch in three at a premium of +2% or less;
- (c) every cost measured, not modelled;
- (d) all of it re-verified after Alpenglow (Oct 21 2026) before any live start.

The per-operator risk latches stay; operator counts, matched controls and vault EV are no longer gate criteria (the LEAD is a class arm).

Until probes measure landing, farming and exit fills, every EV is an upper bound and no ArmSet can carry `gate_passed = true`.
The hot box runs **exits only** if the ArmSet is unsigned, older than `armset_max_age_s` or from the wrong regime. The default is 21,600 s (6 h), the life of a paper ArmSet; a probe signs one ArmSet per stage with `make_paper_armset.py --valid-hours 360` and sets `armset_max_age_s = 1_296_000` (15 days), so a running probe does not drop to paper after 6 hours (docs/08 section 9 step 1).

**Alpenglow** activates at epoch 1064, Oct 21 2026 ~15:00 UTC. `regime_freeze_slots = [[459_640_000, 459_869_000]]` forces buys to paper.
Re-run the feed and decoder tests that week. Unfreezing needs decode-miss < 1% for 6 h.

## 4. Costs

| Item | $/month | When | Source |
|---|---|---|---|
| AWS Ohio (ohio1) | existing | now | your bill |
| **TeraSwitch FRA2** bare metal (9950X) | **~425** | shadow+ | docs/03 section 4 (list price; stock on request from sales) |
| Cherry Servers FRA (same campus, 0.09 ms; EPYC 9375F, in stock) | €989 (or €1.69/h) | shadow+, immediate fallback | docs/03 section 4 |
| Latitude.sh FRA (different building, 0.7 ms away) | ~400 | only if neither of the above | docs/03 section 4 |
| AWS eu-central-1 fallback (c7i/c8g 2xlarge-4xlarge) | on-demand pricing | only if bare metal is impossible | docs/03 section 4 |
| **SVS Yellowstone gRPC** (shadow feed) | **150** | shadow | docs/06 section 12 |
| **Helius LaserStream** fra/ams (post-exec gRPC) | **499-999** | probe (alternative to SVS) | docs/03 section 5 |
| **DoubleZero Edge** FRA seat (leader shreds, 53% of slots) | **1,500** | after the gate | docs/03 section 5 (DZ subscribe page) |
| **Helius shred delivery** per IP | **800-1,000** ($800 on Pro) | after the gate | docs/03 section 5 |
| NextBlock (optional sender) | 249+ | after the gate, if A/B shows value | docs/03 section 6 |
| **Nozomi / Astralane tips** | per landed tx: min 0.001 SOL (Astralane VIP3 0.00001) | probe/live | docs/03 section 6 |
| Helius Sender | min 0.001 SOL, or 5,000 lamports with `swqos_only` | probe/live | docs/03 section 6 |
| Landed block-0 buy, all-in (observed) | median 0.004 SOL, p75 0.014, p90 0.05 per buy | live | docs/03 section 6 |

Totals:
- paper: **$0** extra;
- shadow: **~$575/mo**;
- probe: **~$575-1,425/mo** + probe-A ≤ 0.6 SOL/day under the cap (expected 0.19-0.31 SOL/day, about 1.3-4.3 SOL over 7-14 days) + probe-B ≤ 1 SOL/day under the kill rule;
- live: box + DZ + Helius shreds + the post-exec gRPC feed that probe/live require (SVS $150 or LaserStream $499-999) ≈ **$2,850-3,925/mo** + tips.

## 5. Install and update

```bash
# both roles: always dry-run first, then the real thing (idempotent, safe to re-run)
bash deploy/install.sh --role analytics|hot --dry-run
sudo bash deploy/install.sh --role analytics|hot [--user rugsnipe] [--ntp-region frankfurt] [--hot-cpus 8-15,24-31] [--enable]

# update
cd ~/rugsnipe && git pull && sudo bash deploy/install.sh --role <role>
sudo systemctl restart rugsnipe-collector                      # ohio1
sudo systemctl restart rugsnipe-guard                          # fra1, then hot (see the note below); there is no keeper binary
sudo systemctl restart rugsnipe-hot

# rollback: check out the previous commit (see /opt/rugsnipe/REVISION) and re-run install.sh
```

Restarting `rugsnipe-hot` with open positions is safe by design. It recovers from the chain before `READY=1`, and guard takes the exits
after 5 s. Even so, prefer restarting while flat.

**Never restart guard and hot at the same time while holding positions.**

Hot-box OS tuning (docs/03 section 7):

```bash
sudo /opt/rugsnipe/bin/tune-hot.sh --check      # report only (the default)
sudo /opt/rugsnipe/bin/tune-hot.sh --dry-run    # what --apply would change
sudo /opt/rugsnipe/bin/tune-hot.sh --apply      # governor, irqbalance off + NIC IRQ pinning, sysctls, THP madvise
# printed only, never applied: the C-state cap and isolcpus/nohz_full/rcu_nocbs/irqaffinity for GRUB
sudo systemctl enable rugsnipe-tune.service     # re-apply the runtime part at every boot
```

Time: chrony on fra1 uses `ntp.frankfurt.jito.wtf` (prefer), plus Amsterdam and London. On EC2, ohio1 uses Amazon Time Sync.
Probe/live are refused above a 5 ms offset, and the kill latch fires above 50 ms.

## 6. Kill and alert procedures

### Stop buying (the kill latch): buys stop, exits continue

Any one of these:

```bash
sudo touch /run/rugsnipe/KILL                   # file latch (kill_file in rugsnipe.toml)
sudo systemctl kill -s USR1 rugsnipe-hot        # signal latch
# Telegram: /kill to the alert bot
```

The latch is recorded in `state.db` (`kv.kill_latch`, plus `risk.killed` for the day), so it survives a
crash, a restart and a reboot (which clears `/run`). **Unkill is local only.** Log in to fra1, remove the
file, stop the service, `sudo -u rugsnipe rugsnipe-ctl unkill --state /var/lib/rugsnipe/state.db`, start it
(docs/08 section 10). A Telegram message can kill but never unkill.

### Automatic kill triggers (docs/06 section 13)

Implemented (task #12, 2026-10-16; probe/live latch, paper reports them as `would_kill`; each writes a
`kv.kill_trigger:<name>` row; docs/08 section 10 has the exact rules):
- the daily loss marked to market (closed P&L + failed attempts' `meta.fee` + open positions at their sell quote);
- an unexplained wallet drop beyond our known spends + 0.05 SOL;
- send errors > 20% of the last 50 real sends;
- decode-miss > 1%;
- a pump program upgrade (programdata deployed slot or authority; FeeConfig is not watched yet): run `rugsnipe-ctl sim`, then unkill;
- a stale ArmSet (exits only);
- disk > 90% on the state partition;
- clock offset > 50 ms;
- the gRPC feed stale (no message for `stale_ms`) or more than 8 slots behind `getSlot`.

Not implemented: reconcile drift > 60 s.

### Stop everything

`sudo systemctl stop rugsnipe-hot` stops entries and the hot exit loop. **Guard takes over the exits after 5 s**
(probe: only with `rugsnipe-guard.service.d/30-probe.conf`, written by `install.sh --probe-armset`, which adds
`--real-sell --armset ... --rpc-url cred:RPC_URL`; without it the guard only reports what it would sell). The guard
retries each sell on a fresh blockhash until the wallet's balance is 0, closes the position in `state.db`, and
hands the lease back once hot's heartbeat returns (docs/08 section 9).

`sudo systemctl stop rugsnipe.target` also stops guard. Any open position is then **stranded**: it is bounded by
`max_open_positions × max_snipe` (4 × 0.25 SOL) but unmanaged. Do this only when flat. Check first with
`sqlite3 -readonly /var/lib/rugsnipe/state.db "select mint,state from positions where state!='closed'"`.

### Alerts

Critical alerts go **directly from fra1** to Telegram. They never route via Ohio. Telegram is the only alert path in this build (the healthchecks.io dead-man below is not built yet), so it must be configured for the probe (`TG_TOKEN`, `TG_CHAT`; docs/08 section 10).

| Alert | Meaning | Do this |
|---|---|---|
| dead-man missed / hot down 30 s | hot is not feeding the watchdog, or the box is dead | `systemctl status rugsnipe-hot rugsnipe-guard`; `journalctl -u rugsnipe-hot -n 200`. Guard owns the exits meanwhile. If the box is unreachable, open the provider console (positions are bounded by the caps) |
| killed | a latch fired | read `kill_reason` (journal / `risk` table), fix the cause, unkill locally |
| exit feed down / position owned by guard | the post-exec feed for a held mint is lost, or hot's exit loop stalled | check the feed health metrics. Guard sells on a ≥ 30% drop from peak, MaxHold or `grace_s` |
| sell failing 5 tries | the sell bound or sender path is broken | inspect `exits`/`sends`; the per-position sell-spend cap stops escalation; sell manually if needed |
| drift | ledger ≠ chain | not built yet (reconcile drift); the gRPC slot drift trigger is (docs/08 section 10) |
| upgrade | pump / PumpSwap / pump-fees programdata or FeeConfig changed | buys go to paper automatically; wait for `rugsnipe-sim` to pass |
| low balance | hot wallet < `min_hot_balance` | top up from cold, or leave it in exits-only |

Observe:
- Prometheus metrics on `127.0.0.1:9464` and `10.77.0.2:9464` (wg0 only): feed lag, decode misses, stage latency, sends, landed slot offset, positions, nonces, wallet, daily P&L, ArmSet age, lease owner, drift;
- a daily digest with per-operator stats.

### Failover (phase 1)

| Failure | Response |
|---|---|
| hot crash | systemd restarts it and runs recovery; guard takes over after 5 s and hands back when hot beats again. A panic latches the kill first; the 3rd unclean start within an hour latches `crash_loop`; `10-start-limit.conf` stops the unit after 5 starts per hour |
| Ohio down | fra1 keeps the last ArmSet until `armset_max_age_s` (6 h by default; 15 days for a probe stage), then runs exits only |
| fra1 dead | positions stranded, bounded by `max_open_positions × max_snipe`; the dead-man alert fires |
| paid feeds down | entries go to paper; exits poll within the RPC budget |

## 7. Validation done in this session

- `bash -n` and `shellcheck` 0.11 (via `shellcheck-py`) are clean on `deploy/install.sh`, `deploy/tune-hot.sh` and `deploy/bin/rugsnipe-pyrun`. Run `bash deploy/selftest.sh` to repeat this offline.
- `systemd-analyze verify` (systemd 255) is clean on every unit, run in a scratch root with stub binaries. `systemd-analyze security --offline`: hot 1.8, guard/keeper 1.5, collector/discover 1.3.
- The `SetCredential=` fallback semantics were checked against the systemd v249 man page (the Ubuntu 22.04 version). An empty value is rejected, hence the `ABSENT` sentinel.
- `install.sh --dry-run` was run for both roles in this Ubuntu 24.04 container, without systemd. Key-permission refusal was tested with a 0644 dummy file.
- `tune-hot.sh --check` and `--dry-run` were run here. This is a VM, so there is no cpufreq or cpuidle, and those steps report SKIP.
- **Not done here:** a real install as root, starting units under a live systemd, and running on bare metal. These happen on ohio1 and fra1 using the guides above.
