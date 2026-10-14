//! Row types of the `state.db` tables (owned; built on the cold thread).

use pump_codec::Pubkey;

/// `intents.state` (docs/06 section 5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IntentState {
    /// Paper decision: recorded, never sent.
    Paper,
    Sent,
    Landed,
    Failed,
    Burned,
    /// A buy variant beat the nonce burn (docs/06 section 7.3).
    LateFill,
}

impl IntentState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Sent => "sent",
            Self::Landed => "landed",
            Self::Failed => "failed",
            Self::Burned => "burned",
            Self::LateFill => "late_fill",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "paper" => Self::Paper,
            "sent" => Self::Sent,
            "landed" => Self::Landed,
            "failed" => Self::Failed,
            "burned" => Self::Burned,
            "late_fill" => Self::LateFill,
            _ => return None,
        })
    }
    /// States an intent may move to from `self` (paper is terminal: a paper
    /// intent's simulated fill lives in `fills`/`positions`, never here).
    pub fn next_allowed(self) -> &'static [IntentState] {
        match self {
            Self::Sent => &[Self::Landed, Self::Failed, Self::Burned, Self::LateFill],
            Self::Burned => &[Self::LateFill],
            Self::Paper | Self::Landed | Self::Failed | Self::LateFill => &[],
        }
    }
    /// States from which `to` is reachable in one step.
    pub fn allowed_from(to: IntentState) -> Vec<IntentState> {
        [
            Self::Paper,
            Self::Sent,
            Self::Landed,
            Self::Failed,
            Self::Burned,
            Self::LateFill,
        ]
        .into_iter()
        .filter(|f| f.next_allowed().contains(&to))
        .collect()
    }
}

/// `intents.via`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Signer,
    FeePayer,
    Funding,
    /// The ArmSet class arm (`match_all`) took the create.
    Class,
}

impl Via {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Signer => "signer",
            Self::FeePayer => "fee_payer",
            Self::Funding => "funding",
            Self::Class => "class",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct IntentRow {
    pub intent_id: i64,
    pub mint: Pubkey,
    pub op_id: u32,
    pub box_id: String,
    pub mode: String,
    pub armset_ver: u64,
    pub slot: u64,
    pub bank_id: u64,
    pub via: Via,
    pub feed_class: String,
    pub spend: u64,
    pub v_max: u64,
    pub min_tokens_out: u64,
    pub nonce_idx: u16,
    pub nonce_value: [u8; 32],
    pub decided_ns: u64,
    pub state: IntentState,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SendRow {
    pub sig: [u8; 64],
    pub intent_id: i64,
    pub variant: u8,
    pub sender: String,
    pub prio: u64,
    pub tip: u64,
    pub sent_ns: u64,
    pub result: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FillRow {
    /// Transaction id (paper fills use the primary variant's signature).
    pub sig: [u8; 64],
    pub intent_id: i64,
    pub mint: Pubkey,
    pub slot: u64,
    pub tx_index: Option<u32>,
    pub tokens: u64,
    pub sol_delta: i64,
    pub err: Option<String>,
    pub buyers_ahead: u32,
    pub rival_sol_ahead: u64,
    pub late: bool,
    /// A buy that would have failed: `premium`, `slippage`, `not_in_slot0`
    /// or `class` (a skip); `None` for a fill.
    pub fail_reason: Option<String>,
    /// What the failed attempt cost (its priority fee; 0 for `class`).
    pub fail_cost: u64,
}

/// `shadow`: the shadow-mode entry of one paper intent (docs/04 section 7).
#[derive(Debug, Clone, PartialEq)]
pub struct ShadowRow {
    pub intent_id: i64,
    pub mint: Pubkey,
    /// `e1` or `e2`.
    pub model: String,
    /// `filled`, `premium`, `slippage`, `not_in_slot0`, `class`, `unresolved`.
    pub outcome: String,
    pub create_slot: u64,
    /// Index of the create transaction in its block.
    pub create_index: Option<u32>,
    pub block_txs: Option<u32>,
    /// Trades and wallets (outside the creator set) of the bundle run.
    pub bundle_trades: u32,
    pub bundle_wallets: u32,
    /// Where our buy would have landed (right behind the bundle run).
    pub our_index: Option<u32>,
    /// Third-party slot-0 buyers ahead of / behind that index.
    pub tp_ahead: u32,
    pub tp_behind: u32,
    /// Failed slot-0 transactions on the curve (lost races of others).
    pub s0_failed: u32,
    pub premium_bps: u32,
    pub post_bundle_v: u64,
    pub post_bundle_t: u64,
    /// Post-bundle market cap, lamports (1e9-token supply).
    pub post_bundle_mcap: u64,
    /// Priority fee of a failed attempt (0 otherwise).
    pub cost: u64,
}

/// `positions.origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Intent,
    Orphan,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Orphan => "orphan",
        }
    }
}

/// `positions.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PosState {
    Open,
    Exiting,
    Closed,
}

impl PosState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Exiting => "exiting",
            Self::Closed => "closed",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "open" => Self::Open,
            "exiting" => Self::Exiting,
            "closed" => Self::Closed,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionRow {
    pub mint: Pubkey,
    pub intent_id: i64,
    pub opened_slot: u64,
    pub tokens: u64,
    pub cost: u64,
    pub v_fill: u64,
    pub peak_v: u64,
    pub bundle_json: String,
    pub policy_json: String,
    pub origin: Origin,
    pub state: PosState,
    pub sell_spend: u64,
    pub closed_slot: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExitRow {
    pub sig: [u8; 64],
    pub mint: Pubkey,
    pub reason: String,
    pub venue: String,
    pub tokens: u64,
    pub sol_delta: i64,
    pub slot: u64,
    pub owner: Pubkey,
    pub try_no: u32,
}

/// `nonces.state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonceRowState {
    Free,
    InFlight,
    Burning,
    Quarantined,
}

impl NonceRowState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::InFlight => "inflight",
            Self::Burning => "burning",
            Self::Quarantined => "quarantined",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "free" => Self::Free,
            "inflight" => Self::InFlight,
            "burning" => Self::Burning,
            "quarantined" => Self::Quarantined,
            _ => return None,
        })
    }
    pub fn from_pool(s: rugsnipe_tx::nonce::NonceState) -> Self {
        use rugsnipe_tx::nonce::NonceState as N;
        match s {
            N::Free => Self::Free,
            N::InFlight => Self::InFlight,
            N::Burning => Self::Burning,
            N::Quarantined => Self::Quarantined,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct NonceRow {
    pub pubkey: Pubkey,
    /// `buy` or `sell`.
    pub pool: &'static str,
    pub idx: u16,
    pub value: [u8; 32],
    /// `None` = unknown (never fire).
    pub last_change_slot: Option<u64>,
    pub state: NonceRowState,
    pub intent_id: Option<i64>,
    pub since_slot: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WatchDynRow {
    pub key: Pubkey,
    pub kind: String,
    pub op_id: u32,
    pub rule_id: u32,
    pub added_slot: u64,
    pub expires_unix: u64,
    pub remove_if_no_launch_unix: u64,
    pub keep: bool,
    pub parent: Option<Pubkey>,
    pub state: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RiskRow {
    /// `YYYY-MM-DD` (UTC).
    pub day: String,
    pub start_balance: u64,
    pub deposits: u64,
    pub sweeps: u64,
    pub realized: i64,
    pub buys: u64,
    pub killed: bool,
    pub kill_reason: Option<String>,
}

/// `YYYY-MM-DD` for a Unix time (UTC, proleptic Gregorian).
pub fn utc_day(unix: u64) -> String {
    let days = (unix / 86_400) as i64;
    // civil_from_days (Howard Hinnant)
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Base58 of a signature / hash.
pub fn b58(bytes: &[u8]) -> String {
    bs58::encode(bytes).into_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_days() {
        assert_eq!(utc_day(0), "1970-01-01");
        assert_eq!(utc_day(1_791_968_762), "2026-10-14");
        assert_eq!(utc_day(951_782_400), "2000-02-29");
    }

    #[test]
    fn intent_transitions() {
        assert!(IntentState::Sent
            .next_allowed()
            .contains(&IntentState::Landed));
        assert!(IntentState::Paper.next_allowed().is_empty());
        assert_eq!(
            IntentState::allowed_from(IntentState::LateFill),
            vec![IntentState::Sent, IntentState::Burned]
        );
        for s in ["paper", "sent", "landed", "failed", "burned", "late_fill"] {
            assert_eq!(IntentState::parse(s).unwrap().as_str(), s);
        }
    }
}
