//! `[feeds]` configuration (docs/06 section 6). Safe defaults: LogsWs on,
//! relay off, PumpPortal **off** (one connection per IP, normally the collector's).

use serde::Deserialize;
use std::path::PathBuf;

/// `logsws = { enabled, url, stale_ms }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct LogsWsSection {
    pub enabled: bool,
    pub url: String,
    pub stale_ms: u64,
    /// HTTPS RPC for funding lookups (empty = disabled).
    pub rpc_url: String,
    pub max_watches: usize,
}

impl Default for LogsWsSection {
    fn default() -> Self {
        LogsWsSection {
            enabled: true,
            url: "wss://api.mainnet-beta.solana.com".into(),
            stale_ms: 10_000,
            rpc_url: "https://api.mainnet-beta.solana.com".into(),
            max_watches: 16,
        }
    }
}

/// `relay = { enabled, socket }`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RelaySection {
    pub enabled: bool,
    pub socket: PathBuf,
}

impl Default for RelaySection {
    fn default() -> Self {
        RelaySection {
            enabled: false,
            socket: "/run/rugsnipe/pp.sock".into(),
        }
    }
}

/// `pumpportal = { enabled, url }` (default off).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct PumpPortalSection {
    pub enabled: bool,
    pub url: String,
}

impl Default for PumpPortalSection {
    fn default() -> Self {
        PumpPortalSection {
            enabled: false,
            url: "wss://pumpportal.fun/api/data".into(),
        }
    }
}

/// `record = { enabled, dir, max_file_mb, max_files }` (raw frames for replay).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct RecordSection {
    pub enabled: bool,
    pub dir: PathBuf,
    pub max_file_mb: u64,
    pub max_files: usize,
}

impl Default for RecordSection {
    fn default() -> Self {
        RecordSection {
            enabled: false,
            dir: "data/raw/feed".into(),
            max_file_mb: 64,
            max_files: 8,
        }
    }
}

/// The feed part of the config. Unknown keys (grpc, shreds, probe_live_require)
/// belong to other crates and are ignored here.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct FeedConfig {
    pub logsws: LogsWsSection,
    pub relay: RelaySection,
    pub pumpportal: PumpPortalSection,
    pub record: RecordSection,
}

impl FeedConfig {
    /// Recorder settings when recording is enabled.
    pub fn record_config(&self, prefix: &str) -> Option<crate::record::RecordConfig> {
        if !self.record.enabled {
            return None;
        }
        let mut c = crate::record::RecordConfig::new(&self.record.dir, prefix);
        c.max_file_bytes = self.record.max_file_mb.max(1) << 20;
        c.max_files = self.record.max_files.max(1);
        Some(c)
    }

    /// Build the enabled feeds behind one [`crate::FeedMux`].
    #[cfg(feature = "ws")]
    pub fn build(&self) -> std::io::Result<crate::FeedMux> {
        let mut feeds: Vec<Box<dyn crate::Feed + Send>> = Vec::new();
        if self.logsws.enabled {
            let cfg = crate::ws::LogsWsConfig {
                url: self.logsws.url.clone(),
                stale_ms: self.logsws.stale_ms,
                max_watches: self.logsws.max_watches,
                rpc_url: (!self.logsws.rpc_url.is_empty()).then(|| self.logsws.rpc_url.clone()),
                record: self.record_config("logsws"),
                ..Default::default()
            };
            feeds.push(Box::new(crate::ws::LogsWs::spawn(cfg)?));
        }
        if self.relay.enabled {
            feeds.push(Box::new(crate::relay::CollectorRelay::spawn(
                crate::relay::RelayConfig::new(&self.relay.socket),
            )?));
        }
        if self.pumpportal.enabled {
            let cfg = crate::ws::PumpPortalConfig {
                url: self.pumpportal.url.clone(),
                record: self.record_config("pumpportal"),
                ..Default::default()
            };
            feeds.push(Box::new(crate::ws::PumpPortal::spawn(cfg)?));
        }
        Ok(crate::FeedMux::new(feeds))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_safe() {
        let c = FeedConfig::default();
        assert!(c.logsws.enabled);
        assert!(!c.relay.enabled);
        assert!(
            !c.pumpportal.enabled,
            "PumpPortal must be opt-in (one connection per IP)"
        );
        assert!(!c.record.enabled);
        assert!(c.record_config("x").is_none());
    }

    #[test]
    fn parses_spec_feeds_table() {
        let json = r#"{
            "logsws": {"enabled": true, "url": "wss://api.mainnet-beta.solana.com", "stale_ms": 10000},
            "relay": {"enabled": false, "socket": "/run/rugsnipe/pp.sock"},
            "grpc": {"enabled": false},
            "probe_live_require": "post_exec"
        }"#;
        let c: FeedConfig = serde_json::from_str(json).unwrap();
        assert_eq!(c.logsws.stale_ms, 10_000);
        assert!(!c.pumpportal.enabled);
        let rc = FeedConfig {
            record: RecordSection {
                enabled: true,
                ..Default::default()
            },
            ..Default::default()
        }
        .record_config("logsws")
        .unwrap();
        assert_eq!(rc.max_file_bytes, 64 << 20);
    }
}
