//! Third-party launcher programs that create pump coins by CPI.
//!
//! A create that arrives inside another program's instruction has no
//! top-level `create`/`create_v2`: the wire bytes alone do not reveal the
//! mint or the arguments (each launcher has its own layout), so
//! [`crate::create_hit`] needs the transaction's inner instructions (a
//! post-execution feed or `getTransaction`) or the logs path
//! ([`crate::create_hit::from_logs`]).
//!
//! The table holds the launcher programs counted as the first non-infra
//! top-level program of SOL creates in `data/analytics.db` (`creates.launcher`,
//! 2026-10-14; counts out of ~2,100 decoded creates) and in
//! `docs/research/onchain-firstslot-stats.md`. Identities are unknown
//! (unlabelled programs); the table only names them. A launcher is **never
//! a match key** (docs/06 section 1, ruling 5): it only explains why
//! `creator_arg` can differ from the signer, and an unknown one is reported
//! as [`Launcher::Unknown`] so it can be added here after review.

use crate::constants as c;
use solana_pubkey::Pubkey;

/// One known launcher program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownLauncher {
    pub program_id: Pubkey,
    /// Short label used in logs and the analytics DB.
    pub name: &'static str,
    /// Creates seen in `analytics.db` on 2026-10-14 (provenance only).
    pub seen: u32,
}

/// Launcher programs seen in the fixtures and the research data.
pub const KNOWN_LAUNCHERS: &[KnownLauncher] = &[
    KnownLauncher {
        program_id: Pubkey::from_str_const("J7pourVwqP1VtRNBiFNqbdGxwkx7ta2LHAURURRRJqmd"),
        name: "J7pour",
        seen: 58,
    },
    KnownLauncher {
        program_id: Pubkey::from_str_const("6Vo3245eszAb5wuqEMw8mGdbfRUdKbHhDHP5LcaGuTAB"),
        name: "6Vo3245",
        seen: 49,
    },
    KnownLauncher {
        program_id: Pubkey::from_str_const("rapidV5DAKtecYzetwCNoxXWHpezjeg4hGt9BGiVcGW"),
        name: "rapidV",
        seen: 8,
    },
    KnownLauncher {
        program_id: Pubkey::from_str_const("FAdo9NCw1ssek6Z6yeWzWjhLVsr8uiCwcWNUnKgzTnHe"),
        name: "FAdo9NC",
        seen: 8,
    },
    KnownLauncher {
        program_id: Pubkey::from_str_const("6CYGWqReXth5n7q5Y7yKFXpeMjt7Qyjibq6tMrNgnSF3"),
        name: "6CYGWq",
        seen: 5,
    },
    KnownLauncher {
        program_id: Pubkey::from_str_const("47db2zQxySA2fRDrnFKi8cvZRqkZTUUid6Lg4ktCZ1jU"),
        name: "47db2z",
        seen: 3,
    },
];

/// How a create reached the pump program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Launcher {
    /// Top-level `create` / `create_v2` (no launcher).
    #[default]
    Direct,
    /// CPI from a program in [`KNOWN_LAUNCHERS`] (index into the table).
    Known(u8),
    /// CPI from a program not in the table (reported, never matched).
    Unknown(Pubkey),
}

impl Launcher {
    /// Classify the top-level program that CPI'd into pump's create.
    pub fn from_program(program_id: &Pubkey) -> Self {
        match KNOWN_LAUNCHERS
            .iter()
            .position(|l| l.program_id == *program_id)
        {
            Some(i) => Self::Known(i as u8),
            None => Self::Unknown(*program_id),
        }
    }

    /// The launcher program id (`None` for a direct create).
    pub fn program_id(&self) -> Option<Pubkey> {
        match self {
            Self::Direct => None,
            Self::Known(i) => KNOWN_LAUNCHERS.get(*i as usize).map(|l| l.program_id),
            Self::Unknown(p) => Some(*p),
        }
    }

    /// Short label: `"direct"`, the table name, or `"unknown"`.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Known(i) => KNOWN_LAUNCHERS
                .get(*i as usize)
                .map(|l| l.name)
                .unwrap_or("unknown"),
            Self::Unknown(_) => "unknown",
        }
    }

    pub fn is_direct(&self) -> bool {
        matches!(self, Self::Direct)
    }
}

/// Infrastructure programs that are never "launchers" (they appear at top
/// level next to a create without wrapping it): mirrors
/// `analysis/rugsnipe_analysis/store.py::INFRA_PROGRAMS`.
pub fn is_infra_program(program_id: &Pubkey) -> bool {
    const MEMO: Pubkey = Pubkey::from_str_const("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
    const MEMO_V1: Pubkey = Pubkey::from_str_const("Memo1UhkJRfHyvLMcVucJwxXeuD728EqVDDwQDxFMNo");
    [
        c::PUMP_PROGRAM_ID,
        c::SYSTEM_PROGRAM_ID,
        c::COMPUTE_BUDGET_PROGRAM_ID,
        c::SPL_TOKEN_PROGRAM_ID,
        c::TOKEN_2022_PROGRAM_ID,
        c::ASSOCIATED_TOKEN_PROGRAM_ID,
        c::PUMP_FEES_PROGRAM_ID,
        MEMO,
        MEMO_V1,
    ]
    .contains(program_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_unique_and_not_infra() {
        for (i, a) in KNOWN_LAUNCHERS.iter().enumerate() {
            assert!(!is_infra_program(&a.program_id));
            assert_ne!(a.program_id, c::PUMP_SWAP_PROGRAM_ID);
            for b in &KNOWN_LAUNCHERS[i + 1..] {
                assert_ne!(a.program_id, b.program_id);
            }
            assert_eq!(
                Launcher::from_program(&a.program_id),
                Launcher::Known(i as u8)
            );
            assert_eq!(Launcher::Known(i as u8).program_id(), Some(a.program_id));
        }
    }

    #[test]
    fn unknown_is_reported() {
        let p = Pubkey::new_unique();
        let l = Launcher::from_program(&p);
        assert_eq!(l, Launcher::Unknown(p));
        assert_eq!(l.name(), "unknown");
        assert_eq!(l.program_id(), Some(p));
        assert!(Launcher::Direct.is_direct());
        assert_eq!(Launcher::Direct.program_id(), None);
    }
}
