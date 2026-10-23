# 08 Runbook: from an empty server to a go/no-go answer

Date 2026-10-14. This is the operator's checklist: what to do, in order, with the exact commands. Every
command below exists in this repository (or is a standard Ubuntu tool); where a step needs code that is
not built yet, the step says so and stops there.

Background: `docs/04-strategy.md` (what we trade and the go/no-go rule, §7), `docs/03-speed-infra.md`
(where and how to send, §4 and §6.1), `docs/06-architecture.md` (config, §6), `docs/07-deploy.md` and
`deploy/frankfurt-checklist.md` (install details).

**Placeholders.** In the command blocks, UPPERCASE words such as `FRA1_IP`, `SHA`, `ARMSET_PUBKEY` or
`YOUR-ENDPOINT` stand for a value you type in by hand; the text next to the block says where it comes from. Never
paste a line that still contains one. `sudo nano FILE` opens a file in the editor (Ctrl+O and Enter save, Ctrl+X
quits). Files under `/etc/rugsnipe/keys` and `/var/lib/rugsnipe` belong to root or to
the `rugsnipe` user: every command that reads them starts with `sudo` (or `sudo -u rugsnipe`).

## 0. What this build can and cannot do today

| Can | Cannot yet (blocks the step named) |
|---|---|
| Shadow mode (paper, no money): every multi-wallet bundled launch gets a would-be entry right behind the bundle, a measured premium, the LEAD exits replayed on the real tape, and failed attempts charged at their modelled cost | **Live** (refused in this build), and four probe refinements that are not needed for probe-A: the direct TPU sender, the sell nonce pool (partial sells use a recent blockhash), PumpSwap real sells (a real position whose curve completed is not sold by the hot box: sell it by hand), keep-alive pings that hold the senders' connections warm between buys. Shred feeds (DoubleZero Edge, Helius raw shreds) are not in the hot binary: do not buy them yet |
| **Probe** (task #12, both halves, 2026-10-16): probe key loading from systemd credentials, the real senders (Helius incl. the `swqos_only` leg, Jito Frankfurt + Amsterdam `bundleOnly`, Nozomi, Astralane, BlockRazor, NextBlock), the real fill lane (our own TradeEvent, `meta.fee` of our failed buys, rank and premium, drops after the nonce burn, real sells and balances), nonce seeding from chain, recovery adoption, `--probe-dry`; the **Yellowstone gRPC post-execution feed** (creates as wire bytes decoded on the hot thread, our own trades, held mints, the creation slot's bundle run), probe start gated on a healthy gRPC feed, a stale feed latching the kill; the **kill latch persisted** in `state.db` with eight triggers (§10); the **leader schedule** (bid variant per leader class of the create slot and the next, the class recorded on every send and fill); Telegram alerts (the only alert path: required for the probe) | |
| Read rule (a) of the go/no-go gate: EV per attempt with a 90% lower bound (`analysis/scripts/shadow_gate.py`) | **Rule (b)** (are we ahead of the first follower?). The shadow lane *assumes* rank 1; our real rank needs real sends or a shred feed. It is decided in the probe |
| Feeds: `logsSubscribe` over any websocket URL, PumpPortal, the collector relay, **Yellowstone gRPC** (Triton, Alchemy or any standard endpoint; paper runs it only when `[feeds] grpc.enabled = true`, which also gives the first-seen comparison of §9) | DoubleZero Edge or Helius shreds: not in the hot binary |
| Bid policy: tip after the buy (pay on success) where the leader orders by tip (Jito-Agave), priority fee clamped by the class cap `bid.failed_attempt_max_by_class` elsewhere (BAM, Harmonic, other; 0 = the global `failed_attempt_lamports_max`); sells take the same split, the bid paid once (docs/03 §6.1). Probe reads the leader schedule (`getLeaderSchedule` per epoch, `getClusterNodes` every 3 min) and bids for the class of the create slot's leader and the next slot's | Paper uses the stake-weighted mix unless started with `--leader-schedule` |
| The P(TP) entry filter (`[entry] filter_model`, `filter_mode = "log"` or `"skip"`, docs/04 §3a) scores every paper intent from the E1 lane's reconstruction and records the score and tertile in `state.db` `filter`. With the gRPC feed the creation slot's bundle run is assembled from the stream and its 17 bundle features land in `state.db` `feed_bundle` (probe: for every real attempt) | Scoring a probe buy *before* it fires (the bundle run streams in after the create; a real buy cannot wait for it); `filter_mode = "skip"` stays a paper-only rule |

**Probe A measures the unfiltered stream** (readiness audit M4, `docs/research/probe-design-v2.md` §2). This
build cannot apply the skip filter or the multi-bundle gate before it sends (both act on paper intents only), so
probe A buys 0.02 SOL on **every** class-eligible create. The gRPC feed runs with `bundle_run = true`, so every
attempt gets a `feed_bundle` row with its `bundle_wallets`, and the stage-B entry is read on the **in-class subset**
(`bundle_wallets >= 1`) only: unbundled launches face fewer rivals and fill easily, so the landing rate over all
attempts is inflated. Only 10-20% of attempts are in class, so the 200 in-class attempts the B read needs take
1,000-2,000 attempts (about 1.3-4.3 SOL of expected loss at 0.02 SOL). **Decided 2026-10-20 (probe-design-v2
§2 item 4, option ii):** `config/probe-a.example.toml` ships 12 attempts per UTC hour, 300 a day, a 1 SOL daily
loss cap (raised 2026-10-22; expected loss about 0.7-0.85 SOL a day in the no-edge model at the raised bid caps), a hard stop at 14 days and a 15-day ArmSet, so the in-class
subset reaches 200 in 7-14 days. A subset still short at the hard stop ends with `INCONCLUSIVE ... escalate to
the owner`, not with a stop. A filter that acts before the send is a before-stage-B item.

Practical consequence: rule (a) does not depend on where the box is (the lane rebuilds each launch from
confirmed history). You may run steps 5-8 on ohio1 or any Linux box first. The probe build exists now
(step 9): rent Frankfurt and the gRPC feed when step 8 says `GO on (a)`.

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
git clone YOUR-REPO-URL ~/rugsnipe && cd ~/rugsnipe                    # YOUR-REPO-URL: this repository's clone URL
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
ssh root@FRA1_IP                                               # FRA1_IP: the box's address
adduser --disabled-password ops && usermod -aG sudo ops       # then key-only SSH, PasswordAuthentication no
apt-get update && apt-get -y full-upgrade && apt-get -y install sqlite3 && reboot
# as ops
git clone YOUR-REPO-URL ~/rugsnipe && cd ~/rugsnipe
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
own config and ArmSet. install.sh also writes `rugsnipe-{hot,guard}.service.d/10-start-limit.conf`
(`StartLimitIntervalSec=3600`, `StartLimitBurst=5`): a probe start takes 5-20 s, so the units' own
30-in-120 s limit never trips; with the drop-in a crash loop leaves the unit failed after 5 starts in an
hour (`systemctl reset-failed rugsnipe-hot` after fixing the cause). Firewall (ufw): deny incoming by default, allow 22/tcp from your IP, nothing else
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

For the probe only (step 9), on fra1, never on a laptop. `rugsnipe-ctl armset keygen` writes a standard Solana
keypair file (the format `solana-keygen` writes), so no Agave CLI is needed; its pubkey goes into the public file
`/etc/rugsnipe/probe-pubkey.txt`, which later steps read as `$P`. Do not make a `hot-keypair.json`: the live wallet
is not used in this build (the units load the literal `ABSENT` without it).

```bash
sudo install -d -m 0700 -o root -g root /etc/rugsnipe/keys
if sudo test -e /etc/rugsnipe/keys/probe-keypair.json; then echo "probe key exists: kept (never replace it)"; else
  o=$(sudo /opt/rugsnipe/bin/rugsnipe-ctl armset keygen --out /etc/rugsnipe/keys/probe-keypair.json) && printf '%s\n' "$o" | sudo tee /etc/rugsnipe/probe-pubkey.txt; fi
sudo chmod 0400 /etc/rugsnipe/keys/probe-keypair.json
sudo nano /etc/rugsnipe/keys/api-keys.toml && sudo chmod 0400 /etc/rugsnipe/keys/api-keys.toml   # contents: §9 step 2
sudo bash ~/rugsnipe/deploy/install.sh --role hot              # refuses group/world-readable keys
```

What you should see: `wrote /etc/rugsnipe/keys/probe-keypair.json (0600)` and `pubkey ...` (the probe wallet's
address, now also in `/etc/rugsnipe/probe-pubkey.txt`); on a second paste `probe key exists: kept (never replace
it)` instead, and the pubkey file is left as it was. install.sh ends without a refusal. `keygen` never overwrites
an existing file: never delete the key of a funded wallet.

If `/etc/rugsnipe/probe-pubkey.txt` is ever empty or missing (`sudo cat /etc/rugsnipe/probe-pubkey.txt` prints no
`pubkey ...` line, and `$P` in §9 comes out empty), rebuild it from the key itself; this reads the key and sends
nothing:
```bash
o=$(sudo /opt/rugsnipe/bin/rugsnipe-ctl nonce create --keypair /etc/rugsnipe/keys/probe-keypair.json --count 1 --dry-run | awk '{for (i = 1; i < NF; i++) if ($i == "authority") {print "pubkey", $(i + 1); exit}}') && [ -n "$o" ] && printf '%s\n' "$o" | sudo tee /etc/rugsnipe/probe-pubkey.txt
```
What you should see: one line `pubkey ...` (the `authority` that `nonce create --dry-run` prints is the probe
wallet's address).

`api-keys.toml` is one `NAME = "value"` per line. A comment goes on its own line starting with `#`: a `# ...`
after a value on the same line fails the parser (`value must be quoted`). The names are the `key` values of
the `[senders]` tables: `HELIUS_KEY`, `NOZOMI_KEY`, `ASTRALANE_KEY`, `BLOCKRAZOR_KEY`, and optionally
`NEXTBLOCK_KEY`; plus `GRPC_TOKEN` (the gRPC feed's `x-token`, `[feeds] grpc.x_token`), `RPC_URL`
(the **paid** RPC endpoint with its key, e.g. `RPC_URL = "https://mainnet.helius-rpc.com/?api-key=..."`,
passed as `--rpc-url cred:RPC_URL`), optionally `LOGSWS_URL` / `GRPC_URL` for feed URLs that carry a key
(`[feeds] logsws.url = "cred:LOGSWS_URL"`), and `TG_TOKEN` and `TG_CHAT` for Telegram, the only alert path
(required for the probe, §10). The full probe example is in §9 step 2. **A URL with a key never goes on a command line, into a unit file or into the config**: `ps`,
the journal (`systemd-run` logs its command line) and `/etc/systemd/system/*.d/*.conf` are readable by
every user. Probe and live refuse a `--rpc-url` or feed URL with a query string (`api-key=...`) unless it
is `cred:NAME`, refuse a missing `--rpc-url` and the public `api.mainnet-beta.solana.com`; paper still
takes plain URLs. Every URL the binaries print is redacted to `scheme://host/…?…`; the start prints the
redacted RPC and feed hosts and the kill file it watches. The pre-commit hook refuses `api-key=<value>`,
`x-token`/`x_token` values and Telegram bot tokens. `rugsnipe-hot` reads the keys **only** from `$CREDENTIALS_DIRECTORY` (systemd
`LoadCredential=`, already in the units), refuses a missing, malformed or group/world-readable
credential, refuses a sender whose key name is not in the file, and never prints a key.

## 5. The shadow configuration

```bash
sudo install -m 0640 -o root -g rugsnipe /opt/rugsnipe/src/config/paper-lead.example.toml /etc/rugsnipe/shadow.toml
sudo install -d -m 0750 -o rugsnipe -g rugsnipe /var/lib/rugsnipe/shadow /var/lib/rugsnipe/armsets
sudo nano /etc/rugsnipe/shadow.toml
```

Change exactly these lines:

| Key | Set to | Why |
|---|---|---|
| `[secrets] armset_pubkey` | the pubkey printed in step 4 | the example carries a dev key you do not have |
| `[risk] kill_file` | `"/run/rugsnipe/KILL"` | the kill switch of §10 |
| `[obs] rslog_dir` | `"/var/lib/rugsnipe/shadow/rslog"` | the service user can write only under `/var/lib/rugsnipe` |
| `[feeds] logsws.url` | keep `wss://api.mainnet-beta.solana.com`, or your RPC provider's websocket URL | the free create feed; the gRPC feed is optional in shadow (§9 step 0: the first-seen comparison) |
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
sudo /opt/rugsnipe/bin/rugsnipe-ctl config validate /etc/rugsnipe/shadow.toml       # paper: must say OK, no refusals
```

## 6. The shadow ArmSet (a class arm: every create)

On ohio1:

```bash
cd /opt/rugsnipe/src
/opt/rugsnipe/venv/bin/python -I analysis/scripts/make_paper_armset.py --kind class --ver 1 --out /tmp/class.json
sudo /opt/rugsnipe/bin/rugsnipe-ctl armset sign --key /etc/rugsnipe/keys/armset-signing.key --in /tmp/class.json --out-dir /tmp/armset
scp /tmp/armset/armset-1-*.json /tmp/armset/armset-1-*.json.sig ops@FRA1_IP:/tmp/     # FRA1_IP: the box's address
```

On fra1:

```bash
sudo install -m 0640 -o rugsnipe -g rugsnipe /tmp/armset-1-* /var/lib/rugsnipe/armsets/
# both files: the .json and its .sig. Since 2026-10-17 the hot binary refuses to start when the
# --armset it is given does not verify (missing .sig, older than 6 h, wrong key) instead of
# running exits-only with no decisions; omit --armset if exits-only is what you want.
sudo ls /var/lib/rugsnipe/armsets/
sudo /opt/rugsnipe/bin/rugsnipe-ctl armset verify /var/lib/rugsnipe/armsets/armset-1-SHA.json --pubkey ARMSET_PUBKEY --regime paper
```

`SHA` is the 64-character hash in the file name that `ls` prints; `ARMSET_PUBKEY` is the pubkey of step 4.

A running paper process keeps using its ArmSet after the 6-hour freshness window (staleness only blocks
real modes), so one ArmSet covers a multi-day run. A process **started** with an ArmSet older than 6 hours
loads none and makes no decisions: before every new run, repeat this step with the next `--ver`.

## 7. Start shadow mode

```bash
sudo systemd-run --unit=rugsnipe-shadow --uid=rugsnipe --gid=rugsnipe \
  --property=WorkingDirectory=/var/lib/rugsnipe \
  /opt/rugsnipe/bin/rugsnipe-hot --config /etc/rugsnipe/shadow.toml \
    --armset /var/lib/rugsnipe/armsets/armset-1-SHA.json \
    --state /var/lib/rugsnipe/shadow/state.db --rslog-dir /var/lib/rugsnipe/shadow/rslog \
    --summary /var/lib/rugsnipe/shadow/summary.json --duration-s 93600
sudo journalctl -u rugsnipe-shadow -f     # one status line per minute
```

`--duration-s 93600` is 26 hours: the rule needs at least one full day. Stopping early is safe and still
writes the summary: `sudo systemctl stop rugsnipe-shadow`. A run that ends (duration reached, stop, or
crash) is not restarted; start a new one with a **new** `--state` file name per run, or the same file to
keep accumulating (the tables append).

**Read the status line.** `lane R/Q calls C` is the E1 lane: resolved / requested intents and RPC calls.
If `R` falls further and further behind `Q`, the public RPC is the bottleneck: add
`--rpc-url https://YOUR-PROVIDER` (a URL with a key: put it in `api-keys.toml` as `RPC_URL`, add
`-p LoadCredential=api-keys:/etc/rugsnipe/keys/api-keys.toml` and pass `--rpc-url cred:RPC_URL`) and raise both `[rpc_budget] hot` in `shadow.toml` and `--lane-rps`
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
sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py \
  --state /var/lib/rugsnipe/shadow/state.db --summary /var/lib/rugsnipe/shadow/summary.json
# the same with every failed attempt priced at docs/04's all-priority 0.003 SOL:
sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py \
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

## 9. Moving to probe

**When:** only if step 8 says `GO on (a)`. The build has everything probe-A needs (§0, task #12 both
halves): the real senders, the real fill lane, the gRPC post-execution feed, the kill triggers and the
leader schedule.

**Schedule and sizing: `docs/research/probe-design-v2.md` §2 with its readiness-audit M4 block** (2026-10-20,
`docs/research/readiness-2026-10-20.md`; item 4 of it is the owner's open choice):
- probe A runs **unfiltered on every class-eligible create**. The build cannot apply the skip filter or a
  multi-bundle gate before it sends (§0), so the design's skip mode, outcome-store pre-warm and index probes
  have no effect on real buys and are not part of stage A;
- `[feeds] grpc.bundle_run` stays on, so every attempt gets a `feed_bundle` row with `bundle_wallets`;
- the stage-B entry is read on the **in-class subset** (`bundle_wallets >= 1`) only, never on all attempts;
- the caps as shipped (decided 2026-10-20 as 6/150/0.6, raised 2026-10-22): **300 attempts a day (288 reachable at 12 per
  UTC hour), 1 SOL daily loss** (expected loss about 0.7-0.85 SOL a day in the no-edge model at the 2026-10-22 bid caps:
  a bad day can latch `daily_loss` before its last attempts, which is the cap doing its job). The in-class subset (10-20% of attempts)
  reaches 200 in 7-14 days. The three values move together (`risk.max_buys_per_hour`, `[probe] a.max_probes_per_day`,
  `a.max_daily_loss_lamports`): a higher rate under a small cap latches `daily_loss` on ordinary days (a false
  kill); fund 3 SOL and top up per step 7b;
- stage A ends at **≥ 350 attempts and ≥ 7 days**, with ≥ 200 in-class attempts for the B read; **hard stop
  at 14 days** (`--hard-stop-days 14` in step 7). A subset still short of 200 at the
  hard stop reads `INCONCLUSIVE`: escalate to the owner, do not stop the project;
- a filter that acts before the send (the pre-send gate) is a before-stage-B item: without it stage B faces
  the unfiltered break-even of about 36% at 0.25 SOL.

Do not start stage A before Oct 23 (§11: no probe money across the Alpenglow cutover). The kill rule of §10 is
unchanged.

**What** (docs/04 §7 step 2, `[probe]` in the config): two stages, both with `max_sol_cost` +2% and the
LEAD exits. `[probe] stage` selects the caps; each narrows the `[risk]` cap of the same name, and a
signed ArmSet that names `probe_stage` must name the same stage, and stage B needs one that names `"b"`
(moving to B takes both keys).

| Stage | Per buy | Attempts/day | Open positions | Daily loss cap | Runs until | Purpose |
|---|---|---|---|---|---|---|
| **A** (`stage = "a"`) | 0.02 SOL | 300 (12 per UTC hour: 288) | 1 | 1 SOL | ≥ 350 attempts **and** ≥ 7 days (with ≥ 200 in-class attempts); hard stop at 14 days | measure landing, premium, failed-attempt cost per leader class and per sender, which variant lands, on all attempts and on the in-class subset. EV is negative at this size (fixed costs): **not** a kill reason |
| **B** (`stage = "b"`) | 0.25 SOL | 300 | 2 | 1 SOL | about 200 landed trades | the EV per attempt at our real rank and cost; gate (b) |

**Landed** means a fill in the creation slot, or one slot after it (`shadow_gate.py --late-ok 1`, the default
since 2026-10-22: stage A measured one-slot-late fills doing no worse than same-slot fills at 200 ms slots), at a
premium of +2% or less. Fills later than slot+1 (slot+2 and on, until the nonce burn lands: `tx.burn_after_slots = 3`
sends it once a feed event from slot+3 has arrived), fills after the nonce burn started (`intents.state =
'late_fill'`) and fills above +2% are reported apart and do not count as landed; `--late-ok 0` is the strict
creation-slot rule, printed next to the inclusive rate.

**Kill** (either stage; `shadow_gate.py --probe` prints the verdict): the EV per **attempt**, failed
attempts included, has a 90% upper bound below 0 after 100 landed trades or 400 attempts, whichever
comes first (probe-B only); or the median realised premium exceeds +3%; or the landing rate stays below
the break-even for the measured failed-attempt cost over 200 attempts (all attempts, and the in-class subset
once it has 200); or the stage's daily loss cap is hit (the risk gate latches the kill itself). Failures that
are not lost races (`fills.fail_reason` such as `instruction_error`, e.g. a compute-budget overrun) are left out
of the landing rate; when they are more than 10% of the attempts the verdict is `CHECK: ...` instead of a landing
kill. **Enter B** only if probe-A ends with an **in-class** landing rate above the stage-B bar and an in-class
median premium at or below +2% (`PROBE-A COMPLETE: ENTER PROBE-B`). The bar is the larger of the pooled LEAD
break-even (about 9.9% at the measured failed cost and premium) and the unfiltered shadow's break-even at
0.25 SOL (about 36%); the gate prints both inputs.

**How:**

0. **The gRPC feed** (docs/research/infra-shopping-2026.md §1; both are standard Yellowstone
   "Dragon's Mouth", no code difference):
   - **Triton One pay-as-you-go** ($125 prepaid for 12 months, $0.08/GB): create a Dragon's Mouth
     endpoint in the portal; the URL is `https://YOUR-ENDPOINT.rpcpool.com` (YOUR-ENDPOINT is the name the
     portal gives it; the shared Frankfurt front `api.rpcpool.com` resolves to TeraSwitch FRA, very probably
     our building), the token goes only in the `x-token` header.
   - **Alchemy gRPC pay-as-you-go** ($75/TB, not on the Free plan): `https://solana-mainnet.streaming.alchemy.com`
     (TeraSwitch FRA); the token is the app's API key [U: check the dashboard's gRPC page for the
     header it expects; the binary sends `x-token`].
   - The token is a line of `api-keys.toml` (§4): `GRPC_TOKEN = "..."` (or a credential file named
     `GRPC_TOKEN`); the config names it, never holds it. `config/probe-a.example.toml` already has the line:
     ```toml
     [feeds]
     grpc = { enabled = true, url = "https://YOUR-ENDPOINT.rpcpool.com", x_token = "GRPC_TOKEN", stale_ms = 4_000, bundle_run = true }
     ```
     `bundle_run = true` also streams every pump transaction to assemble the creation slot's bundle run
     (about 100 pump transactions/s at a few KB each: an estimate of ~2 GB/hour, i.e. about $4/day on
     Triton). Keep it on for the probe: it writes the `feed_bundle` row that splits the in-class subset.
     `stale_ms = 4_000`: one gap longer than `stale_ms` latches the kill, and 2,000 is too tight.
   - The subscription is one stream at `processed`: creates (`TSLvdd…` mentioned, successful only),
     our wallet (failed transactions too), held mints (watched and unwatched as positions open and
     close), pump (with `bundle_run`), slot updates. Compression is off (latency); the server's pings
     are answered; no data message for `stale_ms` forces a reconnect (jittered backoff, the filters
     re-sent with the current watches).
   - **First-seen comparison** (do this in shadow first, any box): add the `grpc` line above to
     `shadow.toml` with `enabled = true` and keep `logsws` on; start shadow with the credential
     (`-p LoadCredential=api-keys:/etc/rugsnipe/keys/api-keys.toml` on the `systemd-run` line of §7; paper
     loads no wallet, only the token). The status line prints `first-seen logsws_behind_grpc p50 … ms
     (n …)`; at the end:
     ```bash
     sudo python3 -c 'import json,sys; s=json.load(open(sys.argv[1]))["cold"]; print(json.dumps(s["first_seen"], indent=1)); print("grpc", json.dumps(s["grpc"]))' \
       /var/lib/rugsnipe/shadow/summary.json
     ```
     `seen_by` / `first_by` count the creates each feed delivered and won, `lag` holds the late feed's lag
     behind the first (p50/p90/p99/max, ms; one monotonic clock in one process, so no skew). Expect
     the logs path about a second behind; a gRPC feed that is not ahead on nearly every create is the
     wrong endpoint or region. Every create also gets an rslog `FeedSeen` record per source.

1. **The probe ArmSet: one per stage** (on ohio1; readiness audit M1). Sign stage A **once**, valid for 15 days
   (`--valid-hours 360`). The probe config's `[risk] armset_max_age_s = 1_296_000` (15 days) accepts it for the
   whole stage (14 days plus a day of slack). With the script's default 6 hours, a running probe silently drops to
   paper after 6 hours (`armset: v101 is stale` in the journal) and every restart is refused. Sign it at most a day
   before the start: the 15 days run from signing. (For a longer stage: sign with `--valid-hours` = 24 × (days + 1)
   and set `armset_max_age_s` to 3,600 × that.)

   The probe uses its own version numbers: **101** for stage A (102, 103, ... if you ever re-sign it) and **201**
   for stage B. The shadow ArmSets of §6 count 1, 2, 3, ..., so the two never share a file name. The signing goes
   into its own directory, emptied first, so it holds exactly this signing's two files:
   ```bash
   cd /opt/rugsnipe/src
   sudo rm -rf /tmp/armset-probe-a
   /opt/rugsnipe/venv/bin/python -I analysis/scripts/make_paper_armset.py --kind class --ver 101 --probe-allowed --probe-stage a --valid-hours 360 --out /tmp/probe-a.json
   sudo /opt/rugsnipe/bin/rugsnipe-ctl armset sign --key /etc/rugsnipe/keys/armset-signing.key --in /tmp/probe-a.json --out-dir /tmp/armset-probe-a
   ls /tmp/armset-probe-a/
   ```
   What you should see: `/tmp/probe-a.json: 1 operator(s), ... valid 360 h (until ...)` with a date 15 days
   ahead, then `ver 101 sha256 ... probe_allowed=true`, and `ls` lists exactly two files,
   `armset-101-SHA.json` and `armset-101-SHA.json.sig` (`SHA` = 64 hex characters, different for every
   signing). The ArmSet carries `probe_allowed = true`, `probe_stage = "a"`, the class operator in state `probe`
   at 0.02 SOL, never `gate_passed`. For B: `--probe-stage b`, `--ver 201`, `--out /tmp/probe-b.json` and
   `--out-dir /tmp/armset-probe-b`, and again one ArmSet for the whole stage.

   **1b. Copy it to fra1.** On ohio1 (FRA1_IP: the box's address); the first line clears fra1's copy of an
   earlier signing:
   ```bash
   ssh ops@FRA1_IP 'rm -rf /tmp/armset-probe-a'
   scp -r /tmp/armset-probe-a ops@FRA1_IP:/tmp/
   ```
   Then on fra1:
   ```bash
   ls /tmp/armset-probe-a/
   sudo install -m 0640 -o rugsnipe -g rugsnipe /tmp/armset-probe-a/armset-101-* /var/lib/rugsnipe/armsets/
   sudo ls /var/lib/rugsnipe/armsets/
   ```
   What you should see: the first `ls` lists the same two files as on ohio1, and the armsets directory now holds
   them too (it may also hold older files: ignore them). Write the SHA down from the first `ls`: the steps below
   name the file as `/var/lib/rugsnipe/armsets/armset-101-SHA.json`, and you type the real SHA in its place.

2. **Keys** (§4): `probe-keypair.json` and `api-keys.toml` in `/etc/rugsnipe/keys/`, root 0400. Stages A and
   B both trade from the probe wallet. `api-keys.toml` (values from the providers' dashboards,
   docs/research/infra-shopping-2026.md §5); every comment on its own line, every value in double quotes:
   ```toml
   # Helius Sender key: 50 req/s (keyless is 1 req/s per IP)
   HELIUS_KEY = "..."
   NOZOMI_KEY = "..."
   ASTRALANE_KEY = "..."
   # the BlockRazor auth token (header apikey)
   BLOCKRAZOR_KEY = "..."
   # the gRPC feed's x-token (step 0)
   GRPC_TOKEN = "..."
   # the PAID RPC URL with its key (Solana Vibe Station Basic or Helius Developer; never Helius Free)
   RPC_URL = "https://..."
   # Telegram, the only alert path (required for the probe, see section 10)
   TG_TOKEN = "..."
   TG_CHAT = "..."
   # only with a NextBlock plan:
   # NEXTBLOCK_KEY = "..."
   ```
   **Use a paid RPC for `RPC_URL`** (Solana Vibe Station Basic or Helius Developer, docs/research/infra-shopping-2026.md
   §3): the real fill lane alone makes close to 10 requests a second (statuses, landed transactions, nonce
   accounts, balances, blockhashes), more than a free plan allows. **Telegram is the only alert path**: no
   dead-man is built, so without `TG_TOKEN` and `TG_CHAT` nothing tells you that the box latched its kill or
   died. Make the bot with @BotFather, send it one message, and put your chat id in `TG_CHAT`.
   What you should see: nothing yet; the dry run of step 5 is the check (`credential ... value must be quoted`
   means a comment or a quote is wrong on that line).

3. **The probe config: `config/probe-a.example.toml`** (readiness audit M3). It is `config/rugsnipe.example.toml`
   with the probe-A values set, each marked with a `probe-A:` comment that says why. Do not start from
   `rugsnipe.example.toml`: its exits (2x / 3x, a 30-minute hold), its 40 attempts an hour and its 1 SOL
   balance floor would measure a different policy. This replaces the copy install.sh made:
   ```bash
   sudo install -m 0640 -o root -g rugsnipe /opt/rugsnipe/src/config/probe-a.example.toml /etc/rugsnipe/rugsnipe.toml
   sudo nano /etc/rugsnipe/rugsnipe.toml
   sudo /opt/rugsnipe/bin/rugsnipe-ctl config validate /etc/rugsnipe/rugsnipe.toml
   ```
   In the editor change exactly three values: `[secrets] armset_pubkey` (the pubkey printed on ohio1 in §4),
   `[secrets] cold_wallet` (the address of a wallet you control, not on this box) and, in the `[feeds] grpc`
   line, `YOUR-ENDPOINT` (step 0). What you should see: `mode = probe`, then `REFUSED:` lines only for what
   this quick check cannot see (credentials, clock offset, post-execution feed, ArmSet: step 4 checks them),
   and **no** `secrets.armset_pubkey is not a base58 pubkey` or `secrets.cold_wallet is not a base58 pubkey`
   (those mean a placeholder is still in the file). Any other refusal: stop and escalate.

   What the file sets (the `[risk]` values are ceilings for both stages; `[probe] a` narrows the four stage caps):

   | Key | Value | Note |
   |---|---|---|
   | `[probe] a` | 0.02 SOL per buy, 300 attempts/day, 1 open position, 1 SOL daily loss | the stage-A caps raised 2026-10-22 (decided 2026-10-20 as 6/150/0.6, probe-design-v2 §2 item 4, option ii); the three lines move together (`max_buys_per_hour`, `a.max_probes_per_day`, `a.max_daily_loss_lamports`). B keeps 0.25 SOL, 300, 2, 1 SOL |
   | `risk.max_buys_per_hour` | `12` | one buy per 300 s (`spread_buys_per_hour`): 12 × 24 = 288 ≤ 300 a day, so every hour of the day is sampled (40 would use up the day by early morning) |
   | `risk.max_entry_vsol_lamports` | `115_000_000_000` | the LEAD universe the shadow scored (40 SOL would skip large launches) |
   | `[exit.default]` | the LEAD exits of `paper-lead.example.toml` | +15% take-profit, sell when a bundle wallet sells, 1 s without a trade, −15% stop after 1 s, hard exit at slot+5 |
   | `risk.min_hot_balance_lamports` | `100_000_000` | a buy needs this + the stake + the bid in the wallet (the lane reads the balance every 2 s): keep it below what you fund |
   | `risk.max_consec_losses_per_op` | `1_000_000` | the class arm is one operator and the median LEAD trade is a small loss, so a consecutive-loss latch on it is not a stop, it is a silent outage: 20 latched it until the next UTC day on 2026-10-22 (symptom: `positions 0`, `killed no`, `intents` frozen for hours, `op_risk.disarmed_until` = tomorrow's day number; restarts do not clear it). The daily loss cap is the stop. If it ever latches: set the cap, then with hot stopped `update op_risk set consec_losses=0, disarmed_until=0`, then start hot |
   | `risk.armset_max_age_s` | `1_296_000` | 15 days: one ArmSet per stage (step 1) |
   | `risk.kill_file` | `"/run/rugsnipe/KILL"` | the kill switch of §10 |
   | `tx.cu_limit`, `tx.lads_bytes` | `88_000`, `12_124_160` | **not** 65,000: the fresh probe wallet creates its pump volume accumulator on its first buy (+1.3 to +2.8k CU), so at 65,000 every landed buy can fail and the failure undoes the creation each time. Move to 65,000 for B only after `rugsnipe-ctl sim --wallet PROBE_PUBKEY --legacy` passes |
   | `tx.nonce_pool` | `32` | the accounts of step 3c |
   | `entry.require_bundle` | `true` | (2026-10-22) a real buy is sent only once another wallet's buy of the mint shows in the create slot, right behind the bundle: the launches the LEAD policy was scored on (docs/04 §2). An unbundled launch is skipped as `skip_no_bundle` at no cost (no risk, no nonce, no hourly token). Stage B's first 10 attempts were all unbundled (`feed_bundle.bundle_wallets = 0`) and all exited `no_activity` at −2.5%. Needs `[feeds] grpc.bundle_run = true`. Watch `rugsnipe_decisions{outcome="skip_no_bundle"}` next to `fire`  (2026-10-22, evening) the parked create now waits for the **whole** bundle run and bounds the buy at its last trade (docs/06 §7.1 entry bound): before that the bound assumed a fixed 4 SOL bundle and every bigger bundle failed on chain as `slippage` (43 of 61 failures; the landed fills averaged a 0.79 SOL bundle). Watch `fills.fail_reason = 'slippage'` against `feed_bundle.bundle_sol`: a slippage failure on a bundle the run saw means a third-party buyer ahead of us, not the bound. |
   | `entry.dev_buy_sol` / `entry.dev_buy_tol_sol` | `[]` / `0.005` | (2026-10-23) operator template filter: a create fires only when its own dev buy is one of the listed SOL amounts (+- the tolerance); `[]` admits every create. Chosen from the dry run on launches with no sniper in the create slot (the population this box lands): dev buys of 5.00, 2.96, 6.91 and 3.85 SOL averaged +3.7% under LEAD, positive in both halves of the data, the rest -0.95%. Skips count as `skip_dev_buy` (0x50). Re-derive weekly from `outcomes`: operators change templates. |
   | `feeds.deshred` | off | (2026-10-23) the pre-execution feed: Yellowstone `SubscribeDeshred` (transactions decoded from shreds before the block is replayed; Triton Dragon's Mouth, paid beta). The same adapter in deshred mode, source `shreds`: every pump transaction, no execution meta. A create reaches the hot loop as a wire frame with an arrival-order index 50-80 ms before the post-execution copy (which the mux then drops as a duplicate); the create slot's buys are quoted on our own curve from the instruction arguments (`pump_buys`, `CurveState::quote_buy_*`) and reach the bundle gate as the trades the chain is about to execute, so the park fires on the pre-execution run. `feeds.grpc` stays on: fills, exits, our own trades and the kill's post-execution health come from it. Credential: `DESHRED_TOKEN = "..."` in `api-keys.toml`. Status line: `deshred: slot .. tx .. creates->wire .. trades .. run_done ..`. Caveats: a launcher CPI create is invisible pre-execution (the grpc copy still fires it, later); a pre-execution buy that fails on chain is still quoted (the bound is then looser than reality, never tighter). |
   | `feeds.entries` | off | (2026-10-23) the second pre-execution feed: a shredstream proxy on this box (`deploy/systemd/shredstream-proxy.service`: the Jito `jito-shredstream-proxy` protocol against a gateway such as AllenHark's Frankfurt feed, auth by the keypair `/etc/rugsnipe/keys/shred-auth.json`, settings in `/etc/rugsnipe/shredstream.env`) deshreds and serves `ShredstreamProxy/SubscribeEntries` on `--grpc-service-port`; the adapter walks every transaction of each entry, keeps the pump ones and handles them exactly like the deshred stream (source `shredstream`, arrival-order index per slot, pre-execution quotes, bundle runs). No loaded addresses: a create whose accounts hide behind a lookup table is left to the other feeds. Read `cold.first_seen` (`lag."shreds_behind_shredstream"` / `"shredstream_behind_shreds"`) to see which shred source is ahead and by how much; the hot loop fires on whichever copy arrives first. Status line: `shredstream: slot .. entries .. pump_tx .. creates->wire .. trades .. run_done ..`. |
   | `entry.dumper_skip` | `log` | (2026-10-23) the operator-dump skip: the cold thread tracks every bundled launch the feed streams (the creation slot's bundle run, then the trades of its first 3 slots: the gRPC feed now carries a recent create's trades for 4 slots) and marks the create's signer and the bundle wallets when they sold at least half of their tokens by create + 3; the set persists in `state.db` (`dumpers`) and the hot loop checks the dev, the creator and the parked bundle wallets at the create. `log` journals `dumper <mint> logged (would skip)` and fires, `skip` drops the create as `skip_dumper`. Dry run: +0.6 points per fill out of sample for skipping such operators (post-hoc, 52 launches): run `log` until the live hit rate and outcomes are read (`journalctl -o cat \| grep ^dumper`). |
   | `entry.presend_model` / `entry.presend_mode` | `""` / `off` | (2026-10-23) the pre-send entry gate. The entry filter (`filter_model`) scores the E1 verdict after the create slot ran and never stops a real buy; this gate scores the create at the fire point (the park firing on its bundle run, or the decision itself without `require_bundle`), before the nonce claim and the send, on what is known then: the 17 bundle features of the creation slot's trades of the mint seen so far (the create's own dev buy plus the run's pre-execution quotes), the 21 create-transaction features from the wire bytes (`filter::create_tx_features_wire`: compute-budget priority instead of `meta.fee`, tips by CPI only with the gRPC meta), the signer's 6 operator features from the outcome store (republished by the cold thread within a second of a change), funding always unknown (all 10 zero: the model is retrained with funding masked to exactly that), the time. `log` records every decision that reaches the send in `state.db` `presend` (score, tertile, the 60 features) and rslog `Presend`, never skips; `skip` also drops a create scoring below `presend_cut` as `skip_presend` (0x53: nothing sent, risk reservation released, no nonce). A create without its own trade (no dev buy) is unscored (NULL score) and never skipped. The model's MEV tip accounts must equal the decoder's (refused at start otherwise). Status line: `presend <mode> <cut> scored N skipped N low/mid/high a/b/c`. |
   | `entry.presend_cut` | `t2` | the cut `skip` applies: the model's `t1` (drop the bottom tertile) or `t2` (keep only the top tertile). Run `log` first and read `select tertile, count(*) from presend group by 1` against `outcomes` before moving to `skip`; compare a paper run's `presend.score` with `filter.score` of the same `intent_id` to see what the missing post-create information costs. |
   | `entry.leader_regions` / `entry.leader_regions_file` | `[]` / `""` | (2026-10-23) leader filter: a create fires only when its slot's leader is in one of the listed regions (`eu`, `na`, `asia`, `other`; a leader missing from the table is allowed). From Frankfurt the buy landed in the create slot 62% of the time with a European leader, 33% with a North American one, 9% with an Asian one (73 attempts), and a buy in the next slot is where the LEAD edge is already gone, so the far leaders only cost failed attempts. The table is `data/leader_regions.json` (Stakewiz validator locations), installed to `/var/lib/rugsnipe/data/`; regenerate it with `analysis/scripts/leader_regions.py` after validator moves. Skips count as `skip_far_leader` (0x51). |
   | `entry.bundle_wait_ms` | `2` | (2026-10-22) the wait after the first bundle buy for the rest of the run before the parked create fires anyway; the stream's run-complete signal or a trade at create + 5 fires it earlier. The hot loop's counters report `bundle_fired`, `run_done`, `not_first` (a buyer beyond the run executed before we fired: `skip_not_first`, nothing sent) and `bundle_wait_us` (mean wait = `bundle_wait_us / bundle_fired`). Raise it only if `bundle_wait_us / bundle_fired` sits at the cap and slippage failures still hit bundles the run saw only partly (`feed_bundle.trades` beyond what `intents.min_tokens_out` implies). |
   | `entry.operators_file` / `entry.operators_only` | `""` / `false` | (2026-10-23, operator wave, [docs/09](09-operator-wave.md)) the operator table: a JSON object keyed by the create transaction's signer (its fee payer, base58), each value `{hold_slots, n, wins, pnl_sol, dev_sol, bundle_wallets}`, loaded once at start (a missing or broken file refuses the start; the journal prints `operators: N signers from <file>`). A listed signer's position exits at create + `hold_slots` (journal `operator <mint> hold_to create+H`). `operators_only = true` skips every other create as `skip_not_operator` (0x54) before the park: nothing parked, sent or reserved. The shipped table is `data/operators/ahw-2026-10-23.json` (153 signers; the 33 with 3+ round trips carry 289 of 430 trades), installed to `/var/lib/rugsnipe/data/operators/` by `deploy/install.sh`; rebuild it with `analysis/scripts/strategy/operators_from_rt.py rt.json --out ...` (`--min-n 3` keeps the repeat operators only). |
   | `entry.bundle_wallets` | `[]` | (2026-10-23, operator wave) the bundle shape at the fire point: the parked run's distinct bundle buyers outside the creator set (signer, `creator`, fee payer, the create transaction's buyers) must be one of the listed counts (`[3]`: 69% of the copied trades; `[2, 3, 4]`), else `skip_bundle_shape` (0x55) at no cost. Needs `require_bundle = true` (refused otherwise). The park counts at most 5 wallets and only what the stream delivered before the fire, so it can read lower than the confirmed block (rt.json `n_bw`). |
   | `entry.max_post_bundle_sol` | `0` (off) | (2026-10-23, the pick template, [docs/09](09-operator-wave.md) "Picks") the template's price / mcap bound and the main pick filter: a create whose post-bundle virtual SOL reserve, the one the buy would be bound at (the parked run's anchor: its last creator-set or bundle trade; without a run, as in paper or with `require_bundle` off, V0, the curve after the dev buy), is above the cap is skipped as `skip_bundle_too_big` (0x57) at the fire point, before risk or a nonce, journal `template <mint> skip_bundle_too_big post-bundle <v> > <cap> SOL`. Must be 0 or in (30, 1000): the fresh curve holds 30 virtual SOL. The copied wallet bought at about 37-47 (post-bundle mcap 36-65 SOL, mcap = V^2 / 32.19); tonight's -58% launch was bought at about 58. Suggested `47.0`. |
   | `entry.max_bundle_sol` | `0` (off) | (2026-10-23, pick template) the create's own buys + the parked run's creator-set and bundle buys (SOL into the curve; without a run the dev buy alone) above the cap: `skip_bundle_sol` (0x58) at the fire point, nothing sent. Template about 7 SOL, p90 17 (tonight's loss: 4 + 13). In [0, 1000). |
   | `entry.dev_buy_round` / `dev_buy_round_min` / `dev_buy_round_max` | `false` / `1.0` / `6.0` | (2026-10-23, pick template) an alternative to `dev_buy_sol`: the dev buy may be a whole number of SOL within `dev_buy_tol_sol` in `[min, max]`; beside a `dev_buy_sol` list either passes; otherwise `skip_dev_buy` (0x50) before the park. Set `dev_buy_sol = []` for the round rule alone. Refused unless 0 <= min <= max < 1000 with a whole number between them. |
   | `entry.uri_hosts` / `entry.uri_hosts_deny` | `[]` / `[]` | (2026-10-23, pick template) the create's uri host as the wire decoder keeps it (`https?://<host>/`, lower case, at most 64 bytes; the logs path fills it from the CreateEvent) must end in an allowed entry (a label-boundary suffix, any case: `uxento.io` matches `meta.uxento.io`, not `xuxento.io`; a uri without such a host fails a non-empty list) and in no denied one, else `skip_uri_host` (0x59) before the park. Template: `["uxento.io", "j7tracker.io", "gg.sc"]` favoured, `ipfs.io` disfavoured. Entries are host names only (no scheme, path or port). Status line for all four rows: ` \| template too_big N bundle_sol N dev_buy N uri_host N`. |
   | the post-bundle anchor (`require_bundle`) | always on | (2026-10-23, night, Sj3qBo) the parked run's anchor, the state `min_tokens_out` is quoted at, moves only on the creator set's trades and the bundle's (buys chained behind the create at most 2 indices apart within create + 5, rt.json's gap-3 run; sells by those wallets). Any other buy inside the run means someone bought first: `skip_not_first` (0x4f), nothing sent; before the fix it re-anchored the bound at the outsider's price (+11.4% over the post-bundle spot at zero premium). An outside buy chained to the bundle (gap <= 2) still counts as a bundle wallet. The gate fragment's ` wallets@fire 0-5 a/b/c/d/e/f` is the park's bundle-wallet count at each fire: compare it with `feed_bundle.bundle_wallets` before trusting `bundle_wallets = [3]` (the 300 us wait runs from the first bundle buy; a bundle transaction later than that is missed). |
   | `exit.default.hold_to_create_slots` | `0` (off) | (2026-10-23, operator wave) sell 100% at create slot + N, at dump bounds (sells at any price, like the copied wallet's `min_sol_output = 0`; no dump signal, no nonce burn), sent at the first slot the feed shows at or past it (fills about a slot later). Counted from the **create**, not the landing (`max_hold_slots` counts from the landing). The value is the hold of a signer not in `entry.operators_file` (6 or 10 at even odds); a listed signer's `hold_slots` replaces it per position. The other exit rules stay and the first trigger wins; the wave runs with them off: `tp = []`, `insider_sell = "off"`, `no_activity_ms = 0`, `phantom_sl = { after_ms = 0, drop = 0.0 }`, `max_hold_slots = 0`, `max_hold_ms = 0` (all accepted by validation). Exits count as `rugsnipe_exits{reason="hold_to"}`, `exits.reason = 'hold_to'`. |
   | `entry.max_premium` = `0.0` | `0.02` (shipped) | (2026-10-23, operator wave) valid: with the parked run observed, `min_tokens_out` is the exact quote at the run's last trade (the post-bundle state before our own buy; our own impact is in the quote, not premium), so any buy ahead of ours fails the buy on chain at base + priority fee (the tip reverts): the copied wallet's on-chain state guard. Without an observed run the bound falls back to the quote at `V0 + bundle_lamports`. A paper run needs `paper.entry_premium = 0` with it, or every E1 attempt reads `premium`. Watch `fills.fail_reason`: at zero premium every buy that did not land first is a cheap failure, by design. |
   | `feeds.grpc_b` | off | (2026-10-22) a second Yellowstone endpoint beside `feeds.grpc`, for one purpose: measuring a provider before paying for it. Same adapter, source `grpc_b`, no bundle run, never the kill's post-execution feed; the hot loop fires on whichever copy of a create arrives first (the other is a `Dup`). Read the race in the hourly report JSON: `"first_seen": {"first_by": {"grpc": n, "grpc_b": m}, "lag": {"grpc_behind_grpc_b": {p50_ms, p90_ms, p99_ms}, ...}}`: `sudo journalctl -u rugsnipe-hot --since -2h --no-pager | grep -A 12 '"first_seen"' | tail -14`. A day pass is enough: a feed that is first on most creates by 10 ms or more at p50 is worth its month; one within 2 ms of `grpc` is not. Add the trial key as a line `GRPC_B_TOKEN=...` of the api-keys credential, or leave `x_token = ""` for an IP-allow-listed endpoint |
   | `exit.default.sell_tip_lamports`, `dump_tip_lamports` | `3_400_000`, `3_400_000` | (2026-10-22, night) the sell bids the same as the buy. At the shipped 50,000 / 200,000 a sell is the lowest-priority transaction in the block: 20 of 43 stage-B positions closed 3 to 8 slots after the policy's slot+5 (`positions.closed_slot - opened_slot` of 9 to 13), and `HaFiVKB6` sold one slot after a 9.7 SOL dump, -44% instead of the -3% the dry run's engine gets on the same launch. Watch `closed_slot - opened_slot`: the policy's exits are at most 6 to 7 on a launch with no trades; the sell's fee shows in `exits`. The bid is split by leader class like the buy's (priority fee on BAM/Harmonic/other, tip on Jito-Agave) and `risk.max_sell_spend_lamports` caps the retries |
   | `tx.burn_after_slots` | `3` (as shipped) | kept on 2026-10-22 although the readiness audit (`docs/research/readiness-2026-10-20.md` §5) lists it among the constants sized for 400 ms slots. A nonce's `since_slot` is the hot thread's feed slot at claim (the create slot s); the burn is due once a post-execution event from s+3 has arrived (`due_for_burn`, keeper every 100 ms) and the AdvanceNonce then lands a slot or so later, so slot+1 and slot+2 fills are never pre-empted: the 12 of 22 contested stage-A launches that landed by slot+1 were measured at 3. Going to 6 would only let variants still queued at s+3..s+5 land; `shadow_gate.py --late-ok 1` counts those as late, not landed, and each that lands and fails slippage costs the class priority fee of the `bid.failed_attempt_max_by_class` row (3,405,000 BAM/other, 1,995,000 Harmonic) where a burnt nonce resolves `dropped` at 0. Raise it only with `fills.slot - shadow.create_slot` per leader class showing s+3..s+5 fills worth that cost |
   | `[feeds]` | gRPC on (`stale_ms = 4_000`, `bundle_run = true`), logsws off | a real mode fires only from the gRPC feed |
   | `[senders]` | `helius`, `nozomi`, `astralane`, `blockrazor`, `jito` on; `rpcsend`, `helius_swqos` off | the five senders of the dry run (step 5) |
   | `bid.failed_attempt_max_by_class`, `rpc_budget.keeper` | `{ jito_agave = 0, bam = 4_000_000, harmonic = 2_000_000, other = 4_000_000 }` (the global `failed_attempt_lamports_max = 1_000_000` and the rest of `[bid]` as shipped), `keeper = 8` | Jito-Agave pays on success (priority 10,000 + tip 3,400,000; a failure costs 15,000). BAM and other carry the whole 3.4M bid as the priority fee (a failure costs 3,405,000), Harmonic 1,995,000 (clamped by its 2M cap). Before 2026-10-22 the 1M global cap clamped every priority-ordered class to 995,000, 3.4x under the median winning bid on 73% of leaders. Expected failed-attempt cost 0.00073 -> 0.00213 SOL, about +0.126 SOL/day at 90 failures a day; sizing, `max_bid_lamports` and the ArmSet are untouched |
   | `[obs] alert` | `TG_TOKEN`, `TG_CHAT` | Telegram, the only alert path (step 2) |

   The senders: `helius` (`key = "HELIUS_KEY"`, 0.001 SOL minimum), `nozomi` (`NOZOMI_KEY`, never below
   0.001 SOL: Nozomi silently drops lower tips), `astralane` (`ASTRALANE_KEY`), `blockrazor` (`BLOCKRAZOR_KEY`,
   mode `fast`, 0.0001 SOL, 3 TPS), `jito` (keyless, `regions = ["frankfurt", "amsterdam"]`, `max_rps = 1`: one
   1 req/s token bucket per region, the pay-on-success primary only). Each sender gets its own variant of every
   attempt (its tip accounts, `tip = max(bid, its minimum)`), all on the intent's one durable nonce, so at most
   one lands and at most one tip is paid; a tip only moves when the buy succeeds.

   **3b. Fund the probe wallet:** 3 SOL for stage A at the shipped caps (2-3 SOL for B) plus about 0.04 SOL for
   the nonce rent of step 3c, sent from your exchange or wallet to the address in `/etc/rugsnipe/probe-pubkey.txt`.
   At the shipped caps stage A is expected to lose about 0.7-0.85 SOL a day in the no-edge model (at most 1 SOL, the
   daily cap; stage A's first 66 real attempts cost 0.0036 SOL each before the 2026-10-22 bid raise), so 3 SOL lasts
   roughly 3 days: check it daily and top up as in step 7b. Then check the balance:
   ```bash
   P=$(sudo awk '$1 == "pubkey" {print $2}' /etc/rugsnipe/probe-pubkey.txt); echo "probe wallet $P"
   curl -s https://api.mainnet-beta.solana.com -X POST -H 'Content-Type: application/json' \
     -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getBalance\",\"params\":[\"$P\"]}" \
     | python3 -c 'import json,sys; print(json.load(sys.stdin)["result"]["value"] / 1e9, "SOL")'
   ```
   What you should see: `probe wallet` and the address, then the balance in SOL (about 1.04 once your transfer
   has arrived). If it prints `probe wallet` with no address, or a Python `KeyError`, the pubkey file is empty:
   rebuild it (§4, the recovery line) and run this again. Never move SOL in or out while the hot unit runs: the
   `unexplained_drop` trigger latches on a manual withdrawal (§10); step 7b is the safe way to top up.

   **3c. The 32 buy nonce accounts, dry run** (once per wallet, before the first real start; readiness audit
   M2). Every real buy uses one of them; without them every decision is `NO_NONCE`. `rugsnipe-ctl nonce create`
   signs with the probe key and sends one transaction per missing account through your paid RPC. It is
   idempotent: an account that already exists is skipped, so running it again is always safe. Run it as root.
   The rent is 1,056,640 lamports per account, about 0.034 SOL for the 32 (it stays in the accounts). First the
   dry run, which only reads the chain:
   ```bash
   sudo /opt/rugsnipe/bin/rugsnipe-ctl nonce create --keypair /etc/rugsnipe/keys/probe-keypair.json --rpc-url cred:RPC_URL --count 32 --dry-run
   ```
   What you should see: `DRY RUN (nothing is signed or sent): nonce create: authority ...` with your `$P`, 32
   rows `would create` (`exists` for any account made before), the wallet balance against what the creates
   need, and `DRY RUN: nothing was signed or sent. Run the same command without --dry-run to create them.` If
   it says `NOT ENOUGH`, fund the wallet (3b) first. A `BAD` row: stop and escalate to an engineer.

   **3d. The 32 buy nonce accounts, for real:**
   ```bash
   sudo /opt/rugsnipe/bin/rugsnipe-ctl nonce create --keypair /etc/rugsnipe/keys/probe-keypair.json --rpc-url cred:RPC_URL --count 32
   ```
   What you should see: one row per account as it lands (`created` and its signature; `exists` for one made
   before), then `done: created 32, existed 0, failed 0` (on a re-run: `existed 32`). If it stops with
   `STOPPED at #N`, read the reason, fix it (too little SOL: fund and re-run) and run the same command again.

   **Order matters. Never start the real unit (step 6), or any real `rugsnipe-hot`, on
   `/var/lib/rugsnipe/state.db` before 3d says `done`.** At start the hot binary reads every pool account from
   chain and quarantines a missing one for good: the pool slot is chosen from the mint, with no fallback, so
   every launch that maps to a quarantined slot is skipped from then on, and no restart resets it. Dry runs
   (step 5) are safe: they use their own `--state`. If it happened anyway, recover with the hot unit stopped,
   then start it again:
   ```bash
   sudo systemctl stop rugsnipe-hot
   sudo -u rugsnipe sqlite3 /var/lib/rugsnipe/state.db "DELETE FROM nonces"
   ```
   What you should see: no output (the pool is re-read from chain at the next start).

4. **Check the config as root, with everything:**
   ```bash
   sudo /opt/rugsnipe/bin/rugsnipe-ctl config validate /etc/rugsnipe/rugsnipe.toml --creds /etc/rugsnipe/keys \
     --clock-offset-ms 0.05 --post-exec-healthy --armset /var/lib/rugsnipe/armsets/armset-101-SHA.json
   ```
   Before you paste it: replace `SHA` (step 1b), and replace `0.05` with the offset `chronyc tracking` prints
   on its `System time` line, in milliseconds (`0.000050000 seconds` is `0.05`). `--post-exec-healthy`
   asserts the feed's health, which only the running binary can measure; validation still refuses a probe
   config without `[feeds] grpc.enabled`. What you should see: `armset: ArmSet ver 101 ... probe_allowed=true
   probe_stage=a`, then `OK`, or `NOT OK` with only these two refusals, a known false alarm of this check:
   `credential "probe-keypair" missing` and `credential "api-keys" missing` (it looks for the file names
   without `.json` / `.toml`; the dry run of step 5 is the real credential check). Any other `REFUSED` line
   (a stage mismatch, `Stale`, a pubkey): stop. `config validate` refuses an ArmSet whose `probe_stage` differs
   from `[probe] stage` (and `stage = "b"` with an ArmSet that names no stage).

5. **Dry run on the box** (works now, before or after funding; sends nothing). The binary loads the real
   probe key from systemd credentials, seeds the nonce pool from chain, signs every variant with the real
   key, records them instead of sending, and prints a self-test: one signed buy per configured sender
   (placeholder mint, the first usable nonce's real value) and the nonce burn (AdvanceNonce only), each
   as base64 with a read-only `simulateTransaction` result (`sigVerify = true`):
   ```bash
   sudo systemd-run --unit rugsnipe-probe-dry --uid rugsnipe --gid rugsnipe --pipe --wait \
     -p LoadCredential=probe-keypair:/etc/rugsnipe/keys/probe-keypair.json \
     -p LoadCredential=api-keys:/etc/rugsnipe/keys/api-keys.toml \
     /opt/rugsnipe/bin/rugsnipe-hot --config /etc/rugsnipe/rugsnipe.toml --probe-dry \
       --armset /var/lib/rugsnipe/armsets/armset-101-SHA.json --state /var/lib/rugsnipe/probe-dry.db \
       --rpc-url cred:RPC_URL --duration-s 60 --no-metrics --pin-core none
   ```
   Read it: `dry self-test: SENDER variant 0 sig ... tip ... prio ...` for each of the five senders (a
   missing one is not enabled or its key is missing), then `dry first signed buy ...:` and
   `dry nonce burn ...:` (each with base64 and `simulateTransaction err=...`). Before funding the fee payer does
   not exist and both say `AccountNotFound`: that already proves the signature verifies. After funding
   and step 3d the burn must simulate with `err=null` (the key, the nonce account and its authority are
   right; `dry self-test: no usable nonce account` means step 3d is missing) and the buy must fail at its ATA
   or pump instruction (index 3 or later: the placeholder mint), never at instruction 0 (the nonce). Any refusal
   (key permissions, ArmSet, stage, clock, caps) is printed with every reason and nothing starts. Use a
   separate `--state` for dry runs. The dry run does not refuse an unhealthy feed: read the
   `grpc feed https://...: connected true slot ...` line yourself.

6. **Start.**

   **6a. Before every start (the first and every restart, unkill or reboot): the wallet must hold no stray
   tokens** (readiness audit M6). At start, recovery adopts every non-zero token balance in the probe wallet as
   an open position; stage A has one position slot, so a single spam airdrop silently stops every attempt, and
   such a position never closes:
   ```bash
   P=$(sudo awk '$1 == "pubkey" {print $2}' /etc/rugsnipe/probe-pubkey.txt); echo "probe wallet $P"
   for prog in TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb; do
     curl -s https://api.mainnet-beta.solana.com -X POST -H 'Content-Type: application/json' \
       -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getTokenAccountsByOwner\",\"params\":[\"$P\",{\"programId\":\"$prog\"},{\"encoding\":\"jsonParsed\"}]}" \
     | python3 -c 'import json,sys; v=json.load(sys.stdin)["result"]["value"]; print(sys.argv[1][:8], len(v), "token account(s)"); [print("  mint", a["account"]["data"]["parsed"]["info"]["mint"], "amount", a["account"]["data"]["parsed"]["info"]["tokenAmount"]["amount"]) for a in v]' "$prog"
   done
   sudo -u rugsnipe sqlite3 -readonly /var/lib/rugsnipe/state.db "select mint,state from positions where state!='closed'"
   ```
   What you should see: `probe wallet` and the address, `Tokenkeg 0 token account(s)` and `TokenzQd 0 token
   account(s)` (or only lines with `amount 0`, which are harmless), and an empty list of open positions (on the
   very first start the last line says `unable to open database`: that is fine). **Do not start** if any line
   shows a non-zero `amount` whose mint is not one of the open positions: escalate to an engineer (the tokens
   must be burned and the account closed with the probe key first).

   **6b. The hot unit's drop-in** (once per ArmSet). It adds the ArmSet and the RPC credential to the hot
   unit's command line. Replace `SHA` (step 1b) before you paste it:
   ```bash
   sudo mkdir -p /etc/systemd/system/rugsnipe-hot.service.d
   printf '%s\n' '[Service]' 'ExecStart=' 'ExecStart=/opt/rugsnipe/bin/rugsnipe-hot --config /etc/rugsnipe/rugsnipe.toml --busy-poll --armset /var/lib/rugsnipe/armsets/armset-101-SHA.json --rpc-url cred:RPC_URL' | sudo tee /etc/systemd/system/rugsnipe-hot.service.d/30-probe.conf
   sudo systemctl daemon-reload
   systemctl cat rugsnipe-hot --no-pager | grep ExecStart
   ```
   What you should see: the three lines echoed back, then the base unit's `ExecStart=` line followed by an empty
   `ExecStart=` (it clears the base line) and your line with `--busy-poll`, the ArmSet and `--rpc-url
   cred:RPC_URL`. Keep `--busy-poll`: the drop-in replaces the whole base line, and without it the hot thread
   sleeps 200 µs when idle.

   **6b-bis. The Jito relay** (once per box; 2026-10-22). The hot senders speak plain HTTP and Jito's block
   engine refuses it, so Jito goes through a local nginx that terminates TLS (`deploy/nginx/jito-relay.conf`,
   install and check commands in the file's header). The config line is
   `jito = { enabled = true, region = "frankfurt", url = "http://127.0.0.1:8081", max_rps = 1 }`. Without the
   relay leave `jito` disabled: every send fails `io` (connection refused) and the sender's breaker opens.

   **6c. The guard's drop-in and the units.** install.sh writes the guard's probe drop-in; then enable the units
   explicitly (the target starts nothing that is not enabled). Replace `SHA` again:
   ```bash
   sudo bash ~/rugsnipe/deploy/install.sh --role hot --probe-armset /var/lib/rugsnipe/armsets/armset-101-SHA.json
   sudo systemctl enable rugsnipe-guard.service rugsnipe-hot.service rugsnipe.target
   systemctl list-dependencies rugsnipe.target --no-pager
   ```
   What you should see: install.sh mentions `/etc/systemd/system/rugsnipe-guard.service.d/30-probe.conf`
   (`--real-sell --armset ... --rpc-url cred:RPC_URL`), and `list-dependencies` lists `rugsnipe-guard.service`
   and `rugsnipe-hot.service`. **Never enable `rugsnipe-keeper`**: it has no binary yet. The hot unit asks for
   it, so a failed `rugsnipe-keeper` (`203/EXEC`) under `list-dependencies` or in `systemctl status` is expected
   and harmless.

   **6d. Start** (only after 6a passed and step 3d said `done`):
   ```bash
   sudo systemctl start rugsnipe.target
   systemctl status rugsnipe-guard rugsnipe-hot --no-pager
   sudo journalctl -u rugsnipe-hot -n 50 --no-pager
   ```
   What you should see: both units `active (running)`; the guard says `REAL sells`, then `exits lease held by
   hot:...`; the hot journal shows the start line with `mode=probe` and the kill file `/run/rugsnipe/KILL`, and
   `grpc feed https://...: connected true slot ...`; a Telegram start message arrives. If the hot unit is
   not running, read its journal: a refused start prints every reason.

   `risk.kill_file` must be an absolute path under `/run/rugsnipe/` in probe and live (validation
   refuses anything else; the shipped `/run/rugsnipe/KILL` is right; the paper examples' relative paths
   are paper-only). A new ArmSet (stage B) means 6b again with the new file name, `install.sh` with the new
   `--probe-armset`, and `sudo systemctl daemon-reload && sudo systemctl restart rugsnipe-guard` (not while the
   guard holds the lease).
   Every other gate is checked first (nothing opens a socket before); then the gRPC feed starts and
   must be connected with a slot update within `--feed-wait-s` (default 20 s): the journal shows
   `grpc feed https://...: connected true slot … (… ms)`. Otherwise probe is refused with
   `no healthy post-execution feed (gRPC ...: connected …, last slot …, reconnects …)`: check the URL,
   the `GRPC_TOKEN` line, and `curl -sv https://YOUR-ENDPOINT.rpcpool.com` from the box. While running, a feed with no data
   message for more than `stale_ms`, or trailing `getSlot` by more than 8 slots, latches the kill (§10).
   The leader schedule loads in the first seconds (`leaders epoch …` in the status line); until it
   does, a buy carries both the pay-on-success and the priority variant.
   A position opens only on our own TradeEvent (the feed's or the landed transaction's) or a token
   balance, never on a send; a buy that lands and fails costs its `meta.fee` (counted toward the daily
   loss); nothing landed means the hot box burns the nonce after `tx.burn_after_slots` and the attempt
   is `dropped` at no cost. Sells go through the same senders on a recent blockhash; a position closes
   on our own sell TradeEvent or a zero balance. If the hot box stops (its heartbeat older than 5 s with
   a position open), the guard (`--real-sell`, from the drop-in above) takes the exits lease over and
   sells through the same senders; a sell counts only when the wallet's token balance reads 0 (then it
   closes the position in `state.db`), otherwise it is re-sent every 3 s (`--retry-ms`) on a fresh
   blockhash until `risk.max_sell_spend_lamports` of bids, then `CRITICAL: sell of MINT not confirmed`
   (sell by hand). When the restarted hot box beats again and no guard sell is in flight, the guard
   hands the lease back (`hot box is back ...`) and hot resumes its exits. **The daily caps survive a
   restart**: the start reads today's (UTC) failed-attempt cost, closed P&L, real buys (per day, hour and
   operator) and the operators' loss latches back from `state.db` (`risk day restored: ...` in the
   journal) and latches `daily_loss` at once if today is already at the cap. The end-of-run summary's
   `cold.probe` block has the landing stats per sender and per variant (bid, rank, premium, failed cost),
   and `state.db` `kv.probe_report` keeps the last one.

7. **Read the kill rule** at least daily, and at the end of the stage:
   ```bash
   cd /opt/rugsnipe/src
   sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I analysis/scripts/shadow_gate.py --probe \
     --state /var/lib/rugsnipe/state.db --config /etc/rugsnipe/rugsnipe.toml --hard-stop-days 14
   ```
   `--hard-stop-days 14` is the stage length decided on 2026-10-20 (probe-design-v2 §2 item 4). The hour cap it
   checks is the config's `risk.max_buys_per_hour`.
   What you should see (probe design v2 rules, `analysis/scripts/shadow_gate.py`):
   - `attempts` and the days covered; `landed rule`: creation-slot and slot+1 fills (`--late-ok 1`) at +2% or
     less count as landed; the later fills (slot+2 and on, bucketed per slot up to +6 and then "slot+7 or later";
     at `tx.burn_after_slots = 3` the nonce burn lands around slot+4) and fills above +2% are counted apart, the
     strict slot-0 rate next to the inclusive one;
   - `landing rate` (of the attempts that raced) and its break-even (`pnl_model.break_even_landing` for the
     0.25 SOL LEAD at the measured mean failed-attempt cost and mean premium; 9.9% at 0.00073 SOL);
   - `failures`: lost races (`slippage`, `premium`, `not_in_slot0`, `dropped`) and, apart, failures that are
     **not** races (`instruction_error`, `custom_...`), which the landing rate leaves out;
   - the failed-attempt cost, the median premium, the EV per attempt with its 90% interval (normal and by
     hour clusters), the hour cap (`max_per_hour` must be at most the config's `risk.max_buys_per_hour`, 6 as
     shipped), the worst day against the cap, and
     attempts and landings per bid variant, per leader class and per sender (`sender helius sent ...,
     accepted ..., landed ...`);
   - `in-class subset`: the same landing, premium and EV on the attempts with `bundle_wallets >= 1`;
   - `stage-B bar`: the larger of the pooled LEAD break-even and the unfiltered shadow break-even at
     0.25 SOL, with both inputs;
   - one verdict: `KILL: REASONS`, `CHECK: ...`, `CONTINUE probe-A|B` (also while 350 attempts and 7 days are
     reached but the in-class subset is below 200), `INCONCLUSIVE: in-class subset N < 200 attempts at the
     10-day hard stop (...): ... escalate to the owner`, `PROBE-A COMPLETE: ENTER PROBE-B`,
     `PROBE-A COMPLETE: DO NOT ENTER PROBE-B (...): stop` or `PROBE-B COMPLETE: read the live gate`.

   On `INCONCLUSIVE`, do not stop the project and do not enter B: stage A has too few in-class attempts for any
   reading, which is not a negative result. Send the whole output to the owner, who decides whether to extend
   stage A (probe-design-v2 §2 item 4). The box may keep running meanwhile; it drops to paper by itself when the
   ArmSet expires. On `KILL`, touch the kill file (§10). On `CHECK`, do not stop the project: our own transaction is failing
   (most likely the CU limit; `sudo -u rugsnipe sqlite3 -readonly /var/lib/rugsnipe/state.db "select
   fail_reason,count(*) from fills group by 1"` shows which error), so escalate to an engineer. Before day 7
   only a `KILL` ends stage A. The end-of-run summary's `cold.paper` block carries the same inputs
   (`attempts_scored`, `landing_rate`, `failed_cost_lamports_mean`, `median_premium_bps_fills`, `ev_per_attempt`,
   `ev_per_attempt_se`, `ev_per_attempt_90ci`). Raw rows are in `state.db` (`fills`, `positions`, `exits`,
   `sends`, `feed_bundle`; failed attempts carry `fills.fail_cost` = the `meta.fee` of the failed transaction,
   0 for a dropped send; `fills.variant` / `fills.sender` / `fills.fee` / `fills.tip` name the landed variant).
   The verdict also prints attempts, landings and failed cost **per leader class** (`fills.leader_class` = the
   class of the slot the transaction landed in, else `intents.leader_class` = the create slot's;
   `sends.leader_class` = the class each variant bid for), and the summary's `cold.probe.by_leader_class` has
   the same split. That split decides whether BAM keeps bidding with the priority fee (docs/03 §6.1).

   **7b. Balance and top-up** (daily, with step 7). A buy needs `risk.min_hot_balance_lamports` (0.1 SOL) plus the
   stake and the bid in the wallet; below that buys stop without an alert (exits continue). Run the balance
   command of step 3b. When it shows less than **0.4 SOL** (about 3-6 days at the shipped caps), top up with the
   hot unit stopped, never while it runs (§10, `unexplained_drop`). First check that no position is open:
   ```bash
   sudo -u rugsnipe sqlite3 -readonly /var/lib/rugsnipe/state.db "select mint,state from positions where state!='closed'"
   ```
   What you should see: no output. If a row is listed, wait a minute and run it again (LEAD positions close within
   seconds); a row that stays: escalate to an engineer. Then stop the hot unit (the guard keeps running):
   ```bash
   sudo systemctl stop rugsnipe-hot
   ```
   Send the SOL (0.5-1 SOL at the shipped caps) from your exchange or wallet to the address in
   `/etc/rugsnipe/probe-pubkey.txt`, wait until the step 3b balance command shows it, run the token check of step
   6a, and start again:
   ```bash
   sudo systemctl start rugsnipe-hot
   sudo journalctl -u rugsnipe-hot -n 30 --no-pager
   ```
   What you should see: the start line with `mode=probe`, `risk day restored: ...` (today's caps carry over) and
   `grpc feed https://...: connected true slot ...`; a Telegram start message arrives.

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

**The latch is persisted.** Whatever latched it (the file, the signal, Telegram `/kill`, or a
trigger below), the cold thread writes `kv.kill_latch` (reason, time, the first trigger's detail) to
`state.db` within ~100 ms, so a crash, a SIGKILL, a restart or a reboot (which clears `/run`) comes
back killed: the start prints `KILL latch restored from state.db: …`, and the summary carries
`cold.kill_restored`. Buys stay off, exits continue.

**Triggers** (probe/live latch; paper only reports them as `cold.would_kill`; each writes a
`kv.kill_trigger:<name>` row with the time and the detail, listed in the summary's
`cold.kill_triggers` and in the status line):

| Trigger | Fires when | Knob (`[risk]`) |
|---|---|---|
| `send_errors` | more than 20% of the last 50 real sends failed | `send_error_kill_ratio`, `send_error_window` |
| `unexplained_drop` | the wallet fell (between two 2-s reads) by more than our known spends of the last 120 s (stakes, bids, the failed-attempt cap, ATA rent, fees, sells, burns: upper bounds) plus a tolerance. A manual sweep or withdrawal trips it: stop the service first | `unexplained_drop_lamports` (0.05 SOL) |
| `program_upgrade` | pump's programdata (deployed slot or upgrade authority, a 45-byte read every 60 s) differs from `kv.pump_programdata` (stored at the first read; an upgrade while the box was down counts too) | |
| `disk` | the state partition is more than 90% used | `max_disk_used_pct` |
| `clock` | `chronyc -c tracking` offset above 50 ms (start is refused above 5 ms or when chrony is unreadable as the service user) | `kill_clock_offset_ms` |
| `drift` | the gRPC feed's slot trails `getSlot` (processed) by more than 8 slots (every 10 s) | `max_slot_drift` |
| `feed_stale` | no data message from the gRPC feed for more than its `stale_ms` | `[feeds] grpc.stale_ms` |
| `daily_loss` | closed P&L + failed attempts' `meta.fee` + open positions at their current sell quote below minus the daily cap (the stage's cap in probe); also at start, from today's rows in `state.db` | `max_daily_loss_lamports` |
| `crash_loop` | the 3rd start within an hour that follows an unclean exit (crash, SIGKILL, watchdog, panic: `kv.run_open` was still set; `kv.unclean_starts` lists them) | |
| `panic` | a panic anywhere in a probe/live process: the hook writes the kill file and `kv.kill_latch`, then aborts (release builds use `panic = "abort"`); systemd restarts it killed | |

The decode-miss ratio (above 1% over 500 creates) and a stale ArmSet keep their earlier behaviour.

**The kill file** is `risk.kill_file`; probe and live refuse anything but an absolute path under
`/run/rugsnipe/` (the units' tmpfiles directory), and every start prints the path it watches
(`... | kill file /run/rugsnipe/KILL`). **Crash loops**: besides `crash_loop`, the start-limit drop-in
(§3) stops a unit after 5 starts in an hour; `systemctl reset-failed <unit>` re-arms it.

**Known gap: no exits-only start.** When a real start is refused (a gate fails: ArmSet, clock, keys,
feed) the hot process exits, even if recovery would find open positions; it does not start in an
exits-only mode. The guard covers this: with the hot box down it holds the exits lease and sells every
open position (grace rule, retried until the balance is 0). Keep the guard running whenever a position
is open, and check `select mint,state from positions where state!='closed'` before fixing the refusal.

```bash
sudo -u rugsnipe /opt/rugsnipe/bin/rugsnipe-ctl kill status --state /var/lib/rugsnipe/state.db
#   KILLED: send_errors at 1792... (send_errors: 11 of the last 50 sends failed (22% > 20%))
#   trigger send_errors: ...
```

**Unkill** (local only, deliberately): remove the kill file if there is one, clear the persisted
latch, restart:

```bash
sudo rm -f /run/rugsnipe/KILL
sudo systemctl stop rugsnipe-hot
sudo -u rugsnipe /opt/rugsnipe/bin/rugsnipe-ctl unkill --state /var/lib/rugsnipe/state.db
sudo systemctl start rugsnipe-hot
```

`unkill` keeps the trigger rows as history. After a `program_upgrade`, run `rugsnipe-ctl sim` against
the new program first; `unkill` then drops the stored programdata, and the next start takes the
deployed program as the new baseline. Once real money is involved, stop everything only when flat:
`sudo sqlite3 -readonly /var/lib/rugsnipe/state.db "select mint,state from positions where state!='closed'"`.

**Telegram** (optional in shadow, **required for the probe**: it is the only alert path in this build besides
the journal; the dead-man line below tells you the box stopped buying, nothing tells you it died): add
`TG_TOKEN = "..."` (the bot token from @BotFather) and `TG_CHAT = "..."` (your chat id) to `api-keys.toml`, each
on its own line; `[obs] alert = { telegram = "TG_TOKEN", chat = "TG_CHAT", hourly = true, commands = false }`
(already in `config/probe-a.example.toml`). The box sends a start message, every kill with its trigger, and
an hourly summary (never from the hot thread; a failed send is only counted, `cold.alerts`). No start message
after a start means the alert path is broken: fix it before you leave the box alone. `commands = true` also polls the bot every 5 s:
`/kill` from that chat latches the kill (there is no remote unkill).

**Dead-man line (no buys while launches keep flowing, 2026-10-22).** In probe and live the cold thread checks once a
minute whether launches kept flowing while the bot bought nothing: a full 15-minute window with at least 50 creates
seen and zero real buys (`rugsnipe_decisions{outcome="fire"}` unchanged), the process at least 15 minutes old. When
it holds, the bot writes one stderr line (in `journalctl -u rugsnipe-hot`) and sends the same text to Telegram (a
no-op when Telegram is off), at most once an hour, whether or not the status line looks normal; it clears, kills and
resets nothing. The line reads `dead-man on fra1 (probe): no buys in the last 15 min, 1204 creates seen; last fire
212 min ago; decisions: skip_op_latched 611, no_match 430, late_slot 71; killed: no`: `last fire` is minutes since
the last real buy this run (`none this run` after a restart that never bought), `decisions` the three most frequent
decision outcomes of the window, `killed` the kill reason or `no`. The top outcome usually names the cause:
`skip_op_latched` is the per-operator consecutive-loss latch (`risk.max_consec_losses_per_op`; clear it as in the
config table above), `skip_killed` the kill latch (possibly restored from `state.db` at start: `rugsnipe-ctl kill
status`, find the cause, then `unkill`), `skip_max_buys_per_hour` the hourly cap (normal when it dominates with
`last fire` under an hour), `no_match` / `skip_class` dominating with no kill means the feed and the gate are fine
and nothing qualifies (normal at quiet hours). Metrics for a rule: `rugsnipe_last_fire_unix` (0 before the first buy
of the run) and `rugsnipe_minutes_since_fire` (since process start when none this run), e.g. `rugsnipe_mode > 0 and
rugsnipe_minutes_since_fire > 15 and increase(rugsnipe_creates[15m]) > 50`. Paper mode runs the ring and the gauges
but never alerts. The constants live in `crates/rugsnipe/src/deadman.rs` (15 min window, 50 creates, 60 min spacing).

**Decode misses (2026-10-22).** Every create the decoder rejects now writes one stderr line: `decode miss:
src=<grpc|logsws|...> n=<ordinal> slot=<slot> sig=<base58> kind=<eof|invalid_bool|invalid_utf8|invalid_length|
invalid_transaction|invalid_base64|bad_discriminator|cpi_needs_meta|unresolved_accounts|logs> err="<decoder error>"
programs=<up to 4 top-level program ids>[+k] v0=<1|0|?> loaded=<1|0|?> inner=<1|0|?>`. `n` is that feed's running
decode-miss count since the process started (the count `risk.decode_miss_kill_ratio` uses); lines are limited to the
first 50 misses per feed per run, then every 100th, so a gap in `n` is the limiter, not lost misses. `eof`,
`invalid_bool`, `invalid_utf8`, `invalid_length` mean a pump instruction's arguments do not match the layout we decode
(a new instruction shape: fetch `sig` with getTransaction and compare the create args); `invalid_transaction` means our
own re-serialised bytes did not parse (a codec bug, must stay 0); `logs` means the logs feed saw a Create with no
CreateEvent. `inner=0` under `src=grpc` is normal (the adapter decodes without inner instructions). The metric
`rugsnipe_feed_decode_miss_kind{src,kind}` splits `rugsnipe_feed_decode_miss` by kind, plus `src="hot"` for the hot
thread's own decode (there, `cpi_needs_meta` / `unresolved_accounts` mean the gRPC trailer lacked inner instructions or
lookup addresses). On the box: `journalctl -u rugsnipe-hot --since -6h | grep 'decode miss:' | awk '{print $7}' | sort
| uniq -c` is the kind histogram of the logged misses (the first 50 carry signatures to look up); `curl -s
127.0.0.1:9464/metrics | grep decode_miss_kind | grep -v ' 0$'` the live counters. When the `decode_miss` kill latches,
the kind with the largest count under `src="grpc"` is the transaction shape to fix. On 2026-10-22 the kill latched at
1.3 misses per 100 creates with the pump program unchanged; `decode_miss_kill_ratio` was raised to 0.05 on fra1 until
the kind is known.

## 11. After Alpenglow (epoch 1064, Oct 21 2026 ~15:00 UTC)

Slot timing changes (confirmation ~150 ms, several banks per slot, ticks collapse). Do this that week:

1. **Split the data at the cutover.** Stop the running shadow before Oct 21 15:00 UTC and start a new
   one with a new `--state` file after it. No probe money across the cutover: stage A starts Oct 23 at the
   earliest, after one full post-cutover shadow day (`docs/research/probe-design-v2.md` §6). The go/no-go rule needs a full post-Alpenglow day; pre-cutover
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
| Triton Dragon's Mouth pay-as-you-go (or Alchemy gRPC PAYG, $75/TB) | ~$125 prepaid + $0.08/GB | probe (the post-execution feed) | **yes** (`[feeds] grpc`, feature `grpc`, on by default) |
| SVS Yellowstone gRPC | 150 | docs/07 shadow plan (AMS/ATL only) | yes, any standard Yellowstone endpoint |
| Helius LaserStream fra (post-exec gRPC) | 499-999 | after the probe, if Triton/Alchemy lag | yes (same adapter) |
| DoubleZero Edge FRA seat (leader shreds) | 1,500 | live, after the gate | **no** (no shred feed yet) |
| Helius shred delivery per IP | 800-1,000 | live, after the gate | **no** |
| NextBlock sender | 249+ | live, if an A/B shows value | yes, optional (`[senders] nextblock`, feature `real-senders`) |
| Landed buy, all-in | ~0.004 SOL median per landed buy (p75 0.014) | probe, live | |
| Failed attempt (modelled, docs/03 §6.1) | ~0.00073 SOL at the shipped caps (range 0.00036-0.001); ~0.00213 SOL at the probe-A caps of 2026-10-22 (`bid.failed_attempt_max_by_class`: 15,000 Jito-Agave, 3,405,000 BAM/other, 2,000,000 Harmonic); at most the class cap, or `bid.failed_attempt_lamports_max` where it is 0 | probe, live | |

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
| `GRPC_TOKEN` | the Yellowstone gRPC `x-token` (Triton, Alchemy, SVS, LaserStream) | per GB / plan | `[feeds] grpc.x_token` (a line of `api-keys.toml`, or a credential file of that name) |
| `RPC_URL` | the paid RPC URL with its key (fill lane, burns, blockhashes, guard balances) | vendor plan | `--rpc-url cred:RPC_URL` (hot and guard; never the URL itself on a command line) |
| `LOGSWS_URL`, `GRPC_URL` (optional) | a feed URL that carries a key | vendor plan | `[feeds] logsws.url = "cred:LOGSWS_URL"`, `grpc.url = "cred:GRPC_URL"` |
| `HELIUS_KEY` | Helius Sender `fra-sender` (free key: 50 req/s per region) | per-tx tip, 0.001 SOL; 5,000 lamports on the `swqos_only` leg | `[senders] helius.key` (`helius_swqos` uses it too) |
| `NOZOMI_KEY` | Nozomi (Temporal) sender | per-tx tip, min 0.001 SOL | `[senders] nozomi.key` |
| `ASTRALANE_KEY` | Astralane sender | per-tx tip, 0.001 SOL (0.00001 at VIP3) | `[senders] astralane.key` |
| `BLOCKRAZOR_KEY` | BlockRazor sender (auth token, header `apikey`), mode `fast` | free, per-tx tip min 0.0001 SOL, 3 TPS | `[senders] blockrazor.key` |
| `NEXTBLOCK_KEY` | NextBlock sender (optional) | $249+/mo (trial 1 tx/10 s) | `[senders] nextblock.key` |
| `TG_TOKEN` | Telegram alert bot token | free | `[obs] alert.telegram` (required for the probe: the only alert path) |
| `TG_CHAT` | the chat id the alerts go to (and `/kill` is accepted from) | free | `[obs] alert.chat` (required for the probe) |
| `HC_URL` | healthchecks.io dead-man URL | free tier | `[obs] alert.deadman` (not built yet: nothing reads it) |
| (none) | Helius Sender without a key | as above | `[senders] helius` with no `key` (1 req/s per IP) |
| (none) | Jito block engine Frankfurt + Amsterdam, `bundleOnly` | per-tx tip, min 1,000 lamports; a failed bundle-only transaction costs nothing | `[senders] jito.regions` (1 req/s per IP per region, one token bucket each) |
| (DZ ID pubkey) | DoubleZero Edge seat | $1,500/mo | applied for at doublezero.xyz with the box's DZ ID; no API key |
| (seat token, IP-bound) | Helius shred delivery | $800-1,000/mo | Helius dashboard, one seat per IP |

## 14. Where things are on fra1

| Path | What |
|---|---|
| `/etc/rugsnipe/shadow.toml` | the shadow config (step 5) |
| `/etc/rugsnipe/rugsnipe.toml` | the service config (probe/live; never overwritten by install.sh; probe A: from `config/probe-a.example.toml`, §9 step 3) |
| `/etc/rugsnipe/probe-pubkey.txt` | the probe wallet's address (public; §4), read as `$P` in §9 |
| `/etc/rugsnipe/operators.json` | the operator table (`[entry] operators_file`, docs/09): rebuilt daily at 23:50 UTC by `rugsnipe-operators.timer` (0640 root:rugsnipe), then `try-restart rugsnipe-hot`, which loads it only at start. Run now: `sudo systemctl start rugsnipe-operators.service`; log: `journalctl -u rugsnipe-operators`; preview: `operators_refresh.py --dry-run` |
| `/var/lib/rugsnipe/armsets/` | signed ArmSets |
| `/var/lib/rugsnipe/shadow/{state.db,rslog,summary.json}` | the shadow run's record |
| `/run/rugsnipe/KILL` | the kill file |
| `/opt/rugsnipe/bin/` | `rugsnipe-hot`, `rugsnipe-guard`, `rugsnipe-ctl`, `rugsnipe-replay`, `rugsnipe-sim`, `tune-hot.sh` |
| `/opt/rugsnipe/src`, `/opt/rugsnipe/venv` | the deployed checkout and its Python |
