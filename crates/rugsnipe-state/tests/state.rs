//! StateStore, Ledger, Lease and Recovery against a real SQLite file.

use pump_codec::Pubkey;
use rugsnipe_state::chain::{ChainRead, TokenBalance};
use rugsnipe_state::lease::{self, Acquire};
use rugsnipe_state::recovery::{load_nonces, load_open_positions, RecoverCtx};
use rugsnipe_state::*;
use std::path::PathBuf;

/// A fresh directory under the system temp dir (removed when the guard drops).
struct Tmp(PathBuf);
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn tmp_guarded(name: &str) -> (Tmp, PathBuf) {
    let p = tmp(name);
    (Tmp(p.parent().unwrap().to_path_buf()), p)
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "rugsnipe-state-{}-{}-{}",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&d).unwrap();
    d.join("state.db")
}

fn intent(mint: Pubkey, mode: &str, state: IntentState) -> IntentRow {
    IntentRow {
        intent_id: rugsnipe_core::IntentId::new(&mint, &Pubkey::new_from_array([9; 32])).as_i64(),
        mint,
        op_id: 7,
        box_id: "test".into(),
        mode: mode.into(),
        armset_ver: 3,
        slot: 456_800_000,
        bank_id: 0,
        via: Via::Signer,
        feed_class: "logsws".into(),
        spend: 100_000_000,
        v_max: 40_000_000_000,
        min_tokens_out: 1_000,
        nonce_idx: 5,
        nonce_value: [1; 32],
        decided_ns: 42,
        state,
    }
}

fn position(mint: Pubkey, tokens: u64, state: PosState) -> PositionRow {
    PositionRow {
        mint,
        intent_id: 1,
        opened_slot: 10,
        tokens,
        cost: 100,
        v_fill: 31_000_000_000,
        peak_v: 31_000_000_000,
        bundle_json: "[]".into(),
        policy_json: "{}".into(),
        origin: Origin::Intent,
        state,
        sell_spend: 0,
        closed_slot: None,
    }
}

fn count(c: &rusqlite::Connection, t: &str) -> i64 {
    c.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn schema_has_every_spec_table_and_column() {
    let (_tmp, p) = tmp_guarded("schema");
    let s = StateStore::open(&p).unwrap();
    let c = s.reader().unwrap();
    let want: &[(&str, &str)] = &[
        ("intents", "intent_id,mint,op_id,box,mode,armset_ver,slot,bank_id,via,feed_class,spend,v_max,min_tokens_out,nonce_idx,nonce_value,decided_ns,state"),
        ("sends", "sig,intent_id,variant,sender,prio,tip,sent_ns,result"),
        ("fills", "sig,intent_id,mint,slot,tx_index,tokens,sol_delta,err,buyers_ahead,rival_sol_ahead,late,fail_reason,fail_cost"),
        ("positions", "mint,intent_id,opened_slot,tokens,cost,v_fill,peak_v,bundle_json,policy_json,origin,state,sell_spend,closed_slot"),
        ("exits", "sig,mint,reason,venue,tokens,sol_delta,slot,owner,try_no"),
        ("nonces", "pubkey,pool,idx,value,last_change_slot,state,intent_id,since_slot"),
        ("watch_dyn", "key,kind,op_id,rule_id,added_slot,expires_unix,remove_if_no_launch_unix,keep,parent,state"),
        ("risk", "day,start_balance,deposits,sweeps,realized,buys,killed,kill_reason"),
        ("op_risk", "op_id,consec_losses,buys_today,disarmed_until"),
        ("lease", "name,owner,hb_unix_ms,epoch"),
        ("kv", "k,v"),
        ("shadow", "intent_id,mint,model,outcome,create_slot,create_index,block_txs,bundle_trades,bundle_wallets,our_index,tp_ahead,tp_behind,s0_failed,premium_bps,post_bundle_v,post_bundle_t,post_bundle_mcap,cost"),
    ];
    for (t, cols) in want {
        let mut st = c.prepare(&format!("PRAGMA table_info({t})")).unwrap();
        let got: Vec<String> = st
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .map(|x| x.unwrap())
            .collect();
        assert_eq!(got.join(","), *cols, "table {t}");
    }
    let mode: String = c
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
}

#[test]
fn ledger_flow_and_intent_state_machine() {
    let (_tmp, p) = tmp_guarded("ledger");
    let s = StateStore::open(&p).unwrap();
    let l = Ledger::new(s.writer());
    let mint = Pubkey::new_unique();
    let me = Pubkey::new_unique();
    let i = intent(mint, "paper", IntentState::Paper);
    let id = i.intent_id;
    l.intent(i.clone());
    l.intent(i.clone()); // duplicate (mint, mode): ignored
    l.sent(SendRow {
        sig: [3; 64],
        intent_id: id,
        variant: 0,
        sender: "paper".into(),
        prio: 2_380_000,
        tip: 1_020_000,
        sent_ns: 50,
        result: "recorded".into(),
    });
    l.fill(
        FillRow {
            sig: [3; 64],
            intent_id: id,
            mint,
            slot: 456_800_001,
            tx_index: None,
            tokens: 3_000_000_000_000,
            sol_delta: -101_250_000,
            err: None,
            buyers_ahead: 4,
            rival_sol_ahead: 2_000_000_000,
            late: false,
            fail_reason: None,
            fail_cost: 0,
        },
        None,
    );
    l.position(position(mint, 3_000_000_000_000, PosState::Open));
    l.exit(ExitRow {
        sig: [4; 64],
        mint,
        reason: "tp0".into(),
        venue: "curve".into(),
        tokens: 3_000_000_000_000,
        sol_delta: 150_000_000,
        slot: 456_800_100,
        owner: me,
        try_no: 1,
    });
    let mut closed = position(mint, 0, PosState::Closed);
    closed.closed_slot = Some(456_800_100);
    l.position(closed);
    // paper intents never move
    l.fill(
        FillRow {
            sig: [5; 64],
            intent_id: id,
            mint,
            slot: 1,
            tx_index: None,
            tokens: 0,
            sol_delta: 0,
            err: Some("x".into()),
            buyers_ahead: 0,
            rival_sol_ahead: 0,
            late: false,
            fail_reason: None,
            fail_cost: 0,
        },
        Some(IntentState::Landed),
    );
    // real intent: sent -> landed ok, landed -> burned refused
    let mint2 = Pubkey::new_unique();
    let r = intent(mint2, "live", IntentState::Sent);
    let id2 = r.intent_id;
    l.intent(r);
    l.writer().send(Write::IntentState {
        intent_id: id2,
        to: IntentState::Landed,
    });
    l.burned(id2);
    // bad CHECK value -> counted error, nothing written
    let mut bad = intent(Pubkey::new_unique(), "paper", IntentState::Paper);
    bad.intent_id += 1;
    l.writer().send(Write::Kv("armset_ver".into(), "3".into()));
    assert!(l.flush());
    let c = s.reader().unwrap();
    assert_eq!(count(&c, "intents"), 2);
    assert_eq!(count(&c, "sends"), 1);
    assert_eq!(count(&c, "fills"), 2);
    assert_eq!(count(&c, "positions"), 1);
    assert_eq!(count(&c, "exits"), 1);
    let st = |id: i64| -> String {
        c.query_row(
            "SELECT state FROM intents WHERE intent_id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    };
    assert_eq!(st(id), "paper");
    assert_eq!(st(id2), "landed");
    assert_eq!(
        s.stats()
            .refused_transitions
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    let pstate: String = c
        .query_row("SELECT state FROM positions", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pstate, "closed");
    assert_eq!(
        rugsnipe_state::store::kv_get(&c, "armset_ver")
            .unwrap()
            .as_deref(),
        Some("3")
    );
    let counts = rugsnipe_state::schema::row_counts(&c).unwrap();
    assert_eq!(counts.len(), 12);
    drop(c);
    // durable across reopen
    assert!(s.close() >= 8);
    let s2 = StateStore::open(&p).unwrap();
    assert_eq!(count(&s2.reader().unwrap(), "intents"), 2);
}

/// A version-1 file (the DDL before the shadow record) upgrades in place:
/// rows kept, failed-buy columns added, `via = 'class'` accepted.
#[test]
fn version_1_file_migrates_to_version_2() {
    let (_tmp, p) = tmp_guarded("migrate");
    {
        let c = rusqlite::Connection::open(&p).unwrap();
        c.execute_batch(
            "CREATE TABLE intents(intent_id INTEGER PRIMARY KEY, mint TEXT NOT NULL, op_id INT, box TEXT, mode TEXT, armset_ver INT, slot INT,
               bank_id INT, via TEXT CHECK(via IN('signer','fee_payer','funding')), feed_class TEXT, spend INT, v_max INT,
               min_tokens_out INT, nonce_idx INT, nonce_value TEXT, decided_ns INT,
               state TEXT CHECK(state IN('paper','sent','landed','failed','burned','late_fill')), UNIQUE(mint, mode));
             CREATE TABLE fills(sig TEXT PRIMARY KEY, intent_id INT, mint TEXT, slot INT, tx_index INT, tokens INT, sol_delta INT, err TEXT,
               buyers_ahead INT, rival_sol_ahead INT, late INT);
             INSERT INTO intents(intent_id, mint, via, mode, state) VALUES(1, 'm1', 'signer', 'paper', 'paper');
             INSERT INTO fills(sig, intent_id, mint) VALUES('s1', 1, 'm1');
             PRAGMA user_version = 1;",
        )
        .unwrap();
    }
    let s = StateStore::open(&p).unwrap();
    let c = s.reader().unwrap();
    let v: i64 = c
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(v, schema::VERSION);
    assert_eq!(count(&c, "intents"), 1);
    assert_eq!(count(&c, "fills"), 1);
    assert!(schema::has_column(&c, "fills", "fail_reason").unwrap());
    let l = Ledger::new(s.writer());
    let mint = Pubkey::new_unique();
    let mut r = intent(mint, "paper", IntentState::Paper);
    r.via = Via::Class;
    l.intent(r.clone());
    l.fill(
        FillRow {
            sig: [8; 64],
            intent_id: r.intent_id,
            mint,
            slot: 456_800_000,
            tx_index: None,
            tokens: 0,
            sol_delta: -3_000_000,
            err: Some("premium".into()),
            buyers_ahead: 2,
            rival_sol_ahead: 0,
            late: false,
            fail_reason: Some("premium".into()),
            fail_cost: 3_000_000,
        },
        None,
    );
    l.shadow(ShadowRow {
        intent_id: r.intent_id,
        mint,
        model: "e1".into(),
        outcome: "premium".into(),
        create_slot: 456_800_000,
        create_index: Some(410),
        block_txs: Some(1_300),
        bundle_trades: 4,
        bundle_wallets: 3,
        our_index: Some(416),
        tp_ahead: 1,
        tp_behind: 3,
        s0_failed: 2,
        premium_bps: 310,
        post_bundle_v: 33_000_000_000,
        post_bundle_t: 975_000_000_000_000,
        post_bundle_mcap: 33_800_000_000,
        cost: 3_000_000,
    });
    l.flush();
    let c = s.reader().unwrap();
    let via: String = c
        .query_row(
            "SELECT via FROM intents WHERE mint = ?1",
            [mint.to_string()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(via, "class");
    let (reason, cost): (String, i64) = c
        .query_row(
            "SELECT fail_reason, fail_cost FROM fills WHERE intent_id = ?1",
            [r.intent_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((reason.as_str(), cost), ("premium", 3_000_000));
    let (outcome, ours): (String, i64) = c
        .query_row(
            "SELECT outcome, our_index FROM shadow WHERE intent_id = ?1",
            [r.intent_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!((outcome.as_str(), ours), ("premium", 416));
    // migrating again is a no-op
    schema::migrate(&c).unwrap();
    assert_eq!(count(&c, "intents"), 2);
}

#[test]
fn check_constraints_reject_bad_rows() {
    let (_tmp, p) = tmp_guarded("check");
    let s = StateStore::open(&p).unwrap();
    let c = s.reader().unwrap();
    let err = c.execute(
        "INSERT INTO intents(intent_id, mint, via, state) VALUES(1, 'm', 'creator_arg', 'paper')",
        [],
    );
    assert!(err.is_err(), "creator_arg is never a via (shadow only)");
    let err = c.execute(
        "INSERT INTO positions(mint, origin, state) VALUES('m', 'intent', 'pending')",
        [],
    );
    assert!(err.is_err());
}

#[test]
fn exits_lease_heartbeat_and_takeover() {
    let (_tmp, p) = tmp_guarded("lease");
    let s = StateStore::open(&p).unwrap();
    let hot_c = s.reader().unwrap();
    let guard_c = s.reader().unwrap();
    let mut hot = Lease::new("exits", "hot:fra1");
    let mut guard = Lease::new("exits", "guard:fra1");
    let t0 = 1_000_000;
    assert_eq!(
        hot.acquire(&hot_c, t0, 5_000).unwrap(),
        Acquire::Held {
            epoch: 1,
            took_over_from: None
        }
    );
    assert!(hot.renew(&hot_c, t0 + 1_000).unwrap());
    // fresh: guard cannot take it
    assert!(matches!(
        guard.acquire(&guard_c, t0 + 4_000, 5_000).unwrap(),
        Acquire::Busy(_)
    ));
    // stale (> 5 s since the last heartbeat): CAS takeover with epoch + 1
    match guard.acquire(&guard_c, t0 + 6_500, 5_000).unwrap() {
        Acquire::Held {
            epoch,
            took_over_from,
        } => {
            assert_eq!(epoch, 2);
            assert_eq!(took_over_from.as_deref(), Some("hot:fra1"));
        }
        a => panic!("{a:?}"),
    }
    // the old holder's renew fails: it must stop selling
    assert!(!hot.renew(&hot_c, t0 + 6_600).unwrap());
    assert!(!hot.is_held());
    let row = lease::read(&hot_c, "exits").unwrap().unwrap();
    assert_eq!((row.owner.as_str(), row.epoch), ("guard:fra1", 2));
    // release makes it immediately available
    guard.release(&guard_c).unwrap();
    assert!(matches!(
        hot.acquire(&hot_c, t0 + 6_700, 5_000).unwrap(),
        Acquire::Held { epoch: 3, .. }
    ));
}

struct MockChain {
    slot: u64,
    balances: Vec<TokenBalance>,
    accounts: std::collections::HashMap<Pubkey, Vec<u8>>,
}

impl ChainRead for MockChain {
    fn slot(&self) -> std::result::Result<u64, String> {
        Ok(self.slot)
    }
    fn token_balances(&self, _owner: &Pubkey) -> std::result::Result<Vec<TokenBalance>, String> {
        Ok(self.balances.clone())
    }
    fn accounts(&self, keys: &[Pubkey]) -> std::result::Result<Vec<Option<Vec<u8>>>, String> {
        Ok(keys.iter().map(|k| self.accounts.get(k).cloned()).collect())
    }
}

fn nonce_data(authority: &Pubkey, value: [u8; 32]) -> Vec<u8> {
    let mut d = vec![];
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(&1u32.to_le_bytes());
    d.extend_from_slice(authority.as_ref());
    d.extend_from_slice(&value);
    d.extend_from_slice(&5_000u64.to_le_bytes());
    d
}

#[test]
fn recovery_reconciles_positions_nonces_and_orphans() {
    let (_tmp, p) = tmp_guarded("recover");
    let s = StateStore::open(&p).unwrap();
    let l = Ledger::new(s.writer());
    let (gone, resized, kept, orphan) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    l.position(position(gone, 100, PosState::Open));
    l.position(position(resized, 100, PosState::Exiting));
    l.position(position(kept, 77, PosState::Open));
    l.position(position(Pubkey::new_unique(), 0, PosState::Closed));
    let auth = Pubkey::new_unique();
    let (n0, n1, n2) = (
        Pubkey::new_unique(),
        Pubkey::new_unique(),
        Pubkey::new_unique(),
    );
    for (i, k) in [n0, n1].iter().enumerate() {
        l.nonce(NonceRow {
            pubkey: *k,
            pool: "buy",
            idx: i as u16,
            value: [i as u8 + 1; 32],
            last_change_slot: Some(100),
            state: NonceRowState::InFlight,
            intent_id: Some(9),
            since_slot: Some(101),
        });
    }
    l.flush();
    let mut accounts = std::collections::HashMap::new();
    accounts.insert(n0, nonce_data(&auth, [0xaa; 32])); // advanced while down
    accounts.insert(n2, nonce_data(&auth, [0xbb; 32])); // new account
    let chain = MockChain {
        slot: 500,
        balances: vec![
            TokenBalance {
                mint: resized,
                account: Pubkey::new_unique(),
                amount: 150,
            },
            TokenBalance {
                mint: kept,
                account: Pubkey::new_unique(),
                amount: 77,
            },
            TokenBalance {
                mint: orphan,
                account: Pubkey::new_unique(),
                amount: 9,
            },
            TokenBalance {
                mint: pump_codec::constants::WSOL_MINT,
                account: Pubkey::new_unique(),
                amount: 5,
            },
        ],
        accounts,
    };
    let nonce_accounts = [(n0, "buy", 0u16), (n1, "buy", 1), (n2, "buy", 2)];
    let c = s.reader().unwrap();
    let rep = recover(
        &c,
        &l,
        &RecoverCtx {
            owner: Some(Pubkey::new_unique()),
            mode: "live",
            nonce_accounts: &nonce_accounts,
            chain: Some(&chain),
        },
    )
    .unwrap();
    assert!(rep.reconciled);
    assert_eq!(rep.closed_missing, vec![gone]);
    assert_eq!(rep.resized, vec![(resized, 100, 150)]);
    assert_eq!(rep.orphans.len(), 1);
    assert_eq!(rep.orphans[0].mint, orphan);
    let mut open: Vec<Pubkey> = rep.open_positions.iter().map(|p| p.mint).collect();
    open.sort();
    let mut want = vec![resized, kept, orphan];
    want.sort();
    assert_eq!(open, want);
    assert_eq!(rep.nonce_changed, vec![0]);
    let db_open = load_open_positions(&c).unwrap();
    assert_eq!(
        db_open.len(),
        3,
        "I6: open positions == non-zero token accounts"
    );
    let nonces = load_nonces(&c).unwrap();
    let by = |k: Pubkey| nonces.iter().find(|n| n.pubkey == k).unwrap().clone();
    assert_eq!(by(n0).state, NonceRowState::Free);
    assert_eq!(by(n0).value, [0xaa; 32]);
    assert_eq!(by(n0).last_change_slot, Some(500));
    assert_eq!(
        by(n1).state,
        NonceRowState::Quarantined,
        "missing account: never fire"
    );
    assert_eq!(by(n2).state, NonceRowState::Free);
    assert_eq!(by(n2).last_change_slot, Some(500));
}

#[test]
fn paper_recovery_abandons_simulated_positions() {
    let (_tmp, p) = tmp_guarded("paper");
    let s = StateStore::open(&p).unwrap();
    let l = Ledger::new(s.writer());
    let m = Pubkey::new_unique();
    l.position(position(m, 10, PosState::Open));
    l.flush();
    let c = s.reader().unwrap();
    let rep = recover(
        &c,
        &l,
        &RecoverCtx {
            owner: None,
            mode: "paper",
            nonce_accounts: &[],
            chain: None,
        },
    )
    .unwrap();
    assert!(!rep.reconciled);
    assert_eq!(rep.abandoned_paper, vec![m]);
    assert!(rep.open_positions.is_empty());
    assert!(load_open_positions(&c).unwrap().is_empty());
}
