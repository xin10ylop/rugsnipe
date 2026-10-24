//! Startup wiring shared by the binaries: config, exit policy, ArmSet
//! verification, templates, nonce pool, the paper signer.

use pump_codec::constants::{self as c, TOKEN_2022_PROGRAM_ID};
use pump_codec::curve::{CurveState, FeeBps, K_INITIAL};
use pump_codec::Pubkey;
use rugsnipe_core::armset::{self, ArmSet, OpState, VerifyCtx};
use rugsnipe_core::config::{ArmSetGate, Config};
use rugsnipe_core::Mode;
use rugsnipe_exit::{ExitConfig, ExitPolicy};
use rugsnipe_tx::nonce::NoncePool;
use rugsnipe_tx::signer::HotSigner;
use rugsnipe_tx::sysix;
use rugsnipe_tx::template::{CurveParams, NonceKeys, Template};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use zeroize::Zeroizing;

/// CU floor when `tx.cu_limit = 0` in paper: the recommended 65,000 (the
/// per-mint formula at S = 0). The hot path patches the per-mint limit
/// `65,000 + 1,500*S` over this floor ([`rugsnipe_tx::cu::snipe_cu_limit`]);
/// sells keep a static 88,000 ([`rugsnipe_tx::cu::sell_cu_limit`]).
pub const PAPER_CU_LIMIT: u32 = rugsnipe_tx::cu::RECOMMENDED_CU_FLOOR;
/// 370 pages.
pub const PAPER_LADS_BYTES: u32 = 12_124_160;
/// Fallback `max_sell_spend` when `[risk]` leaves it 0 (paper only).
pub const DEFAULT_MAX_SELL_SPEND: u64 = 20_000_000;

/// One armed (or shadow) operator as the hot loop sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpInfo {
    pub op_id: u32,
    /// Slot in the RiskGate's per-operator table.
    pub slot: usize,
    pub state: OpState,
    pub size: u64,
    pub v_max: u64,
    pub ev_cap: u64,
    pub ab_skip_p: f64,
}

/// The ArmSet facts the hot loop reads (swapped atomically on reload).
#[derive(Debug, Clone, Default)]
pub struct ArmState {
    pub ver: u64,
    /// Signature, sha, freshness and regime verified.
    pub verified: bool,
    /// Live may buy (signed `gate_passed`).
    pub gate_passed: bool,
    /// Probe may buy (signed `probe_allowed`; the gate is not required).
    pub probe_allowed: bool,
    /// The probe stage the ArmSet names (must match `probe.stage`).
    pub probe_stage: Option<rugsnipe_core::config::ProbeStage>,
    pub created_at: u64,
    pub sha: String,
    pub ops: HashMap<u32, OpInfo>,
}

impl ArmState {
    pub fn from_armset(a: &ArmSet, verified: bool, sha: &str) -> Self {
        let ops = a
            .operators
            .iter()
            .enumerate()
            .filter(|(i, _)| *i < rugsnipe_core::risk::MAX_OPS)
            .map(|(i, o)| {
                (
                    o.op_id,
                    OpInfo {
                        op_id: o.op_id,
                        slot: i,
                        state: o.state,
                        size: o.size_lamports,
                        v_max: o.v_max_lamports,
                        ev_cap: o.ev_cap_lamports,
                        ab_skip_p: o.ab_skip_p,
                    },
                )
            })
            .collect();
        Self {
            ver: a.ver,
            verified,
            gate_passed: verified && a.gate_passed,
            probe_allowed: verified && a.probe_allowed,
            probe_stage: a.probe_stage,
            created_at: a.created_at,
            sha: sha.to_string(),
            ops,
        }
    }

    /// Operators whose size no real buy could pass: in probe mode the
    /// effective `max_probe_lamports` (the `[risk]` cap narrowed by the
    /// stage's), in live `max_snipe_lamports`. Such an ArmSet makes the
    /// box skip every launch as `skip_spend_cap` while looking healthy
    /// (2026-10-22: 25 minutes at 0.5 SOL with the `[risk]` cap left at
    /// 0.25), so the start refuses it. Paper is not checked.
    pub fn size_cap_refusals(
        &self,
        eff: &rugsnipe_core::config::RiskCfg,
        mode: Mode,
    ) -> Vec<String> {
        let (cap, name) = match mode {
            Mode::Probe => (eff.max_probe_lamports, "max_probe_lamports"),
            Mode::Live => (eff.max_snipe_lamports, "max_snipe_lamports"),
            Mode::Paper => return vec![],
        };
        let active = |s: OpState| match mode {
            Mode::Probe => s.may_probe(),
            _ => s == OpState::Armed,
        };
        let mut ops: Vec<&OpInfo> = self.ops.values().filter(|o| active(o.state)).collect();
        ops.sort_by_key(|o| o.op_id);
        ops.iter()
            .filter(|o| cap > 0 && o.size > cap)
            .map(|o| {
                format!(
                    "ArmSet {} op {} size {} lamports exceeds the effective {name} {cap}                      (the smaller of [risk] and the [probe] stage's): the hot loop clamps the buy size to the cap (2026-10-23); raise both caps to buy the ArmSet size",
                    self.ver, o.op_id, o.size
                )
            })
            .collect()
    }

    pub fn gate(&self) -> ArmSetGate {
        if self.verified {
            ArmSetGate::Verified {
                gate_passed: self.gate_passed,
                probe_allowed: self.probe_allowed,
            }
        } else {
            ArmSetGate::Unsigned
        }
    }
}

/// Read and parse the TOML (unknown keys refused).
pub fn load_config(path: &Path) -> Result<Config, String> {
    let s = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Config::from_toml_str(&s).map_err(|e| format!("{}: {e}", path.display()))
}

/// `[exit.default]` as the integer policy (`max_sell_spend` from `[risk]`).
pub fn exit_policy(cfg: &Config) -> Result<ExitPolicy, String> {
    let spend = if cfg.risk.max_sell_spend_lamports > 0 {
        cfg.risk.max_sell_spend_lamports
    } else {
        DEFAULT_MAX_SELL_SPEND
    };
    let ec: ExitConfig = match cfg.exit.get("default") {
        Some(v) => v
            .clone()
            .try_into()
            .map_err(|e| format!("[exit.default]: {e}"))?,
        None => ExitConfig::default(),
    };
    ec.to_policy(spend)
        .map_err(|e| format!("[exit.default]: {e:?}"))
}

/// Result of loading an ArmSet file.
#[derive(Debug, Clone)]
pub struct ArmLoad {
    /// The verified ArmSet (`None` if verification failed).
    pub armset: Option<ArmSet>,
    pub gate: ArmSetGate,
    pub sha: String,
    pub error: Option<String>,
}

/// Verify `path` (+ `path.sig`) against `secrets.armset_pubkey`, the sha in
/// the file name, freshness (`risk.armset_max_age_s`), regime and version.
pub fn load_armset_file(path: &Path, cfg: &Config, now_unix: u64, regime: Option<&str>) -> ArmLoad {
    let fail = |gate: ArmSetGate, e: String| ArmLoad {
        armset: None,
        gate,
        sha: String::new(),
        error: Some(e),
    };
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return fail(ArmSetGate::Missing, format!("{}: {e}", path.display())),
    };
    let sig_path = format!("{}.sig", path.display());
    let sig = std::fs::read(&sig_path).ok();
    let pubkey: Pubkey = match cfg.secrets.armset_pubkey.parse() {
        Ok(k) => k,
        Err(_) => {
            return fail(
                ArmSetGate::BadSignature,
                "secrets.armset_pubkey is not a base58 pubkey".into(),
            )
        }
    };
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let expected = armset::parse_filename(name).map(|(_, sha)| sha);
    let ctx = VerifyCtx {
        pubkey: &pubkey,
        expected_sha: expected.as_deref(),
        now_unix,
        max_age_s: cfg.risk.armset_max_age_s,
        regime,
        hot_version: crate::VERSION,
    };
    match armset::verify(&bytes, sig.as_deref(), &ctx) {
        Ok(v) => ArmLoad {
            gate: v.gate(),
            sha: v.sha256.clone(),
            armset: Some(v.armset),
            error: None,
        },
        Err(e) => fail(e.gate(), e.to_string()),
    }
}

/// Generate the paper-mode throwaway signer (32 random bytes from the OS;
/// never written anywhere, gone when the process exits).
pub fn throwaway_signer() -> std::io::Result<HotSigner> {
    let mut seed = Zeroizing::new([0u8; 32]);
    std::fs::File::open("/dev/urandom")?.read_exact(&mut seed[..])?;
    Ok(HotSigner::from_seed(&seed))
}

/// The buy nonce pool (seed-derived accounts of `authority`). In paper mode
/// every value is a placeholder known since slot 0, so the rule
/// `last_change_slot < create_slot` holds until a simulated advance.
pub fn paper_nonce_pool(authority: Pubkey, n: usize) -> NoncePool {
    let accounts = sysix::pool_addresses(&authority, "buy", n.max(1));
    let pool = NoncePool::new(authority, &accounts);
    for i in 0..pool.len() {
        let mut v = [0u8; 32];
        v[..8].copy_from_slice(&(i as u64 + 1).to_le_bytes());
        pool.on_value(i, rugsnipe_tx::nonce::next_value(&v), 0);
    }
    pool
}

/// Compiled templates: one buy per nonce account (the account key is in
/// the message), a final sell (`CloseAccount`) and a partial sell.
#[derive(Debug, Clone)]
pub struct Templates {
    pub buys: Vec<Template>,
    pub sell_final: Template,
    pub sell_partial: Template,
    pub tip_account: Pubkey,
    /// `tx.cu_limit` (or [`PAPER_CU_LIMIT`]): the floor of the per-mint CU
    /// limit (the sells' static limit is at least 88,000).
    pub cu_floor: u32,
    /// The owner's wallet term W of the per-mint CU limit
    /// ([`rugsnipe_tx::cu::wallet_missing_bumps`]: `255 -` its UVA bump),
    /// added to every mint's S.
    pub cu_wallet_missing: u32,
}

/// Build the templates. `cu_limit` (the per-mint limit's floor) and
/// `lads_bytes` fall back to the research values in paper mode (real modes
/// are refused by validation when 0).
pub fn templates(cfg: &Config, owner: Pubkey, pool: &NoncePool) -> Result<Templates, String> {
    let paper = !cfg.mode.mode.is_real();
    let cu = match cfg.tx.cu_limit {
        0 if paper => PAPER_CU_LIMIT,
        0 => return Err("tx.cu_limit = 0".into()),
        n => n,
    };
    let lads = match cfg.tx.lads_bytes {
        0 if paper => PAPER_LADS_BYTES,
        0 => return Err("tx.lads_bytes = 0".into()),
        n => n,
    };
    let tip_account = c::JITO_TIP_ACCOUNTS[0];
    let base = CurveParams {
        format: cfg.tx.format,
        user: owner,
        nonce: None,
        cu_limit: cu,
        lads_bytes: lads,
        tip_account: Some(tip_account),
        buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[0],
        base_token_program: TOKEN_2022_PROGRAM_ID,
        partial_fill: Some(cfg.tx.partial_fill),
    };
    let mut buys = Vec::with_capacity(pool.len());
    for i in 0..pool.len() {
        let p = CurveParams {
            nonce: Some(NonceKeys {
                account: pool.account(i),
                authority: pool.authority,
            }),
            ..base
        };
        buys.push(Template::buy(&p).map_err(|e| format!("buy template: {e}"))?);
    }
    // sells carry no per-mint limit: a static one that fits every exit
    let sell = CurveParams {
        partial_fill: None,
        cu_limit: rugsnipe_tx::cu::sell_cu_limit(cu),
        ..base
    };
    Ok(Templates {
        buys,
        sell_final: Template::sell(&sell, true).map_err(|e| format!("sell template: {e}"))?,
        sell_partial: Template::sell(&sell, false).map_err(|e| format!("sell template: {e}"))?,
        tip_account,
        cu_floor: cu,
        cu_wallet_missing: rugsnipe_tx::cu::wallet_missing_bumps(&owner),
    })
}

/// The sell templates riding each sell-pool nonce (`(final, partial)`,
/// pool order): `AdvanceNonceAccount` first, the wallet as the nonce
/// authority, otherwise the plain sells of [`templates`]. The hot loop uses
/// them when no blockhash is fresh (docs/06 section 7.3: the sell pool).
pub fn sell_nonce_templates(
    cfg: &Config,
    owner: Pubkey,
    pool: &NoncePool,
) -> Result<Vec<(Template, Template)>, String> {
    let paper = !cfg.mode.mode.is_real();
    let cu = match cfg.tx.cu_limit {
        0 if paper => PAPER_CU_LIMIT,
        0 => return Err("tx.cu_limit = 0".into()),
        n => n,
    };
    let lads = match cfg.tx.lads_bytes {
        0 if paper => PAPER_LADS_BYTES,
        0 => return Err("tx.lads_bytes = 0".into()),
        n => n,
    };
    let mut out = Vec::with_capacity(pool.len());
    for i in 0..pool.len() {
        let p = CurveParams {
            format: cfg.tx.format,
            user: owner,
            nonce: Some(NonceKeys {
                account: pool.account(i),
                authority: pool.authority,
            }),
            cu_limit: rugsnipe_tx::cu::sell_cu_limit(cu),
            lads_bytes: lads,
            tip_account: Some(c::JITO_TIP_ACCOUNTS[0]),
            buyback_fee_recipient: c::BUYBACK_FEE_RECIPIENTS[0],
            base_token_program: TOKEN_2022_PROGRAM_ID,
            partial_fill: None,
        };
        let e = |e| format!("sell nonce template: {e}");
        out.push((
            Template::sell(&p, true).map_err(e)?,
            Template::sell(&p, false).map_err(e)?,
        ));
    }
    Ok(out)
}

/// `min_tokens_out` for spending `spend` at the operator's `v_max`
/// (docs/06 section 7.1 step 11: the on-chain bound is the curve at
/// `V = v_max`, `T = k / v_max`). `u64::MAX` if nothing can be bought.
pub fn min_tokens_out(spend: u64, v_max: u64, creator: Pubkey, fees: FeeBps) -> u64 {
    if v_max == 0 {
        return u64::MAX;
    }
    let t = (K_INITIAL / v_max as u128).min(u64::MAX as u128) as u64;
    CurveState {
        virtual_sol: v_max,
        virtual_token: t,
        real_sol: 0,
        real_token: u64::MAX,
        complete: false,
        creator,
    }
    .quote_buy_exact_sol_in(spend, fees)
    .map(|q| q.tokens)
    .unwrap_or(u64::MAX)
}

/// Paper runs without a config: fill every zero cap with the docs/06
/// section 6 example values so paper decisions report realistic
/// `would_skip`s (real modes never get here: validation refuses zero caps).
pub fn paper_caps(r: &mut rugsnipe_core::config::RiskCfg) {
    let d = [
        (&mut r.max_snipe_lamports, 250_000_000),
        (&mut r.max_probe_lamports, 20_000_000),
        (&mut r.max_probes_per_day, 20),
        (&mut r.max_open_positions, 4),
        (&mut r.max_daily_loss_lamports, 1_000_000_000),
        (&mut r.max_consec_losses_per_op, 5),
        (&mut r.max_buys_per_op_per_day, 10),
        (&mut r.max_buys_per_hour, 20),
        (&mut r.max_bid_lamports, 25_000_000),
        (&mut r.max_sell_spend_lamports, 20_000_000),
        (&mut r.max_entry_vsol_lamports, 40_000_000_000),
        (&mut r.max_hot_balance_lamports, 5_000_000_000),
        (&mut r.min_hot_balance_lamports, 1_000_000_000),
    ];
    for (k, v) in d {
        if *k == 0 {
            *k = v;
        }
    }
}

/// May this operator fire real buys in `mode`?
pub fn op_armed(mode: Mode, s: OpState) -> bool {
    match mode {
        Mode::Paper => false,
        Mode::Probe => s.may_probe(),
        Mode::Live => s.may_live(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn min_out_matches_curve_at_v_max() {
        let creator = Pubkey::new_unique();
        let fees = FeeBps::DEFAULT;
        // at v_max = 30 SOL the bound is a fresh-curve buy
        let fresh = CurveState::fresh(creator)
            .quote_buy_exact_sol_in(100_000_000, fees)
            .unwrap()
            .tokens;
        assert_eq!(
            min_tokens_out(100_000_000, 30_000_000_000, creator, fees),
            fresh
        );
        // a higher v_max allows fewer tokens
        let at40 = min_tokens_out(100_000_000, 40_000_000_000, creator, fees);
        assert!(at40 < fresh);
        // buying at V0 below v_max yields more than the bound
        let at35 = CurveState {
            virtual_sol: 35_000_000_000,
            virtual_token: (K_INITIAL / 35_000_000_000u128) as u64,
            real_sol: 0,
            real_token: u64::MAX,
            complete: false,
            creator,
        }
        .quote_buy_exact_sol_in(100_000_000, fees)
        .unwrap()
        .tokens;
        assert!(at35 > at40);
        assert_eq!(min_tokens_out(1, 0, creator, fees), u64::MAX);
    }

    #[test]
    fn example_config_exit_policy_parses() {
        let cfg = load_config(
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../config/rugsnipe.example.toml"),
        )
        .unwrap();
        let p = exit_policy(&cfg).unwrap();
        assert_eq!(p.max_sell_spend_lamports, 20_000_000);
        assert_eq!(p, ExitConfig::default().to_policy(20_000_000).unwrap());
    }

    #[test]
    fn paper_templates_without_cu_limit() {
        let cfg = Config::default();
        let signer = throwaway_signer().unwrap();
        let pool = paper_nonce_pool(signer.pubkey(), 4);
        let t = templates(&cfg, signer.pubkey(), &pool).unwrap();
        assert_eq!(t.buys.len(), 4);
        assert!(t.buys.iter().all(|b| b.len() <= rugsnipe_tx::MAX_TX_LEN));
        // real mode with cu_limit = 0 is refused
        let mut live = Config::default();
        live.mode.mode = Mode::Live;
        assert!(templates(&live, signer.pubkey(), &pool).is_err());
        // two throwaway signers differ
        assert_ne!(throwaway_signer().unwrap().pubkey(), signer.pubkey());
    }

    #[test]
    fn an_armset_above_the_effective_spend_cap_is_warned_about_in_real_modes() {
        let mut a = ArmSet {
            ver: 202,
            run_id: "test".into(),
            code_sha: String::new(),
            prereg_sha: String::new(),
            created_at: 0,
            valid_until: None,
            regime: "test".into(),
            min_hot_version: String::new(),
            gate_passed: false,
            probe_allowed: true,
            probe_stage: None,
            shared_fee_payers: vec![],
            operators: vec![rugsnipe_core::armset::OpEntry {
                op_id: 1,
                state: OpState::Probe,
                size_lamports: 500_000_000,
                v_max_lamports: 115_000_000_000,
                ev_cap_lamports: 0,
                ab_skip_p: 0.0,
                family: None,
                exit: None,
                stats: None,
            }],
            watch: vec![],
            funding_rules: vec![],
            match_all: None,
        };
        let st = ArmState::from_armset(&a, true, "sha");
        let mut eff = rugsnipe_core::config::RiskCfg {
            max_probe_lamports: 250_000_000, // [risk] left at the 0.25 stage
            max_snipe_lamports: 500_000_000,
            ..Default::default()
        };
        let r = st.size_cap_refusals(&eff, Mode::Probe);
        assert_eq!(r.len(), 1, "{r:?}");
        assert!(
            r[0].contains("op 1 size 500000000") && r[0].contains("max_probe_lamports 250000000"),
            "{}",
            r[0]
        );
        // live looks at the snipe cap; paper never refuses; a cap of 0 is unset
        assert!(st.size_cap_refusals(&eff, Mode::Live).is_empty());
        assert!(st.size_cap_refusals(&eff, Mode::Paper).is_empty());
        eff.max_probe_lamports = 0;
        assert!(st.size_cap_refusals(&eff, Mode::Probe).is_empty());
        eff.max_probe_lamports = 500_000_000;
        assert!(st.size_cap_refusals(&eff, Mode::Probe).is_empty());
        // a suspended operator is not checked
        a.operators[0].state = OpState::Suspended;
        eff.max_probe_lamports = 250_000_000;
        assert!(ArmState::from_armset(&a, true, "sha")
            .size_cap_refusals(&eff, Mode::Probe)
            .is_empty());
    }
}
