//! `CreateHit`: everything the hot path needs about a create, from wire
//! bytes ([`create_hit`]) or from a `logsSubscribe` notification
//! ([`from_logs`]). One information set for live and backtest (docs/06
//! section 7.1, rulings 3-7):
//!
//! * **every** pump buy of the new mint inside the create transaction is
//!   summed (`intx_sol_in` / `intx_tokens_out`), any variant (legacy
//!   `buy`/`buy_exact_sol_in`, v2, v3), top level or (with meta) via CPI;
//! * the bundle set gets each buy's **signer** and the **owner of the token
//!   account it credits** (a v3 buy can credit a third party: fixture
//!   `v1_buy_exact_quote_in_v3`, signer `8Z2tbj…`, owner `5aR7Xt…`);
//! * `aqbc_hint` = the curve's quote ATA when an in-tx v2/v3 buy names it, so
//!   the sniper template can skip one PDA search;
//! * creates that arrive by CPI from a launcher need the inner instructions
//!   (wire bytes alone cannot reveal launcher layouts), see [`HitError`].
//!
//! Amounts. With meta (inner instructions), the in-tx totals come from the
//! `TradeEvent`s (`sol_amount` = lamports into the curve before fees,
//! `token_amount`). From wire bytes alone they are **simulated** on a fresh
//! curve with the program's integer math ([`crate::curve`]) and the default
//! fee schedule: exact for SOL-quoted, non-mayhem coins whose buys are all
//! visible (`intx_exact`). The logs path sums the `Program data:` TradeEvents.

use crate::constants::{self as c, ix as disc, MINT_AUTHORITY, PUMP_PROGRAM_ID};
use crate::curve::{CurveState, FeeBps};
use crate::error::DecodeError;
use crate::events::{
    decode_cpi_event, decode_event_payload, EventSource, InnerInstruction, PumpEvent,
};
use crate::instructions::{
    classify, AccountLookup, Classified, CreateKind, PumpCreate, PumpInstruction, PumpTrade,
    PumpTradeKind, TradeAmounts, TradeLayout,
};
use crate::launchers::{is_infra_program, Launcher};
use crate::tx::{TxVersion, TxView};
use base64::Engine as _;
use solana_pubkey::Pubkey;

/// Signers kept in a [`CreateHit`].
pub const MAX_SIGNERS: usize = 4;
/// Buyers (signers + credited owners) kept in a [`CreateHit`].
pub const MAX_BUYERS: usize = 4;

/// Where a [`CreateHit`] was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HitSource {
    /// Raw transaction bytes (shreds, gRPC, `getTransaction`).
    #[default]
    Wire,
    /// `logsSubscribe` log lines (no fee payer, no owners: paper only).
    Logs,
}

/// Fixed-capacity, `Copy` list of distinct keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyList<const N: usize> {
    keys: [Pubkey; N],
    len: u8,
    /// Distinct keys offered, including the ones that did not fit.
    total: u8,
}

impl<const N: usize> Default for KeyList<N> {
    fn default() -> Self {
        Self {
            keys: [Pubkey::default(); N],
            len: 0,
            total: 0,
        }
    }
}

impl<const N: usize> KeyList<N> {
    /// Add `k` if not present; returns `true` when it was new.
    pub fn push(&mut self, k: Pubkey) -> bool {
        if self.as_slice().contains(&k) {
            return false;
        }
        self.total = self.total.saturating_add(1);
        if (self.len as usize) < N {
            self.keys[self.len as usize] = k;
            self.len += 1;
        }
        true
    }
    pub fn as_slice(&self) -> &[Pubkey] {
        &self.keys[..self.len as usize]
    }
    pub fn contains(&self, k: &Pubkey) -> bool {
        self.as_slice().contains(k)
    }
    pub fn len(&self) -> usize {
        self.len as usize
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Distinct keys seen, including overflow beyond `N`.
    pub fn total(&self) -> usize {
        self.total as usize
    }
    pub fn overflowed(&self) -> bool {
        self.total > self.len
    }
}

/// Bytes of the create's uri host kept in a [`TxShape`].
pub const URI_HOST_MAX: usize = 64;
/// Bytes of the whole create uri kept in a [`TxShape`] (the tweet check,
/// `entry.tweet_gate`, fetches it; the launch tools' uris are 30-60 bytes).
pub const URI_MAX: usize = 160;
/// The runtime's default compute-unit limit per non-builtin instruction,
/// per builtin one (System, ComputeBudget), and the cap (when the
/// transaction sets no limit).
pub const DEFAULT_IX_CU: u64 = 200_000;
pub const BUILTIN_IX_CU: u64 = 3_000;
pub const MAX_CU_LIMIT: u64 = 1_400_000;

/// The create transaction's shape (`Copy`, bounded): what the entry
/// filter's create-transaction and uri features read before the send
/// (`filter::create_tx_features_wire`). Wire path only, except the uri,
/// which the logs path fills from the `CreateEvent`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxShape {
    /// Top-level instructions.
    pub n_ix: u16,
    /// Top-level System and ComputeBudget instructions (the default
    /// compute-unit limit reserves [`BUILTIN_IX_CU`] for each).
    pub n_builtin_ix: u16,
    /// Required signatures (message header).
    pub n_signers: u8,
    /// The last top-level `SetComputeUnitLimit` / `SetComputeUnitPrice`
    /// (micro-lamports per CU) instruction; none on a v1 transaction.
    pub cu_limit: Option<u32>,
    pub cu_price: Option<u64>,
    /// A v1 transaction's total priority fee (lamports, message config).
    pub v1_priority: Option<u64>,
    /// Lamports of the System transfers / account creations (top level and,
    /// with meta, by CPI) into a [`c::JITO_TIP_ACCOUNTS`] account.
    pub tip_lamports: u64,
    /// The create's uri contains `ipfs`.
    pub uri_ipfs: bool,
    /// Host of the uri (`https?://` prefix, up to the first `/`, ASCII lower
    /// case); `uri_host_len == 0`: no such prefix, an empty host, or one
    /// longer than [`URI_HOST_MAX`] (none of the filter's hosts).
    pub uri_host: [u8; URI_HOST_MAX],
    pub uri_host_len: u8,
    /// The whole uri as given (not lower-cased), when it has such a host
    /// and fits in [`URI_MAX`] bytes; `uri_len == 0`: not kept.
    pub uri: [u8; URI_MAX],
    pub uri_len: u8,
}

impl Default for TxShape {
    fn default() -> Self {
        Self {
            n_ix: 0,
            n_builtin_ix: 0,
            n_signers: 0,
            cu_limit: None,
            cu_price: None,
            v1_priority: None,
            tip_lamports: 0,
            uri_ipfs: false,
            uri_host: [0; URI_HOST_MAX],
            uri_host_len: 0,
            uri: [0; URI_MAX],
            uri_len: 0,
        }
    }
}

impl TxShape {
    /// Set the uri fields from a create's uri (no allocation).
    pub fn set_uri(&mut self, uri: &str) {
        self.uri_ipfs = uri.contains("ipfs");
        self.uri_host_len = 0;
        self.uri_len = 0;
        let Some(rest) = uri
            .strip_prefix("https://")
            .or_else(|| uri.strip_prefix("http://"))
        else {
            return;
        };
        let host = rest.split('/').next().unwrap_or("").as_bytes();
        if host.is_empty() || host.len() > URI_HOST_MAX {
            return;
        }
        for (d, b) in self.uri_host.iter_mut().zip(host) {
            *d = b.to_ascii_lowercase();
        }
        self.uri_host_len = host.len() as u8;
        if uri.len() <= URI_MAX {
            self.uri[..uri.len()].copy_from_slice(uri.as_bytes());
            self.uri_len = uri.len() as u8;
        }
    }

    /// The whole uri (the `uri` field), if kept.
    pub fn uri_str(&self) -> Option<&str> {
        let n = self.uri_len as usize;
        if n == 0 {
            return None;
        }
        core::str::from_utf8(&self.uri[..n]).ok()
    }

    /// The uri host (lower case), if any.
    pub fn uri_host(&self) -> Option<&str> {
        let n = self.uri_host_len as usize;
        if n == 0 {
            return None;
        }
        core::str::from_utf8(&self.uri_host[..n]).ok()
    }

    /// The priority fee the runtime charges (lamports): the v1 total, else
    /// `ceil(price * limit / 1e6)` with the requested limit (capped at
    /// [`MAX_CU_LIMIT`]) or, without one, the default limit; 0 without a
    /// price.
    pub fn priority_lamports(&self) -> u64 {
        if let Some(p) = self.v1_priority {
            return p;
        }
        let Some(price) = self.cu_price else {
            return 0;
        };
        let limit = match self.cu_limit {
            Some(l) => (l as u64).min(MAX_CU_LIMIT),
            None => {
                let other = self.n_ix.saturating_sub(self.n_builtin_ix) as u64;
                (other * DEFAULT_IX_CU + self.n_builtin_ix as u64 * BUILTIN_IX_CU).min(MAX_CU_LIMIT)
            }
        };
        u64::try_from((price as u128 * limit as u128).div_ceil(1_000_000)).unwrap_or(u64::MAX)
    }
}

/// `(destination, lamports)` of a System transfer or account creation
/// (`transfer`, `transferWithSeed`, `createAccount`, `createAccountWithSeed`).
fn system_credit<A: AccountLookup + ?Sized>(data: &[u8], a: &A) -> Option<(Pubkey, u64)> {
    let u64_at = |o: usize| {
        data.get(o..o + 8)
            .map(|b| u64::from_le_bytes(b.try_into().unwrap()))
    };
    let tag = u32::from_le_bytes(data.get(..4)?.try_into().ok()?);
    let (dst, lamports) = match tag {
        // CreateAccount { lamports, space, owner } / Transfer { lamports }: [from, to]
        0 | 2 => (a.account(1)?, u64_at(4)?),
        // CreateAccountWithSeed { base, seed, lamports, .. }: [from, to, base?]
        3 => {
            let n = u64_at(36)? as usize;
            (a.account(1)?, u64_at(44usize.checked_add(n)?)?)
        }
        // TransferWithSeed { lamports, .. }: [from, base, to]
        11 => (a.account(2)?, u64_at(4)?),
        _ => return None,
    };
    (lamports > 0 && a.account(0).is_some()).then_some((dst, lamports))
}

/// The shape of a create transaction (see [`TxShape`]); `uri` is the
/// create's argument.
pub fn tx_shape(
    tx: &TxView<'_>,
    loaded: Option<&[Pubkey]>,
    inner: &[InnerInstruction<'_>],
    uri: &str,
) -> TxShape {
    let mut s = TxShape {
        n_ix: tx.num_instructions().min(u16::MAX as usize) as u16,
        n_signers: tx.header().num_required_signatures,
        v1_priority: tx.v1_config().and_then(|c| c.priority_fee_lamports),
        ..TxShape::default()
    };
    s.set_uri(uri);
    let tip = |k: &Pubkey| c::JITO_TIP_ACCOUNTS.contains(k);
    for ix in tx.instructions() {
        let p = tx.program_id(&ix);
        if *p == c::COMPUTE_BUDGET_PROGRAM_ID {
            s.n_builtin_ix = s.n_builtin_ix.saturating_add(1);
            let d = ix.data;
            match d.first() {
                Some(2) if d.len() >= 5 => {
                    s.cu_limit = Some(u32::from_le_bytes(d[1..5].try_into().unwrap()))
                }
                Some(3) if d.len() >= 9 => {
                    s.cu_price = Some(u64::from_le_bytes(d[1..9].try_into().unwrap()))
                }
                _ => {}
            }
        } else if *p == c::SYSTEM_PROGRAM_ID {
            s.n_builtin_ix = s.n_builtin_ix.saturating_add(1);
            if let Some((d, l)) = system_credit(ix.data, &tx.instruction_accounts(&ix, loaded)) {
                if tip(&d) {
                    s.tip_lamports = s.tip_lamports.saturating_add(l);
                }
            }
        }
    }
    for inn in inner {
        if tx.account_key(inn.program_id_index as usize, loaded) != Some(c::SYSTEM_PROGRAM_ID) {
            continue;
        }
        if let Some((d, l)) = system_credit(inn.data, &tx.indexed_accounts(inn.accounts, loaded)) {
            if tip(&d) {
                s.tip_lamports = s.tip_lamports.saturating_add(l);
            }
        }
    }
    s
}

/// Everything the hot path needs about one create (`Copy`, no heap).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreateHit {
    pub source: HitSource,
    /// Filled by the feed (not part of the transaction).
    pub slot: Option<u64>,
    /// First signature (transaction id).
    pub sig: [u8; 64],
    /// `None` on the logs path.
    pub version: Option<TxVersion>,
    /// `None` on the logs path (the event does not say).
    pub kind: Option<CreateKind>,
    pub mint: Pubkey,
    pub bc: Pubkey,
    /// Curve's base token account (create account #3; derived on the logs path).
    pub abc: Pubkey,
    /// Curve's quote ATA when an in-tx v2/v3 buy names it.
    pub aqbc_hint: Option<Pubkey>,
    /// The create's `user` (a signer; pays the create).
    pub user: Pubkey,
    /// `None` on the logs path.
    pub fee_payer: Option<Pubkey>,
    /// The `creator` argument (can differ from the signer: 18.9% of creates).
    /// Logs path: `CreateEvent.creator`, which equals the argument except on
    /// holder-reward coins (a pump-controlled address, see `holder_reward`).
    pub creator_arg: Pubkey,
    /// `is_holder_reward` coin: the curve's recorded creator is not the dev.
    pub holder_reward: bool,
    /// Transaction signers (logs path: the create's user only).
    pub signers: KeyList<MAX_SIGNERS>,
    /// Quote mint; wrapped SOL for SOL-paired coins. `None` if it sits in an
    /// unresolved lookup table.
    pub quote: Option<Pubkey>,
    pub token_prog: Pubkey,
    pub mayhem: bool,
    pub cashback: bool,
    /// Sum over every in-tx buy of the lamports into the curve (before fees).
    pub intx_sol_in: u64,
    /// Sum over every in-tx buy of the tokens out.
    pub intx_tokens_out: u64,
    /// Number of in-tx buys of this mint.
    pub intx_buys: u8,
    /// Buy signers and owners of the credited token accounts.
    pub intx_buyers: KeyList<MAX_BUYERS>,
    /// `true` when the totals are exact (events, or a full simulation of
    /// visible buys on a SOL, non-mayhem curve with no hidden CPI).
    pub intx_exact: bool,
    /// Curve after all in-tx buys when `intx_exact` (reserves are exact; the
    /// `creator` field can differ between paths because pump-fees may migrate
    /// the curve's fee creator inside the create transaction).
    pub curve_after: Option<CurveState>,
    /// How the create reached pump.
    pub launcher: Launcher,
    /// Logs path: the runtime truncated the logs, buys may be missing.
    pub logs_truncated: bool,
    /// The create transaction's shape (entry-filter features before the send).
    pub tx_shape: TxShape,
}

impl CreateHit {
    /// `true` for SOL-paired coins.
    pub fn is_sol_quote(&self) -> bool {
        self.quote.is_some_and(|q| c::is_sol_like_quote_mint(&q))
    }
    /// Base58 transaction id.
    pub fn signature_b58(&self) -> String {
        bs58::encode(&self.sig).into_string()
    }
    /// Virtual SOL after the in-tx buys (`None` if not exact).
    pub fn v_after(&self) -> Option<u64> {
        self.curve_after.map(|s| s.virtual_sol)
    }
}

/// Why no [`CreateHit`] could be built for a transaction that does create.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitError {
    /// Malformed instruction data.
    Decode(DecodeError),
    /// The create is a CPI from a launcher and no inner instructions were
    /// given (shreds / pre-execution). `launcher` is the top-level program
    /// that passes the mint authority (best guess).
    CpiCreateNeedsMeta { launcher: Launcher },
    /// A required create account lives in an unresolved lookup table.
    UnresolvedAccounts,
}

impl From<DecodeError> for HitError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

impl core::fmt::Display for HitError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "{e}"),
            Self::CpiCreateNeedsMeta { launcher } => {
                write!(
                    f,
                    "create via launcher CPI ({}) needs inner instructions",
                    launcher.name()
                )
            }
            Self::UnresolvedAccounts => write!(f, "create accounts in an unresolved lookup table"),
        }
    }
}

impl std::error::Error for HitError {}

/// One pump buy found in a transaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuyInfo {
    pub kind: PumpTradeKind,
    pub amounts: TradeAmounts,
    pub mint: Option<Pubkey>,
    /// Signer of the buy.
    pub user: Option<Pubkey>,
    /// Token account credited with the tokens.
    pub token_account: Option<Pubkey>,
    /// Owner of `token_account` when an ATA create / token-account init in the
    /// same transaction names it.
    pub owner: Option<Pubkey>,
    /// Curve quote ATA (v2 #12, v3 #7).
    pub associated_quote_bonding_curve: Option<Pubkey>,
    pub outer_index: u8,
    /// `None` for a top-level buy.
    pub inner_index: Option<u16>,
}

/// Credited token account of a buy (legacy #5, v2 #14, v3 #9).
fn token_account_of<A: AccountLookup + ?Sized>(t: &PumpTrade, a: &A) -> Option<Pubkey> {
    match t.kind.layout() {
        TradeLayout::LegacyBuy => a.account(5),
        TradeLayout::V2Buy => a.account(14),
        TradeLayout::V3 => a.account(9),
        _ => None,
    }
}

fn aqbc_of<A: AccountLookup + ?Sized>(t: &PumpTrade, a: &A) -> Option<Pubkey> {
    match t.kind.layout() {
        TradeLayout::V2Buy => a.account(12),
        TradeLayout::V3 => a.account(7),
        _ => None,
    }
}

/// `(token_account, owner)` if this instruction creates or initializes a
/// token account: ATA `Create`/`CreateIdempotent` (#1 ata, #2 owner), token
/// `InitializeAccount` (tag 1, #2 owner), `InitializeAccount2/3` (tags 16/18,
/// owner in data).
fn token_account_init<A: AccountLookup + ?Sized>(
    program: &Pubkey,
    data: &[u8],
    a: &A,
) -> Option<(Pubkey, Pubkey)> {
    if *program == c::ASSOCIATED_TOKEN_PROGRAM_ID {
        // data: [] (Create), [0] Create, [1] CreateIdempotent
        if data.len() <= 1 && data.first().is_none_or(|t| *t <= 1) {
            return Some((a.account(1)?, a.account(2)?));
        }
        return None;
    }
    if *program == c::SPL_TOKEN_PROGRAM_ID || *program == c::TOKEN_2022_PROGRAM_ID {
        return match data.first()? {
            1 => Some((a.account(0)?, a.account(2)?)),
            16 | 18 if data.len() >= 33 => Some((
                a.account(0)?,
                Pubkey::new_from_array(data[1..33].try_into().ok()?),
            )),
            _ => None,
        };
    }
    None
}

/// Pump buys a scan keeps per transaction (more are counted, not kept).
pub const MAX_SCAN_BUYS: usize = 32;
/// Token-account creations / inits a scan keeps per transaction.
pub const MAX_SCAN_INITS: usize = 32;

/// A fixed-capacity list (no heap: `create_hit` runs on the hot thread,
/// which must not allocate after warm-up).
struct Fixed<T: Copy, const N: usize> {
    items: [Option<T>; N],
    len: usize,
    /// Pushes refused because the list was full.
    dropped: usize,
}

impl<T: Copy, const N: usize> Fixed<T, N> {
    fn new() -> Self {
        Self {
            items: [None; N],
            len: 0,
            dropped: 0,
        }
    }
    fn push(&mut self, v: T) {
        if self.len < N {
            self.items[self.len] = Some(v);
            self.len += 1;
        } else {
            self.dropped += 1;
        }
    }
    fn iter(&self) -> impl Iterator<Item = &T> {
        self.items[..self.len].iter().flatten()
    }
    fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.items[..self.len].iter_mut().flatten()
    }
}

/// Scan state shared by top-level and inner instructions.
struct Scan<'a> {
    create: Option<(PumpCreate<'a>, u8, Option<u16>)>,
    buys: Fixed<BuyInfo, MAX_SCAN_BUYS>,
    inits: Fixed<(Pubkey, Pubkey), MAX_SCAN_INITS>,
}

/// Every pump buy (any variant, any mint) in a transaction, in execution
/// order, with credited owners resolved from same-transaction ATA creates or
/// token-account inits. `inner` may be empty (pre-execution bytes); then CPI
/// buys are invisible.
pub fn pump_buys(
    tx: &TxView<'_>,
    loaded: Option<&[Pubkey]>,
    inner: &[InnerInstruction<'_>],
) -> Result<Vec<BuyInfo>, DecodeError> {
    Ok(scan(tx, loaded, inner)?.buys.iter().copied().collect())
}

fn on_ix<'a, A: AccountLookup>(
    s: &mut Scan<'a>,
    program: &Pubkey,
    data: &'a [u8],
    a: &A,
    outer: u8,
    inner_idx: Option<u16>,
) -> Result<(), DecodeError> {
    if let Some(init) = token_account_init(program, data, a) {
        s.inits.push(init);
        return Ok(());
    }
    if *program != PUMP_PROGRAM_ID || data.get(..8) == Some(&c::EVENT_IX_TAG[..]) {
        return Ok(());
    }
    match classify(program, data, a)? {
        Classified::Pump(PumpInstruction::Create(cr)) => {
            if s.create.is_none() {
                s.create = Some((cr, outer, inner_idx));
            }
        }
        Classified::Pump(PumpInstruction::Trade(t)) if t.kind.is_buy() => {
            s.buys.push(BuyInfo {
                kind: t.kind,
                amounts: t.amounts,
                mint: t.mint,
                user: t.user,
                token_account: token_account_of(&t, a),
                owner: None,
                associated_quote_bonding_curve: aqbc_of(&t, a),
                outer_index: outer,
                inner_index: inner_idx,
            });
        }
        _ => {}
    }
    Ok(())
}

fn scan<'a>(
    tx: &TxView<'a>,
    loaded: Option<&'a [Pubkey]>,
    inner: &[InnerInstruction<'a>],
) -> Result<Scan<'a>, DecodeError> {
    let mut s = Scan {
        create: None,
        buys: Fixed::new(),
        inits: Fixed::new(),
    };
    // Execution order: top-level ix i, then the CPIs it made (meta lists
    // inner instructions grouped by ascending outer index).
    let mut k = 0usize;
    for (i, ix) in tx.instructions().enumerate() {
        let accts = tx.instruction_accounts(&ix, loaded);
        on_ix(&mut s, tx.program_id(&ix), ix.data, &accts, i as u8, None)?;
        while k < inner.len() && (inner[k].outer_index as usize) < i {
            k += 1; // out-of-order group: skipped
        }
        let mut pos = 0u16;
        while k < inner.len() && inner[k].outer_index as usize == i {
            let inn = &inner[k];
            if let Some(program) = tx.account_key(inn.program_id_index as usize, loaded) {
                let accts = tx.indexed_accounts(inn.accounts, loaded);
                on_ix(
                    &mut s,
                    &program,
                    inn.data,
                    &accts,
                    inn.outer_index,
                    Some(pos),
                )?;
            }
            pos = pos.saturating_add(1);
            k += 1;
        }
    }
    let Scan { buys, inits, .. } = &mut s;
    for b in buys.iter_mut() {
        if let Some(ta) = b.token_account {
            b.owner = inits.iter().find(|(acc, _)| *acc == ta).map(|(_, o)| *o);
        }
    }
    Ok(s)
}

/// Build a [`CreateHit`] from a parsed transaction.
///
/// * `loaded`: `meta.loadedAddresses` (writable then readonly) for v0 lookups,
///   if known;
/// * `inner`: `meta.innerInstructions` (empty for shreds / pre-execution);
/// * `fees`: fee schedule for the wire simulation ([`FeeBps::DEFAULT`]).
///
/// `Ok(None)`: not a create. A create via launcher CPI without `inner`
/// returns [`HitError::CpiCreateNeedsMeta`].
pub fn create_hit(
    tx: &TxView<'_>,
    loaded: Option<&[Pubkey]>,
    inner: &[InnerInstruction<'_>],
    fees: FeeBps,
) -> Result<Option<CreateHit>, HitError> {
    let s = scan(tx, loaded, inner)?;
    let Some((cr, outer, inner_idx)) = s.create else {
        // A CPI create is recognisable by the mint authority + pump program in
        // the keys with no top-level create.
        let keys = tx.static_account_keys();
        let has = |k: &Pubkey| keys.contains(k) || loaded.is_some_and(|l| l.contains(k));
        if inner.is_empty() && has(&MINT_AUTHORITY) && has(&PUMP_PROGRAM_ID) {
            let ma = keys.iter().position(|k| *k == MINT_AUTHORITY);
            let launcher = tx
                .instructions()
                .find(|ix| {
                    !is_infra_program(tx.program_id(ix))
                        && ma.is_none_or(|m| ix.accounts.contains(&(m as u8)))
                })
                .map(|ix| Launcher::from_program(tx.program_id(&ix)))
                .unwrap_or(Launcher::Unknown(Pubkey::default()));
            return Err(HitError::CpiCreateNeedsMeta { launcher });
        }
        return Ok(None);
    };
    let (Some(mint), Some(bc), Some(abc), Some(user), Some(token_prog)) = (
        cr.mint,
        cr.bonding_curve,
        cr.associated_bonding_curve,
        cr.user,
        cr.token_program,
    ) else {
        return Err(HitError::UnresolvedAccounts);
    };
    let launcher = if inner_idx.is_none() {
        Launcher::Direct
    } else {
        tx.instruction(outer as usize)
            .map(|ix| Launcher::from_program(tx.program_id(&ix)))
            .unwrap_or(Launcher::Unknown(Pubkey::default()))
    };
    let quote = match cr.is_sol_quote {
        Some(true) => Some(c::WSOL_MINT),
        _ => cr.quote_mint,
    };
    let mayhem = cr.args.is_mayhem_mode;
    let cashback = cr.args.is_cashback_enabled.unwrap_or(false);

    let mut signers = KeyList::<MAX_SIGNERS>::default();
    for k in tx.signer_keys() {
        signers.push(*k);
    }

    // In-tx buys of this mint, after the create.
    let mut buyers = KeyList::<MAX_BUYERS>::default();
    let mut aqbc_hint = None;
    let mut n = 0u8;
    let mut sim = CurveState::fresh(cr.args.creator);
    let sol_quote = quote.is_some_and(|q| c::is_sol_like_quote_mint(&q));
    // a buy beyond the scan's capacity is invisible to the simulation
    let mut exact = sol_quote && !mayhem && s.buys.dropped == 0;
    let (mut sim_sol, mut sim_tok) = (0u64, 0u64);
    for b in s.buys.iter().filter(|b| b.mint == Some(mint)) {
        n = n.saturating_add(1);
        if let Some(u) = b.user {
            buyers.push(u);
        }
        if let Some(o) = b.owner {
            buyers.push(o);
        }
        if aqbc_hint.is_none() {
            aqbc_hint = b.associated_quote_bonding_curve;
        }
        if !exact {
            continue;
        }
        let q = match b.amounts {
            TradeAmounts::ExactQuoteIn {
                spendable_quote_in, ..
            } => sim.quote_buy_exact_sol_in(spendable_quote_in, fees),
            TradeAmounts::ExactTokensOut {
                amount,
                max_quote_cost,
            } => sim
                .quote_buy_exact_tokens(amount, fees)
                .filter(|q| q.total <= max_quote_cost),
            TradeAmounts::Sell { .. } => None,
        };
        match q {
            Some(q) => {
                sim.apply_buy(q.tokens, q.sol_cost);
                sim_sol = sim_sol.saturating_add(q.sol_cost);
                sim_tok = sim_tok.saturating_add(q.tokens);
            }
            None => exact = false,
        }
    }
    // Hidden buys: a non-infra top-level program (other than a launcher that
    // made the create, whose CPIs we see when `inner` is given) may buy by CPI.
    if inner.is_empty() {
        for ix in tx.instructions() {
            let p = tx.program_id(&ix);
            if !is_infra_program(p) && *p != PUMP_PROGRAM_ID {
                exact = false;
            }
        }
    }

    // With meta: the TradeEvents are the truth.
    let (mut ev_sol, mut ev_tok, mut have_events) = (0u64, 0u64, false);
    let mut ev_state: Option<CurveState> = None;
    for inn in inner {
        let Some(program) = tx.account_key(inn.program_id_index as usize, loaded) else {
            continue;
        };
        if let Ok(Some(PumpEvent::Trade(t))) = decode_cpi_event(&program, inn.data) {
            if t.mint == mint && t.is_buy {
                have_events = true;
                ev_sol = ev_sol.saturating_add(t.sol_amount);
                ev_tok = ev_tok.saturating_add(t.token_amount);
                ev_state = t.post_state();
            }
        }
    }
    let (intx_sol_in, intx_tokens_out, intx_exact, curve_after) = if have_events {
        (ev_sol, ev_tok, true, ev_state)
    } else {
        (
            sim_sol,
            sim_tok,
            exact,
            if exact { Some(sim) } else { None },
        )
    };

    let mut sig = [0u8; 64];
    if let Some(s0) = tx.signature(0) {
        sig = *s0;
    }
    Ok(Some(CreateHit {
        source: HitSource::Wire,
        slot: None,
        sig,
        version: Some(tx.version()),
        kind: Some(cr.kind),
        mint,
        bc,
        abc,
        aqbc_hint,
        user,
        fee_payer: Some(*tx.fee_payer()),
        creator_arg: cr.args.creator,
        holder_reward: cr.args.is_holder_reward.unwrap_or(false),
        signers,
        quote,
        token_prog,
        mayhem,
        cashback,
        intx_sol_in,
        intx_tokens_out,
        intx_buys: n,
        intx_buyers: buyers,
        intx_exact,
        curve_after,
        launcher,
        logs_truncated: false,
        tx_shape: tx_shape(tx, loaded, inner, cr.args.uri),
    }))
}

/// Build a [`CreateHit`] from a `logsSubscribe` notification of a
/// **successful** transaction (`value.err == null`; the caller filters
/// failures). `signature` is the notification's base58 signature. Returns
/// `Ok(None)` when the logs hold no pump `CreateEvent`.
///
/// No fee payer and no token owners (logs do not carry them), so a hit
/// from this path is paper-only (docs/06 section 7.1 "Free mode"). The
/// in-tx totals sum the `TradeEvent`s logged after the create; when the
/// runtime truncated the logs, `logs_truncated` is set and `intx_exact` is
/// false.
pub fn from_logs<S: AsRef<str>>(
    signature: &str,
    logs: &[S],
) -> Result<Option<CreateHit>, DecodeError> {
    let mut stack: Vec<Pubkey> = Vec::with_capacity(8);
    let mut hit: Option<CreateHit> = None;
    let mut truncated = false;
    let mut buf: Vec<u8> = Vec::with_capacity(512);
    for line in logs {
        let line = line.as_ref();
        if let Some(rest) = line.strip_prefix("Program data: ") {
            if stack.last() != Some(&PUMP_PROGRAM_ID) {
                continue;
            }
            buf.clear();
            for part in rest.split(' ').filter(|p| !p.is_empty()) {
                base64::engine::general_purpose::STANDARD
                    .decode_vec(part, &mut buf)
                    .map_err(|_| DecodeError::InvalidBase64)?;
            }
            match decode_event_payload(EventSource::Pump, &buf)? {
                PumpEvent::Create(ev) if hit.is_none() => {
                    let token_prog = ev.token_program.unwrap_or(c::SPL_TOKEN_PROGRAM_ID);
                    let quote = match ev.quote_mint {
                        None => Some(c::WSOL_MINT),
                        Some(q) if c::is_sol_like_quote_mint(&q) => Some(c::WSOL_MINT),
                        Some(q) => Some(q),
                    };
                    let launcher = match stack.first() {
                        Some(root) if *root != PUMP_PROGRAM_ID => Launcher::from_program(root),
                        _ => Launcher::Direct,
                    };
                    let mayhem = ev.is_mayhem_mode.unwrap_or(false);
                    let sol_quote = quote.is_some_and(|q| c::is_sol_like_quote_mint(&q));
                    let mut signers = KeyList::default();
                    signers.push(ev.user);
                    let mut sig = [0u8; 64];
                    if let Ok(v) = bs58::decode(signature).into_vec() {
                        if v.len() == 64 {
                            sig.copy_from_slice(&v);
                        }
                    }
                    let fresh = CurveState {
                        virtual_sol: ev.virtual_sol_reserves,
                        virtual_token: ev.virtual_token_reserves,
                        real_sol: 0,
                        real_token: ev.real_token_reserves,
                        complete: false,
                        creator: ev.creator,
                    };
                    hit = Some(CreateHit {
                        source: HitSource::Logs,
                        slot: None,
                        sig,
                        version: None,
                        kind: None,
                        mint: ev.mint,
                        bc: ev.bonding_curve,
                        abc: crate::pda::associated_bonding_curve(&ev.mint, &token_prog),
                        aqbc_hint: None,
                        user: ev.user,
                        fee_payer: None,
                        creator_arg: ev.creator,
                        holder_reward: ev.is_holder_reward.unwrap_or(false),
                        signers,
                        quote,
                        token_prog,
                        mayhem,
                        cashback: ev.is_cashback_enabled.unwrap_or(false),
                        intx_sol_in: 0,
                        intx_tokens_out: 0,
                        intx_buys: 0,
                        intx_buyers: KeyList::default(),
                        intx_exact: sol_quote && !mayhem,
                        curve_after: if sol_quote && !mayhem {
                            Some(fresh)
                        } else {
                            None
                        },
                        launcher,
                        logs_truncated: false,
                        tx_shape: {
                            let mut s = TxShape::default();
                            s.set_uri(ev.uri);
                            s
                        },
                    });
                }
                PumpEvent::Trade(t) => {
                    if let Some(h) = hit.as_mut() {
                        if t.mint == h.mint && t.is_buy {
                            h.intx_sol_in = h.intx_sol_in.saturating_add(t.sol_amount);
                            h.intx_tokens_out = h.intx_tokens_out.saturating_add(t.token_amount);
                            h.intx_buys = h.intx_buys.saturating_add(1);
                            h.intx_buyers.push(t.user);
                            if h.curve_after.is_some() {
                                h.curve_after = t.post_state();
                            }
                        }
                    }
                }
                _ => {}
            }
        } else if line == "Log truncated" {
            truncated = true;
        } else if let Some(rest) = line.strip_prefix("Program ") {
            let mut it = rest.splitn(2, ' ');
            let (Some(id), Some(tail)) = (it.next(), it.next()) else {
                continue;
            };
            if tail.starts_with("invoke [") {
                if let Ok(pk) = id.parse::<Pubkey>() {
                    stack.push(pk);
                }
            } else if tail == "success" || tail.starts_with("failed") {
                if let Ok(pk) = id.parse::<Pubkey>() {
                    if stack.last() == Some(&pk) {
                        stack.pop();
                    }
                }
            }
        }
    }
    if let Some(h) = hit.as_mut() {
        if truncated {
            h.logs_truncated = true;
            h.intx_exact = false;
            h.curve_after = None;
        }
    }
    Ok(hit)
}

/// Discriminators of every pump create instruction (cheap pre-filter on raw
/// instruction data).
pub const CREATE_DISCRIMINATORS: [[u8; 8]; 2] = [disc::CREATE, disc::CREATE_V2];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_uri_keeps_the_host_lower_case_and_the_whole_uri_as_given() {
        let mut s = TxShape::default();
        s.set_uri("https://Metadata.J7tracker.io/metadata/qxtvmbCFDM.json");
        assert_eq!(s.uri_host(), Some("metadata.j7tracker.io"));
        assert_eq!(
            s.uri_str(),
            Some("https://Metadata.J7tracker.io/metadata/qxtvmbCFDM.json")
        );
        // too long for the buffer: the host stays, the uri is not kept
        let long = format!("https://meta.uxento.io/data/{}", "x".repeat(URI_MAX));
        s.set_uri(&long);
        assert_eq!(s.uri_host(), Some("meta.uxento.io"));
        assert_eq!(s.uri_str(), None);
        // no http(s) host: neither
        s.set_uri("ipfs://bafkreicze4mkc5faqjzqq6rxgmlurz26cr6mknsmekyd6abtwmjqelh5ei");
        assert_eq!((s.uri_host(), s.uri_str()), (None, None));
        assert!(s.uri_ipfs);
    }

    #[test]
    fn keylist_dedups_and_counts_overflow() {
        let mut l = KeyList::<2>::default();
        let k: Vec<Pubkey> = (0..4).map(|_| Pubkey::new_unique()).collect();
        assert!(l.push(k[0]));
        assert!(!l.push(k[0]));
        assert!(l.push(k[1]));
        assert!(l.push(k[2]));
        assert_eq!(l.as_slice(), &k[..2]);
        assert_eq!(l.total(), 3);
        assert!(l.overflowed());
        assert!(l.contains(&k[1]) && !l.contains(&k[2]));
    }

    #[test]
    fn logs_without_create_give_none() {
        let logs = [
            "Program 11111111111111111111111111111111 invoke [1]",
            "Program 11111111111111111111111111111111 success",
        ];
        assert!(from_logs("1", &logs).unwrap().is_none());
    }
}
