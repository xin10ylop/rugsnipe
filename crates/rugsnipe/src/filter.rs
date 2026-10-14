//! The LEAD entry filter (docs/04 section 2, docs/research/strategy-angles-critic.md
//! section 6): G1's frozen P(take-profit) model (gradient-boosted trees)
//! re-frozen on every launch known on Oct 14 without the metadata JSON and
//! `cu_used`, exported by `analysis/scripts/strategy/entry_filter_freeze.py`
//! to `data/reports/strategy/entry_filter_v1.json` and loaded here once at
//! startup (`entry.filter_model`). The rule: **skip if score < t1** (the
//! bottom training tertile); `entry.filter_mode = "log"` (default) only
//! records the score and its tertile on every intent.
//!
//! Where each feature comes from, and when it is visible:
//!
//! | group | features | paper (E1 lane) | live, from the shreds |
//! |---|---|---|---|
//! | bundle | `nB` .. `tok_pct_insiders` (17) | the reconstructed creation slot, gap-3 bundle definition ([`bundle_features`]) | **not hot-visible in full**: the run behind the create (index <= create + 5) streams in with the slot's entries; a shred-level slot-0 assembler must score once the run is closed (index create + 6 seen, or a gap > 2) |
//! | create tx | `tx_index` .. `payer_ne_signer` (14) | the create's `getTransaction` (jsonParsed) and `getBlock` index ([`create_tx_features`]) | the create transaction itself: compute-budget instructions, tip transfers, signers, version, instruction count. `prio_sol` from `meta.fee` here, from the compute-budget price x limit live. `tx_index` is the entry position, hot-visible only approximately |
//! | uri | `uri_ipfs`, `host_*` (7) | the CreateEvent's uri | the create instruction's uri: hot-visible |
//! | funding | `fund_*` (10) | the watchlist's funding watcher, when it saw the signer funded ([`funding_features`]); else unknown | same: hot-visible |
//! | operator | `op_*` (6) | [`OutcomeStore`]: our own LEAD outcomes per signer, written 10 s after each launch, persisted in `state.db` | same: hot-visible |
//! | time | hour, weekend, regime, creates in the last 60 / 600 s (6) | block time, slot, the hot loop's [`CreateRate`] | same: hot-visible |
//!
//! So in paper mode the filter runs on the E1 lane's result (the score is
//! computed on the hot thread when the verdict arrives, before the position
//! opens); live it needs the bundle run from the shreds. [`Model::score`]
//! walks fixed node arrays: no allocation, about a microsecond in release
//! (`cargo run --release --example filter_bench`).

use pump_codec::Pubkey;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Number of features of `entry_filter_v1`.
pub const NF: usize = 60;

/// The feature order of `entry_filter_v1` (checked against the JSON).
pub const FEATURES: [&str; NF] = [
    "nB",
    "bsol",
    "dev_sol",
    "tot_sol",
    "me",
    "dev_share",
    "b_n_tx",
    "b_max",
    "b_min",
    "b_mean",
    "b_cv",
    "b_round",
    "b_tok_eq",
    "b_desc",
    "n_trades_to_pb",
    "dev_round",
    "tok_pct_insiders",
    "tx_index",
    "tx_index_log",
    "jito_tip",
    "tip_sol",
    "prio_sol",
    "prio_log",
    "cu_limit_set",
    "cu_limit",
    "cu_price_log",
    "n_top_ix",
    "tx_v0",
    "n_signers",
    "creator_ne_signer",
    "payer_ne_signer",
    "uri_ipfs",
    "host_j7tracker",
    "host_ipfsio",
    "host_uxento",
    "host_mypinata",
    "host_rapidlaunch",
    "host_other",
    "fund_serial",
    "fund_mother_fresh",
    "fund_cex_fresh",
    "fund_mother_old",
    "fund_cex_old",
    "fund_swap",
    "fund_n_prior_log",
    "fund_amount_log",
    "fund_lag_log",
    "fund_round",
    "op_prior_log",
    "op_lead_n_log",
    "op_has_hist",
    "op_lead_hit",
    "op_lead_ev",
    "op_s0t",
    "hour_sin",
    "hour_cos",
    "weekend",
    "regime_post",
    "create_rate60",
    "create_rate600",
];

/// Offsets of the feature blocks in [`FEATURES`].
pub const BUNDLE: usize = 0;
pub const N_BUNDLE: usize = 17;
pub const CREATE_TX: usize = 17;
/// Create-transaction (14) and uri (7) features.
pub const N_CREATE: usize = 21;
pub const FUNDING: usize = 38;
pub const N_FUNDING: usize = 10;
pub const OPERATOR: usize = 48;
pub const N_OPERATOR: usize = 6;
pub const TIME: usize = 54;
pub const N_TIME: usize = 6;

/// The uri hosts with a feature of their own, in feature order.
pub const URI_HOSTS: [&str; 5] = [
    "metadata.j7tracker.io",
    "ipfs.io",
    "meta.uxento.io",
    "pump.mypinata.cloud",
    "m.rapidlaunch.io",
];
/// `ComputeBudget111111111111111111111111111111`.
pub const COMPUTE_BUDGET: &str = "ComputeBudget111111111111111111111111111111";
/// First slot of 2026-10-09 (200 ms slots): `regime_post`.
pub const SPLIT_SLOT: u64 = 454_896_000;
/// Defaults of unknown values (`predictor_models.feature_row`).
pub const DEFAULT_TX_INDEX: f64 = 500.0;
pub const DEFAULT_LEAD_HIT: f64 = 0.22;
pub const DEFAULT_LEAD_EV: f64 = 0.035;
pub const DEFAULT_S0T: f64 = 1.0;
pub const DEFAULT_RATE60: f64 = 35.0;
pub const DEFAULT_RATE600: f64 = 350.0;
/// The outcome of a launch is usable as history this long after it.
pub const OUTCOME_DELAY_MS: u64 = 10_000;

/// One feature vector in [`FEATURES`] order (`Copy`: it rides the hot ring).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Feat {
    pub x: [f64; NF],
}

impl Default for Feat {
    fn default() -> Self {
        Self { x: [0.0; NF] }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Node {
    /// Split threshold (`x <= thr` goes left) or the leaf value.
    v: f64,
    feat: u16,
    leaf: bool,
    missing_left: bool,
    left: u32,
    right: u32,
}

/// The exported model and the constants its features need.
#[derive(Debug, Clone)]
pub struct Model {
    pub name: String,
    pub baseline: f64,
    nodes: Box<[Node]>,
    roots: Box<[u32]>,
    /// Optional standardisation (`x' = (x - mean) / scale`); `None` for trees.
    scaler: Option<Box<[(f64, f64)]>>,
    pub t1: f64,
    pub t2: f64,
    /// MEV (Jito) tip accounts: `jito_tip`, `tip_sol`.
    pub tip_accounts: HashSet<Pubkey>,
    /// Funders labelled `cex` / `swap_service` or `bridge` (funding class).
    pub cex_funders: HashSet<Pubkey>,
    pub swap_funders: HashSet<Pubkey>,
    /// Outcomes become history this long after the launch (ms).
    pub outcome_delay_ms: u64,
}

fn num(v: &Value, what: &str) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("{what}: not a number"))
}

fn keys(v: &Value) -> HashSet<Pubkey> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().and_then(|s| s.parse().ok()))
                .collect()
        })
        .unwrap_or_default()
}

impl Model {
    /// Load and check an exported model (`entry_filter_v1.json`).
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("entry filter {}: {e}", path.display()))?;
        Self::from_json(&text).map_err(|e| format!("entry filter {}: {e}", path.display()))
    }

    /// Parse the exported JSON; refuses a feature list other than [`FEATURES`],
    /// a model type other than binary-logistic trees, or broken trees.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let d: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let feats: Vec<&str> = d["features"]
            .as_array()
            .ok_or("no feature list")?
            .iter()
            .map(|f| f.as_str().unwrap_or(""))
            .collect();
        if feats != FEATURES {
            return Err(format!(
                "feature list differs from this binary's ({} vs {NF} features)",
                feats.len()
            ));
        }
        let m = &d["model"];
        if m["type"] != "gbt_binary_logistic" {
            return Err(format!("unsupported model type {}", m["type"]));
        }
        let baseline = num(&m["baseline"], "baseline")?;
        let mut nodes = vec![];
        let mut roots = vec![];
        for (ti, tree) in m["trees"].as_array().ok_or("no trees")?.iter().enumerate() {
            let tree = tree.as_array().ok_or("tree is not a list")?;
            let base = nodes.len();
            roots.push(base as u32);
            for n in tree {
                let leaf = n["leaf"].as_bool().ok_or("node without leaf flag")?;
                if leaf {
                    nodes.push(Node {
                        v: num(&n["value"], "leaf value")?,
                        feat: 0,
                        leaf: true,
                        missing_left: false,
                        left: 0,
                        right: 0,
                    });
                    continue;
                }
                let feat = n["feature"].as_u64().ok_or("split without feature")? as usize;
                let (l, r) = (
                    n["left"].as_u64().ok_or("no left")? as usize,
                    n["right"].as_u64().ok_or("no right")? as usize,
                );
                if feat >= NF || l >= tree.len() || r >= tree.len() || l == 0 || r == 0 {
                    return Err(format!("tree {ti}: bad split node"));
                }
                nodes.push(Node {
                    v: num(&n["threshold"], "threshold")?,
                    feat: feat as u16,
                    leaf: false,
                    missing_left: n["missing_left"].as_bool().unwrap_or(false),
                    left: (base + l) as u32,
                    right: (base + r) as u32,
                });
            }
            // every walk from the root must end at a leaf (children after parents)
            for (k, n) in nodes[base..].iter().enumerate() {
                if !n.leaf && (n.left as usize <= base + k || n.right as usize <= base + k) {
                    return Err(format!("tree {ti}: child before parent"));
                }
            }
        }
        if roots.is_empty() {
            return Err("no trees".into());
        }
        let scaler = match &d["scaler"] {
            Value::Null => None,
            s => {
                let mean = s["mean"].as_array().ok_or("scaler without mean")?;
                let scale = s["scale"].as_array().ok_or("scaler without scale")?;
                if mean.len() != NF || scale.len() != NF {
                    return Err("scaler length".into());
                }
                Some(
                    mean.iter()
                        .zip(scale)
                        .map(|(m, s)| Ok((num(m, "mean")?, num(s, "scale")?)))
                        .collect::<Result<Vec<_>, String>>()?
                        .into_boxed_slice(),
                )
            }
        };
        let t = &d["tertiles"];
        let (t1, t2) = (num(&t["t1"], "t1")?, num(&t["t2"], "t2")?);
        if t1 > t2 {
            return Err("t1 > t2".into());
        }
        let c = &d["constants"];
        let hosts: Vec<&str> = c["uri_hosts"]
            .as_array()
            .map(|a| a.iter().filter_map(|s| s.as_str()).collect())
            .unwrap_or_default();
        if hosts != URI_HOSTS {
            return Err("uri hosts differ from this binary's".into());
        }
        if c["compute_budget_program"] != COMPUTE_BUDGET
            || c["split_slot"].as_u64() != Some(SPLIT_SLOT)
            || c["base_fee_per_signature"].as_u64() != Some(5_000)
        {
            return Err("constants differ from this binary's".into());
        }
        let tip_accounts = keys(&c["mev_tip_accounts"]);
        if tip_accounts.is_empty() {
            return Err("no MEV tip accounts".into());
        }
        let delay = c["op_outcome_delay_s"].as_f64().unwrap_or(10.0);
        Ok(Self {
            name: d["name"].as_str().unwrap_or("entry_filter").to_string(),
            baseline,
            nodes: nodes.into_boxed_slice(),
            roots: roots.into_boxed_slice(),
            scaler,
            t1,
            t2,
            tip_accounts,
            cex_funders: keys(&c["cex_funders"]),
            swap_funders: keys(&c["swap_funders"]),
            outcome_delay_ms: (delay.max(0.0) * 1_000.0) as u64,
        })
    }

    /// P(take-profit): `1 / (1 + exp(-(baseline + sum of leaves)))`. Walks
    /// the fixed node array; no allocation.
    #[inline]
    pub fn score(&self, f: &Feat) -> f64 {
        let mut raw = self.baseline;
        for &r in self.roots.iter() {
            let mut i = r as usize;
            loop {
                let n = &self.nodes[i];
                if n.leaf {
                    raw += n.v;
                    break;
                }
                let k = n.feat as usize;
                let mut v = f.x[k];
                if let Some(sc) = &self.scaler {
                    v = (v - sc[k].0) / sc[k].1;
                }
                let left = if v.is_nan() { n.missing_left } else { v <= n.v };
                i = if left { n.left } else { n.right } as usize;
            }
        }
        1.0 / (1.0 + (-raw).exp())
    }

    /// 0 = bottom (`score < t1`), 1 = middle, 2 = top (`score >= t2`).
    #[inline]
    pub fn tertile(&self, score: f64) -> u8 {
        if score < self.t1 {
            0
        } else if score < self.t2 {
            1
        } else {
            2
        }
    }

    pub fn n_trees(&self) -> usize {
        self.roots.len()
    }
}

/// `entry.filter_model` as a path: as given (relative to the working
/// directory, like every other path of the config), else, for a relative
/// path that does not exist there, relative to the source tree's root (test
/// runs start in the crate directory).
pub fn resolve_path(p: &str) -> std::path::PathBuf {
    let path = Path::new(p);
    if path.is_absolute() || path.exists() {
        return path.to_path_buf();
    }
    let alt = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(p);
    if alt.exists() {
        alt
    } else {
        path.to_path_buf()
    }
}

// ----------------------------------------------------------------------------- bundle

/// One trade of the creation slot (block order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S0Trade {
    /// Transaction index in the block.
    pub index: u32,
    /// In the create transaction.
    pub is_create: bool,
    pub user: Pubkey,
    pub is_buy: bool,
    pub sol: u64,
    /// Raw token units (6 decimals).
    pub tok: u64,
    /// Post-trade virtual reserves.
    pub v: u64,
    pub t: u64,
}

/// What the bundle features say beyond the vector: the bundle wallets and
/// the third-party slot-0 buyers (gap-3 definitions, `tapes.build_tape`).
#[derive(Debug, Clone, PartialEq)]
pub struct BundleInfo {
    pub x: [f64; N_BUNDLE],
    pub wallets: Vec<Pubkey>,
    pub s0_third: u32,
    /// Index into the trades of the post-bundle trade.
    pub pb: usize,
}

fn roundish(x: f64) -> bool {
    (x * 10.0 - (x * 10.0).round()).abs() < 0.002
        || (x * 100.0 - (x * 100.0).round()).abs() < 0.0005
}

/// Market cap (SOL, 1e9-token supply) of a curve state.
pub fn mcap_sol(v: u64, t: u64) -> f64 {
    if t == 0 {
        return 0.0;
    }
    (v as f64 / 1e9) / (t as f64 / 1e6) * 1e9
}

/// The 17 bundle features of `predictor.bundle_features` on a gap-3 tape
/// (`tapes._raw_bundle` + `build_tape`): `trades` are the creation slot's
/// trades in block order, `creator_set` the creator, the create's signer and
/// fee payer, `me` the post-bundle market cap in SOL (`None`: from the
/// post-bundle trade's reserves). `None` when the create has no trade of
/// its own in the slot.
pub fn bundle_features(
    trades: &[S0Trade],
    creator_set: &[Pubkey],
    me: Option<f64>,
) -> Option<BundleInfo> {
    let in_c = |u: &Pubkey| creator_set.contains(u);
    let ctx = trades.iter().find(|t| t.is_create)?.index;
    let n = trades.len();
    let mut idx = vec![false; n];
    for (i, t) in trades.iter().enumerate() {
        if t.is_buy && (t.is_create || in_c(&t.user)) {
            idx[i] = true;
        }
    }
    let mut prev = ctx;
    for (i, t) in trades.iter().enumerate() {
        if t.index <= ctx {
            continue;
        }
        if t.index - prev <= 2 && t.index - ctx <= 5 && t.is_buy {
            idx[i] = true;
            prev = t.index;
        } else if t.index - prev > 2 || t.index - ctx > 5 {
            break;
        }
    }
    let pb = idx.iter().rposition(|b| *b)?;
    let mut wallets: Vec<Pubkey> = vec![];
    let (mut dev_sol, mut bsol) = (0.0f64, 0.0f64);
    for (i, t) in trades.iter().enumerate() {
        if !idx[i] {
            continue;
        }
        let s = t.sol as f64 / 1e9;
        if in_c(&t.user) {
            dev_sol += s;
        } else {
            bsol += s;
            if !wallets.contains(&t.user) {
                wallets.push(t.user);
            }
        }
    }
    let mut third: Vec<Pubkey> = vec![];
    for t in trades {
        if t.is_buy && !in_c(&t.user) && !wallets.contains(&t.user) && !third.contains(&t.user) {
            third.push(t.user);
        }
    }
    // the bundle wallets' slot-0 buys up to the post-bundle trade
    let bidx: Vec<usize> = (0..=pb)
        .filter(|&i| trades[i].is_buy && wallets.contains(&trades[i].user))
        .collect();
    let sizes: Vec<f64> = bidx.iter().map(|&i| trades[i].sol as f64 / 1e9).collect();
    let toks: Vec<f64> = bidx.iter().map(|&i| trades[i].tok as f64 / 1e6).collect();
    let mut per_w: Vec<(Pubkey, f64)> = vec![];
    for &i in &bidx {
        let t = &trades[i];
        match per_w.iter_mut().find(|(k, _)| *k == t.user) {
            Some((_, s)) => *s += t.sol as f64 / 1e9,
            None => per_w.push((t.user, t.sol as f64 / 1e9)),
        }
    }
    let wsz: Vec<f64> = if per_w.is_empty() {
        vec![0.0]
    } else {
        per_w.iter().map(|(_, s)| *s).collect()
    };
    let wmean = wsz.iter().sum::<f64>() / wsz.len() as f64;
    let wmax = wsz.iter().cloned().fold(f64::MIN, f64::max);
    let wmin = wsz.iter().cloned().fold(f64::MAX, f64::min);
    let wstd = (wsz.iter().map(|x| (x - wmean).powi(2)).sum::<f64>() / wsz.len() as f64).sqrt();
    let b_cv = if wmean > 0.0 && wsz.len() > 1 {
        wstd / wmean
    } else {
        0.0
    };
    let b_round = if bidx.is_empty() {
        0.0
    } else {
        sizes.iter().filter(|x| roundish(**x)).count() as f64 / sizes.len() as f64
    };
    let tmax = toks.iter().cloned().fold(f64::MIN, f64::max);
    let b_tok_eq = if !bidx.is_empty() && tmax > 0.0 {
        toks.iter()
            .filter(|x| (*x / tmax - 1.0).abs() < 0.002)
            .count() as f64
            / toks.len() as f64
    } else {
        0.0
    };
    let b_desc = sizes.len() > 1 && sizes.windows(2).all(|w| w[1] - w[0] <= 1e-9);
    let tok_ins: f64 = trades[..=pb]
        .iter()
        .filter(|t| t.is_buy)
        .map(|t| t.tok as f64 / 1e6)
        .sum::<f64>()
        / 1e9;
    let me = me.unwrap_or_else(|| mcap_sol(trades[pb].v, trades[pb].t));
    let tot = bsol + dev_sol;
    let x = [
        wallets.len() as f64,
        bsol,
        dev_sol,
        tot,
        me,
        if tot > 0.0 { dev_sol / tot } else { 0.0 },
        bidx.len() as f64,
        wmax,
        wmin,
        wmean,
        b_cv,
        b_round,
        b_tok_eq,
        b_desc as u8 as f64,
        (pb + 1) as f64,
        (dev_sol > 0.0 && roundish(dev_sol)) as u8 as f64,
        tok_ins,
    ];
    Some(BundleInfo {
        x,
        wallets,
        s0_third: third.len() as u32,
        pb,
    })
}

// ----------------------------------------------------------------------------- create tx

/// `(source, destination, lamports)` of the System transfers of a jsonParsed
/// transaction, top level and inner (`funding.sol_transfers`).
fn sol_transfers(tx: &Value) -> Vec<(String, String, u64)> {
    let mut out = vec![];
    let top = tx["transaction"]["message"]["instructions"].as_array();
    let inner = tx["meta"]["innerInstructions"].as_array();
    let it = top
        .into_iter()
        .flatten()
        .chain(inner.into_iter().flatten().flat_map(|g| {
            g["instructions"]
                .as_array()
                .map(|a| a.iter())
                .into_iter()
                .flatten()
        }));
    for ix in it {
        if ix["program"] != "system" {
            continue;
        }
        let p = &ix["parsed"];
        let ty = p["type"].as_str().unwrap_or("");
        if !matches!(
            ty,
            "transfer" | "transferWithSeed" | "createAccount" | "createAccountWithSeed"
        ) {
            continue;
        }
        let info = &p["info"];
        let dst = info["destination"]
            .as_str()
            .or_else(|| info["newAccount"].as_str())
            .unwrap_or("");
        let src = info["source"].as_str().unwrap_or("");
        let lam = info["lamports"].as_u64().unwrap_or(0);
        if !src.is_empty() && !dst.is_empty() && lam > 0 {
            out.push((src.to_string(), dst.to_string(), lam));
        }
    }
    out
}

/// Host of a uri as `predictor.text_features` reads it (`https?://([^/]+)/`
/// on `uri + "/"`, lower case).
pub fn uri_host(uri: &str) -> Option<String> {
    let rest = uri
        .strip_prefix("https://")
        .or_else(|| uri.strip_prefix("http://"))?;
    let host = rest.split('/').next().unwrap_or("");
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// The 7 uri features (`uri_ipfs`, one-hot host, `host_other`).
pub fn uri_features(uri: &str) -> [f64; 7] {
    let mut x = [0.0; 7];
    x[0] = uri.contains("ipfs") as u8 as f64;
    let host = uri_host(uri);
    match host
        .as_deref()
        .and_then(|h| URI_HOSTS.iter().position(|k| *k == h))
    {
        Some(k) => x[1 + k] = 1.0,
        None => x[6] = 1.0,
    }
    x
}

/// The create-transaction and uri features (21) of a jsonParsed
/// `getTransaction` result, as `predictor.create_tx_features` +
/// `feature_row` compute them. `tx_index`: the create's index in its block
/// (`None`: the 500 default); `creator` the creator argument, `signer` the
/// create's user; `uri` from the CreateEvent.
pub fn create_tx_features(
    tx: &Value,
    tx_index: Option<u32>,
    creator: &Pubkey,
    signer: &Pubkey,
    uri: &str,
    tip_accounts: &HashSet<Pubkey>,
) -> [f64; N_CREATE] {
    let msg = &tx["transaction"]["message"];
    let ixs = msg["instructions"].as_array();
    let tip: u64 = sol_transfers(tx)
        .iter()
        .filter(|(_, d, _)| d.parse::<Pubkey>().is_ok_and(|k| tip_accounts.contains(&k)))
        .map(|(_, _, l)| *l)
        .sum();
    let n_sigs = tx["transaction"]["signatures"]
        .as_array()
        .map_or(0, |a| a.len()) as u64;
    let prio = match tx["meta"]["fee"].as_u64() {
        Some(fee) if n_sigs > 0 => fee.saturating_sub(5_000 * n_sigs),
        _ => 0,
    };
    let (mut lim, mut price) = (None::<u64>, None::<u64>);
    for ix in ixs.into_iter().flatten() {
        if ix["programId"] != COMPUTE_BUDGET {
            continue;
        }
        let Some(d) = ix["data"]
            .as_str()
            .and_then(|s| bs58::decode(s).into_vec().ok())
        else {
            continue;
        };
        if d.first() == Some(&2) && d.len() >= 5 {
            lim = Some(u32::from_le_bytes([d[1], d[2], d[3], d[4]]) as u64);
        } else if d.first() == Some(&3) && d.len() >= 9 {
            let mut b = [0u8; 8];
            b.copy_from_slice(&d[1..9]);
            price = Some(u64::from_le_bytes(b));
        }
    }
    let keys = msg["accountKeys"].as_array();
    let n_signers = keys
        .into_iter()
        .flatten()
        .filter(|k| k["signer"].as_bool() == Some(true))
        .count();
    let fee_payer: Option<Pubkey> = keys.and_then(|a| a.first()).and_then(|k| {
        k["pubkey"]
            .as_str()
            .or_else(|| k.as_str())
            .and_then(|s| s.parse().ok())
    });
    let version_0 = tx["version"].as_u64() == Some(0) || tx["version"].as_str() == Some("0");
    let txi = tx_index.map_or(DEFAULT_TX_INDEX, |i| i as f64);
    let d = Pubkey::default();
    let mut x = [0.0; N_CREATE];
    x[0] = txi;
    x[1] = txi.ln_1p();
    x[2] = (tip > 0) as u8 as f64;
    x[3] = tip as f64 / 1e9;
    x[4] = prio as f64 / 1e9;
    x[5] = (1.0 + prio as f64).log10();
    x[6] = lim.is_some() as u8 as f64;
    x[7] = lim.unwrap_or(0) as f64 / 1e5;
    x[8] = (1.0 + price.unwrap_or(0) as f64).log10();
    x[9] = ixs.map_or(0, |a| a.len()) as f64;
    x[10] = version_0 as u8 as f64;
    x[11] = n_signers as f64;
    x[12] = (*creator != d && *signer != d && creator != signer) as u8 as f64;
    x[13] = fee_payer.is_some_and(|f| *signer != d && f != *signer) as u8 as f64;
    x[14..].copy_from_slice(&uri_features(uri));
    x
}

/// `CreateEvent.uri` of a jsonParsed transaction's logs (empty if none).
pub fn create_uri(tx: &Value) -> String {
    use pump_codec::events::{events_from_log_scan, scan_program_data_logs, PumpEvent};
    let logs: Vec<&str> = tx["meta"]["logMessages"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    let Ok(scan) = scan_program_data_logs(&logs) else {
        return String::new();
    };
    let Ok(evs) = events_from_log_scan(&scan) else {
        return String::new();
    };
    evs.iter()
        .find_map(|(_, e)| match e {
            PumpEvent::Create(c) => Some(c.uri.to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

// ----------------------------------------------------------------------------- funding

/// Signer funding classes (`predictor_models.FUND_CLS` order).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FundClass {
    Serial = 0,
    MotherFresh = 1,
    CexFresh = 2,
    MotherOld = 3,
    CexOld = 4,
    Swap = 5,
}

/// What is known about the signer's funding (all `None`: unknown, the
/// training rows' "unknown" / "nofund" encoding).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Funding {
    pub class: Option<FundClass>,
    pub n_prior: Option<u64>,
    pub amount_sol: Option<f64>,
    pub lag_h: Option<f64>,
}

impl Funding {
    /// From the watchlist's funding watcher: the signer was funded by
    /// `funder` (fresh: the rules match recipients with no balance) at
    /// `added_unix`.
    pub fn from_watch(m: &Model, funder: &Pubkey, added_unix: u64, now_unix: u64) -> Self {
        let class = if m.cex_funders.contains(funder) {
            FundClass::CexFresh
        } else if m.swap_funders.contains(funder) {
            FundClass::Swap
        } else {
            FundClass::MotherFresh
        };
        Self {
            class: Some(class),
            n_prior: Some(0),
            amount_sol: None,
            lag_h: Some(now_unix.saturating_sub(added_unix) as f64 / 3_600.0),
        }
    }
}

fn l1p(v: Option<f64>) -> f64 {
    v.map_or(0.0, |v| v.max(0.0).ln_1p())
}

/// The 10 funding features.
pub fn funding_features(f: &Funding) -> [f64; N_FUNDING] {
    let mut x = [0.0; N_FUNDING];
    if let Some(c) = f.class {
        x[c as usize] = 1.0;
    }
    x[6] = l1p(f.n_prior.map(|n| n as f64));
    x[7] = l1p(f.amount_sol);
    x[8] = l1p(f.lag_h);
    x[9] = f
        .amount_sol
        .is_some_and(|a| a != 0.0 && (a * 10.0 - (a * 10.0).round()).abs() < 1e-3) as u8
        as f64;
    x
}

// ----------------------------------------------------------------------------- outcome store

/// Our own outcomes of one key's launches (a signer, or a bundle wallet).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct OpStats {
    /// Launches decided on (in the entry class).
    pub n_launch: u32,
    /// Launches with a LEAD result (a closed position).
    pub n_lead: u32,
    pub tp_sum: u32,
    pub ret_sum: f64,
    /// Launches whose creation slot was reconstructed.
    pub n_s0: u32,
    pub s0_sum: u64,
}

impl OpStats {
    pub fn lead_hit(&self) -> Option<f64> {
        (self.n_lead > 0).then(|| self.tp_sum as f64 / self.n_lead as f64)
    }
    pub fn lead_ev(&self) -> Option<f64> {
        (self.n_lead > 0).then(|| self.ret_sum / self.n_lead as f64)
    }
    /// The 6 operator features (`op_all_*` of `feature_row`).
    pub fn features(&self) -> [f64; N_OPERATOR] {
        [
            (self.n_launch as f64).ln_1p(),
            (self.n_lead as f64).ln_1p(),
            (self.n_lead > 0) as u8 as f64,
            self.lead_hit().unwrap_or(DEFAULT_LEAD_HIT),
            self.lead_ev().unwrap_or(DEFAULT_LEAD_EV),
            if self.n_s0 > 0 {
                self.s0_sum as f64 / self.n_s0 as f64
            } else {
                DEFAULT_S0T
            },
        ]
    }
    fn add(&mut self, o: &rugsnipe_state::rows::OutcomeRow) {
        self.n_launch += 1;
        if o.lead {
            self.n_lead += 1;
            self.tp_sum += o.tp_hit as u32;
            self.ret_sum += o.ret;
        }
        if let Some(s) = o.s0_third {
            self.n_s0 += 1;
            self.s0_sum += s as u64;
        }
    }
}

/// The live per-signer outcome store (docs/04 section 6: our own LEAD
/// outcomes per signer and per bundle-wallet component), fed 10 s after each
/// launch and rebuilt from `state.db` (`outcomes`) at start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OutcomeStore {
    pub signers: HashMap<Pubkey, OpStats>,
    pub wallets: HashMap<Pubkey, OpStats>,
    pub rows: u64,
}

impl OutcomeStore {
    pub fn from_rows(rows: &[rugsnipe_state::rows::OutcomeRow]) -> Self {
        let mut s = Self::default();
        for r in rows {
            s.apply(r);
        }
        s
    }
    pub fn apply(&mut self, o: &rugsnipe_state::rows::OutcomeRow) {
        self.rows += 1;
        self.signers.entry(o.signer).or_default().add(o);
        for w in &o.bundle_wallets {
            self.wallets.entry(*w).or_default().add(o);
        }
    }
    pub fn signer(&self, k: &Pubkey) -> OpStats {
        self.signers.get(k).copied().unwrap_or_default()
    }
    /// The bundle wallet of `wallets` with the most LEAD outcomes.
    pub fn best_wallet(&self, wallets: &[Pubkey]) -> OpStats {
        wallets
            .iter()
            .filter_map(|w| self.wallets.get(w))
            .max_by_key(|s| (s.n_lead, s.n_launch))
            .copied()
            .unwrap_or_default()
    }
}

// ----------------------------------------------------------------------------- time

/// The 6 time features: hour (sin, cos) and weekend of the block time
/// (`predictor_build`: weekday = (days + 4) mod 7), the regime, and the
/// creates of the previous 60 / 600 s (`None`: not covered yet, the
/// training defaults).
pub fn time_features(
    block_time: i64,
    slot: u64,
    rate60: Option<u32>,
    rate600: Option<u32>,
) -> [f64; N_TIME] {
    let hour = block_time.div_euclid(3_600).rem_euclid(24) as f64;
    let weekday = (block_time.div_euclid(86_400) + 4).rem_euclid(7);
    let a = 2.0 * std::f64::consts::PI * hour / 24.0;
    [
        a.sin(),
        a.cos(),
        (weekday >= 5) as u8 as f64,
        (slot >= SPLIT_SLOT) as u8 as f64,
        rate60.map_or(DEFAULT_RATE60, |r| r as f64),
        rate600.map_or(DEFAULT_RATE600, |r| r as f64) / 10.0,
    ]
}

/// The full vector.
pub fn assemble(
    bundle: &[f64; N_BUNDLE],
    create: &[f64; N_CREATE],
    funding: &[f64; N_FUNDING],
    op: &[f64; N_OPERATOR],
    time: &[f64; N_TIME],
) -> Feat {
    let mut f = Feat::default();
    f.x[BUNDLE..BUNDLE + N_BUNDLE].copy_from_slice(bundle);
    f.x[CREATE_TX..CREATE_TX + N_CREATE].copy_from_slice(create);
    f.x[FUNDING..FUNDING + N_FUNDING].copy_from_slice(funding);
    f.x[OPERATOR..OPERATOR + N_OPERATOR].copy_from_slice(op);
    f.x[TIME..TIME + N_TIME].copy_from_slice(time);
    f
}

/// Creates seen in the last 60 / 600 s, for the hot thread: a fixed ring of
/// arrival times (no allocation after `new`). A window is `None` until the
/// ring has been running that long (the training default applies).
#[derive(Debug)]
pub struct CreateRate {
    ts: Box<[u64]>,
    head: usize,
    len: usize,
    start_ms: u64,
}

impl CreateRate {
    pub const CAP: usize = 8_192;

    pub fn new(start_ms: u64) -> Self {
        Self {
            ts: vec![0u64; Self::CAP].into_boxed_slice(),
            head: 0,
            len: 0,
            start_ms,
        }
    }

    /// Creates in `[now - w, now)` (saturates at the ring size).
    fn count(&self, now_ms: u64, w_ms: u64) -> u32 {
        let lo = now_ms.saturating_sub(w_ms);
        let mut n = 0u32;
        for k in 0..self.len {
            let i = (self.head + Self::CAP - 1 - k) % Self::CAP;
            let t = self.ts[i];
            if t < lo {
                break;
            }
            if t < now_ms {
                n += 1;
            }
        }
        n
    }

    /// `(last 60 s, last 600 s)` before `now_ms`, then record this create.
    pub fn on_create(&mut self, now_ms: u64) -> (Option<u32>, Option<u32>) {
        let up = now_ms.saturating_sub(self.start_ms);
        let r = (
            (up >= 60_000).then(|| self.count(now_ms, 60_000)),
            (up >= 600_000).then(|| self.count(now_ms, 600_000)),
        );
        self.ts[self.head] = now_ms;
        self.head = (self.head + 1) % Self::CAP;
        self.len = (self.len + 1).min(Self::CAP);
        r
    }
}

/// Encode an optional rate for a `Copy` message (`u32::MAX` = unknown).
pub fn rate_code(r: Option<u32>) -> u32 {
    r.unwrap_or(u32::MAX)
}

/// Decode [`rate_code`].
pub fn rate_decode(r: u32) -> Option<u32> {
    (r != u32::MAX).then_some(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model_path() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/reports/strategy/entry_filter_v1.json")
    }

    /// The gap-3 run behind the create (`tapes._raw_bundle`): buys at most
    /// 2 indices apart and at most create + 5; a gap of 3 or index create + 6
    /// closes the run. The real-data fixtures of `tests/filter_parity.rs`
    /// never hit either boundary, so this pins them.
    #[test]
    fn bundle_run_stops_at_a_gap_of_three_and_at_create_plus_six() {
        let dev = Pubkey::new_unique();
        let w: Vec<Pubkey> = (0..4).map(|_| Pubkey::new_unique()).collect();
        let tr = |index, is_create, user| S0Trade {
            index,
            is_create,
            user,
            is_buy: true,
            sol: 500_000_000,
            tok: 15_000_000_000,
            v: 31_000_000_000,
            t: 1_000_000_000_000_000,
        };
        let run = |idx: &[u32]| -> Vec<Pubkey> {
            let mut trades = vec![tr(10, true, dev)];
            trades.extend(idx.iter().zip(&w).map(|(i, u)| tr(*i, false, *u)));
            bundle_features(&trades, &[dev], Some(30.0))
                .unwrap()
                .wallets
        };
        assert_eq!(run(&[12, 14]), vec![w[0], w[1]]); // gaps of 2: one run
        assert_eq!(run(&[12, 15]), vec![w[0]]); // gap of 3 closes it
        assert_eq!(run(&[11, 13, 15, 16]), vec![w[0], w[1], w[2]]); // 16 = create + 6
    }

    #[test]
    fn loads_the_export_and_refuses_a_different_feature_list() {
        let m = Model::load(&model_path()).unwrap();
        assert_eq!(m.n_trees(), 150);
        assert!(m.t1 < m.t2 && m.t1 > 0.0);
        assert_eq!(m.tip_accounts.len(), 8);
        assert_eq!(m.outcome_delay_ms, OUTCOME_DELAY_MS);
        let text = std::fs::read_to_string(model_path()).unwrap();
        let mut v: Value = serde_json::from_str(&text).unwrap();
        v["features"][0] = "something_else".into();
        assert!(Model::from_json(&v.to_string())
            .unwrap_err()
            .contains("feature list"));
        let mut v: Value = serde_json::from_str(&text).unwrap();
        v["model"]["trees"][0][0]["left"] = 0.into();
        assert!(Model::from_json(&v.to_string()).is_err());
    }

    fn e1res(intent: u64, mint: Pubkey, dev: Pubkey, b1: Pubkey, bt: i64) -> crate::e1::E1Res {
        let tx = serde_json::json!({
            "blockTime": bt, "version": 0,
            "meta": {"fee": 15_000, "logMessages": []},
            "transaction": {"signatures": ["s1"], "message": {
                "instructions": [{"programId": COMPUTE_BUDGET, "data": bs58::encode([2u8, 0x40, 0x0d, 0x03, 0]).into_string()}],
                "accountKeys": [{"pubkey": dev.to_string(), "signer": true}]}}
        });
        let tr = |index, is_create, user, sol: u64| S0Trade {
            index,
            is_create,
            user,
            is_buy: true,
            sol,
            tok: sol * 30,
            v: 31_000_000_000,
            t: 1_000_000_000_000_000,
        };
        crate::e1::E1Res {
            req: crate::e1::E1Req {
                intent,
                mint,
                bc: Pubkey::new_unique(),
                create_sig: [1; 64],
                slot: SPLIT_SLOT + 10,
                creator: dev,
                user: dev,
                decided_unix_ms: 0,
            },
            outcome: ShadowOutcome::Filled,
            slot0: None,
            premium_bps: 110,
            s0_failed: 0,
            intx_trades: 1,
            tape: vec![],
            horizon: 0,
            calls: 0,
            error: None,
            create_tx: Some(tx),
            s0_trades: vec![
                tr(10, true, dev, 1_000_000_000),
                tr(11, false, b1, 500_000_000),
                tr(40, false, Pubkey::new_unique(), 100_000_000),
            ],
            fee_payer: Some(dev),
        }
    }

    #[test]
    fn lane_assembles_point_in_time_features_and_feeds_the_store() {
        let m = Arc::new(Model::load(&model_path()).unwrap());
        let mut lane = FilterLane::new(
            Some(m.clone()),
            rugsnipe_core::config::FilterMode::Log,
            &[],
            4_000_000,
        );
        let (dev, b1) = (Pubkey::new_unique(), Pubkey::new_unique());
        let now = 1_791_990_000_000u64;
        // launch 1: no history yet
        let m1 = Pubkey::new_unique();
        lane.on_intent(1, 500_000_000);
        lane.on_pending(1, m1, dev, now, 40, u32::MAX);
        let (f1, op_n, bw_n) = lane
            .on_e1(&e1res(1, m1, dev, b1, 1_791_989_000), Funding::default())
            .unwrap();
        assert_eq!((op_n, bw_n), (0, 0));
        let op = &f1.x[OPERATOR..OPERATOR + N_OPERATOR];
        assert_eq!(
            op,
            &[
                0.0,
                0.0,
                0.0,
                DEFAULT_LEAD_HIT,
                DEFAULT_LEAD_EV,
                DEFAULT_S0T
            ]
        );
        assert_eq!(&f1.x[..3], &[1.0, 0.5, 1.0], "nB, bsol, dev_sol");
        assert_eq!(
            f1.x[CREATE_TX + 4],
            10_000.0 / 1e9,
            "priority = fee - 5000 x 1 sig"
        );
        assert_eq!(f1.x[CREATE_TX + 7], 200_000.0 / 1e5, "cu_limit");
        assert_eq!((f1.x[TIME + 4], f1.x[TIME + 5]), (40.0, 35.0));
        let s = m.score(&f1);
        let row = lane.on_score(1, m1, true, s, m.tertile(s), false);
        assert_eq!((row.op_lead_n, row.tertile), (0, Some(m.tertile(s))));
        lane.on_shadow(1, ShadowOutcome::Filled, 0, now);
        lane.on_exit(&m1, true);
        lane.on_exit(&m1, false);
        lane.on_closed(&m1, 60_000_000, now);
        // launch 2 by the same signer and bundle wallet, before the outcome is applied
        let m2 = Pubkey::new_unique();
        lane.on_pending(2, m2, dev, now, 40, 400);
        let (f2, ..) = lane
            .on_e1(&e1res(2, m2, dev, b1, 1_791_989_500), Funding::default())
            .unwrap();
        assert_eq!(f2.x[OPERATOR], 0.0, "launch 1 is not history yet");
        let rows = lane.take_due(now, false);
        assert_eq!(rows.len(), 1);
        assert!(rows[0].lead && rows[0].tp_hit);
        assert_eq!(rows[0].ret, (60_000_000.0 - 4_000_000.0) / 500_000_000.0);
        assert_eq!(
            (rows[0].s0_third, rows[0].bundle_wallets.clone()),
            (Some(1), vec![b1])
        );
        // launch 3: the store now knows launch 1 (signer and bundle wallet)
        let m3 = Pubkey::new_unique();
        lane.on_pending(3, m3, dev, now, 40, 400);
        let (f3, op_n, bw_n) = lane
            .on_e1(&e1res(3, m3, dev, b1, 1_791_989_900), Funding::default())
            .unwrap();
        assert_eq!((op_n, bw_n), (1, 1));
        let ev = (60_000_000.0 - 4_000_000.0) / 500_000_000.0;
        assert_eq!(
            &f3.x[OPERATOR..OPERATOR + N_OPERATOR],
            &[1f64.ln_1p(), 1f64.ln_1p(), 1.0, 1.0, ev, 1.0]
        );
        // a failed attempt counts as a launch without a LEAD result; a recent launch waits 10 s
        lane.on_shadow(2, ShadowOutcome::Premium, 3_000_000, now);
        lane.on_pending(4, Pubkey::new_unique(), dev, now, 40, 400);
        lane.on_shadow(4, ShadowOutcome::NotInSlot0, 3_000_000, now);
        assert_eq!(
            lane.take_due(now, false).len(),
            1,
            "launch 4 is due at now + 10 s"
        );
        assert_eq!(lane.take_due(now + OUTCOME_DELAY_MS, false).len(), 1);
        let s = lane.store.signer(&dev);
        assert_eq!((s.n_launch, s.n_lead, s.tp_sum), (3, 1, 1));
        assert_eq!(lane.store.wallets[&b1].n_launch, 2);
        assert_eq!(lane.report.store_rows, 3);
        // a class verdict is not a launch of the store
        lane.on_pending(5, Pubkey::new_unique(), dev, now, 0, 0);
        lane.on_shadow(5, ShadowOutcome::Class, 0, now);
        assert_eq!(lane.take_due(u64::MAX, true).len(), 0);
        assert_eq!(lane.pending(), 1, "launch 3 still open");
    }

    #[test]
    fn create_rate_windows() {
        let mut r = CreateRate::new(0);
        assert_eq!(r.on_create(500), (None, None));
        for k in 0..100 {
            r.on_create(30_000 + k);
        }
        // 100 creates at 30.0-30.1 s, one at 0.5 s; at 61 s the window is
        // [1 s, 61 s) (`searchsorted` over [bt - 60, bt) in the training)
        assert_eq!(r.on_create(61_000), (Some(100), None));
        assert_eq!(r.on_create(700_000), (Some(0), Some(0)));
        assert_eq!(r.on_create(700_001), (Some(1), Some(1)));
    }

    #[test]
    fn time_and_uri_features() {
        // 2026-10-14 15:20 UTC, a Wednesday by predictor_build's weekday (Fri = 4 at day 0)
        let t = time_features(1_791_990_000, SPLIT_SLOT, None, Some(300));
        let h = (1_791_990_000i64 / 3_600 % 24) as f64;
        assert_eq!(t[0], (2.0 * std::f64::consts::PI * h / 24.0).sin());
        assert_eq!((t[3], t[4], t[5]), (1.0, 35.0, 30.0));
        assert_eq!(
            uri_features("https://meta.uxento.io/x/1"),
            [0., 0., 0., 1., 0., 0., 0.]
        );
        assert_eq!(
            uri_features("https://ipfs.io/ipfs/Qm"),
            [1., 0., 1., 0., 0., 0., 0.]
        );
        assert_eq!(uri_features(""), [0., 0., 0., 0., 0., 0., 1.]);
        assert_eq!(
            uri_features("HTTPS://ipfs.io/"),
            [1., 0., 0., 0., 0., 0., 1.]
        );
        assert_eq!(
            uri_host("https://Pump.MyPinata.cloud"),
            Some("pump.mypinata.cloud".into())
        );
    }
}

// ----------------------------------------------------------------------------- cold-side lane

use rugsnipe_core::rslog::ShadowOutcome;
use rugsnipe_state::rows::{FilterRow, OutcomeRow};
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::Arc;

/// Session totals of one tertile (or of the unscored intents).
#[derive(Debug, Clone, Default, Serialize)]
pub struct TertileStats {
    /// Scored intents in this tertile (every resolved verdict for `unscored`).
    pub intents: u64,
    pub mean_score: f64,
    /// Sends that would have been made: fills + failed attempts.
    pub attempts: u64,
    pub fills: u64,
    pub failed_attempts: u64,
    /// Dropped by the filter (`skip` mode; never sent, no cost).
    pub filtered: u64,
    pub failed_cost_lamports: u64,
    pub closed: u64,
    /// Closed positions whose first exit was the take-profit.
    pub tp_exits: u64,
    pub realized_lamports: i64,
    /// realized - round trips - failed attempts.
    pub net_lamports: i64,
    /// Mean net return per closed position (fraction of the stake).
    pub return_per_fill_net: f64,
    /// net / (closed + failed attempts), as a fraction of the mean stake.
    pub ev_per_attempt: f64,
    #[serde(skip)]
    score_sum: f64,
    #[serde(skip)]
    spend_sum: u64,
    #[serde(skip)]
    closed_spend: u64,
}

impl TertileStats {
    fn derive(&mut self, round_trip: u64) {
        let f = |a: f64, b: f64| if b > 0.0 { a / b } else { 0.0 };
        let rt = (self.closed * round_trip) as i64;
        self.net_lamports = self.realized_lamports - rt - self.failed_cost_lamports as i64;
        self.return_per_fill_net = f(
            (self.realized_lamports - rt) as f64,
            self.closed_spend as f64,
        );
        let n = (self.closed + self.failed_attempts) as f64;
        self.ev_per_attempt = f(f(self.net_lamports as f64, n), f(self.spend_sum as f64, n));
        self.mean_score = f(self.score_sum, self.intents as f64);
    }
}

/// The entry filter's part of the session summary (`cold.paper.filter`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct FilterReport {
    /// Model name, or empty when no model is configured (the outcome store
    /// still runs).
    pub model: String,
    pub mode: String,
    pub t1: f64,
    pub t2: f64,
    pub scored: u64,
    pub unscored: u64,
    pub skipped: u64,
    /// `low` (score < t1), `mid`, `high` (>= t2), `unscored`.
    pub tertiles: BTreeMap<String, TertileStats>,
    /// Outcome rows reloaded from `state.db` at start, written this session,
    /// and the store's size now.
    pub store_rows_loaded: u64,
    pub outcomes_written: u64,
    pub store_rows: u64,
    pub store_signers: u64,
    pub store_wallets: u64,
}

const TERTILE_NAMES: [&str; 4] = ["low", "mid", "high", "unscored"];

#[derive(Debug, Clone, Default)]
struct Pend {
    mint: Pubkey,
    signer: Pubkey,
    launch_ms: u64,
    rate60: u32,
    rate600: u32,
    spend: u64,
    s0_third: Option<u32>,
    wallets: Vec<Pubkey>,
    feat: Option<Feat>,
    op: OpStats,
    bw: OpStats,
    /// 0..=2, or 3 = unscored.
    bucket: Option<usize>,
    first_tp: Option<bool>,
}

/// The cold thread's side of the filter: feature assembly on each E1
/// verdict, the live outcome store (rows due 10 s after their launch,
/// persisted by the caller), the per-tertile session totals.
#[derive(Debug, Default)]
pub struct FilterLane {
    pub model: Option<Arc<Model>>,
    pub store: OutcomeStore,
    pub report: FilterReport,
    delay_ms: u64,
    round_trip: u64,
    pend: HashMap<u64, Pend>,
    by_mint: HashMap<Pubkey, u64>,
    spend: HashMap<u64, u64>,
    due: Vec<OutcomeRow>,
}

impl FilterLane {
    /// `loaded`: the outcome rows of `state.db`.
    pub fn new(
        model: Option<Arc<Model>>,
        mode: rugsnipe_core::config::FilterMode,
        loaded: &[OutcomeRow],
        round_trip: u64,
    ) -> Self {
        let store = OutcomeStore::from_rows(loaded);
        let mut report = FilterReport {
            model: model.as_ref().map(|m| m.name.clone()).unwrap_or_default(),
            mode: match mode {
                rugsnipe_core::config::FilterMode::Log => "log".into(),
                rugsnipe_core::config::FilterMode::Skip => "skip".into(),
            },
            t1: model.as_ref().map_or(f64::NAN, |m| m.t1),
            t2: model.as_ref().map_or(f64::NAN, |m| m.t2),
            store_rows_loaded: loaded.len() as u64,
            ..Default::default()
        };
        for n in TERTILE_NAMES {
            report.tertiles.insert(n.into(), TertileStats::default());
        }
        let mut s = Self {
            delay_ms: model
                .as_ref()
                .map_or(OUTCOME_DELAY_MS, |m| m.outcome_delay_ms),
            model,
            store,
            report,
            round_trip,
            ..Default::default()
        };
        s.sync_store();
        s
    }

    fn sync_store(&mut self) {
        self.report.store_rows = self.store.rows;
        self.report.store_signers = self.store.signers.len() as u64;
        self.report.store_wallets = self.store.wallets.len() as u64;
    }

    fn bucket(&mut self, k: usize) -> &mut TertileStats {
        self.report
            .tertiles
            .get_mut(TERTILE_NAMES[k])
            .expect("all four buckets exist")
    }

    /// The decision's stake (`ColdMsg::Intent`).
    pub fn on_intent(&mut self, intent: u64, spend: u64) {
        self.spend.insert(intent, spend);
        if self.spend.len() > 100_000 {
            self.spend.clear();
        }
    }

    /// A paper intent waits for its fill (`ColdMsg::PaperPending`).
    pub fn on_pending(
        &mut self,
        intent: u64,
        mint: Pubkey,
        signer: Pubkey,
        decided_ms: u64,
        rate60: u32,
        rate600: u32,
    ) {
        let spend = self.spend.remove(&intent).unwrap_or(0);
        self.by_mint.insert(mint, intent);
        self.pend.insert(
            intent,
            Pend {
                mint,
                signer,
                launch_ms: decided_ms,
                rate60,
                rate600,
                spend,
                ..Default::default()
            },
        );
    }

    /// The E1 lane's result: the launch's creation-slot facts and, with a
    /// model and an attempt verdict, the feature vector (point in time: the
    /// store as of now, without this launch) with the signer's and the best
    /// bundle wallet's LEAD-outcome counts.
    pub fn on_e1(&mut self, r: &crate::e1::E1Res, funding: Funding) -> Option<(Feat, u32, u32)> {
        let p = self.pend.get_mut(&r.req.intent)?;
        if let Some(bt) = r.create_tx.as_ref().and_then(|t| t["blockTime"].as_i64()) {
            p.launch_ms = (bt.max(0) as u64) * 1_000;
        }
        let cset = [r.req.creator, r.req.user, r.fee_payer.unwrap_or(r.req.user)];
        let b = bundle_features(&r.s0_trades, &cset, None);
        if let Some(b) = &b {
            p.s0_third = Some(b.s0_third);
            p.wallets = b.wallets.clone();
        }
        let attempt = matches!(
            r.outcome,
            ShadowOutcome::Filled | ShadowOutcome::Premium | ShadowOutcome::NotInSlot0
        );
        let m = self.model.as_ref()?;
        let (b, tx) = (b?, r.create_tx.as_ref()?);
        if !attempt {
            return None;
        }
        let uri = create_uri(tx);
        let create = create_tx_features(
            tx,
            r.slot0.as_ref().map(|s| s.create_index),
            &r.req.creator,
            &r.req.user,
            &uri,
            &m.tip_accounts,
        );
        let op = self.store.signer(&r.req.user);
        let bw = self.store.best_wallet(&b.wallets);
        let time = time_features(
            (p.launch_ms / 1_000) as i64,
            r.req.slot,
            rate_decode(p.rate60),
            rate_decode(p.rate600),
        );
        let f = assemble(
            &b.x,
            &create,
            &funding_features(&funding),
            &op.features(),
            &time,
        );
        p.feat = Some(f);
        p.op = op;
        p.bw = bw;
        Some((f, op.n_lead, bw.n_lead))
    }

    /// The hot loop's score of an intent; the `filter` row to persist.
    pub fn on_score(
        &mut self,
        intent: u64,
        mint: Pubkey,
        scored: bool,
        score: f64,
        tertile: u8,
        skipped: bool,
    ) -> FilterRow {
        let k = if scored { (tertile as usize).min(2) } else { 3 };
        let (op, bw, feat) = match self.pend.get_mut(&intent) {
            Some(p) => {
                p.bucket = Some(k);
                (p.op, p.bw, p.feat)
            }
            None => Default::default(),
        };
        if scored {
            self.report.scored += 1;
        } else {
            self.report.unscored += 1;
        }
        self.report.skipped += skipped as u64;
        let rt = self.round_trip;
        let b = self.bucket(k);
        b.intents += 1;
        if scored {
            b.score_sum += score;
        }
        b.derive(rt);
        FilterRow {
            intent_id: intent as i64,
            mint,
            model: self.report.model.clone(),
            score: scored.then_some(score),
            tertile: scored.then_some(tertile),
            mode: self.report.mode.clone(),
            skipped,
            op_prior: op.n_launch,
            op_lead_n: op.n_lead,
            op_lead_hit: op.lead_hit(),
            op_lead_ev: op.lead_ev(),
            bw_lead_n: bw.n_lead,
            bw_lead_ev: bw.lead_ev(),
            features: feat.map(|f| serde_json::to_string(&f.x.to_vec()).unwrap_or_default()),
        }
    }

    /// A resolved E1 verdict (`ColdMsg::Shadow`).
    pub fn on_shadow(&mut self, intent: u64, outcome: ShadowOutcome, cost: u64, now_ms: u64) {
        let Some(p) = self.pend.get(&intent) else {
            return;
        };
        let (k, spend) = (p.bucket.unwrap_or(3), p.spend);
        let rt = self.round_trip;
        let b = self.bucket(k);
        match outcome {
            ShadowOutcome::Filled => {
                b.attempts += 1;
                b.fills += 1;
            }
            o if o.is_failed_attempt() => {
                b.attempts += 1;
                b.failed_attempts += 1;
                b.failed_cost_lamports += cost;
                b.spend_sum += spend;
            }
            ShadowOutcome::Filtered => b.filtered += 1,
            _ => {}
        }
        b.derive(rt);
        match outcome {
            ShadowOutcome::Filled => {}
            ShadowOutcome::Class | ShadowOutcome::Unresolved => {
                // outside the entry class (or unknown): not a launch of the store
                if let Some(p) = self.pend.remove(&intent) {
                    self.by_mint.remove(&p.mint);
                }
            }
            o => self.finalize(intent, o.as_str(), None, now_ms),
        }
    }

    /// An E2 buy above `v_max` (no verdict of its own).
    pub fn on_unfilled(&mut self, intent: u64, now_ms: u64) {
        if self.pend.contains_key(&intent) {
            self.finalize(intent, "unfilled", None, now_ms);
        }
    }

    /// A sell fill: the first one says whether the take-profit fired.
    pub fn on_exit(&mut self, mint: &Pubkey, take_profit: bool) {
        if let Some(p) = self.by_mint.get(mint).and_then(|i| self.pend.get_mut(i)) {
            p.first_tp.get_or_insert(take_profit);
        }
    }

    /// A position closed with `pnl` (realized, before the round trip).
    pub fn on_closed(&mut self, mint: &Pubkey, pnl: i64, now_ms: u64) {
        let Some(&intent) = self.by_mint.get(mint) else {
            return;
        };
        let Some(p) = self.pend.get(&intent) else {
            return;
        };
        let (k, spend, tp) = (p.bucket.unwrap_or(3), p.spend, p.first_tp == Some(true));
        let rt = self.round_trip;
        let b = self.bucket(k);
        b.closed += 1;
        b.tp_exits += tp as u64;
        b.realized_lamports += pnl;
        b.closed_spend += spend;
        b.spend_sum += spend;
        b.derive(rt);
        let ret = if spend > 0 {
            (pnl - rt as i64) as f64 / spend as f64
        } else {
            0.0
        };
        self.finalize(intent, "filled", Some((ret, tp)), now_ms);
    }

    fn finalize(&mut self, intent: u64, outcome: &str, lead: Option<(f64, bool)>, now_ms: u64) {
        let Some(p) = self.pend.remove(&intent) else {
            return;
        };
        self.by_mint.remove(&p.mint);
        let (ret, tp) = lead.unwrap_or((0.0, false));
        self.due.push(OutcomeRow {
            intent_id: intent as i64,
            mint: p.mint,
            signer: p.signer,
            launch_ms: p.launch_ms,
            outcome: outcome.to_string(),
            lead: lead.is_some(),
            ret,
            tp_hit: tp,
            s0_third: p.s0_third,
            bundle_wallets: p.wallets,
            written_ms: now_ms.max(p.launch_ms + self.delay_ms),
        });
    }

    /// Outcome rows whose time has come (`written_ms <= now_ms`; every row
    /// when `all`): applied to the store and returned for `state.db`.
    pub fn take_due(&mut self, now_ms: u64, all: bool) -> Vec<OutcomeRow> {
        let (ready, wait): (Vec<OutcomeRow>, Vec<OutcomeRow>) = std::mem::take(&mut self.due)
            .into_iter()
            .partition(|r| all || r.written_ms <= now_ms);
        self.due = wait;
        for r in &ready {
            self.store.apply(r);
        }
        self.report.outcomes_written += ready.len() as u64;
        self.sync_store();
        ready
    }

    /// Pending launches (tests, reports).
    pub fn pending(&self) -> usize {
        self.pend.len() + self.due.len()
    }
}
