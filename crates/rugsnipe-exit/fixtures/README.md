# rugsnipe-exit fixtures

Conformance tapes for the exit engine (format and harness rules: `src/tape.rs` docs).

| dir | what | generator |
|---|---|---|
| `tapes/` | 20 tapes from real pump.fun trade histories (swap-api JSON cached in `data/cache/pumpapi`): reserves rebuilt from a fresh curve and checked against the API's `priceSol` on every trade (<= 0.5%), bundle = creator + create-slot buyers, our 0.1 SOL buy inserted at entry model E2, at most 300 trades after it. `MANIFEST.json` pins the mints | `python3 -I scripts/pumpapi_tapes.py` (`--repick` to choose new mints) |
| `tapes_synth/` | 12 seeded synthetic tapes for paths real histories rarely reach: failed fills, retries, escalation to dump bounds, spend-cap exhaustion, consolidation transfers, balance resyncs, completion + PumpSwap pool events, big buys | `python3 -I scripts/synth_tapes.py` |
| `expected/`, `expected_synth/` | the Rust outputs (`{"step","slot","ms","action":{...}}` lines, then `{"final":{...}}`) | `RUGSNIPE_BLESS=1 cargo test -p rugsnipe-exit --test tapes` |

`python3 -I scripts/exit_mirror.py` is an integer-only Python port of `decide` and the
harness; it must reproduce every expected file exactly (32/32 on 2026-10-14).
