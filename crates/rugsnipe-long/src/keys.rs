//! Secrets. Under systemd they come from the unit's credentials
//! (`$CREDENTIALS_DIRECTORY/api-keys`, `.../long-wallet`, as the hot unit
//! loads its own); from a shell, from the paths in `long.toml`. A
//! `cred:NAME` URL is a line of the api-keys file, so a key never sits in a
//! config, a unit or a command line.

use rugsnipe_tx::senders::ApiKeys;
use rugsnipe_tx::signer::HotSigner;
use std::path::PathBuf;
use zeroize::Zeroizing;

fn cred(name: &str) -> Option<PathBuf> {
    let d = std::env::var_os("CREDENTIALS_DIRECTORY")?;
    let p = PathBuf::from(d).join(name);
    p.exists().then_some(p)
}

/// The api-keys file (`NAME = "value"` lines).
pub fn api_keys(fallback: &str) -> Result<ApiKeys, String> {
    let p = cred("api-keys").unwrap_or_else(|| PathBuf::from(fallback));
    let text =
        Zeroizing::new(std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?);
    if text.trim() == "ABSENT" {
        return Ok(ApiKeys::default());
    }
    ApiKeys::parse(&text)
}

/// The long wallet.
pub fn wallet(fallback: &str) -> Result<HotSigner, String> {
    let p = cred("long-wallet").unwrap_or_else(|| PathBuf::from(fallback));
    HotSigner::load_file(&p).map_err(|e| format!("{}: {e}", p.display()))
}

/// A URL that may be `cred:NAME`.
pub fn resolve_url(keys: &ApiKeys, url: &str) -> Result<String, String> {
    match url.strip_prefix("cred:").map(str::trim) {
        None => Ok(url.to_string()),
        Some(name) => keys
            .get(name)
            .map(str::to_string)
            .ok_or_else(|| format!("{url}: {name:?} is not in the api-keys file")),
    }
}

/// `https://host/...?...` with its path and query hidden.
pub fn redact(url: &str) -> String {
    match url.split_once("://") {
        Some((s, r)) => format!("{s}://{}/...", r.split(['/', '?']).next().unwrap_or("")),
        None => "<url>".into(),
    }
}
