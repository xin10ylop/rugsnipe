//! `Ledger`: the typed write path intent -> send -> fill -> position -> exit
//! (docs/06 sections 5.1, 7.1 step 14, 8). Every call queues one or more
//! [`Write`]s on the store's writer thread; the intent state machine
//! ([`IntentState::next_allowed`]) is enforced in SQL so a late or
//! duplicated event can never move an intent backwards.

use crate::rows::*;
use crate::store::{StateWriter, Write};

#[derive(Debug, Clone)]
pub struct Ledger {
    w: StateWriter,
}

impl Ledger {
    pub fn new(w: StateWriter) -> Self {
        Self { w }
    }

    pub fn writer(&self) -> &StateWriter {
        &self.w
    }

    /// A decision that produced an intent (paper or real). Idempotent per
    /// `(mint, mode)`.
    pub fn intent(&self, r: IntentRow) -> bool {
        self.w.send(Write::Intent(r))
    }

    /// One variant handed to one sender (real intents are inserted as
    /// `sent`; paper intents stay `paper`).
    pub fn sent(&self, r: SendRow) -> bool {
        self.w.send(Write::Send(r))
    }

    /// The buy's outcome on chain (or the paper simulation). Real intents
    /// move to `landed` / `failed` / `late_fill`; paper intents stay `paper`.
    pub fn fill(&self, r: FillRow, real_outcome: Option<IntentState>) -> bool {
        let id = r.intent_id;
        let mut ok = self.w.send(Write::Fill(r));
        if let Some(to) = real_outcome {
            ok &= self.w.send(Write::IntentState { intent_id: id, to });
        }
        ok
    }

    /// The nonce burn won (real modes).
    pub fn burned(&self, intent_id: i64) -> bool {
        self.w.send(Write::IntentState {
            intent_id,
            to: IntentState::Burned,
        })
    }

    /// Open / update / close a position (upsert by mint).
    pub fn position(&self, r: PositionRow) -> bool {
        self.w.send(Write::Position(r))
    }

    /// A sell fill.
    pub fn exit(&self, r: ExitRow) -> bool {
        self.w.send(Write::Exit(r))
    }

    pub fn nonce(&self, r: NonceRow) -> bool {
        self.w.send(Write::Nonce(r))
    }

    pub fn kv(&self, k: &str, v: impl Into<String>) -> bool {
        self.w.send(Write::Kv(k.to_string(), v.into()))
    }

    pub fn flush(&self) -> bool {
        self.w.flush()
    }
}
