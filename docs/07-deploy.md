# 07 Deployment

Date 2026-10-14. This guide covers what runs where, how to install it, what it costs, how to move from paper to live, and how to stop it.
It implements docs/06-architecture.md sections 2, 6, 10, 12 and 13, and docs/03-speed-infra.md sections 4-7.

**Status.** The deployment layer is ready. The binaries are not.
- `deploy/install.sh`, the systemd units and `deploy/tune-hot.sh` are linted and verified in this container (see "Validation").
- The hot units call binaries that the build plan still has to deliver: `rugsnipe-hot`, `-guard` and `-keeper` (M3-M5).
- The collector unit runs the existing `analysis/scripts/collector.py`. The discover timer is a placeholder that runs the selftest until M2.
- Python units exit cleanly with code 78 if their module or script is missing, instead of restart-looping. install.sh does not enable a Rust unit whose binary is missing.

## 1. Topology

```
                         you (laptop) ── ssh ──┬───────────────────────────────┐
                                               ▼                               ▼
 public RPC, PumpPortal(x1), ┌──── ohio1  AWS us-east-2  18.188.14.68 ────┐   ┌──── fra1  Frankfurt bare metal (TeraSwitch) ─────────┐
 pump.fun API ──────────────►│ role analytics (exists)                     │   │ role hot (to rent)                                   │
                             │  rugsnipe-collector  → analytics.db         │   │  rugsnipe-keeper  recovery, nonces, leaders, upgrade │
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
| **fra1** (to rent) | `sudo deploy/install.sh --role hot`, then `deploy/tune-hot.sh` | `rugsnipe-keeper`, `rugsnipe-guard`, `rugsnipe-hot`, optional `rugsnipe-tune`, `rugsnipe.target` | `hot-keypair.json`, `probe-keypair.json`, `api-keys.toml`. **Never** the ArmSet signing key (install.sh refuses) |
| second box | phase 2, after the gate. docs/06 section 11 says ams1; docs/03 section 4 now recommends Tokyo (TeraSwitch TYO2) first | same as fra1 | same as fra1 |

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
| **probe** | fra1 + paid post-exec feed (LaserStream fra or SVS); lock probes, then 0.01-0.02 SOL buys, ≤ 20/day | true `buyers_ahead`, landing per sender × leader class, exit fills; replaces the guessed k̂ | `cu_limit` set by `ctl sim`, keys 0400, all caps non-zero, clock offset ≤ 5 ms, healthy post-exec feed, `max_probe_lamports` 0.02 SOL; ≤ 0.5 SOL in total (M7) | + LaserStream $499-999 (if replacing SVS) + ~0.004 SOL overhead per probe |
| **live** | fra1; after the gate add DZ Edge + Helius shreds | money | **gate (below)** + `mode = "live"` in TOML + CLI flag + local `rugsnipe-ctl arm-live` token for that gate run (valid 7 days) | + DZ $1,500 + Helius shreds $800-1,000 + tips |

**The gate** (docs/06 section 9.5), signed into the ArmSet as `gate_passed`. All of these must hold:
- ≥ 300 out-of-sample snipes from ≥ 15 operators, and no operator above 40% of P&L;
- the 95% operator×day cluster-bootstrap lower bound of EV(selected) − EV(matched control) is > 0 at E1, base costs and the **measured** k̂;
- EV ≥ 0 at high costs;
- vault EV > 0;
- landing holds after Alpenglow (Oct 21 2026).

Until probes measure landing, farming and exit fills, every EV is an upper bound and no ArmSet can carry `gate_passed = true`.
The hot box runs **exits only** if the ArmSet is unsigned, older than `armset_max_age_s` (6 h) or from the wrong regime.

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
- probe: **~$575-1,425/mo** + ≤ 0.5 SOL;
- live: box + DZ + Helius shreds + the post-exec gRPC feed that probe/live require (SVS $150 or LaserStream $499-999) ≈ **$2,850-3,925/mo** + tips.

## 5. Install and update

```bash
# both roles: always dry-run first, then the real thing (idempotent, safe to re-run)
bash deploy/install.sh --role analytics|hot --dry-run
sudo bash deploy/install.sh --role analytics|hot [--user rugsnipe] [--ntp-region frankfurt] [--hot-cpus 8-15,24-31] [--enable]

# update
cd ~/rugsnipe && git pull && sudo bash deploy/install.sh --role <role>
sudo systemctl restart rugsnipe-collector                      # ohio1
sudo systemctl restart rugsnipe-keeper rugsnipe-guard          # fra1, then hot (see the note below)
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

Per docs/06 section 5.1 the latch is also to be recorded in `state.db` (`risk.killed`), so that it survives a restart and a reboot (which clears `/run`). This is not implemented yet (M3+): until then a reboot clears the file latch. **Unkill is local only.** Log in to fra1, remove the file and clear the
latch with `rugsnipe-ctl` (M3+). A Telegram message can kill but never unkill.

### Automatic kill triggers (docs/06 section 13)

The latch fires automatically on any of these:
- the daily-loss cap (chain-derived, marked to the sell quote);
- an unexplained balance drop > 0.05 SOL;
- send errors > 20% over 5 min;
- decode-miss > 1%;
- a program upgrade or FeeConfig change (buys go to paper until `rugsnipe-sim` passes);
- a stale ArmSet;
- disk > 90%;
- clock offset > 50 ms;
- reconcile drift > 60 s.

### Stop everything

`sudo systemctl stop rugsnipe-hot` stops entries and the hot exit loop. **Guard takes over the exits after 5 s.**

`sudo systemctl stop rugsnipe.target` also stops guard. Any open position is then **stranded**: it is bounded by
`max_open_positions × max_snipe` (4 × 0.25 SOL) but unmanaged. Do this only when flat. Check first with
`sqlite3 -readonly /var/lib/rugsnipe/state/state.db "select mint,state from positions where state!='closed'"`.

### Alerts

Critical alerts go **directly from fra1** to Telegram, plus a healthchecks.io dead-man ping. They never route via Ohio.

| Alert | Meaning | Do this |
|---|---|---|
| dead-man missed / hot down 30 s | hot is not feeding the watchdog, or the box is dead | `systemctl status rugsnipe-hot rugsnipe-guard`; `journalctl -u rugsnipe-hot -n 200`. Guard owns the exits meanwhile. If the box is unreachable, open the provider console (positions are bounded by the caps) |
| killed | a latch fired | read `kill_reason` (journal / `risk` table), fix the cause, unkill locally |
| exit feed down / position owned by guard | the post-exec feed for a held mint is lost, or hot's exit loop stalled | check the feed health metrics. Guard sells on a ≥ 30% drop from peak, MaxHold or `grace_s` |
| sell failing 5 tries | the sell bound or sender path is broken | inspect `exits`/`sends`; the per-position sell-spend cap stops escalation; sell manually if needed |
| drift | ledger ≠ chain | the keeper reconciles. Buys stop above 60 s |
| upgrade | pump / PumpSwap / pump-fees programdata or FeeConfig changed | buys go to paper automatically; wait for `rugsnipe-sim` to pass |
| low balance | hot wallet < `min_hot_balance` | top up from cold, or leave it in exits-only |

Observe:
- Prometheus metrics on `127.0.0.1:9464` and `10.77.0.2:9464` (wg0 only): feed lag, decode misses, stage latency, sends, landed slot offset, positions, nonces, wallet, daily P&L, ArmSet age, lease owner, drift;
- a daily digest with per-operator stats.

### Failover (phase 1)

| Failure | Response |
|---|---|
| hot crash | systemd restarts it and runs recovery; guard takes over after 5 s |
| Ohio down | fra1 keeps the last ArmSet until `armset_max_age_s`, then runs exits only |
| fra1 dead | positions stranded, bounded by `max_open_positions × max_snipe`; the dead-man alert fires |
| paid feeds down | entries go to paper; exits poll within the RPC budget |

## 7. Validation done in this session

- `bash -n` and `shellcheck` 0.11 (via `shellcheck-py`) are clean on `deploy/install.sh`, `deploy/tune-hot.sh` and `deploy/bin/rugsnipe-pyrun`. Run `bash deploy/selftest.sh` to repeat this offline.
- `systemd-analyze verify` (systemd 255) is clean on every unit, run in a scratch root with stub binaries. `systemd-analyze security --offline`: hot 1.8, guard/keeper 1.5, collector/discover 1.3.
- The `SetCredential=` fallback semantics were checked against the systemd v249 man page (the Ubuntu 22.04 version). An empty value is rejected, hence the `ABSENT` sentinel.
- `install.sh --dry-run` was run for both roles in this Ubuntu 24.04 container, without systemd. Key-permission refusal was tested with a 0644 dummy file.
- `tune-hot.sh --check` and `--dry-run` were run here. This is a VM, so there is no cpufreq or cpuidle, and those steps report SKIP.
- **Not done here:** a real install as root, starting units under a live systemd, and running on bare metal. These happen on ohio1 and fra1 using the guides above.
