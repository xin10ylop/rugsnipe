//! Minimal fixed-buffer HTTP/1.1 client over a warm `TcpStream`.
//!
//! Plain `http://` only (the same-building sender endpoints are plain HTTP;
//! TLS endpoints go through the `rpc` feature). One connection per sender,
//! kept alive; the request and response buffers are allocated once and
//! reused, so a send after warm-up does not allocate. A broken keep-alive
//! connection is re-opened and the request retried once (re-sending the
//! same signed transaction is harmless: the chain de-duplicates by signature).

use super::SendError;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// A parsed `http://host[:port]/path?query` URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpTarget {
    pub host: String,
    pub port: u16,
    /// Path and query (starts with `/`).
    pub path: String,
}

impl HttpTarget {
    pub fn parse(url: &str) -> Result<Self, String> {
        let rest = url.strip_prefix("http://").ok_or_else(|| {
            format!("{url:?}: only http:// is supported by the fixed-buffer client")
        })?;
        let (hostport, path) = match rest.find(['/', '?']) {
            Some(i) if rest.as_bytes()[i] == b'/' => (&rest[..i], rest[i..].to_string()),
            Some(i) => (&rest[..i], format!("/{}", &rest[i..])),
            None => (rest, "/".to_string()),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) => (
                h,
                p.parse::<u16>()
                    .map_err(|_| format!("bad port in {url:?}"))?,
            ),
            None => (hostport, 80),
        };
        if host.is_empty() || host.contains(['@', ' ']) {
            return Err(format!("bad host in {url:?}"));
        }
        Ok(Self {
            host: host.to_string(),
            port,
            path,
        })
    }

    /// Same target with extra query parameters appended.
    pub fn with_query(&self, extra: &str) -> Self {
        let sep = if self.path.contains('?') { '&' } else { '?' };
        Self {
            path: format!("{}{}{}", self.path, sep, extra),
            ..self.clone()
        }
    }

    /// Same host with another path.
    pub fn with_path(&self, path: &str) -> Self {
        Self {
            path: path.to_string(),
            ..self.clone()
        }
    }
}

/// A warm keep-alive connection with reusable buffers.
#[derive(Debug)]
pub struct HttpConn {
    target: HttpTarget,
    stream: Option<TcpStream>,
    timeout: Duration,
    req: Vec<u8>,
    resp: Box<[u8; 16 * 1024]>,
    /// Bytes of the last response body kept in `resp` (status line onwards).
    last_len: usize,
    last_body: (usize, usize),
}

impl HttpConn {
    pub fn new(target: HttpTarget, timeout: Duration) -> Self {
        Self {
            target,
            stream: None,
            timeout,
            req: Vec::with_capacity(8 * 1024),
            resp: Box::new([0u8; 16 * 1024]),
            last_len: 0,
            last_body: (0, 0),
        }
    }

    pub fn target(&self) -> &HttpTarget {
        &self.target
    }

    fn connect(&mut self) -> Result<(), SendError> {
        let addr = (self.target.host.as_str(), self.target.port)
            .to_socket_addrs()
            .map_err(|e| SendError::Io(e.to_string()))?
            .next()
            .ok_or_else(|| SendError::Io("no address".into()))?;
        let s = TcpStream::connect_timeout(&addr, self.timeout)
            .map_err(|e| SendError::Io(e.to_string()))?;
        let _ = s.set_nodelay(true);
        let _ = s.set_read_timeout(Some(self.timeout));
        let _ = s.set_write_timeout(Some(self.timeout));
        self.stream = Some(s);
        Ok(())
    }

    /// Open the connection now (warm-up) if it is not open.
    pub fn warm(&mut self) -> Result<(), SendError> {
        if self.stream.is_none() {
            self.connect()?;
        }
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// `POST path` with `body`; returns the HTTP status. The response body
    /// is available from [`HttpConn::last_body`] until the next request.
    pub fn post(
        &mut self,
        path: &str,
        content_type: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<u16, SendError> {
        self.req.clear();
        let w = &mut self.req;
        w.extend_from_slice(b"POST ");
        w.extend_from_slice(path.as_bytes());
        w.extend_from_slice(b" HTTP/1.1\r\nHost: ");
        w.extend_from_slice(self.target.host.as_bytes());
        w.extend_from_slice(b"\r\nContent-Type: ");
        w.extend_from_slice(content_type.as_bytes());
        w.extend_from_slice(b"\r\nContent-Length: ");
        let mut num = [0u8; 20];
        w.extend_from_slice(fmt_usize(body.len(), &mut num));
        w.extend_from_slice(b"\r\nConnection: keep-alive\r\n");
        for (k, v) in headers {
            w.extend_from_slice(k.as_bytes());
            w.extend_from_slice(b": ");
            w.extend_from_slice(v.as_bytes());
            w.extend_from_slice(b"\r\n");
        }
        w.extend_from_slice(b"\r\n");
        w.extend_from_slice(body);
        let reused = self.stream.is_some();
        match self.round_trip() {
            Ok(s) => Ok(s),
            Err(_) if reused => {
                self.stream = None;
                self.round_trip()
            }
            Err(e) => Err(e),
        }
    }

    fn round_trip(&mut self) -> Result<u16, SendError> {
        if self.stream.is_none() {
            self.connect()?;
        }
        let r = self.exchange();
        if r.is_err() {
            self.stream = None;
        }
        r
    }

    fn exchange(&mut self) -> Result<u16, SendError> {
        let io = |e: std::io::Error| SendError::Io(e.to_string());
        let s = self.stream.as_mut().unwrap();
        s.write_all(&self.req).map_err(io)?;
        // headers
        let mut n = 0usize;
        let head_end = loop {
            if n == self.resp.len() {
                return Err(SendError::Io("response headers too large".into()));
            }
            let k = s.read(&mut self.resp[n..]).map_err(io)?;
            if k == 0 {
                return Err(SendError::Io("connection closed".into()));
            }
            n += k;
            if let Some(i) = find(&self.resp[..n], b"\r\n\r\n") {
                break i + 4;
            }
        };
        let head = &self.resp[..head_end];
        let status = parse_status(head).ok_or_else(|| SendError::Io("bad status line".into()))?;
        let lower_has = |name: &str| header_value(head, name);
        let close = lower_has("connection").is_some_and(|v| v.eq_ignore_ascii_case(b"close"));
        let chunked =
            lower_has("transfer-encoding").is_some_and(|v| v.eq_ignore_ascii_case(b"chunked"));
        let clen = lower_has("content-length")
            .and_then(|v| core::str::from_utf8(v).ok()?.trim().parse::<usize>().ok());
        let mut total = n;
        let mut sink = [0u8; 1024];
        match (clen, chunked) {
            (Some(len), _) => {
                let want = head_end + len;
                while total < want {
                    let buf: &mut [u8] = if total < self.resp.len() {
                        &mut self.resp[total..]
                    } else {
                        &mut sink
                    };
                    let k = s.read(buf).map_err(io)?;
                    if k == 0 {
                        return Err(SendError::Io("body truncated".into()));
                    }
                    total += k;
                }
                self.last_body = (head_end, want.min(self.resp.len()));
            }
            (None, true) => {
                while find(
                    &self.resp[head_end..total.min(self.resp.len())],
                    b"0\r\n\r\n",
                )
                .is_none()
                {
                    let buf: &mut [u8] = if total < self.resp.len() {
                        &mut self.resp[total..]
                    } else {
                        &mut sink
                    };
                    let k = s.read(buf).map_err(io)?;
                    if k == 0 {
                        break;
                    }
                    total += k;
                }
                self.last_body = (head_end, total.min(self.resp.len()));
            }
            (None, false) => {
                // read to EOF, then the connection is gone
                loop {
                    let buf: &mut [u8] = if total < self.resp.len() {
                        &mut self.resp[total..]
                    } else {
                        &mut sink
                    };
                    let k = s.read(buf).map_err(io)?;
                    if k == 0 {
                        break;
                    }
                    total += k;
                }
                self.last_body = (head_end, total.min(self.resp.len()));
                self.stream = None;
            }
        }
        self.last_len = total;
        if close {
            self.stream = None;
        }
        Ok(status)
    }

    /// Body of the last response (raw; chunk framing kept if chunked).
    pub fn last_body(&self) -> &[u8] {
        &self.resp[self.last_body.0..self.last_body.1]
    }
}

fn fmt_usize(mut v: usize, buf: &mut [u8; 20]) -> &[u8] {
    let mut i = buf.len();
    if v == 0 {
        i -= 1;
        buf[i] = b'0';
    }
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    &buf[i..]
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

fn parse_status(head: &[u8]) -> Option<u16> {
    let line_end = find(head, b"\r\n")?;
    let line = core::str::from_utf8(&head[..line_end]).ok()?;
    let mut it = line.split(' ');
    let v = it.next()?;
    if !v.starts_with("HTTP/1.") {
        return None;
    }
    it.next()?.parse().ok()
}

/// Value of header `name` (case-insensitive) in a raw header block.
fn header_value<'a>(head: &'a [u8], name: &str) -> Option<&'a [u8]> {
    for line in head.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some(colon) = line.iter().position(|&b| b == b':') else {
            continue;
        };
        if line[..colon].eq_ignore_ascii_case(name.as_bytes()) {
            let v = &line[colon + 1..];
            let start = v
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .unwrap_or(v.len());
            return Some(&v[start..]);
        }
    }
    None
}

/// Write `{"jsonrpc":"2.0","id":1,"method":"<m>","params":["<base64 tx>",<opts>]}`
/// into `out` without intermediate allocations (`out` keeps its capacity).
pub fn json_rpc_send_body(out: &mut Vec<u8>, method: &str, tx: &[u8], opts_json: &str) {
    use base64::Engine as _;
    out.clear();
    out.extend_from_slice(br#"{"jsonrpc":"2.0","id":1,"method":""#);
    out.extend_from_slice(method.as_bytes());
    out.extend_from_slice(br#"","params":[""#);
    let start = out.len();
    let enc = base64::encoded_len(tx.len(), true).unwrap_or(0);
    out.resize(start + enc, 0);
    let n = base64::engine::general_purpose::STANDARD
        .encode_slice(tx, &mut out[start..])
        .unwrap_or(0);
    out.truncate(start + n);
    out.extend_from_slice(b"\",");
    out.extend_from_slice(opts_json.as_bytes());
    out.extend_from_slice(b"]}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_urls() {
        let t = HttpTarget::parse("http://fra-sender.helius-rpc.com/fast").unwrap();
        assert_eq!(
            (t.host.as_str(), t.port, t.path.as_str()),
            ("fra-sender.helius-rpc.com", 80, "/fast")
        );
        let t = HttpTarget::parse("http://127.0.0.1:8080?c=k").unwrap();
        assert_eq!((t.port, t.path.as_str()), (8080, "/?c=k"));
        assert_eq!(t.with_query("x=1").path, "/?c=k&x=1");
        assert!(HttpTarget::parse("https://x").is_err());
        assert!(HttpTarget::parse("http://:80/").is_err());
    }

    #[test]
    fn helpers() {
        let mut b = [0u8; 20];
        assert_eq!(fmt_usize(0, &mut b), b"0");
        assert_eq!(fmt_usize(1234, &mut b), b"1234");
        let head = b"HTTP/1.1 429 Too Many\r\nContent-Length: 5\r\nconnection: Close\r\n\r\n";
        assert_eq!(parse_status(head), Some(429));
        assert_eq!(header_value(head, "content-length"), Some(&b"5"[..]));
        assert_eq!(header_value(head, "Connection"), Some(&b"Close"[..]));
        let mut out = Vec::new();
        json_rpc_send_body(
            &mut out,
            "sendTransaction",
            &[1, 2, 3],
            r#"{"encoding":"base64"}"#,
        );
        assert_eq!(
            std::str::from_utf8(&out).unwrap(),
            r#"{"jsonrpc":"2.0","id":1,"method":"sendTransaction","params":["AQID",{"encoding":"base64"}]}"#
        );
    }
}
