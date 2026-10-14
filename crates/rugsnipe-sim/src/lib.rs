//! # rugsnipe-sim
//!
//! Simulates our real buy transactions against a live bonding curve
//! (docs/06 sections 13-14.6): template legality, compute units consumed,
//! loaded-accounts bytes, and errors, for the legacy and the v1 templates,
//! with loose limits (to measure) and the configured tight limits (to
//! check they are enough). Simulation uses `sigVerify: false`, so any
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
use pump_codec::Pubkey;
use rugsnipe_core::config::TxFormat;
use rugsnipe_tx::template::{CurveParams, MintKeys, NonceKeys, Patch, Template, TemplateError};

/// BPF upgradeable loader.
pub const BPF_UPGRADEABLE_LOADER: Pubkey =
    Pubkey::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");
/// Loose limits used to measure (max CU, 64 MiB LADS).
pub const LOOSE: (u32, u32) = (1_400_000, 64 * 1024 * 1024);
/// Recommended tight limits (docs/03 section 6): CU without / with nonce, LADS.
pub const TIGHT_CU: u32 = 72_000;
pub const TIGHT_CU_NONCE: u32 = 72_150;
pub const TIGHT_LADS: u32 = 12_124_160;

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
    pub fn limits(&self) -> (u32, u32) {
        if self.tight {
            (
                if self.nonce { TIGHT_CU_NONCE } else { TIGHT_CU },
                TIGHT_LADS,
            )
        } else {
            LOOSE
        }
    }
}

/// Build and patch the buy template for `mint` (SOL-quoted curve).
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
    let (cu, lads) = case.limits();
    let p = CurveParams {
        format: case.format,
        user: *wallet,
        nonce: if case.nonce { nonce } else { None },
        cu_limit: cu,
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
    });
    Ok(t)
}

/// Result of one simulation.
#[derive(Debug, Clone, PartialEq)]
pub struct SimRow {
    pub label: String,
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
            .limits(),
            (72_150, 12_124_160)
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
}
