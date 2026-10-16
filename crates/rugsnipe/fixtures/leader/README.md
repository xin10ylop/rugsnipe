# Leader schedule fixtures

Recorded from `https://api.mainnet-beta.solana.com` on 2026-10-16 (~10:18 UTC), JSON-RPC
responses verbatim except for trimming:

| file | method | trimming |
|---|---|---|
| `epoch_info.json` | `getEpochInfo` | none (epoch 1059, absolute slot 457,601,307, slot index 113,307) |
| `leader_schedule.json` | `getLeaderSchedule` (current epoch) | only slot indices 113,000-116,999 of the epoch (4,000 slots, 304 leaders) |
| `cluster_nodes.json` | `getClusterNodes` | the 303 leaders of that window found in gossip, plus 40 other nodes |

Client classes of the window (docs/03 section 6 rules, `deploy/stake_geo_v2.py`): BAM 1,436 slots,
Harmonic 1,152, Jito-Agave 1,064, Firedancer 256, other 92 (one leader is missing from the recorded
gossip and counts as other). Used by `tests/leader_schedule.rs`.
