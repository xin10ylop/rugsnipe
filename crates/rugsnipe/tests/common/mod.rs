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
    }
}

pub fn tmpdir(name: &str) -> PathBuf {
    let base = std::env::var_os("RUGSNIPE_TEST_TMP")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
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
