//! RPC driver (feature `rpc`): pick a fresh mint, read the chain, simulate.

use crate::*;
use base64::Engine as _;
use pump_codec::curve::BondingCurve;
use rugsnipe_tx::nonce::parse_nonce_account;
use rugsnipe_tx::rpc::RpcClient;
use serde_json::{json, Value};
use std::time::Duration;

/// pump.fun frontend API.
pub const PUMP_API: &str = "https://frontend-api-v3.pump.fun";

/// A coin to test.
#[derive(Debug, Clone)]
pub struct Coin {
    pub mint: Pubkey,
    pub bonding_curve: Pubkey,
    pub token_program: Pubkey,
    pub created_ms: u64,
    pub symbol: String,
    pub virtual_sol: u64,
    pub real_sol: u64,
}

/// `rpc.call` retried on rate limiting (HTTP 429 / "rate limit" errors)
/// with a growing pause; public endpoints throttle per IP.
pub fn call_retry(rpc: &RpcClient, method: &str, params: Value) -> Result<Value, String> {
    let mut last = String::new();
    for attempt in 0..6u64 {
        match rpc.call(method, params.clone()) {
            Ok(v) => return Ok(v),
            Err(e) if e.contains("429") || e.to_ascii_lowercase().contains("rate limit") => {
                last = e;
                std::thread::sleep(Duration::from_millis(1_500 * (attempt + 1)));
            }
            Err(e) => return Err(e),
        }
    }
    Err(last)
}

fn pk(v: &Value) -> Option<Pubkey> {
    v.as_str()?.parse().ok()
}

/// Newest SOL-quoted, non-mayhem, incomplete coin created within `max_age_s`.
pub fn fresh_coin(rpc: &RpcClient, now_ms: u64, max_age_s: u64) -> Result<Coin, String> {
    fresh_coins(rpc, now_ms, max_age_s, 1)?
        .into_iter()
        .next()
        .ok_or_else(|| format!("no SOL-quoted coin younger than {max_age_s}s in the latest 50"))
}

/// Up to `n` newest SOL-quoted, non-mayhem, incomplete coins created within
/// `max_age_s` (from the latest 50 of the pump.fun API).
pub fn fresh_coins(
    rpc: &RpcClient,
    now_ms: u64,
    max_age_s: u64,
    n: usize,
) -> Result<Vec<Coin>, String> {
    let url = format!(
        "{PUMP_API}/coins?offset=0&limit=50&sort=created_timestamp&order=DESC&includeNsfw=true"
    );
    let v = rpc.get_json(&url)?;
    let mut out = vec![];
    for c in v.as_array().ok_or("coins: not an array")? {
        if out.len() >= n {
            break;
        }
        let created = c["created_timestamp"].as_u64().unwrap_or(0);
        let sol = c["quote_mint"].as_str() == Some("11111111111111111111111111111111")
            || c["quote_mint"].is_null();
        let mayhem = c["is_mayhem_mode"].as_bool().unwrap_or(false);
        if c["complete"].as_bool() == Some(false)
            && sol
            && !mayhem
            && now_ms.saturating_sub(created) <= max_age_s * 1_000
        {
            out.push(Coin {
                mint: pk(&c["mint"]).ok_or("mint")?,
                bonding_curve: pk(&c["bonding_curve"]).ok_or("bonding_curve")?,
                token_program: pk(&c["token_program"])
                    .unwrap_or(pump_codec::constants::TOKEN_2022_PROGRAM_ID),
                created_ms: created,
                symbol: c["symbol"].as_str().unwrap_or("").to_string(),
                virtual_sol: 0,
                real_sol: 0,
            });
        }
    }
    Ok(out)
}

/// `getMultipleAccounts` with a 36-byte slice (sizes from `space`).
pub fn accounts(rpc: &RpcClient, keys: &[Pubkey]) -> Result<Vec<Option<AcctInfo>>, String> {
    let mut out = vec![];
    for chunk in keys.chunks(100) {
        let ks: Vec<String> = chunk.iter().map(|k| k.to_string()).collect();
        let r = call_retry(
            rpc,
            "getMultipleAccounts",
            json!([ks, {"encoding": "base64", "dataSlice": {"offset": 0, "length": 36}, "commitment": "processed"}]),
        )?;
        for (k, a) in chunk.iter().zip(r["value"].as_array().ok_or("value")?) {
            if a.is_null() {
                out.push(None);
                continue;
            }
            let data = base64::engine::general_purpose::STANDARD
                .decode(a["data"][0].as_str().unwrap_or(""))
                .unwrap_or_default();
            let owner = pk(&a["owner"]).unwrap_or_default();
            let executable = a["executable"].as_bool().unwrap_or(false);
            let programdata = (owner == BPF_UPGRADEABLE_LOADER && executable && data.len() >= 36)
                .then(|| Pubkey::new_from_array(data[4..36].try_into().unwrap()));
            out.push(Some(AcctInfo {
                key: *k,
                owner,
                space: a["space"].as_u64().ok_or("space missing")?,
                executable,
                programdata,
            }));
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    Ok(out)
}

/// Full account data (small accounts only).
pub fn account_data(
    rpc: &RpcClient,
    key: &Pubkey,
) -> Result<Option<(Vec<u8>, Pubkey, u64)>, String> {
    let r = call_retry(
        rpc,
        "getAccountInfo",
        json!([key.to_string(), {"encoding": "base64", "commitment": "processed"}]),
    )?;
    let v = &r["value"];
    if v.is_null() {
        return Ok(None);
    }
    let data = base64::engine::general_purpose::STANDARD
        .decode(v["data"][0].as_str().unwrap_or(""))
        .map_err(|e| e.to_string())?;
    Ok(Some((
        data,
        pk(&v["owner"]).unwrap_or_default(),
        v["lamports"].as_u64().unwrap_or(0),
    )))
}

/// Loaded bytes computed from live account sizes for the keys of `tx`.
pub fn computed_loaded_bytes(rpc: &RpcClient, tx_keys: &[Pubkey]) -> Result<u64, String> {
    let accts = accounts(rpc, tx_keys)?;
    let pds: Vec<Pubkey> = accts
        .iter()
        .flatten()
        .filter_map(|a| a.programdata)
        .collect();
    let pd_accts = accounts(rpc, &pds)?;
    let pd_sizes: Vec<(Pubkey, u64)> = pd_accts
        .iter()
        .flatten()
        .map(|a| (a.key, a.space))
        .collect();
    Ok(loaded_bytes(&accts, &pd_sizes))
}

/// `simulateTransaction` with `sigVerify: false`.
pub fn simulate(rpc: &RpcClient, tx: &[u8], replace_blockhash: bool) -> Result<Value, String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(tx);
    call_retry(
        rpc,
        "simulateTransaction",
        json!([b64, {"encoding": "base64", "sigVerify": false, "replaceRecentBlockhash": replace_blockhash, "commitment": "processed"}]),
    )
}

/// Pick a funded wallet: the fee payer of a recent successful trade on the
/// curve with at least `min_lamports`, preferring one whose pump
/// `user_volume_accumulator` already exists (as on our own pre-created
/// wallet); else `fallback`. Returns `(wallet, lamports, uva_exists)`.
pub fn pick_wallet(
    rpc: &RpcClient,
    bonding_curve: &Pubkey,
    min_lamports: u64,
    fallback: &Pubkey,
) -> Result<(Pubkey, u64, bool), String> {
    let sigs = call_retry(
        rpc,
        "getSignaturesForAddress",
        json!([bonding_curve.to_string(), {"limit": 20, "commitment": "confirmed"}]),
    )?;
    let mut without_uva = None;
    for s in sigs.as_array().into_iter().flatten().take(10) {
        if !s["err"].is_null() {
            continue;
        }
        std::thread::sleep(Duration::from_millis(300));
        let Ok(t) = call_retry(
            rpc,
            "getTransaction",
            json!([s["signature"], {"encoding": "json", "maxSupportedTransactionVersion": 1, "commitment": "confirmed"}]),
        ) else {
            continue;
        };
        let Some(payer) = pk(&t["transaction"]["message"]["accountKeys"][0]) else {
            continue;
        };
        let bal = t["meta"]["postBalances"][0].as_u64().unwrap_or(0);
        if bal < min_lamports {
            continue;
        }
        std::thread::sleep(Duration::from_millis(300));
        if account_data(rpc, &pump_codec::pda::user_volume_accumulator(&payer))?.is_some() {
            return Ok((payer, bal, true));
        }
        without_uva.get_or_insert((payer, bal, false));
    }
    if let Some(w) = without_uva {
        return Ok(w);
    }
    let r = call_retry(rpc, "getBalance", json!([fallback.to_string()]))?;
    let uva = account_data(rpc, &pump_codec::pda::user_volume_accumulator(fallback))?.is_some();
    Ok((*fallback, r["value"].as_u64().unwrap_or(0), uva))
}

/// Simulate `cases` for `coin` from `wallet`; `nonce` = (account, authority, value).
pub fn run_cases(
    rpc: &RpcClient,
    coin: &Coin,
    wallet: &Pubkey,
    nonce: Option<(Pubkey, Pubkey, [u8; 32])>,
    cases: &[Case],
    spend: u64,
) -> Result<Vec<(SimRow, u64)>, String> {
    run_cases_opt(rpc, coin, wallet, nonce, cases, spend, true)
}

/// [`run_cases`]; `compute_loaded = false` skips the loaded-bytes
/// computation (two `getMultipleAccounts` per case) and reports 0.
pub fn run_cases_opt(
    rpc: &RpcClient,
    coin: &Coin,
    wallet: &Pubkey,
    nonce: Option<(Pubkey, Pubkey, [u8; 32])>,
    cases: &[Case],
    spend: u64,
    compute_loaded: bool,
) -> Result<Vec<(SimRow, u64)>, String> {
    let mut rows = vec![];
    for case in cases {
        if case.nonce && nonce.is_none() {
            continue;
        }
        let nk = nonce.map(|(a, auth, _)| NonceKeys {
            account: a,
            authority: auth,
        });
        let lifetime = if case.nonce {
            nonce.unwrap().2
        } else {
            [0u8; 32]
        };
        let t = build_case(
            case,
            wallet,
            &coin.mint,
            &coin.token_program,
            nk,
            &lifetime,
            spend,
            10_000,
            1_000,
        )
        .map_err(|e| e.to_string())?;
        let view = pump_codec::tx::TxView::parse(t.bytes()).map_err(|e| e.to_string())?;
        let keys = view.static_account_keys().to_vec();
        let loaded = if compute_loaded {
            computed_loaded_bytes(rpc, &keys)?
        } else {
            0
        };
        std::thread::sleep(Duration::from_millis(300));
        let v = simulate(rpc, t.bytes(), !case.nonce)?;
        let val = &v["value"];
        let logs: Vec<String> = val["logs"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|l| l.as_str().map(str::to_string))
            .collect();
        let s = snipe_s(wallet, &coin.mint, &coin.token_program);
        let (cu, lads) = case.limits(s);
        rows.push((
            SimRow {
                label: case.label(),
                format: case.format,
                nonce: case.nonce,
                tight: case.tight,
                s,
                size: t.len(),
                cu_limit: cu,
                lads_limit: lads,
                err: (!val["err"].is_null()).then(|| val["err"].to_string()),
                units_consumed: val["unitsConsumed"].as_u64(),
                loaded_reported: val["loadedAccountsDataSize"].as_u64(),
                fee: val["fee"].as_u64(),
                logs_tail: logs.iter().rev().take(4).rev().cloned().collect(),
            },
            loaded,
        ));
        std::thread::sleep(Duration::from_millis(300));
    }
    Ok(rows)
}

/// Read the curve (quote must be SOL) and fill reserves.
pub fn read_curve(rpc: &RpcClient, coin: &mut Coin) -> Result<(), String> {
    let (data, owner, _) =
        account_data(rpc, &coin.bonding_curve)?.ok_or("bonding curve missing")?;
    if owner != pump_codec::constants::PUMP_PROGRAM_ID {
        return Err("bonding curve not owned by pump".into());
    }
    let bc = BondingCurve::decode(&data).map_err(|e| e.to_string())?;
    if !bc.is_sol_quoted() {
        return Err("curve is not SOL-quoted".into());
    }
    if bc.complete {
        return Err("curve already complete".into());
    }
    // the API's `is_mayhem_mode` can be absent: the chain flag decides (a
    // mayhem curve has another k and costs ~400 CU less, so it is no test)
    if bc.is_mayhem_mode {
        return Err("curve is in mayhem mode (rerun for another coin)".into());
    }
    let s = bc.state();
    coin.virtual_sol = s.virtual_sol;
    coin.real_sol = s.real_sol;
    Ok(())
}

/// Nonce account -> (authority, value).
pub fn read_nonce(rpc: &RpcClient, account: &Pubkey) -> Result<(Pubkey, [u8; 32]), String> {
    let (data, _, _) = account_data(rpc, account)?.ok_or("nonce account missing")?;
    let (auth, value, _) = parse_nonce_account(&data).ok_or("not an initialized nonce account")?;
    Ok((auth, value))
}

/// One mint of a [`fit_live`] run.
#[derive(Debug, Clone)]
pub struct MintFit {
    pub coin: Coin,
    pub rows: Vec<(SimRow, u64)>,
    pub fits: Vec<Fit>,
    /// Tight (per-mint limit) runs that failed.
    pub tight_failures: Vec<String>,
}

/// Measured vs formula on up to `n` fresh mints: for each, the `formats`
/// templates loose (to measure) and tight (the per-mint limit, to check),
/// with and without the nonce when one is given. Read-only
/// (`simulateTransaction`, `sigVerify: false`). Coins whose curve cannot be
/// read (complete, mayhem, gone) are skipped with a note in `skipped`.
pub fn fit_live(
    rpc: &RpcClient,
    now_ms: u64,
    max_age_s: u64,
    n: usize,
    wallet: &Pubkey,
    nonce: Option<(Pubkey, Pubkey, [u8; 32])>,
    formats: &[TxFormat],
    spend: u64,
) -> Result<(Vec<MintFit>, Vec<String>), String> {
    let mut out = vec![];
    let mut skipped = vec![];
    let mut cases = vec![];
    for &f in formats {
        for nn in [false, true] {
            if nn && nonce.is_none() {
                continue;
            }
            for tight in [false, true] {
                cases.push(Case {
                    format: f,
                    nonce: nn,
                    tight,
                });
            }
        }
    }
    for mut coin in fresh_coins(rpc, now_ms, max_age_s, n.saturating_mul(2))? {
        if out.len() >= n {
            break;
        }
        if let Err(e) = read_curve(rpc, &mut coin) {
            skipped.push(format!("{}: {e}", coin.mint));
            continue;
        }
        let rows = run_cases_opt(rpc, &coin, wallet, nonce, &cases, spend, false)?;
        let fits = rows.iter().filter_map(|(r, _)| fit(r)).collect();
        let tight_failures = rows
            .iter()
            .filter(|(r, _)| r.tight && r.err.is_some())
            .map(|(r, _)| {
                format!(
                    "{} (limit {}): {}",
                    r.label,
                    r.cu_limit,
                    r.err.clone().unwrap_or_default()
                )
            })
            .collect();
        out.push(MintFit {
            coin,
            rows,
            fits,
            tight_failures,
        });
    }
    Ok((out, skipped))
}
