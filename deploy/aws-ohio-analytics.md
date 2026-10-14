# ohio1: run the analytics role on the existing AWS Ohio box

Box: AWS us-east-2, `ssh ubuntu@18.188.14.68` with your key. Role `analytics`: collector,
backfill, the 6-hourly discovery timer, reports, and later the ArmSet signing key and S3
backup. **Ohio never trades.** It has 0% of leader slots and is ~105 ms from Frankfurt
(docs/03-speed-infra.md section 3). The hot path goes on fra1 (`deploy/frankfurt-checklist.md`).

Every command below runs on your laptop (`laptop$`) or on the box (`ohio1$`).

## 0. Before you start

```bash
laptop$ ssh -i ~/.ssh/<your-key>.pem ubuntu@18.188.14.68
ohio1$ lsb_release -ds; nproc; free -h; df -h /       # Ubuntu 22.04 or 24.04; >= 2 vCPU, >= 4 GB RAM
```

- **Disk.** The backfill (docs/06 section 9.1, ~0.5-1M RPC calls) caches raw responses under
  `/var/lib/rugsnipe/data/cache`. Plan for **≥ 100 GB** free (an estimate). Grow the EBS volume
  (gp3) first if `df` shows less: EC2 console → Volumes → Modify, then
  `sudo growpart /dev/nvme0n1 1 && sudo resize2fs /dev/nvme0n1p1`. Device names vary; check `lsblk`.
- **Elastic IP.** Attach one so 18.188.14.68 never changes. WireGuard and the provider
  allow-lists depend on it.
- **Security group.** Allow inbound TCP 22 from your IP only. Later add inbound UDP 51820 from fra1's
  public IP only (`deploy/wireguard-example.conf`). Allow nothing else inbound.

## 1. Clone

The repository is private, so give the box a **read-only deploy key**:

```bash
ohio1$ ssh-keygen -t ed25519 -N '' -f ~/.ssh/rugsnipe_deploy -C ohio1-deploy
ohio1$ cat ~/.ssh/rugsnipe_deploy.pub     # GitHub → repo → Settings → Deploy keys → Add (read-only)
ohio1$ cat >> ~/.ssh/config <<'EOF'
Host github-rugsnipe
  HostName github.com
  User git
  IdentityFile ~/.ssh/rugsnipe_deploy
  IdentitiesOnly yes
EOF
ohio1$ git clone github-rugsnipe:xin10ylop/rugsnipe.git ~/rugsnipe
ohio1$ cd ~/rugsnipe && git checkout main     # or the branch you are deploying
```

## 2. Install

```bash
ohio1$ cd ~/rugsnipe
ohio1$ bash deploy/install.sh --role analytics --dry-run     # read what it will do
ohio1$ sudo bash deploy/install.sh --role analytics
```

The installer:

- installs apt packages, plus Python 3.11 on 22.04;
- creates the `rugsnipe` system user;
- installs the pinned Rust toolchain from `rust-toolchain.toml` and builds the workspace (allow 10-20 min the first time; skip it with `--no-build`);
- creates `/opt/rugsnipe/{src,venv,bin}`;
- writes `/etc/rugsnipe/rugsnipe.toml` from the example with `id = "ohio1"`, `role = "analytics"` and `mode = "paper"`;
- sets chrony to Amazon Time Sync (169.254.169.123);
- creates `/var/lib/rugsnipe/...` and `/run/rugsnipe`;
- installs the systemd units;
- runs the offline Python selftest as `rugsnipe`.

Re-run it after every `git pull`. It is idempotent and never overwrites `/etc/rugsnipe/rugsnipe.toml`.

## 3. Start

```bash
ohio1$ sudo systemctl enable --now rugsnipe-collector.service rugsnipe-discover.timer
ohio1$ systemctl status rugsnipe-collector --no-pager
ohio1$ journalctl -u rugsnipe-collector -f
ohio1$ systemctl list-timers 'rugsnipe-*'
```

The collector runs `analysis/scripts/collector.py --config /etc/rugsnipe/rugsnipe.toml --db
/var/lib/rugsnipe/analytics.db` from the venv, through `/opt/rugsnipe/bin/rugsnipe-pyrun --script collector`.
That is the single PumpPortal connection plus `logsSubscribe(TSLvdd…)`, with decode, funding, rivals and tape jobs inside
`[rpc_budget] collector = 12`. If the script is missing (an old checkout), the unit logs `... is not deployed yet`
and stays stopped with exit code 78 instead of restart-looping. A config error exits 1 and is retried at most 30 times in 10 min.
It prints a status line every minute: `journalctl -u rugsnipe-collector -f`.

The discover timer runs every 6 h (00:17, 06:17, 12:17 and 18:17 UTC). For now it is a **placeholder** that runs the
selftest; M2 replaces its `ExecStart` with the discovery cycle (docs/06 section 9.4).

Only one PumpPortal connection is allowed per IP (`subscribeNewToken` is free for one connection). Do not run a second
collector on this box or anywhere else behind the same IP.

## 4. Where data lands

| Path | What |
|---|---|
| `/var/lib/rugsnipe/analytics.db` | collector/backfill SQLite (WAL), docs/06 section 5.2 |
| `/var/lib/rugsnipe/data/` | the package's `data/` (symlinked from `/opt/rugsnipe/src/data`): `cache/` (RPC + API responses), `reports/`, datasets |
| `/var/lib/rugsnipe/data/reports/` | report JSON/markdown (go/no-go report once M2 lands) |
| `/var/lib/rugsnipe/armsets/` | signed ArmSets (M2), pushed to fra1 over wg0 |
| `/var/lib/rugsnipe/rslog/`, `state/` | rslog and state.db snapshots pulled from fra1 (phase 1+) |
| `/etc/rugsnipe/rugsnipe.toml` | config (root:rugsnipe 0640) |
| `/etc/rugsnipe/keys/` | root 0700. Here: only `armset-signing.key` (0400) and `api-keys.toml`. **Never** hot or probe wallets; install.sh refuses them on this role |
| `journalctl -u 'rugsnipe-*'` | logs |
| `/opt/rugsnipe/REVISION` | deployed commit |

## 5. Pull reports to your laptop

`/var/lib/rugsnipe` is `0750 rugsnipe`, so use sudo rsync on the remote side:

```bash
laptop$ rsync -avz -e "ssh -i ~/.ssh/<your-key>.pem" --rsync-path="sudo rsync" \
          ubuntu@18.188.14.68:/var/lib/rugsnipe/data/reports/ ./ohio-reports/
laptop$ ssh -i ~/.ssh/<your-key>.pem ubuntu@18.188.14.68 \
          "sudo sqlite3 -readonly /var/lib/rugsnipe/analytics.db '.tables'"
```

Run a script ad hoc as the service user, so it shares the caches and the RPC budget:

```bash
ohio1$ sudo -u rugsnipe /opt/rugsnipe/venv/bin/python -I /opt/rugsnipe/src/analysis/scripts/pull_dataset.py --stages stats
ohio1$ sudo -u rugsnipe /opt/rugsnipe/bin/rugsnipe-pyrun selftest
```

Public RPC allows about 4 req/s per method per IP. The collector and backfill share that budget (`[rpc_budget]`).
Do not run heavy pulls from your laptop through the same IP at the same time.

## 6. Latency reference (optional)

```bash
ohio1$ bash ~/rugsnipe/deploy/latency_probe.sh 10
```

Expect ~100 ms to the Frankfurt targets. This is the measured reason Ohio stays analytics-only.

## 7. Later: WireGuard to fra1

Once fra1 exists, set up `wg0` from `deploy/wireguard-example.conf`. Ohio is 10.77.0.1 and fra1 is 10.77.0.2.
Then add the 5-minute rslog/state snapshot pull and the ArmSet push. Both run over wg0.

## Uninstall

```bash
ohio1$ sudo systemctl disable --now rugsnipe-collector rugsnipe-discover.timer rugsnipe.target
ohio1$ sudo rm -f /etc/systemd/system/rugsnipe* /etc/tmpfiles.d/rugsnipe.conf /etc/chrony/sources.d/rugsnipe.sources
ohio1$ sudo systemctl daemon-reload
# data and config stay in /var/lib/rugsnipe and /etc/rugsnipe; delete them by hand if wanted
```
