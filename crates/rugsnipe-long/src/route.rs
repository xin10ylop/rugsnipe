//! Our route for a trigger. The anchors are what we pay with: wrapped SOL
//! (wrapped and unwrapped inside the transaction) and the floats we hold.
//!
//! * Its own `multi_hop_swap` route is copied when it starts from an
//!   anchor; otherwise a pool hop from an anchor to its input is put in
//!   front (`canonical_pump_swap_pool(its input, anchor)`), within
//!   [`MAX_OUR_HOPS`].
//! * A direct curve buy (no route: it paid the curve's quote, often after a
//!   Jupiter leg) becomes the curve hop, from the quote when it is an
//!   anchor, else behind the quote's pool against an anchor.

use crate::detect::Trigger;
use pump_codec::constants as c;
use pump_codec::multihop::{Hop, Route};
use pump_codec::{pda, Pubkey};

/// Longest route we send: 3 hops is about 1,270 bytes with the wrap,
/// budget, ATA and tip instructions, over the 1,232-byte limit (docs/20).
pub const MAX_OUR_HOPS: usize = 2;

/// Chain facts the planner needs (cached by the caller).
pub trait Chain {
    /// `true` when a PumpSwap pool account exists at `pool`.
    fn pool_exists(&mut self, pool: &Pubkey) -> bool;
    /// The token program owning `mint`.
    fn mint_program(&mut self, mint: &Pubkey) -> Option<Pubkey>;
}

/// A route to buy along.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub route: Route,
    /// What we pay with (the route's input).
    pub anchor: Pubkey,
    pub anchor_program: Pubkey,
    /// `copied`, `copied+pool`, `curve` or `pool+curve`.
    pub how: &'static str,
}

/// Why there is no route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoRoute {
    /// Its route is longer than we send.
    TooLong(usize),
    /// Nothing links an anchor to this mint.
    NoAnchor(Pubkey),
    /// A token program is unknown.
    Program(Pubkey),
}

impl core::fmt::Display for NoRoute {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::TooLong(n) => write!(f, "route of {n} hops"),
            Self::NoAnchor(m) => write!(f, "no anchor reaches {}", &m.to_string()[..8]),
            Self::Program(m) => write!(f, "token program of {} unknown", &m.to_string()[..8]),
        }
    }
}

fn program_of(chain: &mut dyn Chain, m: &Pubkey) -> Result<Pubkey, NoRoute> {
    if *m == c::WSOL_MINT {
        return Ok(c::SPL_TOKEN_PROGRAM_ID);
    }
    chain.mint_program(m).ok_or(NoRoute::Program(*m))
}

/// `anchors`: wrapped SOL and the floats, in preference order.
pub fn plan(t: &Trigger, anchors: &[Pubkey], chain: &mut dyn Chain) -> Result<Plan, NoRoute> {
    let is_anchor = |m: &Pubkey| anchors.contains(m);
    // the pool hop from an anchor to `to` (its base), first one that exists
    let front = |to: &Pubkey,
                 to_program: &Pubkey,
                 chain: &mut dyn Chain|
     -> Result<(Pubkey, Hop), NoRoute> {
        for a in anchors {
            let pool = pda::canonical_pump_swap_pool(to, a);
            if chain.pool_exists(&pool) {
                let ap = program_of(chain, a)?;
                return Ok((*a, Hop::pool(&pool, to, to_program, a, &ap)));
            }
        }
        Err(NoRoute::NoAnchor(*to))
    };
    match &t.route {
        Some(r) if r.hops.len() > MAX_OUR_HOPS => Err(NoRoute::TooLong(r.hops.len())),
        Some(r) if is_anchor(&r.input) => Ok(Plan {
            route: r.clone(),
            anchor: r.input,
            anchor_program: program_of(chain, &r.input)?,
            how: "copied",
        }),
        Some(r) => {
            if r.hops.len() + 1 > MAX_OUR_HOPS {
                return Err(NoRoute::TooLong(r.hops.len() + 1));
            }
            let ip = program_of(chain, &r.input)?;
            let (a, hop) = front(&r.input, &ip, chain)?;
            let mut hops = vec![hop];
            hops.extend(r.hops.iter().copied());
            Ok(Plan {
                route: Route::new(a, hops).map_err(|_| NoRoute::NoAnchor(r.input))?,
                anchor: a,
                anchor_program: program_of(chain, &a)?,
                how: "copied+pool",
            })
        }
        None => {
            let curve = Hop::curve(&t.mint, &t.coin_program, &t.quote_mint, &t.quote_program);
            if is_anchor(&t.quote_mint) {
                return Ok(Plan {
                    route: Route::new(t.quote_mint, vec![curve])
                        .map_err(|_| NoRoute::NoAnchor(t.mint))?,
                    anchor: t.quote_mint,
                    anchor_program: t.quote_program,
                    how: "curve",
                });
            }
            let (a, hop) = front(&t.quote_mint, &t.quote_program, chain)?;
            Ok(Plan {
                route: Route::new(a, vec![hop, curve])
                    .map_err(|_| NoRoute::NoAnchor(t.quote_mint))?,
                anchor: a,
                anchor_program: program_of(chain, &a)?,
                how: "pool+curve",
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct Fake {
        pools: HashSet<Pubkey>,
    }
    impl Chain for Fake {
        fn pool_exists(&mut self, pool: &Pubkey) -> bool {
            self.pools.contains(pool)
        }
        fn mint_program(&mut self, _: &Pubkey) -> Option<Pubkey> {
            Some(c::TOKEN_2022_PROGRAM_ID)
        }
    }
    fn k(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }
    fn trig(route: Option<Route>, quote: Pubkey) -> Trigger {
        Trigger {
            sig: String::new(),
            slot: 1,
            mint: k(7),
            quote_mint: quote,
            coin_program: c::TOKEN_2022_PROGRAM_ID,
            quote_program: c::TOKEN_2022_PROGRAM_ID,
            its_tokens: 1,
            its_quote_in: 1,
            vt_post: 1,
            vq_post: 1,
            real_token_post: None,
            input_mint: route.as_ref().map_or(quote, |r| r.input),
            input_amount: 1,
            sol_value: Some(1_500_000_000),
            route,
            via_jupiter: false,
        }
    }

    #[test]
    fn its_route_from_an_anchor_is_copied() {
        let q = k(5);
        let pool = Hop::pool(
            &k(9),
            &q,
            &c::TOKEN_2022_PROGRAM_ID,
            &c::WSOL_MINT,
            &c::SPL_TOKEN_PROGRAM_ID,
        );
        let curve = Hop::curve(
            &k(7),
            &c::TOKEN_2022_PROGRAM_ID,
            &q,
            &c::TOKEN_2022_PROGRAM_ID,
        );
        let r = Route::new(c::WSOL_MINT, vec![pool, curve]).unwrap();
        let mut f = Fake {
            pools: HashSet::new(),
        };
        let p = plan(&trig(Some(r.clone()), q), &[c::WSOL_MINT], &mut f).unwrap();
        assert_eq!(p.route, r);
        assert_eq!(p.how, "copied");
        assert_eq!(p.anchor_program, c::SPL_TOKEN_PROGRAM_ID);
        // from a float we do not hold, and no pool to put in front within 2 hops
        let wbtc = k(3);
        let pool_w = Hop::pool(
            &k(8),
            &q,
            &c::TOKEN_2022_PROGRAM_ID,
            &wbtc,
            &c::SPL_TOKEN_PROGRAM_ID,
        );
        let rw = Route::new(wbtc, vec![pool_w, curve]).unwrap();
        assert_eq!(
            plan(&trig(Some(rw.clone()), q), &[c::WSOL_MINT], &mut f),
            Err(NoRoute::TooLong(3))
        );
        // held as a float: copied as is
        assert_eq!(
            plan(&trig(Some(rw.clone()), q), &[c::WSOL_MINT, wbtc], &mut f)
                .unwrap()
                .how,
            "copied"
        );
    }

    #[test]
    fn a_direct_buy_goes_behind_the_quotes_pool() {
        let q = k(5);
        let mut f = Fake {
            pools: [pda::canonical_pump_swap_pool(&q, &c::WSOL_MINT)]
                .into_iter()
                .collect(),
        };
        let p = plan(&trig(None, q), &[c::WSOL_MINT], &mut f).unwrap();
        assert_eq!(p.how, "pool+curve");
        assert_eq!(p.route.mints(), vec![c::WSOL_MINT, q, k(7)]);
        assert_eq!(p.route.buys(), vec![true, true]);
        // the quote held as a float: the curve alone
        let p = plan(&trig(None, q), &[c::WSOL_MINT, q], &mut f).unwrap();
        assert_eq!(p.how, "curve");
        assert_eq!(p.route.hops.len(), 1);
        // no pool: no route
        let mut f = Fake {
            pools: HashSet::new(),
        };
        assert_eq!(
            plan(&trig(None, q), &[c::WSOL_MINT], &mut f),
            Err(NoRoute::NoAnchor(q))
        );
    }
}
