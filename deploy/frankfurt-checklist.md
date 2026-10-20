# fra1: rent, accept and onboard the Frankfurt hot box

Why Frankfurt: it produces 32-35% of leader slots and is the home of almost every low-latency vendor, all in
TeraSwitch (AS20326). P(same slot) is 0.78 from Frankfurt and 0.51 from Ohio (docs/03-speed-infra.md
sections 2-4). fra1 runs `rugsnipe-hot` and `rugsnipe-guard` (docs/06 section 12). There is no `rugsnipe-keeper` binary
(its jobs run inside `rugsnipe-hot`): never enable `rugsnipe-keeper.service`, it would restart-loop with `203/EXEC`
(docs/08 section 9 step 6). Phase 1 is fra1
alone until the gate passes. A second box comes in phase 2 (docs/03 section 4, docs/06 section 11: Tokyo TYO2 first, then Amsterdam AMS3).

## 1. Choose the provider

| Option | Price (list) | Notes |
|---|---|---|
| **TeraSwitch FRA** bare metal (Ryzen 9 9950X) | **~$425/mo** | Best building. It holds Jito FRA, Helius `fra-sender`/LaserStream, Nozomi, NextBlock, bloXroute, Triton and the DZ device `dz100a-fra2-tsw`. The price is from an aggregator (the vendor page returned 403), so get a quote |
| **Cherry Servers FRA** (EPYC 9375F 32-core) | **€989/mo or €1.69/h** | Same campus as TeraSwitch FRA2 (0.09 ms) and in stock: the immediate fallback (docs/03 section 4) |
| Latitude.sh FRA | ~$400/mo | A different building, 0.7 ms away: fails the in-building target below; last resort |
| Hetzner | excluded | Its terms have a crypto clause |
| Equinix Metal | gone | Closed June 2026 |

Ask for:
- x86_64 with ≥ 8 physical cores at high clock (9950X: 16C/32T);
- 64-128 GB RAM;
- 1-2 TB NVMe;
- a 10/25 GbE NIC with MSI-X (Intel or Mellanox);
- a **public IPv4 without NAT** (DoubleZero Edge requires this);
- ≥ 20 TB/mo or unmetered traffic (shred feeds are bursty);
- an IPMI/KVM console;
- Ubuntu 24.04 installed.

## 2. Test IP before you sign: accept only below 0.2 ms

Ask for a test IP or a trial server **in the same facility**. Then run:

```bash
test$ git clone YOUR-REPO-URL rugsnipe && cd rugsnipe     # YOUR-REPO-URL: this repository's clone URL (or scp deploy/latency_probe.sh)
test$ bash deploy/latency_probe.sh 30 | tee latency-fra-candidate.txt
test$ for ip in 64.130.50.88 64.130.50.91 198.13.140.105 198.13.140.227 198.13.140.16; do
        ping -c 100 -i 0.2 -q $ip | tail -2; done
test$ mtr -rwc 50 64.130.50.88          # path should stay inside AS20326 (TeraSwitch)
```

**Acceptance: average RTT < 0.2 ms (docs/03 section 4), max < 2 ms, 0% loss to all five targets.**

| Target | IP |
|---|---|
| Jito block engine Frankfurt | 64.130.50.88 |
| Helius `fra-sender` | 64.130.50.91 |
| Harmonic Frankfurt remote TPU | 198.13.140.105 |
| BAM Frankfurt node | 198.13.140.227 |
| DoubleZero device `fra2-tsw` | 198.13.140.16 |

These in-building targets are not measured yet (docs/research/critique-speed.md section 4, item 10). Your probe is the first
measurement. Keep `latency-fra-candidate.txt`. If a candidate box fails, it is not in the building: reject it.

### Fallback: AWS eu-central-1 (if bare metal is impossible)

- Instance: `c7i.4xlarge` or `c7i.2xlarge` (Intel), or `c8g.4xlarge` / `c8g.2xlarge` (Graviton4, aarch64; install.sh supports it). Take at least 2xlarge so the ENA gets its full queue count.
- An Elastic IP, a public subnet, and **no NAT gateway**. ENA Express and placement groups do nothing for traffic to leaders.
- Expect +0.5-2 ms compared with the TeraSwitch building. Re-run the probe. Accept below ~2.5 ms to the five targets.
- **DoubleZero Edge will most likely not work** (EC2 public IPs are 1:1 NAT, and DZ needs GRE/BGP/PIM). Helius shreds and the gRPC feeds do work.

## 3. Bring the box up

```bash
laptop$ ssh root@FRA1_IP                                        # FRA1_IP: the box's address
fra1# adduser --disabled-password ops && usermod -aG sudo ops   # then key-only SSH, PasswordAuthentication no
fra1# apt-get update && apt-get -y full-upgrade && reboot
ops@fra1$ git clone YOUR-REPO-URL ~/rugsnipe && cd ~/rugsnipe
ops@fra1$ bash deploy/install.sh --role hot --dry-run
ops@fra1$ sudo bash deploy/install.sh --role hot                # chrony -> ntp.frankfurt.jito.wtf
ops@fra1$ sudo /opt/rugsnipe/bin/tune-hot.sh --check
ops@fra1$ sudo /opt/rugsnipe/bin/tune-hot.sh --apply             # governor, irqbalance, sysctls, THP
# Paste the printed GRUB line (isolcpus/nohz_full/rcu_nocbs/irqaffinity + C-state cap) into
# /etc/default/grub GRUB_CMDLINE_LINUX_DEFAULT. Also set BIOS: C-states off, performance profile, SMT as you prefer.
ops@fra1$ sudo update-grub && sudo reboot
ops@fra1$ cat /sys/devices/system/cpu/isolated                  # must list the isolated cores
ops@fra1$ sudo bash ~/rugsnipe/deploy/install.sh --role hot     # re-pins rugsnipe-hot to them
ops@fra1$ sudo systemctl enable rugsnipe-tune.service
ops@fra1$ chronyc tracking                                       # |System time| well under 1 ms
```

Firewall (ufw):
- default deny incoming;
- allow 22/tcp from your IP and from wg0;
- allow 51820/udp from ohio1;
- allow 9464/tcp on wg0 only;
- open each feed port to its vendor's source IPs only.

## 4. Keys

Create the wallets **on fra1**, as `ops` with `sudo` (the same commands as docs/08 section 4). Never on a laptop,
never in git.

```bash
ops@fra1$ sudo install -d -m 0700 -o root -g root /etc/rugsnipe/keys
# a standard Solana keypair file without the Agave CLI; the pubkey goes into a public file (docs/08 section 4).
# Safe to paste twice: an existing key (and its pubkey file) is kept
ops@fra1$ if sudo test -e /etc/rugsnipe/keys/probe-keypair.json; then echo "probe key exists: kept (never replace it)"; else
  o=$(sudo /opt/rugsnipe/bin/rugsnipe-ctl armset keygen --out /etc/rugsnipe/keys/probe-keypair.json) && printf '%s\n' "$o" | sudo tee /etc/rugsnipe/probe-pubkey.txt; fi
ops@fra1$ sudo chmod 0400 /etc/rugsnipe/keys/probe-keypair.json
ops@fra1$ sudo bash ~/rugsnipe/deploy/install.sh --role hot     # re-checks: refuses group/world-readable keys
```

If `/etc/rugsnipe/probe-pubkey.txt` is ever empty, rebuild it from the key with the recovery line of docs/08
section 4 (`rugsnipe-ctl nonce create --dry-run` prints the probe wallet as its `authority`; it sends nothing).

- No `hot-keypair.json`: the live wallet is not used in this build (the units load the literal `ABSENT` without it).
- The probe config is `config/probe-a.example.toml` (docs/08 section 9 step 3). Put the pubkeys of the ArmSet signing key (which lives on ohio1) and of your cold wallet into its `[secrets]` in `/etc/rugsnipe/rugsnipe.toml`.
- The probe ArmSet is signed once per stage with `make_paper_armset.py --ver 101 --valid-hours 360` (15 days; the probe config's `armset_max_age_s = 1_296_000`), not the 6-hour default, into its own emptied directory (docs/08 section 9 step 1).
- Fund the probe wallet only for probe, one stage at a time (docs/08 section 9): about 3 SOL for probe-A at the shipped caps (0.6 SOL/day loss cap; the expected loss is about 0.19-0.31 SOL a day, so check the balance daily and top up with the hot unit stopped, docs/08 section 9 step 7b) plus about 0.04 SOL for the rent of the 32 nonce accounts (`rugsnipe-ctl nonce create`, docs/08 section 9 step 3c); 2-3 SOL for probe-B (1 SOL/day). The probe-A rate was decided on 2026-10-20 (docs/research/probe-design-v2.md section 2, item 4).
- Paper mode needs no keys. The units then load the literal `ABSENT`.

## 5. Feeds (in this order)

1. **Free (now):** `logsSubscribe` on mainnet-beta (`[feeds] logsws`). Paper only, slot +1 at best.
2. **Shadow: SVS Yellowstone gRPC, $150/mo.**
   - Buy at solanavibestation.com (pricing page / Discord). Locations are AMS/ATL, so take **AMS** (~7 ms from FRA).
   - Put the endpoint in `[feeds] grpc.url` (and `x_token = "GRPC_TOKEN"`); docs/08 section 9 step 0.
   - Put the token in `/etc/rugsnipe/keys/api-keys.toml` as `GRPC_TOKEN = "..."`.
   - Set `grpc.enabled = true` and keep `mode = "paper"`. This is the "shadow" phase: it measures detection lead against rivals' landing slots.
   - Paid alternative for probe (which needs `probe_live_require = "post_exec"`): **Helius LaserStream fra, $499-999/mo**, which is in-building.
3. **After the gate passes: DoubleZero Edge FRA seat, $1,500/mo.** It covers 53% of slots at hop 0.
   - Requirements: bare metal, x86_64 Ubuntu, a public IP without NAT.
   - Install the `doublezero` and `doublezero-solana` packages (docs.doublezero.xyz/solana/edge/subscribe).
   - Firewall: GRE (IP proto 47) out; BGP tcp/179 on 169.254.0.0/16 over `doublezero1`; PIM out; udp/7733 in on `doublezero1`; udp/44880 in on `doublezero0`.
   - Apply at doublezero.xyz/edge/subscribe with metro **FRA** and your DZ ID pubkey. There is no IP binding. Approval takes 1-3 business days, then an invoice arrives.
   - Connect: `doublezero connect multicast --subscribe-feed solana-shreds-full`.
   - Groups: the leader group, root `233.84.178.16:7733`, and the EU/APAC/AMER retransmit groups `233.84.178.12-.14`.
   - Packets arrive **GRE-encapsulated** on `doublezero1`. The `Shreds` feed must strip GRE and handle the Alpenglow markers and the 8-byte abort batch. It is our own decoder.
   - The terms of service forbid retransmitting the data.
4. **After the gate passes: Helius shred delivery, $1,000/IP/mo ($800 on Pro).** It covers every leader at hop ≤ 2 and is the complement for the ~47% of slots whose leaders do not publish to DZ.
   - In the Helius dashboard: Shreds → Add Seat. Pay by card or USDC.
   - Enter `FRA1_IP:PORT` (the box's address and a dedicated UDP port, for example 7734, separate from DZ's 7733).
   - Shreds arrive in about 5 s. A seat binds to one IP, and changing the IP issues a new token.
   - Open the port to Helius's source IPs only.

   Deduplicate per `(slot, fec_set, index)` and use eager Reed-Solomon recovery: any 32 of 64 shreds rebuild a set.
   Run a 7-day first-seen test per leader class before adding a third source (docs/03 section 5).

## 6. Paid senders: key list

Keys go in `/etc/rugsnipe/keys/api-keys.toml` (root 0400), loaded as the `api-keys` credential. The names match
`config/probe-a.example.toml`, which enables `helius`, `nozomi`, `astralane`, `blockrazor` and `jito` (docs/08
section 9 steps 2-3). A sender whose key name is missing from the file is refused at the dry run.

| Credential name | Service | Endpoint (FRA) | Min tip / cost | How to get it |
|---|---|---|---|---|
| `HELIUS_KEY` | Helius Sender | `http://fra-sender.helius-rpc.com/fast` | 0.001 SOL, or 5,000 lamports with `swqos_only=true` | Helius dashboard (a key gives 50 req/s; keyless is 1 req/s per IP) |
| `NOZOMI_KEY` | Nozomi (Temporal) | `http://fra2.nozomi.temporal.xyz` | 0.001 SOL | sign up at temporal.xyz |
| `ASTRALANE_KEY` | Astralane | `http://fr.gateway.astralane.io/irisb` | 0.001 SOL (0.00001 at VIP3) | astralane.io dashboard |
| `BLOCKRAZOR_KEY` | BlockRazor (auth token, header `apikey`), mode `fast` | `http://frankfurt.solana.blockrazor.xyz:443/sendTransaction` | free, tip min 0.0001 SOL, 3 TPS | the BlockRazor dashboard |
| (none) | Jito block engine | `frankfurt.mainnet.block-engine.jito.wtf` (+ amsterdam) | tip floor p50 ~0.00001 SOL, contested p90 ~0.04 SOL | keyless (1 req/s per IP per region) |
| `GRPC_TOKEN` | the gRPC feed: Triton Dragon's Mouth, SVS gRPC or Helius LaserStream | vendor endpoint | Triton pay-as-you-go / $150 / $499-999 per month | vendor dashboard (docs/08 section 9 step 0) |
| `RPC_URL` | the **paid** RPC endpoint with its key (Solana Vibe Station Basic or Helius Developer, never Helius Free) | vendor URL | plan price | vendor dashboard |
| `TG_TOKEN` | Telegram bot for alerts and `/kill` (the only alert path; required for the probe) | api.telegram.org | free | @BotFather |
| `TG_CHAT` | your Telegram chat id | | free | send your bot one message, then read your chat id (docs/08 section 9 step 2) |
| `NEXTBLOCK_KEY` | NextBlock (optional; off in the probe config) | `frankfurt:11100` (QUIC) | subscription $249+/mo | nextblock.io |

There is no dead-man key: the healthchecks.io ping is not built yet, so nothing reads one.

Example `api-keys.toml`, the same block as docs/08 section 9 step 2 (fill in each `...` with the value from the
provider's dashboard). Every comment on its own line, every value in double quotes; this file never leaves fra1:

```toml
# Helius Sender key: 50 req/s (keyless is 1 req/s per IP)
HELIUS_KEY = "..."
NOZOMI_KEY = "..."
ASTRALANE_KEY = "..."
# the BlockRazor auth token (header apikey)
BLOCKRAZOR_KEY = "..."
# the gRPC feed's x-token (docs/08 section 9 step 0)
GRPC_TOKEN = "..."
# the PAID RPC URL with its key (Solana Vibe Station Basic or Helius Developer; never Helius Free)
RPC_URL = "https://..."
# Telegram, the only alert path (required for the probe, docs/08 section 10)
TG_TOKEN = "..."
TG_CHAT = "..."
# only with a NextBlock plan:
# NEXTBLOCK_KEY = "..."
```

## 7. Acceptance summary

- [ ] Probe: < 0.2 ms to 64.130.50.88, 64.130.50.91, 198.13.140.105, 198.13.140.227 and 198.13.140.16 (AWS fallback: < ~2.5 ms).
- [ ] `install.sh --role hot` completes. `tune-hot.sh --check` shows no DIFF. `isolated` is non-empty after reboot.
- [ ] `chronyc tracking` offset < 1 ms against `ntp.frankfurt.jito.wtf`.
- [ ] Keys are in `/etc/rugsnipe/keys` with mode 0400 root, and install.sh accepts them.
- [ ] `sudo systemctl enable rugsnipe-guard.service rugsnipe-hot.service rugsnipe.target`, then `sudo systemctl start rugsnipe.target` (never enable `rugsnipe-keeper`: no binary). The guard and hot units reach `active (running)` in paper mode, and the watchdog does not fire.
- [ ] wg0 to ohio1 is up. Prometheus on 10.77.0.2:9464 is reachable from ohio1 only.
- [ ] A Telegram start message arrives (Telegram is the only alert path; the dead-man ping is not built yet). Touching `/run/rugsnipe/KILL` latches buys.
