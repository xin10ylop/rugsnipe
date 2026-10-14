//! The official IDLs (`fixtures/idl/pump.json`, `pump_amm.json`: verbatim
//! copies of `pump-fun/pump-public-docs` `idl/`, byte-identical to the IDLs
//! bundled in `pump-rust-client` 0.4.1) as an oracle:
//!
//! * every discriminator in `constants` equals the IDL's, by name;
//! * every account of our builders equals the IDL spec: fixed `address`,
//!   `pda` seeds derived from the IDL itself, writable and signer flags;
//! * every positional account our parsers read has the IDL name we assume,
//!   and every argument list has the order and types our parsers read;
//! * borsh payloads generated from the IDL type definitions (a unique value
//!   per field) decode with every field landing in the field of that name.

mod common;

use common::*;
use pump_codec::constants as c;
use pump_codec::curve::{BondingCurve, FeeConfig, Global};
use pump_codec::events::{decode_event_payload, EventSource, PumpEvent};
use pump_codec::instructions::{self as ixs, LegacyTradeKeys, SolV3Keys};
use pump_codec::{AccountMeta, Pubkey};
use serde_json::Value;
use std::collections::HashMap;

fn idl(name: &str) -> Value {
    read_json(&format!("idl/{name}.json"))
}

fn bytes(v: &Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|b| b.as_u64().unwrap() as u8)
        .collect()
}

fn find<'a>(list: &'a Value, name: &str) -> Option<&'a Value> {
    list.as_array()?.iter().find(|x| x["name"] == name)
}

fn disc8(v: &Value) -> [u8; 8] {
    bytes(&v["discriminator"]).try_into().unwrap()
}

#[test]
fn program_ids_and_discriminators_match_the_idls() {
    let pump = idl("pump");
    let amm = idl("pump_amm");
    assert_eq!(pump["address"], c::PUMP_PROGRAM_ID.to_string());
    assert_eq!(amm["address"], c::PUMP_SWAP_PROGRAM_ID.to_string());
    for (name, d) in c::ix::ALL {
        let ix = find(&pump["instructions"], name).unwrap_or_else(|| panic!("pump ix {name}"));
        assert_eq!(disc8(ix), *d, "pump ix {name}");
    }
    for (name, d) in c::amm_ix::ALL {
        let ix = find(&amm["instructions"], name).unwrap_or_else(|| panic!("amm ix {name}"));
        assert_eq!(disc8(ix), *d, "amm ix {name}");
    }
    for (name, d) in c::event::ALL {
        let ev = find(&pump["events"], name)
            .or_else(|| find(&amm["events"], name))
            .unwrap_or_else(|| panic!("event {name}"));
        assert_eq!(disc8(ev), *d, "event {name}");
    }
    // PumpSwap's events of the same name are the AMM ones (BuyEvent etc. are
    // AMM-only; pump's TradeEvent is pump-only).
    for (name, d) in [
        ("BuyEvent", c::event::AMM_BUY),
        ("SellEvent", c::event::AMM_SELL),
        ("CreatePoolEvent", c::event::AMM_CREATE_POOL),
    ] {
        assert_eq!(disc8(find(&amm["events"], name).unwrap()), d, "{name}");
    }
    for (name, d) in c::account::ALL {
        let a = find(&pump["accounts"], name)
            .or_else(|| find(&amm["accounts"], name))
            .unwrap_or_else(|| panic!("account {name}"));
        assert_eq!(disc8(a), *d, "account {name}");
    }
}

// ------------------------------------------------- builders vs IDL spec
/// Resolve an IDL seed / program spec to bytes.
fn seed_bytes(
    seed: &Value,
    by_name: &HashMap<String, Pubkey>,
    extra: &HashMap<&str, Pubkey>,
) -> Vec<u8> {
    match seed["kind"].as_str().unwrap() {
        "const" => bytes(&seed["value"]),
        "account" => {
            let path = seed["path"].as_str().unwrap();
            by_name
                .get(path)
                .or_else(|| extra.get(path))
                .unwrap_or_else(|| panic!("unresolved seed path {path}"))
                .to_bytes()
                .to_vec()
        }
        k => panic!("seed kind {k} not used by these instructions"),
    }
}

/// Check `metas` (our builder output) against the IDL accounts of `ix_name`
/// (`metas` may carry `remaining` extra accounts after the IDL ones).
fn check_against_idl(
    program: &Value,
    program_id: &Pubkey,
    ix_name: &str,
    metas: &[AccountMeta],
    remaining: usize,
    extra: &HashMap<&str, Pubkey>,
) -> usize {
    let ix = find(&program["instructions"], ix_name).unwrap();
    let spec = ix["accounts"].as_array().unwrap();
    assert_eq!(
        metas.len(),
        spec.len() + remaining,
        "{ix_name}: account count"
    );
    let by_name: HashMap<String, Pubkey> = spec
        .iter()
        .zip(metas)
        .map(|(a, m)| (a["name"].as_str().unwrap().to_string(), m.pubkey))
        .collect();
    let mut derived = 0;
    for (i, (a, m)) in spec.iter().zip(metas).enumerate() {
        let name = a["name"].as_str().unwrap();
        let ctx = format!("{ix_name} #{i} {name}");
        assert_eq!(m.is_writable, a["writable"] == true, "{ctx}: writable");
        assert_eq!(m.is_signer, a["signer"] == true, "{ctx}: signer");
        if let Some(addr) = a["address"].as_str() {
            assert_eq!(m.pubkey.to_string(), addr, "{ctx}: address");
        }
        if let Some(pda) = a.get("pda") {
            let seeds: Vec<Vec<u8>> = pda["seeds"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| seed_bytes(s, &by_name, extra))
                .collect();
            let seed_refs: Vec<&[u8]> = seeds.iter().map(|s| s.as_slice()).collect();
            let prog = match pda.get("program") {
                Some(p) => Pubkey::try_from(seed_bytes(p, &by_name, extra).as_slice()).unwrap(),
                None => *program_id,
            };
            let want = Pubkey::find_program_address(&seed_refs, &prog).0;
            assert_eq!(m.pubkey, want, "{ctx}: PDA from IDL seeds");
            derived += 1;
        }
    }
    derived
}

#[test]
fn builders_match_the_idl_account_specs_and_derive_every_pda_from_it() {
    let pump = idl("pump");
    let mint = pk("CzBigdLBY9C261pL2fgJ2icAeZcuZFDnivk8FB1mxzCX");
    let user = Pubkey::new_from_array([0x42; 32]);
    let creator = Pubkey::new_from_array([0x24; 32]);
    let none = HashMap::new();
    let mut derived = 0;
    for tp in [c::TOKEN_2022_PROGRAM_ID, c::SPL_TOKEN_PROGRAM_ID] {
        let k = SolV3Keys::derive(mint, user, tp, c::BUYBACK_FEE_RECIPIENTS[2]);
        for (name, ix) in [
            ("buy_v3", ixs::buy_v3(&k, 1, 2, Some(true))),
            ("sell_v3", ixs::sell_v3(&k, 1, 2)),
            (
                "buy_exact_quote_in_v3",
                ixs::buy_exact_quote_in_v3(&k, 1, 2, None),
            ),
        ] {
            assert_eq!(ix.program_id, c::PUMP_PROGRAM_ID);
            derived += check_against_idl(&pump, &c::PUMP_PROGRAM_ID, name, &ix.accounts, 0, &none);
        }
        let lk = LegacyTradeKeys::derive(
            mint,
            user,
            creator,
            tp,
            c::FEE_RECIPIENTS[1],
            c::BUYBACK_FEE_RECIPIENTS[2],
        );
        let extra: HashMap<&str, Pubkey> = [("bonding_curve.creator", creator)].into();
        let buy = ixs::legacy_buy(&lk, 1, 2, Some(true), Some(false)).unwrap();
        let bxsi = ixs::legacy_buy_exact_sol_in(&lk, 1, 2, None, None).unwrap();
        for (name, ix) in [("buy", &buy), ("buy_exact_sol_in", &bxsi)] {
            derived += check_against_idl(&pump, &c::PUMP_PROGRAM_ID, name, &ix.accounts, 2, &extra);
        }
        for cashback in [false, true] {
            let sell = ixs::legacy_sell(&lk, 1, 2, cashback);
            let rem = 2 + cashback as usize;
            derived += check_against_idl(
                &pump,
                &c::PUMP_PROGRAM_ID,
                "sell",
                &sell.accounts,
                rem,
                &extra,
            );
            // remaining: [user_volume_accumulator (cashback)], bonding_curve_v2 (ro), buyback (w)
            let tail = &sell.accounts[14..];
            if cashback {
                assert_eq!(
                    tail[0],
                    AccountMeta::new(pump_codec::pda::user_volume_accumulator(&user), false)
                );
            }
            assert_eq!(
                tail[rem - 2],
                AccountMeta::new_readonly(pump_codec::pda::bonding_curve_v2(&mint), false)
            );
            assert_eq!(
                tail[rem - 1],
                AccountMeta::new(c::BUYBACK_FEE_RECIPIENTS[2], false)
            );
        }
        for ix in [&buy, &bxsi] {
            assert_eq!(
                ix.accounts[16],
                AccountMeta::new_readonly(pump_codec::pda::bonding_curve_v2(&mint), false)
            );
            assert_eq!(
                ix.accounts[17],
                AccountMeta::new(c::BUYBACK_FEE_RECIPIENTS[2], false)
            );
        }
    }
    // global, bonding_curve, the curve ATAs, user volume acc, fee_config,
    // event authority (+ creator_vault, global volume acc on legacy)
    assert!(derived >= 70, "{derived} PDAs derived from IDL seeds");
}

/// (program IDL, instruction names, [(position, IDL account name)]).
type PositionCheck<'a> = (&'a Value, &'a [&'a str], &'a [(usize, &'a str)]);

/// IDL account names at the positions our parsers read.
#[test]
fn parser_account_positions_have_the_expected_idl_names() {
    let pump = idl("pump");
    let amm = idl("pump_amm");
    let name_at = |prog: &Value, ix: &str, i: usize| -> String {
        find(&prog["instructions"], ix).unwrap()["accounts"][i]["name"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let checks: &[PositionCheck<'_>] = &[
        (
            &pump,
            &["create"],
            &[
                (0, "mint"),
                (2, "bonding_curve"),
                (3, "associated_bonding_curve"),
                (6, "metadata"),
                (7, "user"),
                (9, "token_program"),
            ],
        ),
        (
            &pump,
            &["create_v2"],
            &[
                (0, "mint"),
                (2, "bonding_curve"),
                (3, "associated_bonding_curve"),
                (5, "user"),
                (7, "token_program"),
                (10, "global_params"),
                (11, "sol_vault"),
                (12, "mayhem_state"),
                (13, "mayhem_token_vault"),
            ],
        ),
        (
            &pump,
            &["buy", "buy_exact_sol_in"],
            &[
                (1, "fee_recipient"),
                (2, "mint"),
                (3, "bonding_curve"),
                (5, "associated_user"),
                (6, "user"),
                (8, "token_program"),
            ],
        ),
        (
            &pump,
            &["sell"],
            &[
                (1, "fee_recipient"),
                (2, "mint"),
                (3, "bonding_curve"),
                (5, "associated_user"),
                (6, "user"),
                (9, "token_program"),
            ],
        ),
        (
            &pump,
            &["buy_v2", "sell_v2", "buy_exact_quote_in_v2"],
            &[
                (1, "base_mint"),
                (2, "quote_mint"),
                (3, "base_token_program"),
                (6, "fee_recipient"),
                (8, "buyback_fee_recipient"),
                (10, "bonding_curve"),
                (13, "user"),
            ],
        ),
        (
            &pump,
            &["buy_v3", "sell_v3", "buy_exact_quote_in_v3"],
            &[
                (1, "base_mint"),
                (2, "quote_mint"),
                (3, "base_token_program"),
                (5, "bonding_curve"),
                (8, "user"),
                (9, "associated_base_user"),
                (13, "buyback_fee_recipient"),
            ],
        ),
        (
            &amm,
            &[
                "buy",
                "sell",
                "buy_exact_quote_in",
                "buy_v2",
                "sell_v2",
                "buy_exact_quote_in_v2",
            ],
            &[
                (0, "pool"),
                (1, "user"),
                (3, "base_mint"),
                (4, "quote_mint"),
            ],
        ),
        (
            &amm,
            &["create_pool"],
            &[
                (0, "pool"),
                (2, "creator"),
                (3, "base_mint"),
                (4, "quote_mint"),
            ],
        ),
    ];
    for (prog, names, positions) in checks {
        for ix in *names {
            for (i, want) in *positions {
                assert_eq!(name_at(prog, ix, *i), *want, "{ix} #{i}");
            }
        }
    }
    // account counts the parsers / docs assume
    for (prog, ix, n) in [
        (&pump, "create", 14),
        (&pump, "create_v2", 16),
        (&pump, "buy", 16),
        (&pump, "buy_exact_sol_in", 16),
        (&pump, "sell", 14),
        (&pump, "buy_v2", 27),
        (&pump, "buy_exact_quote_in_v2", 27),
        (&pump, "sell_v2", 26),
        (&pump, "buy_v3", 17),
        (&pump, "sell_v3", 17),
        (&pump, "buy_exact_quote_in_v3", 17),
        (&amm, "buy", 23),
        (&amm, "sell", 21),
        (&amm, "buy_exact_quote_in", 23),
        (&amm, "buy_v2", 17),
        (&amm, "sell_v2", 17),
        (&amm, "buy_exact_quote_in_v2", 17),
        (&amm, "create_pool", 18),
    ] {
        let got = find(&prog["instructions"], ix).unwrap()["accounts"]
            .as_array()
            .unwrap()
            .len();
        assert_eq!(got, n, "{ix}");
    }
}

fn type_str(t: &Value) -> String {
    match t {
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            if let Some(d) = o.get("defined") {
                d["name"].as_str().unwrap().to_string()
            } else if let Some(v) = o.get("vec") {
                format!("Vec<{}>", type_str(v))
            } else if let Some(a) = o.get("array") {
                format!("[{};{}]", type_str(&a[0]), a[1])
            } else if let Some(v) = o.get("option") {
                format!("Option<{}>", type_str(v))
            } else {
                panic!("type {t}")
            }
        }
        _ => panic!("type {t}"),
    }
}

/// Argument lists, in the order (and with the types) our parsers read them.
#[test]
fn parser_argument_layouts_match_the_idl() {
    let pump = idl("pump");
    let amm = idl("pump_amm");
    let args = |prog: &Value, ix: &str| -> Vec<String> {
        find(&prog["instructions"], ix).unwrap()["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| format!("{}:{}", a["name"].as_str().unwrap(), type_str(&a["type"])))
            .collect()
    };
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let create = [
        "name:string",
        "symbol:string",
        "uri:string",
        "creator:pubkey",
    ];
    assert_eq!(args(&pump, "create"), s(&create));
    let mut v2 = create.to_vec();
    v2.extend([
        "is_mayhem_mode:bool",
        "is_cashback_enabled:OptionBool",
        "creator_fee_bps:OptionU64",
        "is_holder_reward:OptionBool",
    ]);
    assert_eq!(args(&pump, "create_v2"), s(&v2));
    assert_eq!(
        args(&pump, "buy"),
        s(&[
            "amount:u64",
            "max_sol_cost:u64",
            "track_volume:OptionBool",
            "partial_fill:OptionBool"
        ])
    );
    assert_eq!(
        args(&pump, "buy_exact_sol_in"),
        s(&[
            "spendable_sol_in:u64",
            "min_tokens_out:u64",
            "track_volume:OptionBool",
            "partial_fill:OptionBool"
        ])
    );
    assert_eq!(
        args(&pump, "sell"),
        s(&["amount:u64", "min_sol_output:u64"])
    );
    for v in ["buy_v2", "buy_v3"] {
        assert_eq!(
            args(&pump, v),
            s(&["amount:u64", "max_sol_cost:u64", "partial_fill:OptionBool"]),
            "{v}"
        );
    }
    for v in ["buy_exact_quote_in_v2", "buy_exact_quote_in_v3"] {
        assert_eq!(
            args(&pump, v),
            s(&[
                "spendable_quote_in:u64",
                "min_tokens_out:u64",
                "partial_fill:OptionBool"
            ]),
            "{v}"
        );
    }
    for v in ["sell_v2", "sell_v3"] {
        assert_eq!(
            args(&pump, v),
            s(&["amount:u64", "min_sol_output:u64"]),
            "{v}"
        );
    }
    assert_eq!(
        args(&amm, "buy"),
        s(&[
            "base_amount_out:u64",
            "max_quote_amount_in:u64",
            "track_volume:OptionBool"
        ])
    );
    assert_eq!(
        args(&amm, "buy_exact_quote_in"),
        s(&[
            "spendable_quote_in:u64",
            "min_base_amount_out:u64",
            "track_volume:OptionBool"
        ])
    );
    assert_eq!(
        args(&amm, "buy_v2"),
        s(&["base_amount_out:u64", "max_quote_amount_in:u64"])
    );
    assert_eq!(
        args(&amm, "buy_exact_quote_in_v2"),
        s(&["spendable_quote_in:u64", "min_base_amount_out:u64"])
    );
    for v in ["sell", "sell_v2"] {
        assert_eq!(
            args(&amm, v),
            s(&["base_amount_in:u64", "min_quote_amount_out:u64"]),
            "{v}"
        );
    }
    let cp = args(&amm, "create_pool");
    assert_eq!(
        &cp[..5],
        s(&[
            "index:u16",
            "base_amount_in:u64",
            "quote_amount_in:u64",
            "coin_creator:pubkey",
            "is_mayhem_mode:bool"
        ])
        .as_slice()
    );
    // OptionBool / OptionU64 are single-field structs: 1 byte / 8 bytes on the wire.
    let ty = |n: &str| find(&pump["types"], n).unwrap()["type"]["fields"].clone();
    assert_eq!(ty("OptionBool"), serde_json::json!(["bool"]));
    assert_eq!(ty("OptionU64"), serde_json::json!(["u64"]));
}

// ------------------------------------------ IDL-generated layouts
/// Borsh bytes for an IDL type, with a distinct value per leaf; records
/// `(field name, Debug rendering)` for each named leaf.
struct Gen<'a> {
    types: HashMap<String, &'a Value>,
    n: u64,
    out: Vec<u8>,
    leaves: Vec<(String, String)>,
}

impl Gen<'_> {
    fn next(&mut self) -> u64 {
        self.n += 1;
        self.n
    }
    fn value(&mut self, name: &str, t: &Value) {
        let i = self.next();
        let dbg = match t {
            Value::String(s) => match s.as_str() {
                "u8" => {
                    let v = (i % 200) as u8 + 1;
                    self.out.push(v);
                    v.to_string()
                }
                "u16" => {
                    let v = 1000 + i as u16;
                    self.out.extend(v.to_le_bytes());
                    v.to_string()
                }
                "u64" => {
                    let v = 1_000_000_007 * i;
                    self.out.extend(v.to_le_bytes());
                    v.to_string()
                }
                "i64" => {
                    let v = -(1_000_003 * i as i64);
                    self.out.extend(v.to_le_bytes());
                    v.to_string()
                }
                "u128" => {
                    let v = 10_000_000_000_000_000_019u128 * i as u128;
                    self.out.extend(v.to_le_bytes());
                    v.to_string()
                }
                "i128" => {
                    let v = -(10_000_000_000_000_000_007i128 * i as i128);
                    self.out.extend(v.to_le_bytes());
                    v.to_string()
                }
                "bool" => {
                    let v = i % 2 == 1;
                    self.out.push(v as u8);
                    v.to_string()
                }
                "pubkey" => {
                    let k = Pubkey::new_from_array([i as u8; 32]);
                    self.out.extend(k.to_bytes());
                    format!("{k:?}")
                }
                "string" => {
                    let v = format!("s{i}");
                    self.out.extend((v.len() as u32).to_le_bytes());
                    self.out.extend(v.as_bytes());
                    format!("{v:?}")
                }
                other => panic!("leaf type {other}"),
            },
            Value::Object(o) if o.contains_key("array") => {
                let a = &o["array"];
                assert_eq!(a[0], "pubkey", "only pubkey arrays occur");
                let keys: Vec<Pubkey> = (0..a[1].as_u64().unwrap())
                    .map(|j| Pubkey::new_from_array([(i as u8).wrapping_add(j as u8 * 7); 32]))
                    .collect();
                for k in &keys {
                    self.out.extend(k.to_bytes());
                }
                format!("{keys:?}")
            }
            Value::Object(o) if o.contains_key("vec") => {
                // two elements of a defined struct
                self.out.extend(2u32.to_le_bytes());
                let inner = &o["vec"];
                for _ in 0..2 {
                    self.value("", inner);
                }
                return;
            }
            Value::Object(o) if o.contains_key("defined") => {
                let tn = o["defined"]["name"].as_str().unwrap();
                let fields = self.types[tn]["type"]["fields"].as_array().unwrap().clone();
                for f in &fields {
                    self.value(f["name"].as_str().unwrap(), &f["type"]);
                }
                return;
            }
            other => panic!("type {other}"),
        };
        if !name.is_empty() {
            self.leaves.push((name.to_string(), dbg));
        }
    }
}

fn generate(prog: &Value, ty: &str, disc: [u8; 8]) -> (Vec<u8>, Vec<(String, String)>) {
    let types: HashMap<String, &Value> = prog["types"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["name"].as_str().unwrap().to_string(), t))
        .collect();
    let mut g = Gen {
        types,
        n: 0,
        out: disc.to_vec(),
        leaves: Vec::new(),
    };
    let fields = g.types[ty]["type"]["fields"].as_array().unwrap().clone();
    for f in &fields {
        g.value(f["name"].as_str().unwrap(), &f["type"]);
    }
    (g.out, g.leaves)
}

/// Every generated leaf appears in `debug` as `name: value` (or `Some(value)`).
fn assert_leaves(ctx: &str, debug: &str, leaves: &[(String, String)], rename: &[(&str, &str)]) {
    for (name, val) in leaves {
        let name = rename
            .iter()
            .find(|(from, _)| from == name)
            .map(|(_, to)| *to)
            .unwrap_or(name);
        let hit = [format!("{{ {name}: "), format!(", {name}: ")]
            .iter()
            .any(|prefix| {
                [format!("{prefix}{val}"), format!("{prefix}Some({val})")]
                    .iter()
                    .any(|p| debug.contains(&format!("{p},")) || debug.contains(&format!("{p} }}")))
            });
        assert!(hit, "{ctx}: field {name} should be {val}\n{debug}");
    }
}

/// (program IDL, emitter, event name, discriminator, field renames IDL -> ours).
type EventCase<'a> = (
    &'a Value,
    EventSource,
    &'a str,
    [u8; 8],
    &'a [(&'a str, &'a str)],
);

#[test]
fn event_layouts_match_idl_generated_payloads() {
    let pump = idl("pump");
    let amm = idl("pump_amm");
    let buy_rename = [
        ("base_amount_out", "base_amount"),
        ("max_quote_amount_in", "limit_quote_amount"),
        ("quote_amount_in", "quote_amount"),
        ("quote_amount_in_with_lp_fee", "quote_amount_lp_adjusted"),
        ("user_quote_amount_in", "user_quote_amount"),
    ];
    let sell_rename = [
        ("base_amount_in", "base_amount"),
        ("min_quote_amount_out", "limit_quote_amount"),
        ("quote_amount_out", "quote_amount"),
        (
            "quote_amount_out_without_lp_fee",
            "quote_amount_lp_adjusted",
        ),
        ("user_quote_amount_out", "user_quote_amount"),
    ];
    let cases: &[EventCase<'_>] = &[
        (
            &pump,
            EventSource::Pump,
            "CreateEvent",
            c::event::CREATE,
            &[],
        ),
        (&pump, EventSource::Pump, "TradeEvent", c::event::TRADE, &[]),
        (
            &pump,
            EventSource::Pump,
            "CompleteEvent",
            c::event::COMPLETE,
            &[],
        ),
        (
            &pump,
            EventSource::Pump,
            "CompletePumpAmmMigrationEvent",
            c::event::COMPLETE_PUMP_AMM_MIGRATION,
            &[],
        ),
        (
            &pump,
            EventSource::Pump,
            "PostCompleteBuyEvent",
            c::event::POST_COMPLETE_BUY,
            &[],
        ),
        (
            &amm,
            EventSource::PumpSwap,
            "BuyEvent",
            c::event::AMM_BUY,
            &buy_rename,
        ),
        (
            &amm,
            EventSource::PumpSwap,
            "SellEvent",
            c::event::AMM_SELL,
            &sell_rename,
        ),
        (
            &amm,
            EventSource::PumpSwap,
            "CreatePoolEvent",
            c::event::AMM_CREATE_POOL,
            &[],
        ),
    ];
    for (prog, src, name, disc, rename) in cases {
        let (payload, leaves) = generate(prog, name, *disc);
        let ev = decode_event_payload(*src, &payload).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(ev.name(), *name);
        let debug = match &ev {
            PumpEvent::Create(e) => format!("{e:?}"),
            PumpEvent::Trade(e) => {
                // shareholders: 2 generated (address, share_bps) entries
                let sh: Vec<(Pubkey, u16)> = e.shareholders.unwrap().iter().collect();
                assert_eq!(sh.len(), 2, "{name}");
                let gen_sh: Vec<String> = leaves
                    .iter()
                    .filter(|(n, _)| n == "address" || n == "share_bps")
                    .map(|(_, v)| v.clone())
                    .collect();
                let ours: Vec<String> = sh
                    .iter()
                    .flat_map(|(k, b)| [format!("{k:?}"), b.to_string()])
                    .collect();
                assert_eq!(ours, gen_sh, "{name}: shareholders");
                format!("{e:?}")
            }
            PumpEvent::Complete(e) => format!("{e:?}"),
            PumpEvent::CompletePumpAmmMigration(e) => format!("{e:?}"),
            PumpEvent::PostCompleteBuy(e) => format!("{e:?}"),
            PumpEvent::AmmBuy(e) | PumpEvent::AmmSell(e) => format!("{e:?}"),
            PumpEvent::AmmCreatePool(e) => format!("{e:?}"),
            PumpEvent::Other { .. } => panic!("{name}: undecoded"),
        };
        let leaves: Vec<_> = leaves
            .into_iter()
            .filter(|(n, _)| n != "address" && n != "share_bps")
            .collect();
        assert_leaves(name, &debug, &leaves, rename);
        // every IDL field was generated and checked
        let n_fields = find(&prog["types"], name).unwrap()["type"]["fields"]
            .as_array()
            .unwrap()
            .len();
        let vecs = (*name == "TradeEvent") as usize;
        assert_eq!(leaves.len() + vecs, n_fields, "{name}");
    }
}

#[test]
fn account_layouts_match_idl_generated_data() {
    let pump = idl("pump");
    let (data, leaves) = generate(&pump, "Global", c::account::GLOBAL);
    let g = Global::decode(&data).unwrap();
    assert_leaves("Global", &format!("{g:?}"), &leaves, &[]);
    let (data, leaves) = generate(&pump, "BondingCurve", c::account::BONDING_CURVE);
    let bc = BondingCurve::decode(&data).unwrap();
    assert_eq!(bc.decoded_fields, BondingCurve::FIELDS);
    assert_leaves("BondingCurve", &format!("{bc:?}"), &leaves, &[]);
    let (data, leaves) = generate(&pump, "FeeConfig", c::account::FEE_CONFIG);
    let fc = FeeConfig::decode(&data).unwrap();
    assert_eq!((fc.fee_tiers.len(), fc.stable_fee_tiers.len()), (2, 2));
    assert_leaves("FeeConfig", &format!("{fc:?}"), &leaves, &[]);
    // Both programs' FeeConfig types are the same IDL type.
    let amm = idl("pump_amm");
    assert_eq!(
        find(&pump["types"], "FeeConfig").unwrap()["type"],
        find(&amm["types"], "FeeConfig").unwrap()["type"]
    );
}
