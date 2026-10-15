# 08 Runbook: from an empty server to a go/no-go answer

Date 2026-10-14. This is the operator's checklist: what to do, in order, with the exact commands. Every
command below exists in this repository (or is a standard Ubuntu tool); where a step needs code that is
not built yet, the step says so and stops there.

Background: `docs/04-strategy.md` (what we trade and the go/no-go rule, §7), `docs/03-speed-infra.md`
(where and how to send, §4 and §6.1), `docs/06-architecture.md` (config, §6), `docs/07-deploy.md` and
`deploy/frankfurt-checklist.md` (install details).

## 0. What this build can and cannot do today

| Can | Cannot yet (blocks the step named) |
|---|---|
| Shadow mode (paper, no money): every multi-wallet bundled launch gets a would-be entry right behind the bundle, a measured premium, the LEAD exits replayed on the real tape, and failed attempts charged at their modelled cost | **Probe and live.** `rugsnipe-hot` refuses every real mode ("this build has no real key loading or real senders"). Missing (task list #12): real key loading, real senders in the hot binary, the real fill lane (our own TradeEvent, `meta.fee` of our failed transactions), kill-latch persistence, chain-derived daily loss, the TPU sender, nonce seeding from chain, Telegram alerts. **Step 9 is blocked** until these land |
| Read rule (a) of the go/no-go gate: EV per attempt with a 90% lower bound (`analysis/scripts/shadow_gate.py`) | **Rule (b)** (are we ahead of the first follower?). The shadow lane *assumes* rank 1; our real rank needs real sends or a shred feed. It is decided in the probe |
| Feeds: `logsSubscribe` over any websocket URL, PumpPortal, the collector relay | Yellowstone gRPC, DoubleZero Edge or Helius shreds: not in the hot binary. Do not buy them yet |
| Bid policy: tip after the buy (pay on success) where the leader orders by tip (Jito-Agave), priority fee clamped by `bid.failed_attempt_lamports_max` elsewhere (BAM, Harmonic, other); sells take the same split, the bid paid once (docs/03 §6.1) | The leader's class per slot (no leader schedule in the hot path): paper uses the stake-weighted mix |
| The P(TP) entry filter (`[entry] filter_model`, `filter_mode = "log"` or `"skip"`, docs/04 §3a) scores every paper intent from the E1 lane's reconstruction and records the score and tertile in `state.db` `filter` | Scoring live from shreds (the bundle run behind the create must be assembled from the slot's entries first) |

Practical consequence: rule (a) does not depend on where the box is (the lane rebuilds each launch from
confirmed history). You may run steps 5-8 on ohio1 or any Linux box first and rent Frankfurt only when the
probe build exists. Renting now buys you the acceptance test and a warm box; it does not change rule (a).

## 1. Rent the Frankfurt box (docs/03 §4)

Order in this preference:

| Option | Price | Notes |
|---|---|---|
| TeraSwitch FRA2 bare metal, Ryzen 9 9950X | ~$425/mo (list; stock on request from sales) | Same building as Jito FRA, Helius `fra-sender`, the Harmonic and BAM Frankfurt nodes, DoubleZero `fra2-tsw` |
| Cherry Servers Frankfurt, EPYC 9375F | €989/mo or €1.69/h | Same campus (0.09 ms), in stock: the immediate fallback |
| Latitude.sh FRA | ~$400/mo | Different building (0.7 ms): last resort |

Never Hetzner (crypto clause). Ask for: x86_64 ≥ 8 physical cores, 64-128 GB RAM, 1-2 TB NVMe, a
10/25 GbE NIC, **a public IPv4 without NAT**, ≥ 20 TB/mo traffic, IPMI/KVM, Ubuntu 24.04, and **a test
IP before you sign**.

## 2. Acceptance test (before you pay for a month)

On the test box:

```bash
git clone <your repo URL> ~/rugsnipe && cd ~/rugsnipe
bash deploy/latency_probe.sh 30 | tee latency-fra-candidate.txt      # TCP connect to Jito/Helius/RPC hosts
for ip in 64.130.50.88 64.130.50.91 198.13.140.105 198.13.140.227 198.13.140.16; do
  ping -c 100 -i 0.2 -q $ip | tail -2; done
mtr -rwc 50 64.130.50.88                                              # path stays inside AS20326
```

**Accept only if** the five in-building targets (Jito FRA, Helius `fra-sender`, Harmonic FRA TPU, BAM FRA
node, DZ `fra2-tsw`) answer with **average RTT < 0.2 ms, max < 2 ms, 0% loss**, and `jito-frankfurt` /
`helius-fra` are the fastest rows of `latency_probe.sh`. Otherwise the box is not in the building: reject
it. Keep `latency-fra-candidate.txt`.

## 3. Install (hot role)

```bash
ssh root@<FRA1_IP>
adduser --disabled-password ops && usermod -aG sudo ops       # then key-only SSH, PasswordAuthentication no
apt-get update && apt-get -y full-upgrade && apt-get -y install sqlite3 && reboot
# as ops
git clone <your repo URL> ~/rugsnipe && cd ~/rugsnipe
bash deploy/install.sh --role hot --dry-run                    # read what it will do
sudo bash deploy/install.sh --role hot                         # builds /opt/rugsnipe/bin/*, venv, chrony -> ntp.frankfurt.jito.wtf
sudo /opt/rugsnipe/bin/tune-hot.sh --check
sudo /opt/rugsnipe/bin/tune-hot.sh --apply                     # governor, irqbalance, sysctls, THP
# paste the printed GRUB line into /etc/default/grub (GRUB_CMDLINE_LINUX_DEFAULT), then:
sudo update-grub && sudo reboot
cat /sys/devices/system/cpu/isolated                           # must list the isolated cores
sudo bash ~/rugsnipe/deploy/install.sh --role hot              # re-pins rugsnipe-hot to them
sudo systemctl enable rugsnipe-tune.service
chronyc tracking                                               # |System time| well under 1 ms
```

Do **not** enable `rugsnipe.target` during shadow mode: the shadow run below is started by hand with its
own config and ArmSet. Firewall (ufw): deny incoming by default, allow 22/tcp from your IP, nothing else
is needed for shadow mode.

Updating later: `cd ~/rugsnipe && git pull && sudo bash deploy/install.sh --role hot`.

## 4. Keys and credentials

**Shadow mode needs no wallet and no API key.** `rugsnipe-hot` makes a throwaway paper wallet and never
opens a sending socket.

It needs one thing: an **ArmSet signing key**, which never goes on the hot box (install.sh refuses it
there). Make it on ohio1 (or your laptop, outside any git checkout):

```bash
# ohio1 (installed with: sudo bash deploy/install.sh --role analytics)
sudo /opt/rugsnipe/bin/rugsnipe-ctl armset keygen --out /etc/rugsnipe/keys/armset-signing.key
#   -> "pubkey <BASE58>": you need this pubkey in step 5
sudo chmod 0400 /etc/rugsnipe/keys/armset-signing.key
```

For the probe only (step 9), on fra1, never on a laptop:

```bash
sudo install -d -m 0700 -o root -g root /etc/rugsnipe/keys
sudo solana-keygen new --no-bip39-passphrase -o /etc/rugsnipe/keys/probe-keypair.json
sudo solana-keygen new --no-bip39-passphrase -o /etc/rugsnipe/keys/hot-keypair.json
sudo chmod 0400 /etc/rugsnipe/keys/*
sudo $EDITOR /etc/rugsnipe/keys/api-keys.toml && sudo chmod 0400 /etc/rugsnipe/keys/api-keys.toml   # names in §13
sudo bash ~/rugsnipe/deploy/install.sh --role hot              # refuses group/world-readable keys
```

## 5. The shadow configuration

```bash
sudo install -m 0640 -o root -g rugsnipe /opt/rugsnipe/src/config/paper-lead.example.toml /etc/rugsnipe/shadow.toml
sudo install -d -m 0750 -o rugsnipe -g rugsnipe /var/lib/rugsnipe/shadow /var/lib/rugsnipe/armsets
sudo $EDITOR /etc/rugsnipe/shadow.toml
```

Change exactly these lines:

| Key | Set to | Why |
|---|---|---|
| `[secrets] armset_pubkey` | the pubkey printed in step 4 | the example carries a dev key you do not have |
| `[risk] kill_file` | `"/run/rugsnipe/KILL"` | the kill switch of §10 |
| `[obs] rslog_dir` | `"/var/lib/rugsnipe/shadow/rslog"` | the service user can write only under `/var/lib/rugsnipe` |
| `[feeds] logsws.url` | keep `wss://api.mainnet-beta.solana.com`, or your RPC provider's websocket URL | the only create feed this build has |
| `[entry] filter_model` | `"/etc/rugsnipe/entry_filter_v1.json"` | the frozen P(TP) model; copy it first (below) |
| `[entry] filter_mode` | `"log"` | score and record every launch, drop none (shadow measures selected and unselected launches alike) |

```bash
# the model file: /opt/rugsnipe/src/data is a symlink to the state directory, so copy it from your clone
sudo install -m 0640 -o root -g rugsnipe ~/rugsnipe/data/reports/strategy/entry_filter_v1.json /etc/rugsnipe/entry_filter_v1.json
```

Leave everything else as in the example: `[mode] mode = "paper"`, `[entry] filter = "multi_bundle"`,
`max_premium = 0.02`, the LEAD exits, `[paper] fill_model = "e1"`, `failed_attempt_lamports = 0`
(failed attempts are charged at the modelled per-class cost: base fee plus the priority fee of the
variant that matches the leader, tips revert; about 0.00073 SOL at the defaults, where only Jito-Agave
pays on success and BAM bids with the priority fee) and the `[bid]` block (per-class split,
`failed_attempt_lamports_max = 1_000_000`). Check it:

```bash
/opt/rugsnipe/bin/rugsnipe-ctl config validate /etc/rugsnipe/shadow.toml       # paper: must say OK, no refusals
```

## 6. The shadow ArmSet (a class arm: every create)

On ohio1:

```bash
cd /opt/rugsnipe/src
/opt/rugsnipe/venv/bin/python -I analysis/scripts/make_paper_armset.py --kind class --ver 1 --out /tmp/class.json
sudo /opt/rugsnipe/bin/rugsnipe-ctl armset sign --key /etc/rugsnipe/keys/armset-signing.key --in /tmp/class.json --out-dir /tmp/armset
scp /tmp/armset/armset-1-*.json /tmp/armset/armset-1-*.json.sig ops@<FRA1_IP>:/tmp/
```

On fra1:

```bash
sudo install -m 0640 -o rugsnipe -g rugsnipe /tmp/armset-1-* /var/lib/rugsnipe/armsets/
/opt/rugsnipe/bin/rugsnipe-ctl armset verify /var/lib/rugsnipe/armsets/armset-1-<sha>.json --pubkey <PUBKEY> --regime paper
```

A running paper process keeps using its ArmSet after the 6-hour freshness window (staleness only blocks
real modes), so one ArmSet covers a multi-day run. A process **started** with an ArmSet older than 6 hours
loads none and makes no decisions: before every new run, repeat this step with the next `--ver`.

## 7. Start shadow mode

```bash
sudo systemd-run --unit=rugsnipe-shadow --uid=rugsnipe --gid=rugsnipe \
  --property=WorkingDirectory=/var/lib/rugsnipe \
  /opt/rugsnipe/bin/rugsnipe-hot --config /etc/rugsnipe/shadow.toml \
    --armset /var/lib/rugsnipe/armsets/armset-1-<sha>.json \
    --state /var/lib/rugsnipe/shadow/state.db --rslog-dir /var/lib/rugsnipe/shadow/rslog \
    --summary /var/lib/rugsnipe/shadow/summary.json --duration-s 93600
journalctl -u rugsnipe-shadow -f          # one status line per minute
```

`--duration-s 93600` is 26 hours: the rule needs at least one full day. Stopping early is safe and still
writes the summary: `sudo systemctl stop rugsnipe-shadow`. A run that ends (duration reached, stop, or
crash) is not restarted; start a new one with a **new** `--state` file name per run, or the same file to
keep accumulating (the tables append).

**Read the status line.** `lane R/Q calls C` is the E1 lane: resolved / requested intents and RPC calls.
If `R` falls further and further behind `Q`, the public RPC is the bottleneck: add
`--rpc-url https://<your provider>` and raise both `[rpc_budget] hot` in `shadow.toml` and `--lane-rps`
(the lane uses the lower of the two; the paper run ignores the 32 req/s sum rule). `landing` and `net` are
the running shadow numbers. The 20-minute container runs resolved 320 of 342 intents at 3.5 req/s.

**Entry filter, log mode.** With `filter_mode = "log"` every qualifying launch is still attempted; the
frozen P(TP) score and its tertile (0 = bottom, the one skip mode would drop; 1 = middle; 2 = top) go to
the `filter` table, and `shadow_gate.py` (step 8) splits EV and the shadow landing share by tertile.
Switch to `"skip"` only for the probe, and only after shadow shows the bottom tertile losing; adopt a
tighter cut than the bottom tertile only if its landing rate is at least 0.73× the all-launch rate
(docs/04 §7), which only the probe can measure.

## 8. Go / no-go (docs/04 §7, fixed before any shadow data)

> (a) Over **at least 1,000 qualifying launches and at least one full day**, the shadow EV per attempt at
> our measured rank and premium, failed attempts charged at their cost, must have a **90% lower bound
> above zero** (bootstrap by hour clusters); and (b) we must be **ahead of the first follower on at least one hot launch in three** at a
> premium of +2% or less. Otherwise there is no edge, and we stop.

**Read (a)** after the run:

```bash
cd /opt/rugsnipe/src
/opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py \
  --state /var/lib/rugsnipe/shadow/state.db --summary /var/lib/rugsnipe/shadow/summary.json
# the same with every failed attempt priced at docs/04's all-priority 0.003 SOL:
/opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py \
  --state /var/lib/rugsnipe/shadow/state.db --summary /var/lib/rugsnipe/shadow/summary.json --fail-cost-lamports 3000000
```

It prints the attempts scored, hours covered, the shadow landing share, the failed-attempt cost, the EV
per attempt with its 90% CI (normal, and by a bootstrap over UTC-hour clusters, which rule (a) uses), the split by "a third-party buyer landed behind us or not", the split by
entry-filter tertile (when the `filter` table exists), and the verdict:
`GO on (a)`, `NO-GO on (a): stop (no edge)` or `NOT ENOUGH DATA`. **Decide on the modelled cost; if (a)
passes there but not at 0.003 SOL, the probe must confirm the failed-attempt cost before anything else**
(it is the single biggest lever: break-even landing of the 0.5 SOL LEAD 3.3% at the modelled 0.00073 SOL,
1.6-4.4% over the modelled range 0.00036-0.001 SOL, 12.1% at 0.003; docs/04 §4).

The same from the end-of-run summary (`summary.json`, block `cold.paper`):

| Field | Meaning |
|---|---|
| `in_class` | qualifying launches resolved (multi-wallet bundled, not `class`, not `unresolved`) |
| `attempts`, `fills`, `failed_attempts` | would-be sends; `landing_rate = fills / attempts` (refusals by the +2% cap and the block position only, **not** a measured race) |
| `failed_cost_lamports`, `failed_cost_lamports_mean` | total cost charged to failed attempts, and per failed attempt |
| `ev_per_attempt` | net / attempts as a fraction of the mean stake: the point estimate of (a) |
| `ev_per_attempt_se`, `ev_per_attempt_90ci`, `attempts_scored` | its standard error and normal 90% interval over the scored attempts (the probe kill rule reads the upper bound) |
| `median_premium_bps_fills` | median premium of the fills (the probe kill rule's "median realised premium") |
| `outcomes` | `filled`, `premium`, `slippage`, `not_in_slot0`, `class`, `unresolved` |

The SQL behind it (`sudo sqlite3 -readonly /var/lib/rugsnipe/shadow/state.db`):

```sql
-- outcomes and what failures were charged
SELECT outcome, COUNT(*), SUM(cost) FROM shadow GROUP BY outcome;
SELECT fail_reason, COUNT(*), AVG(fail_cost), SUM(fail_cost) FROM fills
 WHERE fail_reason IS NOT NULL GROUP BY fail_reason;

-- EV per attempt: n, mean, variance (90% lower bound = ev - 1.645 * sqrt(var / n));
-- 4000000 = paper.round_trip_lamports per landed round trip
WITH a AS (
  SELECT CASE WHEN s.outcome = 'filled'
           THEN ((SELECT SUM(e.sol_delta) FROM exits e WHERE e.mint = s.mint) + f.sol_delta - 4000000) * 1.0 / i.spend
           ELSE -f.fail_cost * 1.0 / i.spend END AS r
  FROM shadow s
  JOIN intents i ON i.intent_id = s.intent_id
  JOIN fills f ON f.intent_id = s.intent_id
  LEFT JOIN positions p ON p.mint = s.mint
  WHERE s.outcome IN ('premium', 'slippage', 'not_in_slot0')
     OR (s.outcome = 'filled' AND p.state = 'closed'))
SELECT COUNT(*) AS n, AVG(r) AS ev,
       (AVG(r * r) - AVG(r) * AVG(r)) * COUNT(*) / (COUNT(*) - 1) AS var
FROM a;

-- fills at a premium of +2% or less (the premium half of rule b)
SELECT COUNT(*), SUM(premium_bps <= 200) FROM shadow WHERE outcome = 'filled';
-- exits by reason
SELECT reason, COUNT(*), SUM(sol_delta) FROM exits GROUP BY reason;
-- entry filter (log mode): outcomes by P(TP) tertile (0 = bottom = what skip mode drops)
SELECT f.tertile, s.outcome, COUNT(*) FROM filter f JOIN shadow s ON s.intent_id = f.intent_id
 GROUP BY f.tertile, s.outcome ORDER BY f.tertile;
```

**Rule (b)** cannot be read from shadow data in this build (the lane places us right behind the bundle
by assumption). It is decided in the probe: probe-A measures our real landing rank, and the gate's (b)
is read on probe-B (docs/06 §9.5).

## 9. Moving to probe (blocked in this build)

**When:** only if step 8 says `GO on (a)`, **and** the build has real mode (§0; task #12). Until then,
stop here and keep shadow running.

**What** (docs/04 §7 step 2, `[probe]` in the config): two stages, both with `max_sol_cost` +2% and the
LEAD exits. `[probe] stage` selects the caps; each narrows the `[risk]` cap of the same name, and a
signed ArmSet that names `probe_stage` must name the same stage, and stage B needs one that names `"b"`
(moving to B takes both keys).

| Stage | Per buy | Attempts/day | Open positions | Daily loss cap | Runs until | Purpose |
|---|---|---|---|---|---|---|
| **A** (`stage = "a"`) | 0.02 SOL | 50 | 1 | 0.2 SOL | 300 attempts or 3 days | measure landing rank, premium, failed-attempt cost per leader class, which variant lands. EV is negative at this size (fixed costs): **not** a kill reason |
| **B** (`stage = "b"`) | 0.25 SOL | 300 | 2 | 1 SOL | about 200 landed trades | the EV per attempt at our real rank and cost; gate (b) |

**Kill** (either stage; `shadow_gate.py --probe` prints the verdict): the EV per **attempt**, failed
attempts included, has a 90% upper bound below 0 after 100 landed trades or 400 attempts, whichever
comes first (probe-B only); or the median realised premium exceeds +3%; or the landing rate stays below
the break-even for the measured failed-attempt cost over 200 attempts; or the stage's daily loss cap is
hit (the risk gate latches the kill itself). **Enter B** only if probe-A ends with a landing rate above
that break-even and a median premium at or below +2% (`PROBE-A COMPLETE: ENTER PROBE-B`).

**How**, once the build supports it:

1. The probe ArmSet (on ohio1), stage A first:
   ```bash
   /opt/rugsnipe/venv/bin/python -I analysis/scripts/make_paper_armset.py --kind class --ver 2 --probe-allowed --probe-stage a --out /tmp/probe-a.json
   sudo /opt/rugsnipe/bin/rugsnipe-ctl armset sign --key /etc/rugsnipe/keys/armset-signing.key --in /tmp/probe-a.json --out-dir /tmp/armset
   ```
   It carries `probe_allowed = true`, `probe_stage = "a"`, the class operator in state `probe` at 0.02 SOL,
   never `gate_passed`. For B: `--probe-stage b` (0.25 SOL) and a new `--ver`.
2. `/etc/rugsnipe/rugsnipe.toml` (from `config/rugsnipe.example.toml`): `[mode] mode = "probe"`,
   `[probe] stage = "a"` (the example's `a`/`b` profiles and `kill` table as shipped), and **every** cap
   non-zero (validation refuses a zero). The `[risk]` values are ceilings for both stages:

   | Key | Value | Note |
   |---|---|---|
   | `risk.max_probe_lamports` | `250_000_000` | ceiling; stage A narrows it to 0.02 SOL, B keeps 0.25 SOL |
   | `risk.max_probes_per_day` | `300` | attempts, not fills; A narrows to 50 |
   | `risk.max_open_positions` | `2` (or the example's 4) | A narrows to 1, B to 2; a LEAD position lives about 2 s |
   | `risk.max_daily_loss_lamports` | `1_000_000_000` | A narrows to 0.2 SOL, B keeps 1 SOL |
   | `risk.max_consec_losses_per_op` | `20` | the class arm is one operator and the median LEAD trade is a small loss; 5 would suspend it constantly |
   | `risk.max_buys_per_op_per_day`, `max_buys_per_hour` | `300`, `40` | |
   | `risk.max_bid_lamports` | `25_000_000` | the bid itself; the failed-attempt part is capped below |
   | `bid.failed_attempt_lamports_max` | `1_000_000` | most a lost race may cost (docs/03 §6.1) |
   | `bid.split` | as shipped | Jito-Agave pays on success (tip); BAM, Harmonic, other bid with the capped priority fee. Change BAM only when probe-A shows its variant landing better with the tip |
   | `tx.cu_limit`, `tx.lads_bytes` | `88_000`, `12_124_160` | after `cargo run --release -p rugsnipe-ctl --features rpc -- sim --n 3` confirms the fit |
   | `tx.nonce_pool` | `32` | create them with `rugsnipe-ctl nonce init --authority <PROBE_PUBKEY> --count 32` (prints unsigned transactions for review) |

3. Fund the probe wallet with what the caps allow you to lose plus working capital (1 SOL for A,
   2-3 SOL for B), check and start:
   ```bash
   /opt/rugsnipe/bin/rugsnipe-ctl config validate /etc/rugsnipe/rugsnipe.toml --creds /etc/rugsnipe/keys \
     --clock-offset-ms <from chronyc tracking> --post-exec-healthy --armset /var/lib/rugsnipe/armsets/armset-2-<sha>.json
   sudo systemctl enable --now rugsnipe.target
   ```
   `config validate` prints the ArmSet's `probe_stage` and refuses a mismatch with `[probe] stage` (and
   `stage = "b"` with an ArmSet that names no stage).
4. **Read the kill rule** at least daily, and at the end of each stage:
   ```bash
   cd /opt/rugsnipe/src
   sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py --probe \
     --state /var/lib/rugsnipe/state.db --config /etc/rugsnipe/rugsnipe.toml
   ```
   It prints attempts, landed trades, the landing rate and its break-even (`pnl_model.break_even_landing`
   for the 0.25 SOL LEAD at the measured mean failed-attempt cost and median premium; 5.1-13.1% over the
   modelled 0.00036-0.001 SOL, 9.9% at the default 0.00073), the failed-attempt cost, the median premium,
   the EV per attempt with its 90% interval (normal and by hour clusters), the worst day against the cap, the largest hour's share of
   the P&L, attempts and landings per bid variant, and one verdict: `KILL: <reasons>`, `CONTINUE probe-A|B`,
   `PROBE-A COMPLETE: ENTER PROBE-B`, `PROBE-A COMPLETE: DO NOT ENTER PROBE-B ...: stop` or
   `PROBE-B COMPLETE: read the live gate`. On `KILL`, touch the kill file (§10). The end-of-run summary's
   `cold.paper` block carries the same inputs (`attempts_scored`, `landing_rate`, `failed_cost_lamports_mean`,
   `median_premium_bps_fills`, `ev_per_attempt`, `ev_per_attempt_se`, `ev_per_attempt_90ci`). Raw rows are
   in `state.db` (`fills`, `positions`, `exits`, `sends`; failed attempts carry `fills.fail_cost` = the
   `meta.fee` of the failed transaction). Per leader class needs the leader schedule (task #12); until then
   the split is per variant.

**Live** needs the docs/06 §9.5 gate for the class-arm LEAD (`gate_passed` in a signed ArmSet): (a) shadow
over ≥ 1,000 launches and a full day with the hour-cluster 90% lower bound of the EV per attempt above 0;
(b) probe-B complete with ≥ 200 landed trades, the EV per attempt (failures included) lower bound above 0,
no single hour above 40% of the P&L, ahead of the first follower on at least one hot launch in three at
≤ +2%; (c) every cost measured; (d) all of it re-verified after Alpenglow. Then `mode = "live"`, the
`--live` flag and an arm-live token. None of that exists before probe-B has passed.

## 10. Kill switch

```bash
sudo touch /run/rugsnipe/KILL                          # buys stop, exits continue (file latch)
sudo systemctl kill -s USR1 rugsnipe-shadow            # same, by signal (rugsnipe-hot for the service)
sudo systemctl stop rugsnipe-shadow                    # stops the process; the summary is still written
```

**Unkill:** `sudo rm /run/rugsnipe/KILL`, then restart the process. In this build the latch lives in
the process only (not yet in `state.db`), so a restart clears it once the file is gone, and a reboot
clears `/run`. Once real money is involved, stop everything only when flat:
`sudo sqlite3 -readonly /var/lib/rugsnipe/state.db "select mint,state from positions where state!='closed'"`.

## 11. After Alpenglow (epoch 1064, Oct 21 2026 ~15:00 UTC)

Slot timing changes (confirmation ~150 ms, several banks per slot, ticks collapse). Do this that week:

1. **Split the data at the cutover.** Stop the running shadow before Oct 21 15:00 UTC and start a new
   one with a new `--state` file after it. The go/no-go rule needs a full post-Alpenglow day; pre-cutover
   data does not count toward it.
2. **Watch the feed and the lane** in the status lines: `miss` (decode misses: buys stop above 1%),
   `reconnects`, and the lane's `create signature not in the curve's history` / `unresolved` share in the
   summary (`cold.e1_errors`, `cold.paper.outcomes.unresolved`).
3. **Re-measure the slot length:** `shadow_gate.py --summary ...` prints `ms per slot` (run time over the
   slot span of the scored attempts). It is only valid for an uninterrupted run: a kill latch, a stall or
   a quiet stretch at either end inflates it (a 5-minute test run killed at 150 s printed 509). Check it
   against the chain's own samples before you change anything:
   ```bash
   curl -s https://api.mainnet-beta.solana.com -X POST -H 'Content-Type: application/json' \
     -d '{"jsonrpc":"2.0","id":1,"method":"getRecentPerformanceSamples","params":[60]}' \
     | python3 -c 'import json,sys; s=json.load(sys.stdin)["result"]; print(1000*sum(x["samplePeriodSecs"] for x in s)/sum(x["numSlots"] for x in s), "ms per slot")'
   ```
   If both move away from 217, set `[paper] slot_ms` to the new value in `shadow.toml`.
4. **Re-run the decoder tests** on the new box build: `cd ~/rugsnipe && cargo test -p pump-codec -p rugsnipe-feed`.
5. **Leader classes flip during the cutover** (BAM validators fall back to plain Jito-Agave mode). The
   bid split per class (docs/03 §6.1: Jito-Agave tip, BAM/Harmonic/other priority fee) does not need
   changing, but `[paper] leader_mix` should be updated when the stake shares settle (it moves the
   modelled failed-attempt cost: more Jito-Agave stake makes it cheaper).
6. `[mode] regime_freeze_slots = [[459_640_000, 459_869_000]]` already forces any real buy to paper across
   the cutover.

## 12. Weekly

Run everything as the service user: `/opt/rugsnipe/src` is root-owned, and its `data/` is a symlink to
`/var/lib/rugsnipe/data` (owned by `rugsnipe`), where the caches and reports land. Running as root or
`ops` would leave root-owned files there that the services cannot write.

**A fresh box has no research caches.** `install.sh` copies `data/reports` but not `data/cache` (the
tapes and API caches of the research checkout: gap-3, deep, fresh, the dense window, the copy-trade
holdout). So on fra1 or ohio1:
- step 2 scores **this week's frame only**: the armed-creator and random launches it pulls now
  (`P1_frame_*`, `P1_pooled_random` = the frame's random launches, `P2/P3_*_armed`); the rows that need
  the research caches (`P1_copy_0612_1045`, `sec_window_late_*`) come out empty (n = 0), and
  `critic_holdout.json`'s `combined_realistic_and_hot_split` block stays the critic's Oct 14 figure
  (`critic_holdout_eval.py` never rewrites it). This week's realistic-fill stack and hot/quiet split are
  `critic_realistic.json`;
- step 3 (`pnl_run.py`) measures on the research tapes: it prints `research tape cache missing ...` and
  leaves `pnl_model.json` as committed. Re-run it in the research checkout when the inputs change.

```bash
cd /opt/rugsnipe/src
PY="sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I"
D=/var/lib/rugsnipe/data          # = /opt/rugsnipe/src/data
# 1. the frozen LEAD on the last 7 days of shadow data
$PY analysis/scripts/shadow_gate.py --state /var/lib/rugsnipe/shadow/state.db --last-hours 168
# 2. the freeze-then-holdout re-check on fresh launches (same protocol as docs/04 §3).
#    Keep last week's frame and reports: the stages resume from their cache, so the old frame must move.
W=$(date +%F)
[ -d $D/cache/strategy_holdout ] && sudo -u rugsnipe mv $D/cache/strategy_holdout $D/cache/strategy_holdout.$W
for f in critic_holdout critic_realistic; do
  sudo -u rugsnipe cp $D/reports/strategy/$f.json $D/reports/strategy/$f.$W.json; done
$PY analysis/scripts/strategy/critic_holdout.py index           # anchor = now: frame = now-6 h .. now-1 h
$PY analysis/scripts/strategy/critic_holdout.py decode          # every create of the frame (~10k, ~1 h at 3 req/s)
$PY analysis/scripts/strategy/critic_holdout.py pull            # armed first, then random; cap 800 swap-api requests
$PY analysis/scripts/strategy/critic_holdout_eval.py            # -> $D/reports/strategy/critic_holdout.json
$PY analysis/scripts/strategy/critic_realistic.py               # -> $D/reports/strategy/critic_realistic.json
# 3. the daily P&L model (needs the research caches: on a deployed box it says so and changes nothing)
$PY analysis/scripts/strategy/pnl_run.py                        # -> $D/reports/strategy/pnl_model.json
```

A quick look instead of the full frame: `decode 3.0 300` decodes 300 creates of the seeded order and
`pull both 50` stops at 50 requests; both resume to the full frame later. **Tested** on 2026-10-14 in a
scratch fresh-box layout (source tree without `data/cache`, `data` a symlink to a separate directory,
empty cache): index 15,000 signatures in 9 s, `decode 3.0 300` (379 RPC calls, 2 min; 184 eligible),
`pull both 50` (45 requests, 36 tapes: 10 multi-wallet launches), then `critic_holdout_eval.py`,
`critic_realistic.py` and `pnl_run.py` ran to completion as described above; step 1 ran on a 10-minute
paper shadow `state.db` from this container. That test ran as root (the container has no `rugsnipe`
user), so the `sudo -u rugsnipe` wrapper and the ownership of `/var/lib/rugsnipe/data` were not exercised.

Write down each week: attempts, EV per attempt and its 90% CI, the failed-attempt cost, and the holdout
EV of the frozen LEAD. Two consecutive weeks with the 90% lower bound below zero is a stop.

## 13. Costs per month and the paid keys

> **Shopping list (2026-10-15):** `docs/research/infra-shopping-2026.md` ranks every feed, sender and RPC
> option by value for the probe and for month 1, with prices, Frankfurt hostnames and signup steps.
> Short version: the probe needs about $55-100 all-in (Triton pay-as-you-go feed, free senders with a
> free Helius key at 50 req/s, SVS Basic RPC for a week); month 1 needs about $124/mo of subscriptions
> and the fees are 95% of the cost. Corrections to the table below: Helius Sender is 50 req/s per
> region with a free API key (1/s keyless), BlockRazor is free with a 0.0001 SOL minimum tip, NextBlock
> has a free trial, `instantnodes.io` is not to be used.


| Item | $/month | Needed for | Used by this build? |
|---|---|---|---|
| TeraSwitch FRA2 bare metal (or Cherry €989, Latitude ~$400) | ~425 | probe and later; optional for shadow | yes (any box runs shadow) |
| AWS Ohio analytics box (ohio1) | existing bill | ArmSet signing, holdouts, reports | yes |
| RPC with websocket near Frankfurt (logsSubscribe + the E1 lane) | vendor price | shadow, if the public RPC lane falls behind | yes (`feeds.logsws.url`, `--rpc-url`) |
| SVS Yellowstone gRPC | 150 | docs/07 shadow plan | **no** (no gRPC feed in the hot binary) |
| Helius LaserStream fra (post-exec gRPC) | 499-999 | probe | **no** (same) |
| DoubleZero Edge FRA seat (leader shreds) | 1,500 | live, after the gate | **no** (no shred feed yet) |
| Helius shred delivery per IP | 800-1,000 | live, after the gate | **no** |
| NextBlock sender | 249+ | live, if an A/B shows value | sender code only, not in the hot binary |
| Landed buy, all-in | ~0.004 SOL median per landed buy (p75 0.014) | probe, live | |
| Failed attempt (modelled, docs/03 §6.1) | ~0.00073 SOL at the defaults (range 0.00036-0.001); at most `bid.failed_attempt_lamports_max` | probe, live | |

Totals: **shadow $0-425/mo** (+ an RPC plan if needed); **probe ≈ $425/mo + the probe's SOL**; live per
docs/07 §4 ≈ $2,850-3,925/mo + tips, only after the gate.

**Keys and credential names.** Everything secret lives in `/etc/rugsnipe/keys/` (root 0400), loaded by
systemd as credentials; API keys are one TOML table in `api-keys.toml` whose key names are the names the
config uses.

| Credential / key name | What | Paid? | Where it is referenced |
|---|---|---|---|
| `hot-keypair` (`hot-keypair.json`) | live wallet (fra1 only) | no | `[secrets] hot_keypair` |
| `probe-keypair` (`probe-keypair.json`) | probe wallet (fra1 only) | no | `[secrets] probe_keypair` |
| `armset-signing-key` (`armset-signing.key`) | ArmSet signing key (ohio1 only, never fra1) | no | pubkey in `[secrets] armset_pubkey` |
| `api-keys` (`api-keys.toml`) | the table below | | `[secrets] api_keys` |
| `GRPC_TOKEN` | SVS gRPC or Helius LaserStream token | $150 / $499-999 | `[feeds] grpc.token` (feed not built yet) |
| `NOZOMI_KEY` | Nozomi (Temporal) sender | per-tx tip, min 0.001 SOL | `[senders] nozomi.key` |
| `ASTRALANE_KEY` | Astralane sender | per-tx tip, 0.001 SOL (0.00001 at VIP3) | `[senders] astralane.key` |
| `NEXTBLOCK_KEY` | NextBlock sender | $249+/mo | `[senders] nextblock.key` |
| `TG_TOKEN` | Telegram alert bot | free | `[obs] alert.telegram` (alerts not built yet) |
| `HC_URL` | healthchecks.io dead-man URL | free tier | `[obs] alert.deadman` |
| (none) | Helius Sender `fra-sender` | per-tx tip, 0.001 SOL or 5,000 lamports `swqos_only` | `[senders] helius` (keyless, 1 req/s per IP) |
| (none) | Jito block engine Frankfurt | per-bundle tip, min 1,000 lamports | `[senders] jito` (1 req/s per IP per region) |
| (DZ ID pubkey) | DoubleZero Edge seat | $1,500/mo | applied for at doublezero.xyz with the box's DZ ID; no API key |
| (seat token, IP-bound) | Helius shred delivery | $800-1,000/mo | Helius dashboard, one seat per IP |

## 14. Where things are on fra1

| Path | What |
|---|---|
| `/etc/rugsnipe/shadow.toml` | the shadow config (step 5) |
| `/etc/rugsnipe/rugsnipe.toml` | the service config (probe/live; never overwritten by install.sh) |
| `/var/lib/rugsnipe/armsets/` | signed ArmSets |
| `/var/lib/rugsnipe/shadow/{state.db,rslog,summary.json}` | the shadow run's record |
| `/run/rugsnipe/KILL` | the kill file |
| `/opt/rugsnipe/bin/` | `rugsnipe-hot`, `rugsnipe-guard`, `rugsnipe-ctl`, `rugsnipe-replay`, `rugsnipe-sim`, `tune-hot.sh` |
| `/opt/rugsnipe/src`, `/opt/rugsnipe/venv` | the deployed checkout and its Python |
