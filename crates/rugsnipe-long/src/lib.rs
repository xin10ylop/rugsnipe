//! rugsnipe-long: copy H9kuqGKN's big buys of pump coins priced in other
//! tokens and hold them up to 30 minutes (docs/20).
//!
//! The research (`analysis/research/2026-10-27/h9long/`): it sells
//! everything at 2.5x the price its buy left, at 0.7x, or after 30 minutes;
//! its 1.5-SOL buys of coins priced in other tokens copied at 1 SOL, sold at
//! 2.0x (before its own sale) / 0.8x / 1790 s, made +15.3 SOL out of sample
//! over Oct 23-27 (+12.2 without the 2 best coins), with its sale placed
//! ahead of ours whenever within a slot.
//!
//! A process of its own with its own wallet: its positions last minutes,
//! it holds floats (WBTC), and the hot unit's wallet checks stay exact.

pub mod chain;
pub mod config;
pub mod detect;
pub mod engine;
pub mod feed;
pub mod keys;
pub mod oracle;
pub mod pos;
pub mod route;
pub mod send;
pub mod txb;
pub mod txlite;
