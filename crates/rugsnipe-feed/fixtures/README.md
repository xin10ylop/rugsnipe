# rugsnipe-feed fixtures

Offline test data (the tests never touch the network). Regenerate with
`python3 -I crates/rugsnipe-feed/scripts/fetch_fixtures.py <funding|frames|relay>`.

| file | what | source |
|---|---|---|
| `logs_frames.jsonl` | 22 live `logsSubscribe(mentions: TSLvdd…, processed)` frames in the recorder format (`{"recv_ns","recv_unix_ns","src","frame"}`): subscription acks, failed transactions, creates with 0, 1 and 2+ in-tx buys, mayhem creates | `examples/tail.rs --record` against `wss://api.mainnet-beta.solana.com`, 2026-10-14 ~11:00 UTC, slots 456,813,213-456,815,959 |
| `relay_pp.txt` | PumpPortal `subscribeNewToken` frames in the collector relay line format (`<recv_unix_ns>\t<frame>`): an ack, two creates (one mayhem) and a duplicate | `analysis/rugsnipe_analysis/testdata/collector_frames.json` (live frames recorded by the collector selftest) |
| `funding_get_transaction.json` | `getTransaction(jsonParsed, confirmed)` of a system transfer of 10.248 SOL sent by the Binance hot wallet `5tzFki…` (the funding-watcher example of docs/06 section 9.6), with the query | `https://api.mainnet-beta.solana.com`, 2026-10-14 |

The pump-codec transaction fixtures (`../pump-codec/fixtures/tx/*.json`) are reused: their
`meta.logMessages` are wrapped into `logsNotification` frames by `tests/fixtures.rs`, which
also cross-checks every decoded create against pump-codec's own event decoder.
