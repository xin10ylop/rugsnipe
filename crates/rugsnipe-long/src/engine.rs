//! The loop: triggers, fills, exits and the book. One thread; the feed and
//! the oracle run on their own. Chain reads (blockhash, statuses, a new
//! quote token's pool) are blocking RPC calls on this thread, tens of ms
//! each.
//!
//! **Accounting is chain-true on a real position**: its result is the sum
//! of our wallet's lamport changes over its transactions (buy, sells, the
//! failed ones) plus the anchor's token changes valued at the oracle price
//! when the anchor is a float. A paper position is priced on the live curve
//! with our tokens added (the curve as it would be with us in it), the
//! route's other hops at its own fill's rate, and the fees we would pay.

use crate::chain::Rpc;
use crate::config::{LongCfg, Mode};
use crate::detect::{self, Seen, Trigger};
use crate::feed::{FeedHandle, FeedMsg, Src};
use crate::oracle::{now_ms, Oracle};
use crate::pos::{self, Book, Position, Reason, State};
use crate::route;
use crate::send::Out;
use crate::txb::{self, Fees};
use crate::txlite::TxLite;
use pump_codec::constants as c;
use pump_codec::Pubkey;
use rugsnipe_tx::senders::VariantKind;
use rugsnipe_tx::signer::HotSigner;
use std::collections::{BTreeSet, HashMap};
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

/// Pump's curve fee on coins priced in other tokens (estimate for bounds
/// and paper; the real fee is in the chain-true result).
const CURVE_FEE_BPS: u32 = 125;
/// A buy not seen landed after this is asked for by signature.
const BUY_WAIT_MS: u64 = 20_000;
const SELL_WAIT_MS: u64 = 15_000;
/// SOL kept in the wallet for fees beyond the stakes.
const SOL_RESERVE: u64 = 50_000_000;

pub struct Engine {
    pub cfg: LongCfg,
    me: Pubkey,
    signer: Option<HotSigner>,
    rpc: Rpc,
    oracle: Oracle,
    out_buy: Option<Out>,
    out_sell: Option<Out>,
    feed: Option<FeedHandle>,
    pub book: Book,
    book_path: PathBuf,
    closed_path: PathBuf,
    anchors: Vec<Pubkey>,
    copy: Pubkey,
    slot: u64,
    blockhash: Option<([u8; 32], u64)>,
    its_bought: HashMap<Pubkey, u64>,
    sol_balance: u64,
    floats: HashMap<Pubkey, u64>,
    last_refresh_ms: u64,
    last_save_ms: u64,
    watched: BTreeSet<Pubkey>,
    buyback_rr: usize,
}

fn m8(k: &Pubkey) -> String {
    k.to_string()[..8].to_string()
}

fn sol(l: i128) -> String {
    format!("{:+.4}", l as f64 / 1e9)
}

impl Engine {
    pub fn new(
        cfg: LongCfg,
        me: Pubkey,
        signer: Option<HotSigner>,
        rpc: Rpc,
        oracle: Oracle,
        out_buy: Option<Out>,
        out_sell: Option<Out>,
    ) -> Result<Self, String> {
        let dir = PathBuf::from(&cfg.state_dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let book_path = dir.join("positions.json");
        let book = Book::load(&book_path)?;
        let mut anchors = vec![c::WSOL_MINT];
        anchors.extend(cfg.float_pks());
        Ok(Self {
            copy: cfg.copy_pk(),
            cfg,
            me,
            signer,
            rpc,
            oracle,
            out_buy,
            out_sell,
            feed: None,
            book,
            book_path,
            closed_path: dir.join("closed.jsonl"),
            anchors,
            slot: 0,
            blockhash: None,
            its_bought: HashMap::new(),
            sol_balance: 0,
            floats: HashMap::new(),
            last_refresh_ms: 0,
            last_save_ms: 0,
            watched: BTreeSet::new(),
            buyback_rr: 0,
        })
    }

    pub fn set_feed(&mut self, f: FeedHandle) {
        self.feed = Some(f);
        self.rewatch();
    }

    fn real(&self) -> bool {
        self.cfg.mode == Mode::On
    }

    fn killed(&self) -> bool {
        std::path::Path::new(&self.cfg.state_dir)
            .join("KILL")
            .exists()
    }

    fn save(&mut self) {
        self.book.prune(now_ms(), 3 * 86_400_000);
        if let Err(e) = self.book.save(&self.book_path) {
            eprintln!("long: saving positions: {e}");
        }
        self.last_save_ms = now_ms();
    }

    fn rewatch(&mut self) {
        let want: BTreeSet<Pubkey> = self.book.live().map(|p| p.mint_pk()).collect();
        if want != self.watched {
            self.watched = want.clone();
            if let Some(f) = &self.feed {
                f.watch(want);
            }
        }
    }

    /// Run until the feed channel closes.
    pub fn run(&mut self, rx: mpsc::Receiver<FeedMsg>) {
        self.refresh(true);
        self.recover();
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(FeedMsg::Tx(src, tx)) => self.on_tx(src, &tx),
                Ok(FeedMsg::Slot(s)) => self.on_slot(s),
                Ok(FeedMsg::Up(up)) => println!(
                    "long feed {}",
                    if up { "connected" } else { "disconnected" }
                ),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            self.on_tick(now_ms());
        }
        self.save();
    }

    // ------------------------------------------------------------- inputs
    pub fn on_slot(&mut self, s: u64) {
        self.slot = self.slot.max(s);
    }

    pub fn on_tx(&mut self, src: Src, tx: &TxLite) {
        // the curve of every held coin
        if !tx.err {
            for t in tx.trades() {
                for p in self
                    .book
                    .positions
                    .iter_mut()
                    .filter(|p| p.live() && p.mint_pk() == t.mint)
                {
                    // a paper buy fills on the state at the end of its landing slot
                    if p.paper && p.state == State::Buying && tx.slot > p.its_slot + 1 {
                        continue;
                    }
                    if tx.slot >= p.last_slot {
                        p.vt = t.vt;
                        p.vq = t.vq;
                        p.last_slot = tx.slot;
                        p.peak = p.peak.max(p.price());
                    }
                    if t.complete() && p.state == State::Open && p.reason.is_none() {
                        p.reason = Some(Reason::Complete);
                    }
                }
            }
        }
        if src == Src::Ours || tx.signer() == Some(self.me) {
            self.on_ours(tx);
        }
        if tx.signer() == Some(self.copy) && !tx.err {
            let o = self.oracle.clone();
            for s in detect::read(tx, &self.copy, &|m: &Pubkey| o.lamports_per_unit(m)) {
                match s {
                    Seen::Buy(t) => self.on_trigger(*t),
                    Seen::Sell { mint, .. } => {
                        if self.book.live().any(|p| p.mint_pk() == mint) {
                            println!("long its-sell {} (we hold it)", m8(&mint));
                        }
                    }
                    Seen::SolBuy { .. } => {}
                }
            }
        }
    }

    fn skip(&self, t: &Trigger, why: &str) {
        println!(
            "long skip {} its {} SOL ({}): {why}",
            m8(&t.mint),
            t.sol_value
                .map_or("?".into(), |v| format!("{:.3}", v as f64 / 1e9)),
            if t.route.is_some() {
                "multihop"
            } else {
                "direct"
            }
        );
    }

    pub fn on_trigger(&mut self, t: Trigger) {
        let now = now_ms();
        let rebuy = self
            .its_bought
            .insert(t.mint, now)
            .is_some_and(|at| now.saturating_sub(at) < 3_600_000);
        let Some(v) = t.sol_value else {
            return self.skip(&t, "its buy has no SOL value (paid in an unpriced token)");
        };
        if (v as f64) < self.cfg.min_sol * 1e9 {
            // its smaller tiers: not copied, not journaled
            return;
        }
        if rebuy {
            return self.skip(&t, "its second buy of the coin");
        }
        if self
            .book
            .positions
            .iter()
            .any(|p| p.mint_pk() == t.mint && now.saturating_sub(p.seen_ms) < 3_600_000)
        {
            return self.skip(&t, "coin already ours");
        }
        if t.real_token_post == Some(0) {
            return self.skip(&t, "curve complete");
        }
        if self.cfg.mode == Mode::Off {
            return self.skip(&t, "mode off");
        }
        if self.killed() {
            return self.skip(&t, "KILL file");
        }
        let paper = !self.real();
        let open = self.book.live().filter(|p| p.paper == paper).count();
        if open >= self.cfg.max_open {
            return self.skip(&t, &format!("{open} open (max_open)"));
        }
        let today = self.book.realized_today(now, paper);
        if (today as f64) <= -self.cfg.daily_stop_sol * 1e9 {
            return self.skip(
                &t,
                &format!("daily stop ({} SOL today)", sol(today as i128)),
            );
        }
        let plan = match route::plan(&t, &self.anchors, &mut self.rpc) {
            Ok(p) => p,
            Err(e) => return self.skip(&t, &format!("route: {e}")),
        };
        let stake = (self.cfg.stake_sol * 1e9) as u64;
        // our amount in, in anchor units, and its SOL value
        let amount_in = if plan.anchor == c::WSOL_MINT {
            stake
        } else {
            match self.oracle.lamports_per_unit(&plan.anchor) {
                Some(p) if p > 0.0 => (stake as f64 / p) as u64,
                _ => return self.skip(&t, &format!("no price for the float {}", m8(&plan.anchor))),
            }
        };
        // our share of its buy: in the same unit when we pay what it paid
        let share = if plan.anchor == t.input_mint && t.input_amount > 0 {
            amount_in as f64 / t.input_amount as f64
        } else {
            stake as f64 / v as f64
        };
        let quote_in_est = (t.its_quote_in as f64 * share) as u64;
        let min_out = (t.its_tokens as f64 * share / (1.0 + self.cfg.max_premium)) as u64;
        let (input, hops) = Position::store_route(&plan.route);
        let id = self.book.next_id;
        self.book.next_id += 1;
        let mut p = Position {
            id,
            paper,
            mint: t.mint.to_string(),
            quote_mint: t.quote_mint.to_string(),
            coin_program: t.coin_program.to_string(),
            anchor: plan.anchor.to_string(),
            anchor_program: plan.anchor_program.to_string(),
            route_input: input,
            route: hops,
            how: plan.how.to_string(),
            its_sig: t.sig.clone(),
            its_slot: t.slot,
            its_lamports: v,
            its_quote_in: t.its_quote_in,
            its_tokens: t.its_tokens,
            seen_ms: now,
            vt_ref: t.vt_post,
            vq_ref: t.vq_post,
            amount_in,
            stake_lamports: stake,
            tokens: 0,
            state: State::Buying,
            buy_sig: None,
            buy_sent_ms: now,
            sell_sigs: vec![],
            sell_sent_ms: 0,
            sell_tries: 0,
            reason: None,
            vt: t.vt_post,
            vq: t.vq_post,
            last_slot: t.slot,
            peak: 1.0,
            out_anchor: 0,
            out_lamports: 0,
            net_lamports: 0,
            closed_ms: 0,
            costs: 0,
            quote_in: quote_in_est,
        };
        if paper {
            println!(
                "long paper-buy #{id} {} its {:.3} SOL route {} ({} hops) stake {:.3} SOL",
                m8(&t.mint),
                v as f64 / 1e9,
                plan.how,
                plan.route.hops.len(),
                stake as f64 / 1e9
            );
        } else {
            // money checks
            if plan.anchor == c::WSOL_MINT {
                if self.sol_balance < stake + SOL_RESERVE {
                    return self.skip(
                        &t,
                        &format!(
                            "SOL balance {:.3} under the stake + reserve",
                            self.sol_balance as f64 / 1e9
                        ),
                    );
                }
            } else if self.floats.get(&plan.anchor).copied().unwrap_or(0) < amount_in {
                return self.skip(&t, &format!("float {} under the stake", m8(&plan.anchor)));
            }
            match self.send_buy(&p, &plan.route, min_out) {
                Ok(sig) => {
                    println!(
                        "long buy #{id} {} its {:.3} SOL route {} ({} hops) in {} of {} min_out {} sent {}",
                        m8(&t.mint),
                        v as f64 / 1e9,
                        plan.how,
                        plan.route.hops.len(),
                        amount_in,
                        m8(&plan.anchor),
                        min_out,
                        &sig[..8]
                    );
                    p.buy_sig = Some(sig);
                    if plan.anchor == c::WSOL_MINT {
                        self.sol_balance = self.sol_balance.saturating_sub(stake);
                    } else if let Some(f) = self.floats.get_mut(&plan.anchor) {
                        *f = f.saturating_sub(amount_in);
                    }
                }
                Err(e) => return self.skip(&t, &format!("send: {e}")),
            }
        }
        self.book.positions.push(p);
        self.rewatch();
        self.save();
    }

    fn fees(&self, buy: bool) -> (Fees, Vec<Option<(Pubkey, u64)>>) {
        let tip = if buy {
            self.cfg.buy_tip_lamports
        } else {
            self.cfg.sell_tip_lamports
        };
        let out = if buy { &self.out_buy } else { &self.out_sell };
        let tips = out.as_ref().map(|o| o.tips(tip)).unwrap_or_default();
        (
            Fees {
                cu_limit: self.cfg.cu_limit,
                cu_price: if buy {
                    self.cfg.buy_cu_price
                } else {
                    self.cfg.sell_cu_price
                },
                tip: None,
            },
            tips,
        )
    }

    fn blockhash(&mut self) -> Result<[u8; 32], String> {
        match self.blockhash {
            Some((h, at)) if now_ms().saturating_sub(at) < 20_000 => Ok(h),
            _ => {
                let (h, _) = self.rpc.blockhash()?;
                self.blockhash = Some((h, now_ms()));
                Ok(h)
            }
        }
    }

    /// Sign one copy per sender (each with its tip), send them, and the
    /// first copy through `sendTransaction` too. Returns the first signature.
    fn send_signed(
        &mut self,
        kind: VariantKind,
        ixs_for: &dyn Fn(Option<(Pubkey, u64)>) -> Vec<pump_codec::Instruction>,
    ) -> Result<String, String> {
        let bh = self.blockhash()?;
        let signer = self.signer.as_ref().ok_or("no wallet loaded")?;
        let (_, tips) = self.fees(kind == VariantKind::Buy);
        let tips = if tips.is_empty() { vec![None] } else { tips };
        let mut txs = vec![];
        for tip in &tips {
            let ixs = ixs_for(*tip);
            txs.push(
                txb::sign(&self.me, &ixs, &bh, |m| signer.sign(m)).map_err(|e| e.to_string())?,
            );
        }
        let first = bs58::encode(txs[0].1).into_string();
        let out = if kind == VariantKind::Buy {
            &self.out_buy
        } else {
            &self.out_sell
        };
        if let Some(out) = out {
            for s in out.send_each(kind, &txs, &tips) {
                if !s.ok {
                    eprintln!("long send {} {}: {}", s.label, &s.sig[..8], s.why);
                }
            }
        }
        if self.cfg.rpc_send || out.is_none() {
            if let Err(e) = self.rpc.send(&txs[0].0) {
                eprintln!("long send rpc {}: {e}", &first[..8]);
            }
        }
        Ok(first)
    }

    fn send_buy(
        &mut self,
        p: &Position,
        r: &pump_codec::multihop::Route,
        min_out: u64,
    ) -> Result<String, String> {
        let (fees, _) = self.fees(true);
        let (ap, cp) = (
            Position::pk(&p.anchor_program),
            Position::pk(&p.coin_program),
        );
        let (me, amount_in) = (self.me, p.amount_in);
        self.buyback_rr = self.buyback_rr.wrapping_add(1);
        let bb = self.buyback_rr;
        let r = r.clone();
        self.send_signed(VariantKind::Buy, &move |tip| {
            txb::buy_ixs(
                &me,
                &r,
                &ap,
                &cp,
                amount_in,
                min_out,
                &Fees { tip, ..fees },
                bb,
            )
        })
    }

    /// Expected anchor out for selling our tokens now, and the floor.
    fn sell_min_out(&self, p: &Position) -> u64 {
        let quote_out = pos::curve_sell_live(p.vt, p.vq, p.tokens, CURVE_FEE_BPS) as f64;
        // the route's other hops at our buy's own rate (anchor per quote)
        let rate = if p.quote_in > 0 && p.anchor != p.quote_mint {
            p.amount_in as f64 / p.quote_in as f64
        } else {
            1.0
        };
        let expected = quote_out * rate;
        let slack = match (p.reason, p.sell_tries) {
            (_, n) if n >= 4 => 0.999,
            (_, n) if n >= 2 => 0.7,
            (Some(Reason::Tp), _) => self.cfg.sell_slack_tp,
            _ => self.cfg.sell_slack_exit,
        };
        (expected * (1.0 - slack)) as u64
    }

    fn send_sell(&mut self, i: usize) -> Result<String, String> {
        let p = self.book.positions[i].clone();
        let r = p.buy_route().ok_or("stored route unreadable")?.reversed();
        let min_out = self.sell_min_out(&p);
        let (fees, _) = self.fees(false);
        let (ap, cp, me, tokens) = (
            Position::pk(&p.anchor_program),
            Position::pk(&p.coin_program),
            self.me,
            p.tokens,
        );
        self.buyback_rr = self.buyback_rr.wrapping_add(1);
        let bb = self.buyback_rr;
        self.send_signed(VariantKind::Sell, &move |tip| {
            txb::sell_ixs(
                &me,
                &r,
                &ap,
                &cp,
                tokens,
                min_out,
                true,
                &Fees { tip, ..fees },
                bb,
            )
        })
    }

    /// Our own transaction: a fill, a sell, or a failure of either.
    fn on_ours(&mut self, tx: &TxLite) {
        let sig = tx.sig_b58();
        let me = self.me;
        let Some(i) =
            self.book.positions.iter().position(|p| {
                p.buy_sig.as_deref() == Some(sig.as_str()) || p.sell_sigs.contains(&sig)
            })
        else {
            return;
        };
        let now = now_ms();
        let price = {
            let a = Position::pk(&self.book.positions[i].anchor);
            if a == c::WSOL_MINT {
                Some(1.0)
            } else {
                self.oracle.lamports_per_unit(&a)
            }
        };
        let p = &mut self.book.positions[i];
        let anchor = Position::pk(&p.anchor);
        let mint = p.mint_pk();
        // chain-true running result: lamports plus a float's token change
        let lam = tx.lamport_delta(&me);
        let anc = if anchor == c::WSOL_MINT {
            0
        } else {
            tx.token_delta(&me, &anchor)
        };
        p.net_lamports += lam as i64 + (anc as f64 * price.unwrap_or(0.0)) as i64;
        let is_buy = p.buy_sig.as_deref() == Some(sig.as_str());
        if tx.err {
            p.costs += tx.fee;
            if is_buy {
                p.state = State::Failed;
                p.closed_ms = now;
                println!("long buy-failed #{} {} cost {}", p.id, m8(&mint), sol(lam));
            } else if p.state == State::Selling {
                p.state = State::Open;
                p.sell_tries += 1;
                println!(
                    "long sell-failed #{} {} try {} (retried)",
                    p.id,
                    m8(&mint),
                    p.sell_tries
                );
            }
        } else if is_buy && p.state == State::Buying {
            p.tokens = tx.token_delta(&me, &mint).max(0) as u64;
            // our curve trade's quote in (the rate for the sell bound)
            p.quote_in = tx
                .trades()
                .iter()
                .find(|t| t.user == me && t.mint == mint && t.is_buy)
                .map_or(0, |t| t.quote_amount);
            p.state = if p.tokens > 0 {
                State::Open
            } else {
                State::Failed
            };
            for t in tx.trades().iter().filter(|t| t.mint == mint) {
                p.vt = t.vt;
                p.vq = t.vq;
                p.last_slot = tx.slot;
            }
            println!(
                "long fill #{} {} tokens {} paid {} (vs its price {:+.1}%)",
                p.id,
                m8(&mint),
                p.tokens,
                sol(lam),
                (p.its_tokens as f64
                    / p.its_lamports.max(1) as f64
                    / (p.tokens as f64 / (p.stake_lamports.max(1) as f64))
                    - 1.0)
                    * 100.0
            );
        } else if !is_buy {
            let left = tx.token_delta(&me, &mint);
            p.out_anchor = if anchor == c::WSOL_MINT {
                lam.max(0) as u64
            } else {
                anc.max(0) as u64
            };
            if left < 0 {
                p.state = State::Closed;
                p.closed_ms = now;
                p.tokens = 0;
                let held = now.saturating_sub(p.seen_ms) / 1000;
                println!(
                    "long closed #{} {} {} net {} SOL ({:+.1}%) held {}s peak {:.2}x",
                    p.id,
                    m8(&mint),
                    p.reason.map_or("?", Reason::name),
                    sol(p.net_lamports as i128),
                    p.net_lamports as f64 / p.stake_lamports.max(1) as f64 * 100.0,
                    held,
                    p.peak
                );
                let rec = serde_json::to_string(&*p).unwrap_or_default();
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.closed_path)
                {
                    let _ = writeln!(f, "{rec}");
                }
            }
        }
        self.rewatch();
        self.save();
    }

    // -------------------------------------------------------------- clock
    pub fn on_tick(&mut self, now: u64) {
        if self.real() && now.saturating_sub(self.last_refresh_ms) > 30_000 {
            self.refresh(false);
        }
        enum Act {
            PaperFill,
            Chase(bool),
            Exit(Reason),
        }
        let cfg = &self.cfg;
        let slot = self.slot;
        let acts: Vec<(usize, Act)> = self
            .book
            .positions
            .iter()
            .enumerate()
            .filter_map(|(i, p)| match p.state {
                // a paper buy fills once its landing slot is over
                State::Buying if p.paper && slot > p.its_slot + 1 => Some((i, Act::PaperFill)),
                State::Buying if !p.paper && now.saturating_sub(p.buy_sent_ms) > BUY_WAIT_MS => {
                    Some((i, Act::Chase(true)))
                }
                State::Selling if !p.paper && now.saturating_sub(p.sell_sent_ms) > SELL_WAIT_MS => {
                    Some((i, Act::Chase(false)))
                }
                State::Open => p
                    .reason
                    .filter(|r| *r == Reason::Complete)
                    .or_else(|| pos::exit_due(p, now, cfg.take_profit, cfg.stop, cfg.max_hold_s))
                    .map(|r| (i, Act::Exit(r))),
                _ => None,
            })
            .collect();
        let mut changed = false;
        for (i, a) in acts {
            match a {
                Act::PaperFill => {
                    let p = &mut self.book.positions[i];
                    let share = p.stake_lamports as f64 / p.its_lamports.max(1) as f64;
                    let q = (p.its_quote_in as f64 * share) as u64;
                    p.tokens = pos::curve_buy(p.vt, p.vq, q, CURVE_FEE_BPS);
                    p.quote_in = q;
                    p.state = State::Open;
                    println!(
                        "long paper-fill #{} {} tokens {} at {:.3}x its price",
                        p.id,
                        &p.mint[..8],
                        p.tokens,
                        p.price()
                    );
                    changed = true;
                }
                Act::Chase(buy) => changed |= self.chase(i, buy),
                Act::Exit(r) => {
                    self.exit(i, r, now);
                    changed = true;
                }
            }
        }
        if changed {
            self.rewatch();
            self.save();
        } else if now.saturating_sub(self.last_save_ms) > 30_000 {
            self.save();
        }
    }

    fn exit(&mut self, i: usize, r: Reason, now: u64) {
        let p = &mut self.book.positions[i];
        p.reason = Some(r);
        let px = p.price();
        if p.paper {
            let quote_out = pos::curve_sell_paper(p.vt, p.vq, p.tokens, CURVE_FEE_BPS) as f64;
            let back = quote_out * p.stake_lamports as f64 / p.quote_in.max(1) as f64;
            let fees = 2 * (5_000 + self.cfg.cu_limit as u64 * self.cfg.buy_cu_price / 1_000_000)
                + self.cfg.buy_tip_lamports
                + self.cfg.sell_tip_lamports;
            p.out_lamports = back as u64;
            p.net_lamports = back as i64 - p.stake_lamports as i64 - fees as i64;
            p.state = State::Closed;
            p.closed_ms = now;
            println!(
                "long paper-closed #{} {} {} at {:.2}x net {} SOL ({:+.1}%) held {}s peak {:.2}x",
                p.id,
                &p.mint[..8],
                r.name(),
                px,
                sol(p.net_lamports as i128),
                p.net_lamports as f64 / p.stake_lamports.max(1) as f64 * 100.0,
                now.saturating_sub(p.seen_ms) / 1000,
                p.peak
            );
            let rec = serde_json::to_string(&*p).unwrap_or_default();
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.closed_path)
            {
                let _ = writeln!(f, "{rec}");
            }
            return;
        }
        let (id, mint, tries) = (p.id, p.mint.clone(), p.sell_tries);
        match self.send_sell(i) {
            Ok(sig) => {
                let p = &mut self.book.positions[i];
                p.state = State::Selling;
                p.sell_sent_ms = now;
                p.sell_sigs.push(sig.clone());
                println!(
                    "long sell #{id} {} {} at {px:.2}x try {tries} sent {}",
                    &mint[..8],
                    r.name(),
                    &sig[..8]
                );
            }
            Err(e) => {
                let p = &mut self.book.positions[i];
                p.sell_tries += 1;
                eprintln!("long sell #{id} {}: {e}", &mint[..8]);
            }
        }
    }

    /// A buy or sell not seen landed in time: ask the chain.
    fn chase(&mut self, i: usize, buy: bool) -> bool {
        let p = &self.book.positions[i];
        let sigs: Vec<String> = if buy {
            p.buy_sig.iter().cloned().collect()
        } else {
            p.sell_sigs.clone()
        };
        if sigs.is_empty() {
            return false;
        }
        let st = match self.rpc.statuses(&sigs) {
            Ok(s) => s,
            Err(_) => return false,
        };
        // landed (ok or failed): read it as the feed would have
        for (s, r) in sigs.iter().zip(&st) {
            if r.is_some() {
                if let Ok(v) = self.rpc.transaction(s) {
                    if let Ok(tx) = TxLite::from_rpc_json(&v) {
                        self.on_ours(&tx);
                        return true;
                    }
                }
            }
        }
        // nothing landed: the blockhash is past its life after ~60 s
        let p = &mut self.book.positions[i];
        let age = now_ms().saturating_sub(if buy { p.buy_sent_ms } else { p.sell_sent_ms });
        if age < 75_000 {
            return false;
        }
        if buy {
            p.state = State::Failed;
            p.closed_ms = now_ms();
            println!("long buy-dropped #{} {}", p.id, &p.mint[..8]);
        } else {
            p.state = State::Open;
            p.sell_tries += 1;
            println!(
                "long sell-dropped #{} {} try {} (retried)",
                p.id,
                &p.mint[..8],
                p.sell_tries
            );
        }
        true
    }

    /// Balances (SOL, floats) and the oracle's view of the floats.
    fn refresh(&mut self, log: bool) {
        self.last_refresh_ms = now_ms();
        if !self.real() {
            return;
        }
        if let Ok(b) = self.rpc.balance(&self.me) {
            self.sol_balance = b;
        }
        for f in self
            .anchors
            .clone()
            .into_iter()
            .filter(|a| *a != c::WSOL_MINT)
        {
            let prog =
                route::Chain::mint_program(&mut self.rpc, &f).unwrap_or(c::SPL_TOKEN_PROGRAM_ID);
            if let Ok(b) = self.rpc.token_balance(&self.me, &f, &prog) {
                self.floats.insert(f, b);
            }
        }
        if log {
            let fl: Vec<String> = self
                .floats
                .iter()
                .map(|(m, b)| {
                    let v = self
                        .oracle
                        .lamports_per_unit(m)
                        .map_or("?".into(), |p| format!("{:.3}", *b as f64 * p / 1e9));
                    format!("{} {} ({v} SOL)", m8(m), b)
                })
                .collect();
            println!(
                "long wallet {} SOL {:.4} floats [{}]",
                self.me,
                self.sol_balance as f64 / 1e9,
                fl.join(", ")
            );
        }
    }

    /// After a restart: live positions whose transactions were in flight.
    fn recover(&mut self) {
        let n = self.book.positions.len();
        let live = self.book.live().count();
        if live > 0 {
            println!("long recover: {live} live position(s)");
        }
        for i in 0..n {
            match self.book.positions[i].state {
                State::Buying if !self.book.positions[i].paper => {
                    self.book.positions[i].buy_sent_ms = 0;
                }
                State::Selling => {
                    self.book.positions[i].sell_sent_ms = 0;
                }
                _ => {}
            }
        }
        self.rewatch();
    }
}
