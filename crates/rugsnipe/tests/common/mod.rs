//! Shared helpers: an in-memory feed and builders for creates and trades.
#![allow(dead_code)]

use pump_codec::constants::TOKEN_2022_PROGRAM_ID;
use pump_codec::curve::CurveState;
use pump_codec::{pda, Pubkey};
use rugsnipe::app::{self, App, AppOpts, ArmChoice};
use rugsnipe_core::armset::{ArmSet, OpEntry, OpState, WatchJson, WatchKind};
use rugsnipe_core::config::Config;
use rugsnipe_feed::{
    hit_flags as hf, CreateHitLite, Feed, FeedBuf, FeedEvent, FeedSrc, FeedStatus, Payload, Sig,
    TradeLite,
};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Prepared events handed out in order (no allocation in `poll`).
pub struct VecFeed {
    pub events: Vec<FeedEvent>,
    pub pos: usize,
    pub batch: usize,
    pub end_when_empty: bool,
}

impl VecFeed {
    pub fn new(events: Vec<FeedEvent>) -> Self {
        Self {
            events,
            pos: 0,
            batch: 1,
            end_when_empty: true,
        }
    }
}

impl Feed for VecFeed {
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        if self.pos >= self.events.len() {
            return if self.end_when_empty {
                FeedStatus::Ended
            } else {
                FeedStatus::Idle
            };
        }
        let mut n = 0;
        while n < self.batch && self.pos < self.events.len() && out.push(self.events[self.pos]) {
            self.pos += 1;
            n += 1;
        }
        FeedStatus::Ready(n)
    }
    fn src(&self) -> FeedSrc {
        FeedSrc::Replay
    }
}

pub fn sig(n: u64) -> Sig {
    let mut s = [7u8; 64];
    s[..8].copy_from_slice(&n.to_le_bytes());
    Sig(s)
}

pub fn ev(slot: u64, payload: Payload) -> FeedEvent {
    FeedEvent {
        src: FeedSrc::Replay,
        recv_ns: rugsnipe_feed::clock::mono_ns(),
        slot,
        bank_id: 0,
        payload,
    }
}

/// A SOL / Token-2022 create by `user` with one in-tx buy of `dev_buy` lamports.
pub fn create(
    mint: Pubkey,
    user: Pubkey,
    slot: u64,
    n: u64,
    dev_buy: u64,
) -> (CreateHitLite, CurveState) {
    let mut c = CurveState::fresh(user);
    let tok = c.tokens_for_sol(dev_buy);
    if dev_buy > 0 {
        c.apply_buy(tok, dev_buy);
    }
    let bc = pda::bonding_curve(&mint);
    (
        CreateHitLite {
            mint,
            bc,
            abc: pda::associated_bonding_curve(&mint, &TOKEN_2022_PROGRAM_ID),
            user,
            creator: user,
            token_program: TOKEN_2022_PROGRAM_ID,
            quote_mint: Pubkey::default(),
            mayhem: false,
            slot,
            sig: sig(n),
            intx_sol_in: dev_buy,
            intx_tokens_out: tok,
            intx_buys: (dev_buy > 0) as u8,
            v_after: c.virtual_sol,
            flags: hf::CREATOR_KNOWN | hf::TOKEN_PROGRAM_KNOWN | hf::QUOTE_KNOWN | hf::ABC_KNOWN,
        },
        c,
    )
}

/// Apply a buy (`sol` > 0) or a sell of `sell_tokens` to `c`; returns the TradeEvent.
pub fn trade(
    c: &mut CurveState,
    mint: Pubkey,
    user: Pubkey,
    slot: u64,
    n: u64,
    sol: u64,
    sell_tokens: u64,
) -> TradeLite {
    let (is_buy, sol_amount, token_amount) = if sell_tokens == 0 {
        let t = c.tokens_for_sol(sol);
        c.apply_buy(t, sol);
        (true, sol, t)
    } else {
        let out = c.sell_sol_out(sell_tokens);
        c.apply_sell(sell_tokens, out);
        (false, out, sell_tokens)
    };
    TradeLite {
        mint,
        user,
        is_buy,
        sol_amount,
        token_amount,
        fee: sol_amount * 125 / 10_000,
        virtual_sol_reserves: c.virtual_sol,
        virtual_token_reserves: c.virtual_token,
        slot,
        sig: sig(n),
        ordinal: 0,
        index: u32::MAX,
        tip: 0,
    }
}

pub fn armset(signers: &[Pubkey], creators: &[Pubkey]) -> ArmSet {
    let mut watch: Vec<WatchJson> = signers
        .iter()
        .map(|k| WatchJson {
            key: *k,
            kind: WatchKind::Signer,
            op_id: 1,
            rule_id: 0,
            expires_slot: None,
            keep: false,
        })
        .collect();
    watch.extend(creators.iter().map(|k| WatchJson {
        key: *k,
        kind: WatchKind::Creator,
        op_id: 1,
        rule_id: 0,
        expires_slot: None,
        keep: false,
    }));
    ArmSet {
        ver: 9,
        run_id: "test".into(),
        code_sha: String::new(),
        prereg_sha: String::new(),
        created_at: rugsnipe::now_unix(),
        valid_until: None,
        regime: "test".into(),
        min_hot_version: String::new(),
        gate_passed: false,
        probe_allowed: false,
        probe_stage: None,
        shared_fee_payers: vec![],
        operators: vec![OpEntry {
            op_id: 1,
            state: OpState::Armed,
            size_lamports: 100_000_000,
            v_max_lamports: 40_000_000_000,
            ev_cap_lamports: 0,
            ab_skip_p: 0.0,
            family: None,
            exit: None,
            stats: None,
        }],
        watch,
        funding_rules: vec![],
        match_all: None,
    }
}

pub fn tmpdir(name: &str) -> PathBuf {
    let base = std::env::var_os("RUGSNIPE_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    // a real mode's kill file must sit under /run/rugsnipe/; the suite's
    // live in its temp directories (the same value from every test thread,
    // inherited by the spawned binaries)
    std::env::set_var("RUGSNIPE_KILL_DIR", &base);
    let d = base.join(format!(
        "rugsnipe-it-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn paper_cfg(dir: &std::path::Path) -> Config {
    let mut cfg = Config::default();
    rugsnipe::setup::paper_caps(&mut cfg.risk);
    cfg.risk.kill_file = dir.join("KILL").display().to_string();
    cfg.box_.id = "test".into();
    cfg
}

pub fn build(
    cfg: Config,
    set: ArmSet,
    dir: &std::path::Path,
    fill_timeout_slots: u64,
    lease: bool,
) -> App {
    app::build(AppOpts {
        cfg,
        armset: ArmChoice::Memory(Box::new(set)),
        regime: None,
        paper_unsigned: false,
        state_path: Some(dir.join("state.db")),
        rslog_dir: dir.join("rslog"),
        logsws: None,
        healths: vec![],
        e2: None,
        metrics: false,
        fill_timeout_slots,
        lease,
        sigusr1: Arc::new(AtomicBool::new(false)),
        paper_balance: 5_000_000_000,
        live_flag: false,
        real: None,
        post_exec: None,
        leader_rpc: None,
        alerts: None,
        trigger_tuning: None,
    })
    .expect("paper app builds without keys")
}

/// Every rslog record in `dir` (all streams).
pub fn rslog_records(dir: &std::path::Path) -> Vec<rugsnipe_core::rslog::Rec> {
    let mut out = vec![];
    for e in std::fs::read_dir(dir).unwrap() {
        let b = std::fs::read(e.unwrap().path()).unwrap();
        let (recs, end) = rugsnipe_core::rslog::decode_frames(&b);
        assert_eq!(end, b.len(), "rslog file decodes to the end");
        out.extend(recs.into_iter().map(|(_, r)| r));
    }
    out
}

pub fn count(c: &rusqlite::Connection, sql: &str) -> i64 {
    c.query_row(sql, [], |r| r.get(0)).unwrap()
}

// ------------------------------------------------- process-level harness
//
// The real `rugsnipe-hot` binary fed real recorded frames through the
// collector relay (a Unix socket served by the test), in paper mode.

/// The rugsnipe-feed fixture (22 live `logsSubscribe` frames, 20 creates)
/// as relay lines `<recv_unix_ns>\t<frame>\n`, plus the signer of every
/// create in it.
pub fn fixture_relay() -> (String, Vec<Pubkey>) {
    let p = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../rugsnipe-feed/fixtures/logs_frames.jsonl"
    );
    let text = std::fs::read_to_string(p).unwrap();
    let mut lines = String::new();
    for l in text.lines() {
        let v: serde_json::Value = serde_json::from_str(l).unwrap();
        lines.push_str(&format!("{}\t{}\n", v["recv_unix_ns"], v["frame"]));
    }
    let users = rugsnipe_feed::replay::Replay::from_text(&text, false)
        .events()
        .iter()
        .filter_map(|e| match e.payload {
            Payload::Hit(h) => Some(h.user),
            _ => None,
        })
        .collect();
    (lines, users)
}

/// Serve `lines` to the first relay connection on `sock`, then keep the
/// connection open until `stop`.
pub fn serve_relay(
    sock: &std::path::Path,
    lines: String,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()> {
    use std::io::Write;
    let listener = std::os::unix::net::UnixListener::bind(sock).unwrap();
    listener.set_nonblocking(true).unwrap();
    std::thread::spawn(move || {
        let mut conn = None;
        while !stop.load(std::sync::atomic::Ordering::Acquire) {
            if conn.is_none() {
                if let Ok((mut c, _)) = listener.accept() {
                    c.set_nonblocking(false).unwrap();
                    let _ = c.write_all(lines.as_bytes());
                    conn = Some(c);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    })
}

/// A paper TOML: relay feed only, metrics off, every real sender enabled
/// and pointed at `ports` (rpcsend, helius, nozomi, astralane, jito,
/// nextblock).
pub fn paper_toml(dir: &std::path::Path, sock: &std::path::Path, ports: [u16; 6]) -> String {
    format!(
        r#"
[box]
id = "proc"
[mode]
mode = "paper"
[secrets]
api_keys = "api-keys"
[risk]
max_snipe_lamports = 250_000_000
max_probe_lamports = 20_000_000
max_probes_per_day = 20
max_open_positions = 64
max_daily_loss_lamports = 1_000_000_000
max_consec_losses_per_op = 5
max_buys_per_op_per_day = 100
max_buys_per_hour = 100
max_bid_lamports = 25_000_000
max_sell_spend_lamports = 20_000_000
max_entry_vsol_lamports = 40_000_000_000
max_hot_balance_lamports = 5_000_000_000
min_hot_balance_lamports = 1_000_000_000
kill_file = "{kill}"
[feeds]
logsws = {{ enabled = false }}
relay = {{ enabled = true, socket = "{sock}" }}
pumpportal = {{ enabled = false }}
[senders]
paper = {{ enabled = true }}
rpcsend = {{ enabled = true, url = "http://127.0.0.1:{p0}", sells_only = false }}
helius = {{ enabled = true, url = "http://127.0.0.1:{p1}/fast", swqos_only = true, min_tip = 5_000 }}
nozomi = {{ enabled = true, url = "http://127.0.0.1:{p2}", key = "NOZOMI_KEY", min_tip = 1_000_000 }}
astralane = {{ enabled = true, url = "http://127.0.0.1:{p3}", key = "ASTRALANE_KEY" }}
jito = {{ enabled = true, url = "http://127.0.0.1:{p4}", max_rps = 1 }}
nextblock = {{ enabled = true, url = "http://127.0.0.1:{p5}", key = "NEXTBLOCK_KEY" }}
[obs]
metrics_listen = []
rslog_dir = "{rslog}"
"#,
        kill = dir.join("KILL").display(),
        sock = sock.display(),
        rslog = dir.join("rslog").display(),
        p0 = ports[0],
        p1 = ports[1],
        p2 = ports[2],
        p3 = ports[3],
        p4 = ports[4],
        p5 = ports[5],
    )
}

/// `rugsnipe-hot` in paper mode on `cfg` with an unsigned ArmSet file,
/// no E2 lane, no metrics, unpinned.
pub fn hot_cmd(
    cfg: &std::path::Path,
    armset: &std::path::Path,
    state: &std::path::Path,
    duration_s: u64,
) -> std::process::Command {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_rugsnipe-hot"));
    c.arg("--config")
        .arg(cfg)
        .arg("--armset")
        .arg(armset)
        .arg("--paper-unsigned-armset")
        .arg("--state")
        .arg(state)
        .arg("--duration-s")
        .arg(duration_s.to_string())
        .arg("--no-e2")
        .arg("--no-metrics")
        .arg("--pin-core")
        .arg("none");
    c
}

/// `decisions` count for `label` in a run summary.
pub fn decision(summary: &serde_json::Value, label: &str) -> u64 {
    summary["decisions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|d| d[0] == label)
        .map(|d| d[1].as_u64().unwrap())
        .unwrap_or(0)
}

// ------------------------------------------------- mock JSON-RPC / chain

/// A JSON-RPC server on 127.0.0.1 (HTTP/1.1 keep-alive, one thread per
/// connection): `handler(method, params)` returns the `result` (or `Err` for
/// a JSON-RPC error). Returns the URL and the method log.
pub fn serve_rpc<F>(handler: F) -> (String, Arc<std::sync::Mutex<Vec<String>>>)
where
    F: Fn(&str, &serde_json::Value) -> Result<serde_json::Value, String> + Send + Sync + 'static,
{
    use std::io::{BufRead, BufReader, Read, Write};
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", l.local_addr().unwrap());
    let log = Arc::new(std::sync::Mutex::new(vec![]));
    let handler = Arc::new(handler);
    let log2 = log.clone();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let Ok(s) = s else { continue };
            let (h, log) = (handler.clone(), log2.clone());
            std::thread::spawn(move || {
                let mut w = s.try_clone().unwrap();
                let mut r = BufReader::new(s);
                loop {
                    let mut len = 0usize;
                    let mut first = true;
                    loop {
                        let mut line = String::new();
                        if r.read_line(&mut line).unwrap_or(0) == 0 {
                            return;
                        }
                        if first && line.trim().is_empty() {
                            continue;
                        }
                        first = false;
                        if line == "\r\n" {
                            break;
                        }
                        if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                            len = v.trim().parse().unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; len];
                    if r.read_exact(&mut body).is_err() {
                        return;
                    }
                    let req: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
                    let method = req["method"].as_str().unwrap_or("").to_string();
                    log.lock().unwrap().push(method.clone());
                    let resp = match h(&method, &req["params"]) {
                        Ok(v) => {
                            serde_json::json!({"jsonrpc": "2.0", "id": req["id"], "result": v})
                        }
                        Err(e) => serde_json::json!({"jsonrpc": "2.0", "id": req["id"],
                            "error": {"code": -32000, "message": e}}),
                    }
                    .to_string();
                    let out = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{resp}",
                        resp.len()
                    );
                    if w.write_all(out.as_bytes()).is_err() {
                        return;
                    }
                }
            });
        }
    });
    (url, log)
}

/// `Program data:` line of a TradeEvent (2024 layout) on `c` after the trade.
pub fn trade_log_line(
    mint: &Pubkey,
    user: &Pubkey,
    buy: bool,
    sol: u64,
    tok: u64,
    c: &CurveState,
) -> String {
    use base64::Engine as _;
    let mut d = pump_codec::constants::event::TRADE.to_vec();
    d.extend_from_slice(mint.as_ref());
    d.extend_from_slice(&sol.to_le_bytes());
    d.extend_from_slice(&tok.to_le_bytes());
    d.push(buy as u8);
    d.extend_from_slice(user.as_ref());
    d.extend_from_slice(&0i64.to_le_bytes());
    d.extend_from_slice(&c.virtual_sol.to_le_bytes());
    d.extend_from_slice(&c.virtual_token.to_le_bytes());
    format!(
        "Program data: {}",
        base64::engine::general_purpose::STANDARD.encode(d)
    )
}

/// A `getTransaction` (jsonParsed) result: `signers` sign, the logs carry
/// `logs` (TradeEvent lines), `err` is `meta.err`.
pub fn tx_json(
    slot: u64,
    signers: &[Pubkey],
    logs: &[String],
    err: serde_json::Value,
    fee: u64,
) -> serde_json::Value {
    let mut all = vec![format!(
        "Program {} invoke [1]",
        pump_codec::constants::PUMP_PROGRAM_ID
    )];
    all.extend(logs.iter().cloned());
    all.push(format!(
        "Program {} success",
        pump_codec::constants::PUMP_PROGRAM_ID
    ));
    let keys: Vec<serde_json::Value> = signers
        .iter()
        .map(|k| serde_json::json!({"pubkey": k.to_string(), "signer": true, "writable": true}))
        .collect();
    serde_json::json!({"slot": slot, "blockTime": 1_791_000_000,
        "meta": {"err": err, "fee": fee, "logMessages": all},
        "transaction": {"message": {"accountKeys": keys}}})
}

/// An initialized nonce account (80 bytes) of `authority` holding `value`.
pub fn nonce_account_data(authority: &Pubkey, value: [u8; 32]) -> Vec<u8> {
    let mut d = vec![];
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(authority.as_ref());
    d.extend_from_slice(&value);
    d.extend_from_slice(&5_000u64.to_le_bytes());
    d
}

/// Sign `set` with `key` into `dir` as `armset-<ver>-<sha>.json` + `.sig`
/// (what `rugsnipe-ctl armset sign` writes); returns the JSON path.
pub fn signed_armset(
    dir: &std::path::Path,
    set: &ArmSet,
    key: &rugsnipe_tx::signer::HotSigner,
) -> PathBuf {
    let bytes = serde_json::to_vec(set).unwrap();
    let sha = rugsnipe_core::armset::sha256_hex(&bytes);
    let p = dir.join(format!("armset-{}-{sha}.json", set.ver));
    std::fs::write(&p, &bytes).unwrap();
    std::fs::write(
        format!("{}.sig", p.display()),
        bs58::encode(key.sign(&bytes)).into_string(),
    )
    .unwrap();
    p
}

/// A solana-keygen JSON keypair of `seed`.
pub fn keypair_json(seed: u8) -> String {
    let s = rugsnipe_tx::signer::HotSigner::from_seed(&zeroize::Zeroizing::new([seed; 32]));
    let mut all = vec![seed; 32];
    all.extend_from_slice(s.pubkey().as_ref());
    format!(
        "[{}]",
        all.iter()
            .map(|b| b.to_string())
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// Write `text` at `dir/name` with `mode`.
pub fn write_mode(dir: &std::path::Path, name: &str, text: &str, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let p = dir.join(name);
    std::fs::write(&p, text).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Wire events (gRPC shape: transaction + meta trailer) handed out in order
/// through `push_wire_parts` (no allocation in `poll`).
pub struct WireFeed {
    pub items: Vec<(FeedEvent, Vec<u8>, usize)>,
    pub pos: usize,
}

impl WireFeed {
    /// `(slot, tx bytes, meta trailer)` per event, source gRPC.
    pub fn new(items: Vec<(u64, Vec<u8>, Vec<u8>)>) -> Self {
        Self {
            items: items
                .into_iter()
                .map(|(slot, tx, meta)| {
                    let n = meta.len();
                    let mut b = tx;
                    b.extend_from_slice(&meta);
                    (
                        FeedEvent {
                            src: FeedSrc::Grpc,
                            ..ev(slot, Payload::Hit(CreateHitLite::default()))
                        },
                        b,
                        n,
                    )
                })
                .collect(),
            pos: 0,
        }
    }
}

impl Feed for WireFeed {
    fn poll(&mut self, out: &mut FeedBuf) -> FeedStatus {
        let Some((e, b, m)) = self.items.get(self.pos) else {
            return FeedStatus::Ended;
        };
        if out.push_wire_parts(e.src, e.recv_ns, e.slot, e.bank_id, [b, &[]], *m) {
            self.pos += 1;
            FeedStatus::Ready(1)
        } else {
            FeedStatus::Full
        }
    }
    fn src(&self) -> FeedSrc {
        FeedSrc::Grpc
    }
}

/// A pump-codec transaction fixture.
pub fn codec_fixture(name: &str) -> serde_json::Value {
    let p = format!(
        "{}/../pump-codec/fixtures/tx/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

/// The wire bytes and the gRPC meta trailer the adapter would build for a
/// fixture transaction (through the same protobuf conversion).
pub fn fixture_wire(f: &serde_json::Value, index: u32) -> (Vec<u8>, Vec<u8>) {
    use rugsnipe_feed::grpc::convert::{meta_in, tx_to_wire};
    use yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof;
    let up = rugsnipe_feed::grpc::mock::fixture_update(f, 1, index as u64, &["creates"]).unwrap();
    let Some(UpdateOneof::Transaction(t)) = up.update_oneof else {
        panic!("a transaction update")
    };
    let info = t.transaction.unwrap();
    let wire = tx_to_wire(info.transaction.as_ref().unwrap()).unwrap();
    let mut meta = vec![];
    rugsnipe_feed::wiremeta::encode(&meta_in(info.meta.as_ref(), Some(index)), &mut meta);
    (wire, meta)
}

/// The `logsNotification` frame of a fixture transaction (the logs path's copy).
pub fn fixture_logs_frame(f: &serde_json::Value, slot: u64) -> String {
    let r = &f["result"];
    serde_json::json!({"jsonrpc": "2.0", "method": "logsNotification", "params": {
        "result": {"context": {"slot": slot}, "value": {
            "signature": f["signature"].as_str().unwrap_or_else(|| r["transaction"]["signatures"][0].as_str().unwrap()),
            "err": r["meta"]["err"], "logs": r["meta"]["logMessages"]}},
        "subscription": 1}})
    .to_string()
}
