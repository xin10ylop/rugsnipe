//! # rugsnipe-sim
//!
//! Simulates our real buy transactions against a live bonding curve
//! (docs/06 sections 13-14.6): template legality, compute units consumed,
//! loaded-accounts bytes, and errors, for the legacy and the v1 templates,
//! with loose limits (to measure) and the tight per-mint limits (to
//! check they are enough). The tight CU limit is the hot path's per-mint
//! `65,000 + 1,500·S` (docs/research/gap-5.md section 1,
//! [`rugsnipe_tx::cu`]), patched over a compiled floor exactly as the hot
//! path does, with no floor applied so the formula itself is tested;
//! [`fit`] compares the loose measurement with the model. Simulation uses `sigVerify: false`, so any
//! funded wallet can stand in as the fee payer; without a nonce,
//! `replaceRecentBlockhash: true`; with a nonce the live nonce value is the
//! lifetime and the account's authority is the (unverified) second signer.
//!
//! Loaded bytes are also **computed** from account sizes (speed-gap-4
//! section 2.2): `Σ (data_len + 64)` over every existing account in the
//! transaction, plus `programdata_len + 64` for each upgradeable program.
//! Pure parts live here; the RPC driver is behind the `rpc` feature.

use pump_codec::constants as c;
use pump_codec::instructions::SolV3Keys;
use pump_codec::{pda, Pubkey};
use rugsnipe_core::config::TxFormat;
use rugsnipe_tx::cu::{snipe_cu_limit, RECOMMENDED_CU_FLOOR};
use rugsnipe_tx::template::{CurveParams, MintKeys, NonceKeys, Patch, Template, TemplateError};

/// BPF upgradeable loader.
pub const BPF_UPGRADEABLE_LOADER: Pubkey =
    Pubkey::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");
/// Loose limits used to measure (max CU, 64 MiB LADS).
pub const LOOSE: (u32, u32) = (1_400_000, 64 * 1024 * 1024);
/// Tight LADS (docs/03 section 6: 370 pages). The tight CU limit is per
/// mint: [`tight_cu`].
pub const TIGHT_LADS: u32 = 12_124_160;
/// Our template's measured cost above the gap-5 `buy_v3` envelope
/// (`buy_exact_quote_in_v3`, UVA present; hot-path verifier).
pub const TEMPLATE_CU_OVER_GAP5: u32 = 1_185;
/// A legacy compute-budget instruction (3 in the legacy prelude) [L].
pub const LEGACY_CB_IX_CU: u32 = 150;

/// `S = Σ (255 - bump)` of the snipe's four PDAs for `wallet` on `mint`.
pub fn snipe_s(wallet: &Pubkey, mint: &Pubkey, base_token_program: &Pubkey) -> u32 {
    pda::SnipeBumps::derive(mint, wallet, base_token_program).missing()
}

/// The hot path's per-mint limit for `S`, without the config floor.
pub fn tight_cu(s: u32) -> u32 {
    snipe_cu_limit(s, 0)
}

/// Expected consumption of our buy template (UVA present): the gap-5
/// envelope `62,310 + 1,500·S` (with nonce; 150 less without) plus the
/// template's +1,185, plus 150 per legacy compute-budget instruction.
pub fn template_cu_model(format: TxFormat, nonce: bool, s: u32) -> u32 {
    let mut v = rugsnipe_tx::cu::buy_v3_cu_model(s, nonce, false) + TEMPLATE_CU_OVER_GAP5;
    if format == TxFormat::Legacy {
        v += 3 * LEGACY_CB_IX_CU;
    }
    v
}

/// One account as returned by `getMultipleAccounts` (null = missing).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcctInfo {
    pub key: Pubkey,
    pub owner: Pubkey,
    pub space: u64,
    pub executable: bool,
    /// For upgradeable programs: the programdata address (from data[4..36]).
    pub programdata: Option<Pubkey>,
}

/// `Σ (space + 64)` over existing accounts plus `programdata + 64` per
/// upgradeable program (`programdata` sizes looked up in `pd_sizes`).
pub fn loaded_bytes(accts: &[Option<AcctInfo>], pd_sizes: &[(Pubkey, u64)]) -> u64 {
    let mut total = 0u64;
    for a in accts.iter().flatten() {
        total += a.space + 64;
        if a.owner == BPF_UPGRADEABLE_LOADER && a.executable {
            if let Some(pd) = a.programdata {
                if let Some((_, sz)) = pd_sizes.iter().find(|(k, _)| *k == pd) {
                    total += sz + 64;
                }
            }
        }
    }
    total
}

/// One template to simulate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Case {
    pub format: TxFormat,
    pub nonce: bool,
    pub tight: bool,
}

impl Case {
    pub fn label(&self) -> String {
        format!(
            "{}{} {}",
            match self.format {
                TxFormat::Legacy => "legacy",
                TxFormat::V1 => "v1",
            },
            if self.nonce { "+nonce" } else { "" },
            if self.tight { "tight" } else { "loose" }
        )
    }
    /// `(cu_limit, lads)` for a mint with `s` missing bumps.
    pub fn limits(&self, s: u32) -> (u32, u32) {
        if self.tight {
            (tight_cu(s), TIGHT_LADS)
        } else {
            LOOSE
        }
    }
}

/// Build and patch the buy template for `mint` (SOL-quoted curve). Like
/// the hot path, the template is compiled with a floor
/// ([`RECOMMENDED_CU_FLOOR`] for tight cases) and the case's CU limit is
/// written by the patch.
pub fn build_case(
    case: &Case,
    wallet: &Pubkey,
    mint: &Pubkey,
    base_token_program: &Pubkey,
    nonce: Option<NonceKeys>,
    lifetime: &[u8; 32],
    spend: u64,
    priority_lamports: u64,
    tip_lamports: u64,
) -> Result<Template, TemplateError> {
    let s = snipe_s(wallet, mint, base_token_program);
    let (cu, lads) = case.limits(s);
    let p = CurveParams {
        format: case.format,
        user: *wallet,
        nonce: if case.nonce { nonce } else { None },
        cu_limit: if case.tight { RECOMMENDED_CU_FLOOR } else { cu },
        lads_bytes: lads,
        tip_account: Some(c::JITO_TIP_ACCOUNTS[0]),
        buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[0],
        base_token_program: *base_token_program,
        partial_fill: Some(false),
    };
    let mut t = Template::buy(&p)?;
    let keys = MintKeys::from_v3(&SolV3Keys::derive(
        *mint,
        *wallet,
        *base_token_program,
        p.buyback_fee_recipient,
    ));
    t.patch(&Patch {
        keys: &keys,
        lifetime,
        amount: spend,
        limit: 1,
        priority_lamports,
        tip_lamports,
        cu_limit: Some(cu),
    });
    Ok(t)
}

/// Result of one simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct SimRow {
    pub label: String,
    pub format: TxFormat,
    pub nonce: bool,
    pub tight: bool,
    /// `Σ (255 - bump)` of the four snipe PDAs.
    pub s: u32,
    pub size: usize,
    pub cu_limit: u32,
    pub lads_limit: u32,
    pub err: Option<String>,
    pub units_consumed: Option<u64>,
    /// `loadedAccountsDataSize` as reported by the RPC (if it does).
    pub loaded_reported: Option<u64>,
    pub fee: Option<u64>,
    pub logs_tail: Vec<String>,
}

/// Measured vs formula for one mint (from a loose run).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fit {
    pub label: String,
    pub s: u32,
    pub measured: u64,
    /// [`template_cu_model`].
    pub model: u32,
    /// `measured - model`.
    pub residual: i64,
    /// The per-mint limit `65,000 + 1,500·S` ([`tight_cu`]).
    pub limit: u32,
    /// `limit - measured` (negative = the limit would fail this mint).
    pub headroom: i64,
}

/// The fit of a successful loose run (`None` for tight runs, errors or a
/// missing `unitsConsumed`).
pub fn fit(row: &SimRow) -> Option<Fit> {
    if row.tight || row.err.is_some() {
        return None;
    }
    let measured = row.units_consumed?;
    let model = template_cu_model(row.format, row.nonce, row.s);
    let limit = tight_cu(row.s);
    Some(Fit {
        label: row.label.clone(),
        s: row.s,
        measured,
        model,
        residual: measured as i64 - model as i64,
        limit,
        headroom: limit as i64 - measured as i64,
    })
}

/// One line per fit plus a summary (max |residual|, min headroom).
pub fn fit_report(mint_fits: &[(Pubkey, Fit)]) -> String {
    use std::fmt::Write as _;
    let mut s = format!(
        "{:<46} {:<18} {:>3} {:>9} {:>9} {:>8} {:>9} {:>8}\n",
        "mint", "template", "S", "measured", "model", "resid", "limit", "headroom"
    );
    for (m, f) in mint_fits {
        let _ = writeln!(
            s,
            "{:<46} {:<18} {:>3} {:>9} {:>9} {:>+8} {:>9} {:>8}",
            m.to_string(),
            f.label,
            f.s,
            f.measured,
            f.model,
            f.residual,
            f.limit,
            f.headroom
        );
    }
    if let (Some(r), Some(h)) = (
        mint_fits.iter().map(|(_, f)| f.residual.abs()).max(),
        mint_fits.iter().map(|(_, f)| f.headroom).min(),
    ) {
        let _ = writeln!(
            s,
            "fit: {} run(s), max |residual| {r} CU, min headroom {h} CU -> {}",
            mint_fits.len(),
            if h > 0 && r <= 300 {
                "formula OK"
            } else if h > 0 {
                "limit OK, model off by > 300 CU"
            } else {
                "LIMIT TOO LOW"
            }
        );
    } else {
        s.push_str("fit: no successful loose run\n");
    }
    s
}

#[cfg(feature = "rpc")]
pub mod live;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loaded_bytes_formula() {
        let pd = Pubkey::new_unique();
        let prog = AcctInfo {
            key: c::PUMP_PROGRAM_ID,
            owner: BPF_UPGRADEABLE_LOADER,
            space: 36,
            executable: true,
            programdata: Some(pd),
        };
        let data = AcctInfo {
            key: c::GLOBAL,
            owner: c::PUMP_PROGRAM_ID,
            space: 1_088,
            executable: false,
            programdata: None,
        };
        // pump programdata is 10 MiB preallocated (+45 bytes header)
        let total = loaded_bytes(&[Some(prog), Some(data), None], &[(pd, 10_485_760)]);
        assert_eq!(total, (36 + 64) + (10_485_760 + 64) + (1_088 + 64));
    }

    #[test]
    fn cases_build() {
        let w = Pubkey::new_unique();
        let m = Pubkey::new_unique();
        for f in [TxFormat::Legacy, TxFormat::V1] {
            for tight in [false, true] {
                let case = Case {
                    format: f,
                    nonce: true,
                    tight,
                };
                let n = NonceKeys {
                    account: Pubkey::new_unique(),
                    authority: Pubkey::new_unique(),
                };
                let t = build_case(
                    &case,
                    &w,
                    &m,
                    &c::TOKEN_2022_PROGRAM_ID,
                    Some(n),
                    &[1; 32],
                    10_000_000,
                    1_000,
                    1_000,
                )
                .unwrap();
                assert!(t.len() <= rugsnipe_tx::MAX_TX_LEN);
                assert_eq!(t.table.n_sigs, 2);
            }
        }
        assert_eq!(
            Case {
                format: TxFormat::V1,
                nonce: true,
                tight: true
            }
            .limits(4),
            (71_000, 12_124_160)
        );
        assert_eq!(
            Case {
                format: TxFormat::V1,
                nonce: false,
                tight: false
            }
            .label(),
            "v1 loose"
        );
    }

    #[test]
    fn tight_case_patches_the_per_mint_limit() {
        let w = Pubkey::new_unique();
        let m = Pubkey::new_unique();
        let s = snipe_s(&w, &m, &c::TOKEN_2022_PROGRAM_ID);
        for format in [TxFormat::Legacy, TxFormat::V1] {
            let case = Case {
                format,
                nonce: false,
                tight: true,
            };
            let t = build_case(
                &case,
                &w,
                &m,
                &c::TOKEN_2022_PROGRAM_ID,
                None,
                &[0; 32],
                1,
                5_000,
                0,
            )
            .unwrap();
            let cb = pump_codec::tx::TxView::parse(t.bytes())
                .unwrap()
                .compute_budget();
            assert_eq!(cb.compute_unit_limit, Some(65_000 + 1_500 * s));
            assert_eq!(t.cu_limit(), tight_cu(s));
        }
    }

    #[test]
    fn fit_rows_and_report() {
        let row = |tight: bool, units: Option<u64>, err: Option<&str>| SimRow {
            label: "v1+nonce loose".into(),
            format: TxFormat::V1,
            nonce: true,
            tight,
            s: 4,
            size: 880,
            cu_limit: 1_400_000,
            lads_limit: 1,
            err: err.map(str::to_string),
            units_consumed: units,
            loaded_reported: None,
            fee: None,
            logs_tail: vec![],
        };
        // 62,310 + 6,000 + 1,185 = 69,495 expected; limit 71,000
        let f = fit(&row(false, Some(69_500), None)).unwrap();
        assert_eq!(
            (f.model, f.residual, f.limit, f.headroom),
            (69_495, 5, 71_000, 1_500)
        );
        assert!(fit(&row(true, Some(69_500), None)).is_none());
        assert!(fit(&row(false, Some(69_500), Some("x"))).is_none());
        assert!(fit(&row(false, None, None)).is_none());
        assert_eq!(
            template_cu_model(TxFormat::Legacy, false, 0),
            62_160 + 1_185 + 450
        );
        let m = Pubkey::new_unique();
        let rep = fit_report(&[(m, f.clone())]);
        assert!(rep.contains("formula OK"), "{rep}");
        let mut bad = f;
        bad.headroom = -10;
        assert!(fit_report(&[(m, bad)]).contains("LIMIT TOO LOW"));
        assert!(fit_report(&[]).contains("no successful loose run"));
    }
}
