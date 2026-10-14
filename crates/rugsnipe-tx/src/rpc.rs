//! Cold-path JSON-RPC client (feature `rpc`): `ureq` 3 + rustls, honours
//! `HTTPS_PROXY` and, when `SSL_CERT_FILE` is set, trusts exactly the
//! certificates in that bundle (otherwise Mozilla's roots). Never used on
//! the hot thread.

use serde_json::{json, Value};
use std::time::Duration;
use ureq::tls::{parse_pem, PemItem, RootCerts, TlsConfig};

/// A JSON-RPC endpoint.
#[derive(Debug, Clone)]
pub struct RpcClient {
    url: String,
    agent: ureq::Agent,
}

fn roots() -> RootCerts {
    if let Ok(path) = std::env::var("SSL_CERT_FILE") {
        if let Ok(pem) = std::fs::read(&path) {
            let certs: Vec<_> = parse_pem(&pem)
                .filter_map(|i| match i {
                    Ok(PemItem::Certificate(c)) => Some(c.to_owned()),
                    _ => None,
                })
                .collect();
            if !certs.is_empty() {
                return RootCerts::new_with_certs(&certs);
            }
        }
    }
    RootCerts::WebPki
}

impl RpcClient {
    pub fn new(url: &str) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(20)))
            .http_status_as_error(false)
            .tls_config(TlsConfig::builder().root_certs(roots()).build())
            .build()
            .into();
        Self {
            url: url.to_string(),
            agent,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    /// POST raw bytes with a content type; returns `(status, body)`.
    pub fn post_raw(
        &self,
        url: &str,
        content_type: &str,
        body: &[u8],
    ) -> Result<(u16, String), String> {
        let mut resp = self
            .agent
            .post(url)
            .header("Content-Type", content_type)
            .send(body)
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        Ok((status, text))
    }

    /// GET a URL as JSON.
    pub fn get_json(&self, url: &str) -> Result<Value, String> {
        let mut resp = self
            .agent
            .get(url)
            .header("Accept", "application/json")
            .call()
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !(200..300).contains(&status) {
            return Err(format!(
                "GET {url}: HTTP {status}: {}",
                text.chars().take(200).collect::<String>()
            ));
        }
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }

    /// Call `method` and return `result` (a JSON-RPC `error` becomes `Err`).
    pub fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        let body =
            json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).to_string();
        let (status, text) = self.post_raw(&self.url, "application/json", body.as_bytes())?;
        let v: Value = serde_json::from_str(&text).map_err(|e| {
            format!(
                "HTTP {status}: {e}: {}",
                text.chars().take(200).collect::<String>()
            )
        })?;
        if let Some(e) = v.get("error") {
            return Err(format!("{method}: {e}"));
        }
        if !(200..300).contains(&status) {
            return Err(format!("{method}: HTTP {status}"));
        }
        v.get("result")
            .cloned()
            .ok_or_else(|| format!("{method}: no result"))
    }
}
