//! Operating mode. `Paper` is the default and the only mode that works
//! without keypairs: it decides and records but never sends.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Decide and record; never touch a send path. Default.
    #[default]
    Paper,
    /// Small real buys (`max_probe_lamports`, `max_probes_per_day`) to measure landing.
    Probe,
    /// Real buys under the full risk caps; needs a gate-passed ArmSet.
    Live,
}

impl Mode {
    /// `true` for modes that may spend money (probe, live).
    pub fn is_real(self) -> bool {
        !matches!(self, Self::Paper)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Probe => "probe",
            Self::Live => "live",
        }
    }
    pub fn as_u8(self) -> u8 {
        self as u8
    }
}

impl core::str::FromStr for Mode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "paper" => Ok(Self::Paper),
            "probe" => Ok(Self::Probe),
            "live" => Ok(Self::Live),
            o => Err(format!("unknown mode {o:?} (paper|probe|live)")),
        }
    }
}

impl core::fmt::Display for Mode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_is_paper() {
        assert_eq!(Mode::default(), Mode::Paper);
        assert!(!Mode::Paper.is_real() && Mode::Probe.is_real() && Mode::Live.is_real());
        assert_eq!("live".parse::<Mode>().unwrap(), Mode::Live);
        assert!("LIVE".parse::<Mode>().is_err());
    }
}
