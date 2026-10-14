//! TCP/TLS/websocket plumbing for the adapter threads: URL parsing, HTTP
//! `CONNECT` through `HTTPS_PROXY` (honouring `NO_PROXY`), rustls with the
//! platform roots (which include `SSL_CERT_FILE`), and the websocket handshake.

use std::io;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio_rustls::rustls;
use tokio_tungstenite::WebSocketStream;

/// A parsed `ws://`, `wss://`, `http://` or `https://` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub tls: bool,
    pub host: String,
    pub port: u16,
    /// Path and query (`/` if empty).
    pub path: String,
}

impl Url {
    pub fn parse(u: &str) -> io::Result<Url> {
        let bad = || io::Error::new(io::ErrorKind::InvalidInput, format!("bad url {u:?}"));
        let (scheme, rest) = u.split_once("://").ok_or_else(bad)?;
        let tls = match scheme {
            "wss" | "https" => true,
            "ws" | "http" => false,
            _ => return Err(bad()),
        };
        let (hostport, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, "/"),
        };
        // (no IPv6 literals: every endpoint here is a hostname or IPv4)
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p.parse::<u16>().map_err(|_| bad())?),
            None => (hostport.to_string(), if tls { 443 } else { 80 }),
        };
        if host.is_empty() {
            return Err(bad());
        }
        Ok(Url {
            tls,
            host,
            port,
            path: path.to_string(),
        })
    }

    /// `scheme://host:port/path` with the websocket scheme.
    pub fn ws_string(&self) -> String {
        format!(
            "{}://{}:{}{}",
            if self.tls { "wss" } else { "ws" },
            self.host,
            self.port,
            self.path
        )
    }
}

fn env_any(names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|n| std::env::var(n).ok().filter(|v| !v.is_empty()))
}

/// `NO_PROXY` match: exact host, `.suffix`/`*.suffix` domain match, or `*`.
pub fn no_proxy_matches(no_proxy: &str, host: &str) -> bool {
    no_proxy.split(',').map(str::trim).any(|p| {
        if p.is_empty() {
            return false;
        }
        if p == "*" {
            return true;
        }
        let p = p.trim_start_matches('*');
        if let Some(suffix) = p.strip_prefix('.') {
            host == suffix || host.ends_with(&format!(".{suffix}"))
        } else {
            host == p
        }
    })
}

/// The proxy to use for `host` (from `HTTPS_PROXY`/`https_proxy`), if any.
pub fn proxy_for(host: &str) -> Option<(String, u16)> {
    let proxy = env_any(&["HTTPS_PROXY", "https_proxy"])?;
    if let Some(np) = env_any(&["NO_PROXY", "no_proxy"]) {
        if no_proxy_matches(&np, host) {
            return None;
        }
    }
    let u = Url::parse(&proxy).ok()?;
    Some((u.host, u.port))
}

/// TCP to `host:port`, through an HTTP `CONNECT` tunnel when a proxy applies.
pub async fn connect_tcp(host: &str, port: u16, timeout: Duration) -> io::Result<TcpStream> {
    let fut = async {
        match proxy_for(host) {
            None => {
                let s = TcpStream::connect((host, port)).await?;
                s.set_nodelay(true)?;
                Ok(s)
            }
            Some((ph, pp)) => {
                let mut s = TcpStream::connect((ph.as_str(), pp)).await?;
                s.set_nodelay(true)?;
                let req = format!(
                    "CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\nUser-Agent: rugsnipe-feed\r\n\r\n"
                );
                s.write_all(req.as_bytes()).await?;
                // read the proxy's reply headers byte by byte (tiny, once per connection)
                let mut head = Vec::with_capacity(256);
                let mut b = [0u8; 1];
                while !head.ends_with(b"\r\n\r\n") {
                    if s.read(&mut b).await? == 0 || head.len() > 16 * 1024 {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "proxy closed during CONNECT",
                        ));
                    }
                    head.push(b[0]);
                }
                let status = String::from_utf8_lossy(&head);
                let line = status.lines().next().unwrap_or("");
                if !line.split_whitespace().nth(1).is_some_and(|c| c == "200") {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        format!("proxy refused CONNECT {host}:{port}: {line}"),
                    ));
                }
                Ok(s)
            }
        }
    };
    tokio::time::timeout(timeout, fut)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "connect timeout"))?
}

/// Shared rustls client config (ring provider, platform roots, ALPN http/1.1).
pub fn tls_config() -> io::Result<Arc<rustls::ClientConfig>> {
    static CFG: OnceLock<Result<Arc<rustls::ClientConfig>, String>> = OnceLock::new();
    CFG.get_or_init(|| {
        let mut roots = rustls::RootCertStore::empty();
        let certs = rustls_native_certs::load_native_certs();
        let (ok, _bad) = roots.add_parsable_certificates(certs.certs);
        if ok == 0 {
            return Err("no usable root certificates".into());
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let mut cfg = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| e.to_string())?
            .with_root_certificates(roots)
            .with_no_client_auth();
        cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(cfg))
    })
    .clone()
    .map_err(io::Error::other)
}

/// Plain or TLS stream.
#[derive(Debug)]
pub enum Stream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
}

impl AsyncRead for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Stream {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, b: &[u8]) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_write(cx, b),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, b),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_flush(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// TCP (+ proxy) (+ TLS) to the URL's host.
pub async fn connect_stream(url: &Url, timeout: Duration) -> io::Result<Stream> {
    let tcp = connect_tcp(&url.host, url.port, timeout).await?;
    if !url.tls {
        return Ok(Stream::Plain(tcp));
    }
    let cfg = tls_config()?;
    let name = rustls::pki_types::ServerName::try_from(url.host.clone())
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    let tls = tokio::time::timeout(
        timeout,
        tokio_rustls::TlsConnector::from(cfg).connect(name, tcp),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "tls timeout"))??;
    Ok(Stream::Tls(Box::new(tls)))
}

/// A websocket client connection.
pub type Ws = WebSocketStream<Stream>;

/// Open a websocket to `url` (proxy and TLS as needed).
pub async fn ws_connect(url: &str, timeout: Duration) -> io::Result<Ws> {
    let u = Url::parse(url)?;
    let stream = connect_stream(&u, timeout).await?;
    let (ws, _resp) = tokio::time::timeout(
        timeout,
        tokio_tungstenite::client_async(u.ws_string(), stream),
    )
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "websocket handshake timeout"))?
    .map_err(|e| io::Error::other(format!("websocket handshake: {e}")))?;
    Ok(ws)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_parsing() {
        let u = Url::parse("wss://api.mainnet-beta.solana.com").unwrap();
        assert_eq!(
            (u.tls, u.host.as_str(), u.port, u.path.as_str()),
            (true, "api.mainnet-beta.solana.com", 443, "/")
        );
        let u = Url::parse("http://127.0.0.1:34483").unwrap();
        assert_eq!((u.tls, u.port), (false, 34483));
        let u = Url::parse("https://x.y/api/data?k=1").unwrap();
        assert_eq!(u.path, "/api/data?k=1");
        assert_eq!(u.ws_string(), "wss://x.y:443/api/data?k=1");
        assert!(Url::parse("ftp://x").is_err());
        assert!(Url::parse("wss://:1").is_err());
    }

    #[test]
    fn no_proxy_rules() {
        let np = "localhost,127.0.0.1,.svc.cluster.local,*.example.com";
        assert!(no_proxy_matches(np, "localhost"));
        assert!(no_proxy_matches(np, "127.0.0.1"));
        assert!(no_proxy_matches(np, "a.svc.cluster.local"));
        assert!(no_proxy_matches(np, "b.example.com"));
        assert!(!no_proxy_matches(np, "api.mainnet-beta.solana.com"));
        assert!(no_proxy_matches("*", "anything"));
    }
}
