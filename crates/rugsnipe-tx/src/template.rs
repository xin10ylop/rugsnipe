//! Transaction templates with a fixed-offset [`PatchTable`].
//!
//! A template is compiled once per (wallet, nonce account, sender/tip
//! account, format) with placeholder keys; per create the hot path copies
//! the five mint-dependent keys, the lifetime (nonce value or blockhash) and
//! a few little-endian integers into fixed offsets, then signs. No heap, no
//! re-serialization.
//!
//! **Buy** (docs/03 section 6, docs/06 section 7.1):
//!
//! | legacy | v1 (mask 0x0F) |
//! |---|---|
//! | `AdvanceNonceAccount` (optional, first) | same |
//! | `SetComputeUnitLimit`, `SetComputeUnitPrice`, `SetLoadedAccountsDataSizeLimit` | header config: priority (total lamports), CU limit, LADS |
//! | ATA `CreateIdempotent` (user's Token-2022 account) | same |
//! | `buy_exact_quote_in_v3` (17 accounts; `spendable` is gross) | same |
//! | tip transfer (optional; inside the tx so a reverted buy pays no tip) | same |
//!
//! v1 numbers from docs/research/speed-gap-4.md: LADS 12,124,160 (370
//! pages); 805 B, 880 B with a nonce. The compute-unit limit is patched
//! **per mint** (docs/research/gap-5.md section 1, [`crate::cu`]): the
//! template is compiled with the configured floor, and [`Patch::cu_limit`]
//! writes `65,000 + 1,500·S` (clamped to `[floor, 1,400,000]`) into the v1
//! header field or the legacy `SetComputeUnitLimit` data, and re-derives the
//! legacy CU price from the lamport total so the fee paid stays the total.
//!
//! Key order (fixed so the PatchTable holds): fee payer; nonce authority if
//! distinct; writable `bc, abc, aqbc, user_ata, uva, user_wsol_ata,
//! buyback, [nonce], [tip]`; read-only `mint, global, wsol, base token
//! program, SPL Token, fee_config, system, event_authority, pump, ATA
//! program, [ComputeBudget], [RecentBlockhashes]`.
//!
//! **Sell**: `sell_v3` (+ `CloseAccount` of the user's token account for a
//! final sell, recovering 1,513,840 lamports of rent). **PumpSwap sell**:
//! canonical-pool `sell_v2` (17 accounts) wrapped in an idempotent wSOL ATA
//! create and a wSOL `CloseAccount` (unwrap); see [`AmmSellParams`] for what
//! must be read from chain before using it.

use crate::msg::{compile, CompileError, Format, KeySpec, Layout, V1Config};
use crate::signer::HotSigner;
use crate::MAX_TX_LEN;
use pump_codec::constants::{self as c, amm_ix};
use pump_codec::instructions::{self as ixs, SolV3Keys, UserKeys};
use pump_codec::{pda, AccountMeta, Instruction, Pubkey};
use rugsnipe_core::config::TxFormat;

/// Durable-nonce accounts of a template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonceKeys {
    pub account: Pubkey,
    pub authority: Pubkey,
}

/// The five per-mint keys, in template order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MintKeys(pub [Pubkey; 5]);

impl MintKeys {
    /// Bonding-curve trades: `mint, bc, abc, aqbc, user_ata`.
    pub fn curve(mint: Pubkey, bc: Pubkey, abc: Pubkey, aqbc: Pubkey, user_ata: Pubkey) -> Self {
        Self([mint, bc, abc, aqbc, user_ata])
    }
    /// From derived v3 keys.
    pub fn from_v3(k: &SolV3Keys) -> Self {
        Self::curve(
            k.mint,
            k.bonding_curve,
            k.associated_base_bonding_curve,
            k.associated_quote_bonding_curve,
            k.associated_base_user,
        )
    }
    /// Bonding-curve keys only: the four snipe bumps recovered from these
    /// keys by hashing (`S + 4` SHA-256 rounds); `None` if a key is not the
    /// canonical PDA of its seeds.
    pub fn snipe_bumps(
        &self,
        user: &Pubkey,
        base_token_program: &Pubkey,
    ) -> Option<pda::SnipeBumps> {
        let [mint, bc, abc, aqbc, ua] = &self.0;
        pda::SnipeBumps::of_keys(mint, bc, abc, aqbc, user, ua, base_token_program)
    }
    /// Bonding-curve keys only: the per-mint CU limit
    /// ([`crate::cu::snipe_cu_limit`]) for these keys over `floor`, with the
    /// wallet's own term `wallet_missing` ([`crate::cu::wallet_missing_bumps`]
    /// of `user`, computed once per process) added to the mint's S.
    pub fn snipe_cu_limit(
        &self,
        user: &Pubkey,
        base_token_program: &Pubkey,
        wallet_missing: u32,
        floor: u32,
    ) -> Option<u32> {
        self.snipe_bumps(user, base_token_program)
            .map(|b| crate::cu::snipe_cu_limit(b.missing() + wallet_missing, floor))
    }
    /// PumpSwap canonical pool: `pool, mint, user_base_ata, pool_base_ata, pool_quote_ata`.
    pub fn amm(
        pool: Pubkey,
        mint: Pubkey,
        user_base: Pubkey,
        pool_base: Pubkey,
        pool_quote: Pubkey,
    ) -> Self {
        Self([pool, mint, user_base, pool_base, pool_quote])
    }
}

/// How the bid is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriorityField {
    /// Legacy `SetComputeUnitPrice` u64 micro-lamports per CU at `offset`;
    /// the patch converts a lamport total with `floor(total*1e6/cu_limit)`
    /// using the template's current (per-mint) CU limit.
    LegacyPrice { offset: usize },
    /// v1 config u64 total lamports at `offset`.
    V1Total { offset: usize },
    /// No priority field (legacy template without compute-budget ixs).
    None,
}

/// Byte offsets of everything that changes per create / per variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatchTable {
    /// The five mint-dependent keys (see [`MintKeys`]).
    pub keys: [usize; 5],
    /// Blockhash or nonce value.
    pub lifetime: usize,
    /// First u64 argument (buy: spendable lamports; sell: token amount).
    pub amount: usize,
    /// Second u64 argument (buy: `min_tokens_out`; sell: `min_sol_output`).
    pub limit: usize,
    pub priority: PriorityField,
    /// The u32 LE compute-unit limit: the v1 header config value, or the
    /// legacy `SetComputeUnitLimit` data + 1 (`None` if the template has
    /// neither).
    pub cu_limit: Option<usize>,
    /// Tip lamports (System transfer data + 4).
    pub tip: Option<usize>,
    /// The tip account key (a writable non-signer: any provider's tip
    /// account may be written here per variant, [`Template::set_tip_account`]).
    pub tip_account: Option<usize>,
    /// Signature block and count.
    pub sigs: usize,
    pub n_sigs: usize,
    pub message: (usize, usize),
}

/// Per-create values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Patch<'a> {
    pub keys: &'a MintKeys,
    pub lifetime: &'a [u8; 32],
    pub amount: u64,
    pub limit: u64,
    /// Total priority fee in lamports (converted to a CU price on legacy).
    pub priority_lamports: u64,
    pub tip_lamports: u64,
    /// Per-mint compute-unit limit (`crate::cu::snipe_cu_limit`); `None`
    /// keeps the template's current value (sells, tests).
    pub cu_limit: Option<u32>,
}

/// What a template does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemplateKind {
    Buy,
    Sell { close_account: bool },
    AmmSell,
}

/// A compiled transaction with its patch table.
#[derive(Clone)]
pub struct Template {
    buf: [u8; MAX_TX_LEN],
    len: usize,
    pub table: PatchTable,
    pub kind: TemplateKind,
    pub format: TxFormat,
    /// Signers in signature order (fee payer first).
    pub signers: [Option<Pubkey>; 2],
    /// The CU limit currently written in the message.
    cu_limit: u32,
    /// The priority total currently written (legacy: before conversion).
    priority_lamports: u64,
}

impl core::fmt::Debug for Template {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Template")
            .field("kind", &self.kind)
            .field("format", &self.format)
            .field("len", &self.len)
            .field("cu_limit", &self.cu_limit)
            .field("table", &self.table)
            .finish()
    }
}

/// Distinct placeholder keys (patched before use; never valid on chain).
const PLACEHOLDERS: [Pubkey; 5] = [
    Pubkey::new_from_array([0xf1; 32]),
    Pubkey::new_from_array([0xf2; 32]),
    Pubkey::new_from_array([0xf3; 32]),
    Pubkey::new_from_array([0xf4; 32]),
    Pubkey::new_from_array([0xf5; 32]),
];

/// Static parameters of buy and sell templates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CurveParams {
    pub format: TxFormat,
    /// Fee payer and trader.
    pub user: Pubkey,
    pub nonce: Option<NonceKeys>,
    /// Required (> 0): v1 transactions without it fail and still pay. The
    /// compiled value; buys patch the per-mint limit over it.
    pub cu_limit: u32,
    /// Required (> 0).
    pub lads_bytes: u32,
    /// `None` = no tip instruction (direct-TPU variant).
    pub tip_account: Option<Pubkey>,
    /// One of [`c::BUYBACK_FEE_RECIPIENTS`] (v3 account #13).
    pub buyback_fee_recipient: Pubkey,
    /// Token-2022 for `create_v2` coins, SPL Token for legacy `create` coins.
    pub base_token_program: Pubkey,
    /// Buy only.
    pub partial_fill: Option<bool>,
}

fn push_unique(v: &mut Vec<KeySpec>, k: KeySpec) {
    if !v.iter().any(|s| s.key == k.key) {
        v.push(k);
    }
}

/// SPL / Token-2022 `CloseAccount` (tag 9).
pub fn close_account(
    token_program: &Pubkey,
    account: &Pubkey,
    destination: &Pubkey,
    owner: &Pubkey,
) -> Instruction {
    Instruction {
        program_id: *token_program,
        accounts: vec![
            AccountMeta::new(*account, false),
            AccountMeta::new(*destination, false),
            AccountMeta::new_readonly(*owner, true),
        ],
        data: vec![9],
    }
}

fn v3_keys(p: &CurveParams, k: &MintKeys) -> SolV3Keys {
    let uk = UserKeys::derive(p.user);
    SolV3Keys {
        mint: k.0[0],
        base_token_program: p.base_token_program,
        bonding_curve: k.0[1],
        associated_base_bonding_curve: k.0[2],
        associated_quote_bonding_curve: k.0[3],
        user: p.user,
        associated_base_user: k.0[4],
        associated_quote_user: uk.wsol_ata,
        user_volume_accumulator: uk.user_volume_accumulator,
        buyback_fee_recipient: p.buyback_fee_recipient,
    }
}

fn prelude(format: TxFormat, nonce: Option<NonceKeys>, cu: u32, lads: u32) -> Vec<Instruction> {
    let mut v = vec![];
    if let Some(n) = nonce {
        v.push(ixs::advance_nonce_account(&n.account, &n.authority));
    }
    if format == TxFormat::Legacy {
        v.push(ixs::set_compute_unit_limit(cu));
        v.push(ixs::set_compute_unit_price(0));
        v.push(ixs::set_loaded_accounts_data_size_limit(lads));
    }
    v
}

/// The buy instructions from scratch with real values (the reference the
/// patched template must equal; also used by `ctl template dump` and sims).
pub fn buy_instructions(
    p: &CurveParams,
    k: &MintKeys,
    spend: u64,
    min_tokens_out: u64,
    priority_lamports: u64,
    tip: u64,
) -> Vec<Instruction> {
    let keys = v3_keys(p, k);
    let mut v = prelude(p.format, p.nonce, p.cu_limit, p.lads_bytes);
    if p.format == TxFormat::Legacy {
        v[usize::from(p.nonce.is_some()) + 1] =
            ixs::set_compute_unit_price(legacy_price(priority_lamports, p.cu_limit));
    }
    v.push(
        ixs::create_associated_token_account_idempotent_with_address(
            &p.user,
            &p.user,
            &keys.mint,
            &p.base_token_program,
            &keys.associated_base_user,
        ),
    );
    v.push(ixs::buy_exact_quote_in_v3(
        &keys,
        spend,
        min_tokens_out,
        p.partial_fill,
    ));
    if let Some(t) = p.tip_account {
        v.push(ixs::system_transfer(&p.user, &t, tip));
    }
    v
}

/// The sell instructions from scratch.
pub fn sell_instructions(
    p: &CurveParams,
    k: &MintKeys,
    close: bool,
    tokens: u64,
    min_sol_out: u64,
    priority_lamports: u64,
    tip: u64,
) -> Vec<Instruction> {
    let keys = v3_keys(p, k);
    let mut v = prelude(p.format, p.nonce, p.cu_limit, p.lads_bytes);
    if p.format == TxFormat::Legacy {
        v[usize::from(p.nonce.is_some()) + 1] =
            ixs::set_compute_unit_price(legacy_price(priority_lamports, p.cu_limit));
    }
    v.push(ixs::sell_v3(&keys, tokens, min_sol_out));
    if close {
        v.push(close_account(
            &p.base_token_program,
            &keys.associated_base_user,
            &p.user,
            &p.user,
        ));
    }
    if let Some(t) = p.tip_account {
        v.push(ixs::system_transfer(&p.user, &t, tip));
    }
    v
}

/// `floor(total * 1e6 / cu_limit)` micro-lamports per CU (never pays more than `total`).
pub fn legacy_price(total_lamports: u64, cu_limit: u32) -> u64 {
    if cu_limit == 0 {
        return 0;
    }
    u64::try_from(total_lamports as u128 * 1_000_000 / cu_limit as u128).unwrap_or(u64::MAX)
}

fn curve_key_order(p: &CurveParams, buy: bool) -> Vec<KeySpec> {
    let uk = UserKeys::derive(p.user);
    let mut v = vec![KeySpec::new(p.user, true, true)];
    if let Some(n) = p.nonce {
        if n.authority != p.user {
            push_unique(&mut v, KeySpec::new(n.authority, true, false));
        }
    }
    for k in &PLACEHOLDERS[1..5] {
        push_unique(&mut v, KeySpec::new(*k, false, true));
    }
    for k in [
        uk.user_volume_accumulator,
        uk.wsol_ata,
        p.buyback_fee_recipient,
    ] {
        push_unique(&mut v, KeySpec::new(k, false, true));
    }
    if let Some(n) = p.nonce {
        push_unique(&mut v, KeySpec::new(n.account, false, true));
    }
    if let Some(t) = p.tip_account {
        push_unique(&mut v, KeySpec::new(t, false, true));
    }
    let mut ro = vec![
        PLACEHOLDERS[0],
        c::GLOBAL,
        c::WSOL_MINT,
        p.base_token_program,
        c::SPL_TOKEN_PROGRAM_ID,
        c::FEE_CONFIG,
        c::SYSTEM_PROGRAM_ID,
        c::EVENT_AUTHORITY,
        c::PUMP_PROGRAM_ID,
    ];
    if buy {
        ro.push(c::ASSOCIATED_TOKEN_PROGRAM_ID);
    }
    if p.format == TxFormat::Legacy {
        ro.push(c::COMPUTE_BUDGET_PROGRAM_ID);
    }
    if p.nonce.is_some() {
        ro.push(c::RECENT_BLOCKHASHES_SYSVAR_ID);
    }
    for k in ro {
        push_unique(&mut v, KeySpec::new(k, false, false));
    }
    v
}

fn to_format(f: TxFormat, cu: u32, lads: u32) -> Format {
    match f {
        TxFormat::Legacy => Format::Legacy,
        TxFormat::V1 => Format::V1(V1Config {
            priority_fee_lamports: 0,
            compute_unit_limit: cu,
            loaded_accounts_data_size_limit: lads,
        }),
    }
}

/// Template build error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TemplateError {
    Compile(CompileError),
    /// `cu_limit` or `lads_bytes` is 0.
    MissingBudget,
}

impl From<CompileError> for TemplateError {
    fn from(e: CompileError) -> Self {
        Self::Compile(e)
    }
}

impl core::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TemplateError {}

fn find_key(keys: &[KeySpec], layout: &Layout, k: &Pubkey) -> usize {
    layout.key_offsets[keys
        .iter()
        .position(|s| s.key == *k)
        .expect("placeholder present")]
}

fn finish(
    keys: &[KeySpec],
    bytes: Vec<u8>,
    layout: Layout,
    data_ix: usize,
    price_ix: Option<usize>,
    tip_ix: Option<usize>,
    format: TxFormat,
    cu_limit: u32,
    kind: TemplateKind,
    placeholders: &[Pubkey; 5],
    tip_account: Option<Pubkey>,
) -> Template {
    let priority = match (format, price_ix, layout.v1_priority) {
        (TxFormat::V1, _, Some(o)) => PriorityField::V1Total { offset: o },
        (TxFormat::Legacy, Some(i), _) => PriorityField::LegacyPrice {
            offset: layout.ix_data[i] + 1,
        },
        _ => PriorityField::None,
    };
    // legacy prelude: [AdvanceNonce], SetComputeUnitLimit, SetComputeUnitPrice, ...
    let cu_field = match (format, price_ix, layout.v1_cu_limit) {
        (TxFormat::V1, _, Some(o)) => Some(o),
        (TxFormat::Legacy, Some(i), _) if i > 0 => Some(layout.ix_data[i - 1] + 1),
        _ => None,
    };
    let table = PatchTable {
        keys: placeholders.map(|p| find_key(keys, &layout, &p)),
        lifetime: layout.lifetime,
        amount: layout.ix_data[data_ix] + 8,
        limit: layout.ix_data[data_ix] + 16,
        priority,
        cu_limit: cu_field,
        tip: tip_ix.map(|i| layout.ix_data[i] + 4),
        tip_account: tip_account.map(|k| find_key(keys, &layout, &k)),
        sigs: layout.sigs,
        n_sigs: layout.n_sigs,
        message: layout.message,
    };
    let mut buf = [0u8; MAX_TX_LEN];
    buf[..bytes.len()].copy_from_slice(&bytes);
    let signers = [
        keys.first().map(|k| k.key),
        keys.get(1).filter(|k| k.signer).map(|k| k.key),
    ];
    Template {
        buf,
        len: bytes.len(),
        table,
        kind,
        format,
        signers,
        cu_limit,
        priority_lamports: 0,
    }
}

impl Template {
    /// Compile a buy template.
    pub fn buy(p: &CurveParams) -> Result<Self, TemplateError> {
        if p.cu_limit == 0 || p.lads_bytes == 0 {
            return Err(TemplateError::MissingBudget);
        }
        let keys = curve_key_order(p, true);
        let ph = MintKeys(PLACEHOLDERS);
        let ixs = buy_instructions(p, &ph, 0, 0, 0, 0);
        let (bytes, layout) = compile(
            &keys,
            &ixs,
            &[0u8; 32],
            to_format(p.format, p.cu_limit, p.lads_bytes),
        )?;
        let pre = prelude(p.format, p.nonce, p.cu_limit, p.lads_bytes).len();
        let price_ix = (p.format == TxFormat::Legacy).then_some(usize::from(p.nonce.is_some()) + 1);
        let tip_ix = p.tip_account.map(|_| pre + 2);
        Ok(finish(
            &keys,
            bytes,
            layout,
            pre + 1,
            price_ix,
            tip_ix,
            p.format,
            p.cu_limit,
            TemplateKind::Buy,
            &PLACEHOLDERS,
            p.tip_account,
        ))
    }

    /// Compile a sell template (`close_account` = final sell).
    pub fn sell(p: &CurveParams, close_account: bool) -> Result<Self, TemplateError> {
        if p.cu_limit == 0 || p.lads_bytes == 0 {
            return Err(TemplateError::MissingBudget);
        }
        let keys = curve_key_order(p, false);
        let ph = MintKeys(PLACEHOLDERS);
        let ixs = sell_instructions(p, &ph, close_account, 0, 0, 0, 0);
        let (bytes, layout) = compile(
            &keys,
            &ixs,
            &[0u8; 32],
            to_format(p.format, p.cu_limit, p.lads_bytes),
        )?;
        let pre = prelude(p.format, p.nonce, p.cu_limit, p.lads_bytes).len();
        let price_ix = (p.format == TxFormat::Legacy).then_some(usize::from(p.nonce.is_some()) + 1);
        let tip_ix = p.tip_account.map(|_| pre + 1 + usize::from(close_account));
        Ok(finish(
            &keys,
            bytes,
            layout,
            pre,
            price_ix,
            tip_ix,
            p.format,
            p.cu_limit,
            TemplateKind::Sell { close_account },
            &PLACEHOLDERS,
            p.tip_account,
        ))
    }

    /// Compile a PumpSwap canonical-pool sell template.
    pub fn amm_sell(p: &AmmSellParams) -> Result<Self, TemplateError> {
        if p.cu_limit == 0 || p.lads_bytes == 0 {
            return Err(TemplateError::MissingBudget);
        }
        let keys = amm_key_order(p);
        let ixs = amm_sell_instructions(p, &MintKeys(PLACEHOLDERS), 0, 0, 0, 0);
        let (bytes, layout) = compile(
            &keys,
            &ixs,
            &[0u8; 32],
            to_format(p.format, p.cu_limit, p.lads_bytes),
        )?;
        let pre = prelude(p.format, p.nonce, p.cu_limit, p.lads_bytes).len();
        let price_ix = (p.format == TxFormat::Legacy).then_some(usize::from(p.nonce.is_some()) + 1);
        let tip_ix = p.tip_account.map(|_| pre + 3);
        Ok(finish(
            &keys,
            bytes,
            layout,
            pre + 1,
            price_ix,
            tip_ix,
            p.format,
            p.cu_limit,
            TemplateKind::AmmSell,
            &PLACEHOLDERS,
            p.tip_account,
        ))
    }

    /// Serialized transaction.
    pub fn bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// The signed message.
    pub fn message(&self) -> &[u8] {
        &self.buf[self.table.message.0..self.table.message.1]
    }

    /// The compute-unit limit currently in the message.
    pub fn cu_limit(&self) -> u32 {
        self.cu_limit
    }

    /// The priority total (lamports) currently in the message.
    pub fn priority_lamports(&self) -> u64 {
        self.priority_lamports
    }

    /// The lifetime currently in the message (blockhash or nonce value).
    pub fn lifetime(&self) -> [u8; 32] {
        let o = self.table.lifetime;
        let mut l = [0u8; 32];
        l.copy_from_slice(&self.buf[o..o + 32]);
        l
    }

    /// Change only the lifetime (a blockhash resolved after the patch).
    /// Clears nothing (re-sign afterwards).
    #[inline]
    pub fn set_lifetime(&mut self, l: &[u8; 32]) {
        let o = self.table.lifetime;
        self.buf[o..o + 32].copy_from_slice(l);
    }

    /// Apply per-create values (a handful of `memcpy`s). Clears signatures.
    #[inline]
    pub fn patch(&mut self, p: &Patch<'_>) {
        let t = self.table;
        for (i, k) in p.keys.0.iter().enumerate() {
            self.buf[t.keys[i]..t.keys[i] + 32].copy_from_slice(k.as_ref());
        }
        self.buf[t.lifetime..t.lifetime + 32].copy_from_slice(p.lifetime);
        self.buf[t.amount..t.amount + 8].copy_from_slice(&p.amount.to_le_bytes());
        self.buf[t.limit..t.limit + 8].copy_from_slice(&p.limit.to_le_bytes());
        if let Some(cu) = p.cu_limit {
            self.write_cu_limit(cu);
        }
        self.set_priority(p.priority_lamports);
        if let Some(o) = t.tip {
            self.buf[o..o + 8].copy_from_slice(&p.tip_lamports.to_le_bytes());
        }
        self.buf[t.sigs..t.sigs + 64 * t.n_sigs].fill(0);
    }

    #[inline]
    fn write_cu_limit(&mut self, cu: u32) {
        if let Some(o) = self.table.cu_limit {
            self.buf[o..o + 4].copy_from_slice(&cu.to_le_bytes());
            self.cu_limit = cu;
        }
    }

    /// Change only the compute-unit limit (header field or legacy
    /// `SetComputeUnitLimit`), keeping the priority **total**: the legacy CU
    /// price is re-derived from it. Clears nothing (re-sign afterwards).
    #[inline]
    pub fn set_cu_limit(&mut self, cu: u32) {
        self.write_cu_limit(cu);
        self.set_priority(self.priority_lamports);
    }

    /// Change only the bid (per variant).
    #[inline]
    pub fn set_priority(&mut self, total_lamports: u64) {
        self.priority_lamports = total_lamports;
        match self.table.priority {
            PriorityField::LegacyPrice { offset } => self.buf[offset..offset + 8]
                .copy_from_slice(&legacy_price(total_lamports, self.cu_limit).to_le_bytes()),
            PriorityField::V1Total { offset } => {
                self.buf[offset..offset + 8].copy_from_slice(&total_lamports.to_le_bytes())
            }
            PriorityField::None => {}
        }
    }

    /// Change only the tip (per variant).
    #[inline]
    pub fn set_tip(&mut self, lamports: u64) {
        if let Some(o) = self.table.tip {
            self.buf[o..o + 8].copy_from_slice(&lamports.to_le_bytes());
        }
    }

    /// Change only the tip account (per sender: each provider has its own
    /// tip accounts). The key sits in the writable non-signer class, so any
    /// plain system account keeps the message valid; it must not equal
    /// another key of the message (provider tip accounts never do). Clears
    /// nothing (re-sign afterwards). `false` if the template has no tip.
    #[inline]
    pub fn set_tip_account(&mut self, account: &Pubkey) -> bool {
        match self.table.tip_account {
            Some(o) => {
                self.buf[o..o + 32].copy_from_slice(account.as_ref());
                true
            }
            None => false,
        }
    }

    /// The tip account currently in the message.
    pub fn tip_account(&self) -> Option<Pubkey> {
        self.table
            .tip_account
            .map(|o| Pubkey::new_from_array(self.buf[o..o + 32].try_into().unwrap()))
    }

    /// The tip lamports currently in the message.
    pub fn tip_lamports(&self) -> Option<u64> {
        self.table
            .tip
            .map(|o| u64::from_le_bytes(self.buf[o..o + 8].try_into().unwrap()))
    }

    /// Write signature `i` (signature order = signer key order).
    pub fn set_signature(&mut self, i: usize, sig: &[u8; 64]) {
        assert!(i < self.table.n_sigs);
        let o = self.table.sigs + 64 * i;
        self.buf[o..o + 64].copy_from_slice(sig);
    }

    /// Sign with the fee payer (and the nonce authority if it is a second
    /// signer). Panics if a signer's pubkey does not match the template.
    pub fn sign(&mut self, payer: &HotSigner, authority: Option<&HotSigner>) {
        assert_eq!(Some(payer.pubkey()), self.signers[0], "payer key mismatch");
        let sig = payer.sign(self.message());
        self.set_signature(0, &sig);
        if self.table.n_sigs > 1 {
            let a = authority.expect("template needs the nonce authority signature");
            assert_eq!(Some(a.pubkey()), self.signers[1], "authority key mismatch");
            let sig = a.sign(self.message());
            self.set_signature(1, &sig);
        }
    }

    /// First signature (the transaction id).
    pub fn signature(&self) -> [u8; 64] {
        self.buf[self.table.sigs..self.table.sigs + 64]
            .try_into()
            .unwrap()
    }
}

/// Static parameters of the PumpSwap canonical-pool sell.
///
/// Derivable without RPC (verified against a real `sell_v2` in the tests):
/// the canonical pool `["pool", 0u16, pool_authority(mint), mint, wSOL]`,
/// the pool's base / quote token accounts (ATAs of the pool), the user's
/// ATAs, PumpSwap's user volume accumulator, fee config and event authority.
///
/// **Must be fetched before use** (cold path, once per position):
/// * the `Pool` account: confirms the pool exists (migration happened),
///   `is_cashback_coin == false` (the v2 account set excludes cashback
///   pools) and the token program of the base mint;
/// * `GlobalConfig.buyback_fee_recipients`: the template's
///   `buyback_wallet` must be one of them (its wSOL ATA must exist);
/// * the pool reserves, for `min_quote_amount_out`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AmmSellParams {
    pub format: TxFormat,
    pub user: Pubkey,
    pub nonce: Option<NonceKeys>,
    pub cu_limit: u32,
    pub lads_bytes: u32,
    pub tip_account: Option<Pubkey>,
    /// A `GlobalConfig.buyback_fee_recipients` wallet (its wSOL ATA is account #14).
    pub buyback_wallet: Pubkey,
    pub base_token_program: Pubkey,
}

/// Canonical-pool per-mint keys for a SOL-quoted coin.
pub fn amm_mint_keys(mint: &Pubkey, user: &Pubkey, base_token_program: &Pubkey) -> MintKeys {
    amm_mint_keys_for_quote(
        mint,
        &c::WSOL_MINT,
        user,
        base_token_program,
        &c::SPL_TOKEN_PROGRAM_ID,
    )
}

/// Canonical-pool per-mint keys for any quote mint (`["pool", 0u16,
/// pool_authority(mint), mint, quote]`; the pool's vaults are its ATAs).
pub fn amm_mint_keys_for_quote(
    mint: &Pubkey,
    quote: &Pubkey,
    user: &Pubkey,
    base_token_program: &Pubkey,
    quote_token_program: &Pubkey,
) -> MintKeys {
    let pool = pda::canonical_pump_swap_pool(mint, quote);
    MintKeys::amm(
        pool,
        *mint,
        pda::associated_token_address(user, mint, base_token_program),
        pda::associated_token_address(&pool, mint, base_token_program),
        pda::associated_token_address(&pool, quote, quote_token_program),
    )
}

/// `sell_v2` on the canonical pool (17 accounts, pump_amm IDL order).
pub fn amm_sell_v2(
    p: &AmmSellParams,
    k: &MintKeys,
    base_amount_in: u64,
    min_quote_amount_out: u64,
) -> Instruction {
    let w = |k: Pubkey| AccountMeta::new(k, false);
    let ro = |k: Pubkey| AccountMeta::new_readonly(k, false);
    let mut data = Vec::with_capacity(24);
    data.extend_from_slice(&amm_ix::SELL_V2);
    data.extend_from_slice(&base_amount_in.to_le_bytes());
    data.extend_from_slice(&min_quote_amount_out.to_le_bytes());
    Instruction {
        program_id: c::PUMP_SWAP_PROGRAM_ID,
        accounts: vec![
            w(k.0[0]),
            AccountMeta::new(p.user, true),
            ro(c::PUMP_SWAP_GLOBAL_CONFIG),
            ro(k.0[1]),
            ro(c::WSOL_MINT),
            w(k.0[2]),
            w(pda::ata_spl(&p.user, &c::WSOL_MINT)),
            w(k.0[3]),
            w(k.0[4]),
            ro(p.base_token_program),
            ro(c::SPL_TOKEN_PROGRAM_ID),
            ro(c::SYSTEM_PROGRAM_ID),
            w(pda::pump_swap_user_volume_accumulator(&p.user)),
            ro(c::PUMP_SWAP_FEE_CONFIG),
            w(pda::ata_spl(&p.buyback_wallet, &c::WSOL_MINT)),
            ro(c::PUMP_SWAP_EVENT_AUTHORITY),
            ro(c::PUMP_SWAP_PROGRAM_ID),
        ],
        data,
    }
}

/// The PumpSwap sell from scratch: create wSOL ATA, `sell_v2`, unwrap.
pub fn amm_sell_instructions(
    p: &AmmSellParams,
    k: &MintKeys,
    tokens: u64,
    min_out: u64,
    priority_lamports: u64,
    tip: u64,
) -> Vec<Instruction> {
    let mut v = prelude(p.format, p.nonce, p.cu_limit, p.lads_bytes);
    if p.format == TxFormat::Legacy {
        v[usize::from(p.nonce.is_some()) + 1] =
            ixs::set_compute_unit_price(legacy_price(priority_lamports, p.cu_limit));
    }
    let wsol_ata = pda::ata_spl(&p.user, &c::WSOL_MINT);
    v.push(
        ixs::create_associated_token_account_idempotent_with_address(
            &p.user,
            &p.user,
            &c::WSOL_MINT,
            &c::SPL_TOKEN_PROGRAM_ID,
            &wsol_ata,
        ),
    );
    v.push(amm_sell_v2(p, k, tokens, min_out));
    v.push(close_account(
        &c::SPL_TOKEN_PROGRAM_ID,
        &wsol_ata,
        &p.user,
        &p.user,
    ));
    if let Some(t) = p.tip_account {
        v.push(ixs::system_transfer(&p.user, &t, tip));
    }
    v
}

fn amm_key_order(p: &AmmSellParams) -> Vec<KeySpec> {
    let mut v = vec![KeySpec::new(p.user, true, true)];
    if let Some(n) = p.nonce {
        if n.authority != p.user {
            push_unique(&mut v, KeySpec::new(n.authority, true, false));
        }
    }
    // writable: pool, user_base, pool_base, pool_quote, then statics
    for i in [0, 2, 3, 4] {
        push_unique(&mut v, KeySpec::new(PLACEHOLDERS[i], false, true));
    }
    for k in [
        pda::ata_spl(&p.user, &c::WSOL_MINT),
        pda::pump_swap_user_volume_accumulator(&p.user),
        pda::ata_spl(&p.buyback_wallet, &c::WSOL_MINT),
    ] {
        push_unique(&mut v, KeySpec::new(k, false, true));
    }
    if let Some(n) = p.nonce {
        push_unique(&mut v, KeySpec::new(n.account, false, true));
    }
    if let Some(t) = p.tip_account {
        push_unique(&mut v, KeySpec::new(t, false, true));
    }
    let mut ro = vec![
        PLACEHOLDERS[1],
        c::PUMP_SWAP_GLOBAL_CONFIG,
        c::WSOL_MINT,
        p.base_token_program,
        c::SPL_TOKEN_PROGRAM_ID,
        c::SYSTEM_PROGRAM_ID,
        c::PUMP_SWAP_FEE_CONFIG,
        c::PUMP_SWAP_EVENT_AUTHORITY,
        c::PUMP_SWAP_PROGRAM_ID,
        c::ASSOCIATED_TOKEN_PROGRAM_ID,
    ];
    if p.format == TxFormat::Legacy {
        ro.push(c::COMPUTE_BUDGET_PROGRAM_ID);
    }
    if p.nonce.is_some() {
        ro.push(c::RECENT_BLOCKHASHES_SYSVAR_ID);
    }
    for k in ro {
        push_unique(&mut v, KeySpec::new(k, false, false));
    }
    v
}
