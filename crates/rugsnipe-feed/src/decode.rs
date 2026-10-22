//! Frame decoders (run on adapter threads or at replay load, never in the hot poll).
//!
//! * `logsSubscribe` notifications -> [`CreateHitLite`] + [`TradeLite`]s, using
//!   `pump_codec::events::{scan_program_data_logs, decode_event_payload}` on the
//!   `Program data:` lines.
//! * PumpPortal `subscribeNewToken` frames -> partial [`CreateHitLite`].
//! * `getTransaction` (`jsonParsed`) results -> [`FundingEvent`]s (system
//!   transfers by a watched funder; the System program logs no amounts, so the
//!   funding watcher needs this cold-path lookup).

use crate::event::{hit_flags, CreateHitLite, FundingEvent, Sig, TradeLite};
use pump_codec::constants::{PUMP_PROGRAM_ID, SPL_TOKEN_PROGRAM_ID};
use pump_codec::events::{decode_event_payload, scan_program_data_logs, EventSource, PumpEvent};
use pump_codec::{pda, Pubkey};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::borrow::Cow;

// ------------------------------------------------------------ websocket JSON
#[derive(Deserialize)]
struct Envelope<'a> {
    #[serde(borrow, default)]
    method: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    params: Option<&'a RawValue>,
    #[serde(default)]
    id: Option<u64>,
    #[serde(borrow, default)]
    result: Option<&'a RawValue>,
    #[serde(borrow, default)]
    error: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct NotifParams<'a> {
    #[serde(borrow)]
    result: LogsResult<'a>,
    subscription: u64,
}

#[derive(Deserialize)]
struct Ctx {
    slot: u64,
}

#[derive(Deserialize)]
struct LogsResult<'a> {
    context: Ctx,
    #[serde(borrow)]
    value: LogsValue<'a>,
}

#[derive(Deserialize)]
struct LogsValue<'a> {
    #[serde(borrow)]
    signature: Cow<'a, str>,
    #[serde(borrow, default)]
    err: Option<&'a RawValue>,
    #[serde(borrow, default)]
    logs: Vec<Cow<'a, str>>,
}

/// One `logsNotification`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogsNote<'a> {
    pub subscription: u64,
    pub slot: u64,
    pub signature: Cow<'a, str>,
    /// The transaction failed (`err != null`).
    pub failed: bool,
    pub logs: Vec<Cow<'a, str>>,
}

/// A websocket text frame from a Solana RPC node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WsMsg<'a> {
    Logs(LogsNote<'a>),
    /// Subscription confirmed: request `id` -> server subscription id.
    SubAck {
        id: u64,
        subscription: u64,
    },
    /// JSON-RPC error (or an unexpected result).
    Error {
        id: Option<u64>,
        message: String,
    },
    /// Another notification or message.
    Other,
}

/// Parse a websocket text frame.
pub fn parse_ws_message(text: &str) -> Result<WsMsg<'_>, serde_json::Error> {
    let env: Envelope<'_> = serde_json::from_str(text)?;
    if env.method.as_deref() == Some("logsNotification") {
        let Some(p) = env.params else {
            return Ok(WsMsg::Other);
        };
        let p: NotifParams<'_> = serde_json::from_str(p.get())?;
        let failed = p.result.value.err.is_some_and(|e| e.get().trim() != "null");
        return Ok(WsMsg::Logs(LogsNote {
            subscription: p.subscription,
            slot: p.result.context.slot,
            signature: p.result.value.signature,
            failed,
            logs: p.result.value.logs,
        }));
    }
    if let Some(e) = env.error {
        return Ok(WsMsg::Error {
            id: env.id,
            message: e.get().chars().take(300).collect(),
        });
    }
    if let (Some(id), Some(r)) = (env.id, env.result) {
        if let Ok(sub) = r.get().trim().parse::<u64>() {
            return Ok(WsMsg::SubAck {
                id,
                subscription: sub,
            });
        }
        if r.get().trim() == "true" || r.get().trim() == "false" {
            return Ok(WsMsg::Other); // unsubscribe ack
        }
        return Ok(WsMsg::Error {
            id: Some(id),
            message: format!("unexpected result {}", &r.get()[..r.get().len().min(120)]),
        });
    }
    Ok(WsMsg::Other)
}

// --------------------------------------------------------------- logs decode
/// What one transaction's logs contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LogsOutcome {
    /// A CreateEvent was decoded.
    pub hit: Option<CreateHitLite>,
    /// The logs show a pump create instruction (`Instruction: Create`/`CreateV2`).
    pub create_attempt: bool,
    /// The transaction failed (nothing decoded).
    pub failed: bool,
    /// The runtime truncated the logs.
    pub truncated: bool,
    /// Pump event payloads that failed to decode.
    pub decode_errors: u32,
    /// TradeEvents appended to the caller's vector.
    pub trades: u32,
}

impl LogsOutcome {
    /// A create attempt (or decoded create) without a usable CreateEvent.
    pub fn is_miss(&self) -> bool {
        !self.failed && self.create_attempt && self.hit.is_none()
    }
}

fn pump_instruction_line(prev: &str, line: &str) -> Option<bool> {
    // Anchor logs "Program log: Instruction: X" right after "Program <id> invoke [n]".
    let name = line.strip_prefix("Program log: Instruction: ")?;
    let rest = prev.strip_prefix("Program ")?;
    let (id, tail) = rest.split_once(' ')?;
    if !tail.starts_with("invoke [") || id != "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P" {
        return None;
    }
    match name {
        "CreateV2" => Some(false),
        "Create" => Some(true),
        _ => None,
    }
}

/// Decode a transaction's logs: the CreateEvent (if any) into a
/// [`CreateHitLite`] whose in-tx fields sum every buy TradeEvent of the new
/// mint, and every pump TradeEvent into `trades` (as [`TradeLite`]).
pub fn decode_logs<S: AsRef<str>>(
    slot: u64,
    sig: Sig,
    failed: bool,
    logs: &[S],
    trades: &mut Vec<TradeLite>,
) -> LogsOutcome {
    let mut out = LogsOutcome {
        failed,
        ..LogsOutcome::default()
    };
    let mut legacy_create = false;
    let mut prev = "";
    for l in logs {
        let l = l.as_ref();
        if let Some(legacy) = pump_instruction_line(prev, l) {
            out.create_attempt = true;
            legacy_create |= legacy;
        }
        prev = l;
    }
    if failed {
        return out;
    }
    let scan = match scan_program_data_logs(logs) {
        Ok(s) => s,
        Err(_) => {
            out.decode_errors += 1;
            return out;
        }
    };
    out.truncated = scan.truncated;
    let mut hit: Option<CreateHitLite> = None;
    let first_trade = trades.len();
    for d in &scan.data {
        if d.program != PUMP_PROGRAM_ID {
            continue;
        }
        match decode_event_payload(EventSource::Pump, &d.data) {
            Ok(PumpEvent::Create(c)) => {
                if hit.is_some() {
                    continue; // one create per tx; keep the first
                }
                let mut flags = hit_flags::CREATOR_KNOWN;
                let token_program = match c.token_program {
                    Some(tp) => {
                        flags |= hit_flags::TOKEN_PROGRAM_KNOWN;
                        tp
                    }
                    None if legacy_create => {
                        flags |= hit_flags::TOKEN_PROGRAM_KNOWN;
                        SPL_TOKEN_PROGRAM_ID
                    }
                    None => Pubkey::default(),
                };
                if legacy_create {
                    flags |= hit_flags::LEGACY_CREATE;
                }
                // `quote_mint` absent on older events means SOL.
                flags |= hit_flags::QUOTE_KNOWN;
                if c.is_cashback_enabled == Some(true) {
                    flags |= hit_flags::CASHBACK;
                }
                if scan.truncated {
                    flags |= hit_flags::LOGS_TRUNCATED;
                }
                let abc = if flags & hit_flags::TOKEN_PROGRAM_KNOWN != 0 {
                    flags |= hit_flags::ABC_KNOWN;
                    pda::associated_bonding_curve(&c.mint, &token_program)
                } else {
                    Pubkey::default()
                };
                hit = Some(CreateHitLite {
                    mint: c.mint,
                    bc: c.bonding_curve,
                    abc,
                    user: c.user,
                    creator: c.creator,
                    token_program,
                    quote_mint: c.quote_mint.unwrap_or_default(),
                    mayhem: c.is_mayhem_mode.unwrap_or(false),
                    slot,
                    sig,
                    intx_sol_in: 0,
                    intx_tokens_out: 0,
                    intx_buys: 0,
                    v_after: c.virtual_sol_reserves,
                    flags,
                });
            }
            Ok(PumpEvent::Trade(t)) => {
                let ordinal = (trades.len() - first_trade).min(255) as u8;
                trades.push(TradeLite {
                    mint: t.mint,
                    user: t.user,
                    is_buy: t.is_buy,
                    sol_amount: t.sol_amount,
                    token_amount: t.token_amount,
                    fee: t
                        .fee
                        .unwrap_or(0)
                        .saturating_add(t.creator_fee.unwrap_or(0)),
                    virtual_sol_reserves: t.virtual_sol_reserves,
                    virtual_token_reserves: t.virtual_token_reserves,
                    slot,
                    sig,
                    ordinal,
                });
                out.trades += 1;
            }
            Ok(_) => {}
            Err(_) => out.decode_errors += 1,
        }
    }
    if let Some(mut h) = hit {
        for t in &trades[first_trade..] {
            if t.mint == h.mint && t.is_buy {
                h.intx_sol_in = h.intx_sol_in.saturating_add(t.sol_amount);
                h.intx_tokens_out = h.intx_tokens_out.saturating_add(t.token_amount);
                h.intx_buys = h.intx_buys.saturating_add(1);
                h.v_after = t.virtual_sol_reserves;
            }
        }
        out.hit = Some(h);
        out.create_attempt = true;
    }
    out
}

// --------------------------------------------------------------- PumpPortal
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PpFrame<'a> {
    #[serde(borrow, default)]
    signature: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    mint: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    trader_public_key: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    tx_type: Option<Cow<'a, str>>,
    #[serde(default)]
    initial_buy: Option<f64>,
    #[serde(default)]
    sol_amount: Option<f64>,
    #[serde(borrow, default)]
    bonding_curve_key: Option<Cow<'a, str>>,
    #[serde(default)]
    v_sol_in_bonding_curve: Option<f64>,
    #[serde(default, rename = "is_mayhem_mode")]
    is_mayhem_mode: Option<bool>,
    #[serde(borrow, default)]
    message: Option<Cow<'a, str>>,
    #[serde(borrow, default)]
    errors: Option<&'a RawValue>,
}

/// A PumpPortal frame (transient: returned by value, never stored).
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub enum PpMsg {
    /// A new token (partial create: no creator, token program, quote, slot).
    Create(CreateHitLite),
    /// Subscription acknowledgement.
    Ack(String),
    Error(String),
    Other,
}

fn round_units(x: Option<f64>, scale: f64) -> u64 {
    match x {
        Some(v) if v.is_finite() && v > 0.0 => (v * scale).round() as u64,
        _ => 0,
    }
}

/// Parse a PumpPortal `subscribeNewToken` frame.
pub fn parse_pumpportal(text: &str) -> PpMsg {
    let Ok(f) = serde_json::from_str::<PpFrame<'_>>(text) else {
        return PpMsg::Other;
    };
    if f.tx_type.as_deref() == Some("create") {
        let pk = |s: &Option<Cow<'_, str>>| s.as_deref().and_then(|s| s.parse::<Pubkey>().ok());
        let (Some(mint), Some(sig)) = (pk(&f.mint), f.signature.as_deref().and_then(Sig::from_b58))
        else {
            return PpMsg::Other;
        };
        let sol = round_units(f.sol_amount, 1e9);
        return PpMsg::Create(CreateHitLite {
            mint,
            bc: pk(&f.bonding_curve_key).unwrap_or_default(),
            user: pk(&f.trader_public_key).unwrap_or_default(),
            mayhem: f.is_mayhem_mode.unwrap_or(false),
            sig,
            intx_sol_in: sol,
            intx_tokens_out: round_units(f.initial_buy, 1e6),
            intx_buys: u8::from(sol > 0),
            v_after: round_units(f.v_sol_in_bonding_curve, 1e9),
            flags: hit_flags::PARTIAL,
            ..CreateHitLite::default()
        });
    }
    if let Some(m) = f.message {
        return PpMsg::Ack(m.into_owned());
    }
    if let Some(e) = f.errors {
        return PpMsg::Error(e.get().chars().take(200).collect());
    }
    PpMsg::Other
}

// ------------------------------------------------------------ funding (cold)
/// Extract the system transfers sent by `funder` from a `getTransaction`
/// response (`encoding: "jsonParsed"`; either the full JSON-RPC response or
/// its `result`). Returns the number of events appended.
pub fn funding_from_get_transaction(
    json: &str,
    funder: &Pubkey,
    out: &mut Vec<FundingEvent>,
) -> Result<usize, String> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|e| e.to_string())?;
    let r = if v.get("jsonrpc").is_some() {
        if let Some(e) = v.get("error") {
            return Err(format!("rpc error: {e}"));
        }
        v.get("result").cloned().unwrap_or(serde_json::Value::Null)
    } else {
        v
    };
    if r.is_null() {
        return Err("transaction not found (yet)".into());
    }
    let meta = &r["meta"];
    if !meta["err"].is_null() {
        return Ok(0);
    }
    let slot = r["slot"].as_u64().unwrap_or(0);
    let sig = r["transaction"]["signatures"][0]
        .as_str()
        .and_then(Sig::from_b58)
        .unwrap_or_default();
    let keys: Vec<String> = r["transaction"]["message"]["accountKeys"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|k| {
                    k.get("pubkey")
                        .and_then(|p| p.as_str())
                        .or_else(|| k.as_str())
                        .unwrap_or("")
                        .to_string()
                })
                .collect()
        })
        .unwrap_or_default();
    let pre = meta["preBalances"].as_array();
    let funder_s = funder.to_string();
    let mut ixs: Vec<&serde_json::Value> = Vec::new();
    if let Some(a) = r["transaction"]["message"]["instructions"].as_array() {
        ixs.extend(a.iter());
    }
    if let Some(inner) = meta["innerInstructions"].as_array() {
        for set in inner {
            if let Some(a) = set["instructions"].as_array() {
                ixs.extend(a.iter());
            }
        }
    }
    let before = out.len();
    for ix in ixs {
        if ix["program"].as_str() != Some("system") {
            continue;
        }
        let p = &ix["parsed"];
        let ty = p["type"].as_str().unwrap_or("");
        if ty != "transfer" && ty != "transferWithSeed" {
            continue;
        }
        let info = &p["info"];
        if info["source"].as_str() != Some(funder_s.as_str()) {
            continue;
        }
        let Some(dest) = info["destination"].as_str() else {
            continue;
        };
        let Ok(recipient) = dest.parse::<Pubkey>() else {
            continue;
        };
        let lamports = info["lamports"].as_u64().unwrap_or(0);
        let recipient_pre_lamports = keys
            .iter()
            .position(|k| k == dest)
            .and_then(|i| pre.and_then(|p| p.get(i)).and_then(|x| x.as_u64()))
            .unwrap_or(u64::MAX);
        out.push(FundingEvent {
            funder: *funder,
            recipient,
            lamports,
            slot,
            sig,
            recipient_pre_lamports,
        });
    }
    Ok(out.len() - before)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_acks_errors_and_garbage() {
        assert_eq!(
            parse_ws_message(r#"{"jsonrpc":"2.0","result":320495701,"id":1}"#).unwrap(),
            WsMsg::SubAck {
                id: 1,
                subscription: 320495701
            }
        );
        assert!(matches!(
            parse_ws_message(r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"bad"},"id":3}"#)
                .unwrap(),
            WsMsg::Error { id: Some(3), .. }
        ));
        assert!(parse_ws_message("not json").is_err());
        assert_eq!(
            parse_ws_message(r#"{"jsonrpc":"2.0","method":"slotNotification","params":{}}"#)
                .unwrap(),
            WsMsg::Other
        );
    }

    #[test]
    fn failed_tx_and_escaped_logs() {
        let f = r#"{"jsonrpc":"2.0","method":"logsNotification","params":{"result":{"context":{"slot":5},"value":{"signature":"x","err":{"InstructionError":[0,"Custom"]},"logs":["Program log: \"quoted\""]}},"subscription":9}}"#;
        let WsMsg::Logs(n) = parse_ws_message(f).unwrap() else {
            panic!()
        };
        assert!(n.failed);
        assert_eq!(n.logs[0], "Program log: \"quoted\"");
        assert_eq!((n.slot, n.subscription), (5, 9));
    }

    #[test]
    fn pumpportal_partial_hit() {
        let f = r#"{"signature":"5J7T55JQzepJ1UQF5rsgjav1SXWoKr8jzSJx2dDBTcidSy3nXUV8PBcqveo8X859JYUQesZh7TFUXd7bAPxJe5rg","mint":"J7SgVKDNGFSQwmC9PR5FRa1Y3EJd9B3HWSVGLzekpump","traderPublicKey":"Cyf57tki4VP9vugMLbteYhRFSrrn1fQQum5b5bfF38U7","txType":"create","initialBuy":24170555.050808,"solAmount":0.691358023,"bondingCurveKey":"F4hQehY7Wh4Naug9kCTztC4sd3dHQooDcrvzT5KECZFv","vTokensInBondingCurve":1048829444.949192,"vSolInBondingCurve":30.691358022999978,"marketCapSol":29.262487023795128,"name":"Agentopia","symbol":"AGENTOPIA","uri":"https://ipfs.io/x","is_mayhem_mode":false,"pool":"pump"}"#;
        let PpMsg::Create(h) = parse_pumpportal(f) else {
            panic!()
        };
        assert_eq!(
            h.mint.to_string(),
            "J7SgVKDNGFSQwmC9PR5FRa1Y3EJd9B3HWSVGLzekpump"
        );
        assert_eq!(h.intx_sol_in, 691_358_023);
        assert_eq!(h.intx_tokens_out, 24_170_555_050_808);
        assert_eq!(h.v_after, 30_691_358_023);
        assert!(h.has(hit_flags::PARTIAL) && !h.is_sol_quote());
        assert_eq!(
            parse_pumpportal(r#"{"message":"Successfully subscribed to token creation events."}"#),
            PpMsg::Ack("Successfully subscribed to token creation events.".into())
        );
        assert_eq!(parse_pumpportal("[]"), PpMsg::Other);
    }

    #[test]
    fn logs_create_without_event_is_a_logs_miss() {
        let sig = bs58::encode(&[5u8; 64][..]).into_string();
        let logs = |failed: bool| LogsNote {
            subscription: 1,
            slot: 5,
            signature: Cow::Owned(sig.clone()),
            failed,
            logs: vec![
                Cow::Borrowed("Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P invoke [1]"),
                Cow::Borrowed("Program log: Instruction: Create"),
                Cow::Borrowed("Program 6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P success"),
            ],
        };
        let health = FeedHealth::new(FeedSrc::LogsWs, 0);
        let mut dec = FrameDecoder::new();
        let n = logs(false);
        let o = dec.logs_note(FeedSrc::LogsWs, 0, &n, false, &health, &mut |_| {
            panic!("nothing emitted")
        });
        assert!(o.is_miss());
        assert_eq!(
            health
                .decode_miss
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(
            health.decode_miss_kind[MissKind::Logs as usize]
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(health.creates.load(std::sync::atomic::Ordering::Relaxed), 0);
        // a failed transaction is not a miss
        let n = logs(true);
        let o = dec.logs_note(FeedSrc::LogsWs, 0, &n, false, &health, &mut |_| {
            panic!("nothing emitted")
        });
        assert!(!o.is_miss() && o.failed);
        assert_eq!(
            health.failed_tx.load(std::sync::atomic::Ordering::Relaxed),
            1
        );
        assert_eq!(
            health
                .decode_miss
                .load(std::sync::atomic::Ordering::Relaxed),
            1
        );
    }
}

// ------------------------------------------------------- frame -> FeedEvents
use crate::event::{FeedEvent, FeedSrc, Payload};
use crate::health::{log_gate, miss_line, FeedHealth, MissInfo, MissKind};

/// Turns raw frames into [`FeedEvent`]s and keeps the health counters.
/// Owns a reusable scratch vector (no per-frame allocation besides the JSON parse).
#[derive(Debug, Default)]
pub struct FrameDecoder {
    trades: Vec<TradeLite>,
}

impl FrameDecoder {
    pub fn new() -> FrameDecoder {
        FrameDecoder {
            trades: Vec::with_capacity(64),
        }
    }

    /// Decode one `logsNotification`; emits the create (if any) and, with
    /// `emit_trades`, every pump TradeEvent of the transaction.
    pub fn logs_note(
        &mut self,
        src: FeedSrc,
        recv_ns: u64,
        note: &LogsNote<'_>,
        emit_trades: bool,
        health: &FeedHealth,
        emit: &mut dyn FnMut(FeedEvent),
    ) -> LogsOutcome {
        let sig = Sig::from_b58(&note.signature).unwrap_or_default();
        self.trades.clear();
        let o = decode_logs(note.slot, sig, note.failed, &note.logs, &mut self.trades);
        if o.failed {
            FeedHealth::inc(&health.failed_tx);
        }
        if o.truncated {
            FeedHealth::inc(&health.truncated);
        }
        if o.decode_errors > 0 {
            health
                .decode_errors
                .fetch_add(o.decode_errors as u64, std::sync::atomic::Ordering::Relaxed);
        }
        if o.is_miss() {
            let n = health.miss(MissKind::Logs);
            // replay loads stay quiet (the loader's summary prints decode_miss)
            if log_gate(n) && src != FeedSrc::Replay {
                let mut info = MissInfo::blank(MissKind::Logs);
                info.slot = note.slot;
                info.sig = sig.0;
                info.ordinal = n;
                for l in &note.logs {
                    // top-level programs: "Program <id> invoke [1]"
                    let Some((id, tail)) = l
                        .as_ref()
                        .strip_prefix("Program ")
                        .and_then(|r| r.split_once(' '))
                    else {
                        continue;
                    };
                    if tail != "invoke [1]" {
                        continue;
                    }
                    if (info.n_programs as usize) < info.programs.len() {
                        if let Ok(p) = id.parse::<Pubkey>() {
                            info.programs[info.n_programs as usize] = p;
                            info.n_programs += 1;
                        }
                    }
                    info.n_ix = info.n_ix.saturating_add(1);
                }
                eprintln!(
                    "{}",
                    miss_line(
                        src.name(),
                        &info,
                        &format_args!(
                            "no CreateEvent (decode_errors {}, truncated {}, trades {})",
                            o.decode_errors, o.truncated, o.trades
                        )
                    )
                );
            }
        }
        if let Some(h) = o.hit {
            FeedHealth::inc(&health.creates);
            health.event(note.slot);
            emit(FeedEvent {
                src,
                recv_ns,
                slot: note.slot,
                bank_id: 0,
                payload: Payload::Hit(h),
            });
        }
        if emit_trades {
            for t in &self.trades {
                health.event(note.slot);
                emit(FeedEvent {
                    src,
                    recv_ns,
                    slot: note.slot,
                    bank_id: 0,
                    payload: Payload::Trade(*t),
                });
            }
        }
        o
    }

    /// Decode a PumpPortal frame; emits a partial create.
    pub fn pp_frame(
        &mut self,
        src: FeedSrc,
        recv_ns: u64,
        text: &str,
        health: &FeedHealth,
        emit: &mut dyn FnMut(FeedEvent),
    ) -> PpMsg {
        let m = parse_pumpportal(text);
        if let PpMsg::Create(h) = &m {
            FeedHealth::inc(&health.creates);
            health.event(0);
            emit(FeedEvent {
                src,
                recv_ns,
                slot: 0,
                bank_id: 0,
                payload: Payload::Hit(*h),
            });
        }
        m
    }

    /// Auto-detect the frame kind (RPC websocket JSON or PumpPortal) and decode it.
    pub fn any_frame(
        &mut self,
        src: FeedSrc,
        recv_ns: u64,
        text: &str,
        emit_trades: bool,
        health: &FeedHealth,
        emit: &mut dyn FnMut(FeedEvent),
    ) {
        if text.contains("\"jsonrpc\"") {
            match parse_ws_message(text) {
                Ok(WsMsg::Logs(n)) => {
                    self.logs_note(src, recv_ns, &n, emit_trades, health, emit);
                }
                Ok(_) => {}
                Err(_) => FeedHealth::inc(&health.decode_errors),
            }
        } else {
            self.pp_frame(src, recv_ns, text, health, emit);
        }
    }
}
