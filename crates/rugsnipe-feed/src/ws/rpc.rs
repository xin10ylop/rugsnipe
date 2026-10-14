//! Minimal HTTP/1.1 JSON-RPC client for the cold path (funding lookups,
//! `getBlockTime` sampling in the tail example). Keep-alive, one request in
//! flight, `Content-Length` and chunked responses, bounded sizes. Not for the
//! hot path (no reqwest/hyper in this crate).

use super::net::{connect_stream, Stream, Url};
use std::io;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

const MAX_BODY: usize = 16 << 20;
const MAX_HEAD: usize = 64 << 10;

/// JSON-RPC over HTTPS with a reused connection.
#[derive(Debug)]
pub struct RpcClient {
    url: Url,
    conn: Option<BufReader<Stream>>,
    timeout: Duration,
}

impl RpcClient {
    pub fn new(url: &str) -> io::Result<RpcClient> {
        Ok(RpcClient {
            url: Url::parse(url)?,
            conn: None,
            timeout: Duration::from_secs(15),
        })
    }

    /// POST `body` and return the response body. Retries once on a stale connection.
    pub async fn call(&mut self, body: &str) -> io::Result<String> {
        match self.call_once(body).await {
            Ok(r) => Ok(r),
            Err(_) => {
                self.conn = None;
                self.call_once(body).await
            }
        }
    }

    async fn call_once(&mut self, body: &str) -> io::Result<String> {
        let t = self.timeout;
        tokio::time::timeout(t, self.exchange(body))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rpc timeout"))?
            .inspect_err(|_| self.conn = None)
    }

    async fn exchange(&mut self, body: &str) -> io::Result<String> {
        if self.conn.is_none() {
            let s = connect_stream(&self.url, self.timeout).await?;
            self.conn = Some(BufReader::new(s));
        }
        let c = self.conn.as_mut().expect("connected");
        let req = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: rugsnipe-feed\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            self.url.path,
            self.url.host,
            body.len()
        );
        c.get_mut().write_all(req.as_bytes()).await?;
        c.get_mut().write_all(body.as_bytes()).await?;
        c.get_mut().flush().await?;
        // status line + headers
        let mut line = String::new();
        let mut status = 0u16;
        let mut content_length: Option<usize> = None;
        let mut chunked = false;
        let mut close = false;
        let mut head = 0usize;
        loop {
            line.clear();
            let n = c.read_line(&mut line).await?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "eof in headers",
                ));
            }
            head += n;
            if head > MAX_HEAD {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "headers too large",
                ));
            }
            let l = line.trim_end();
            if status == 0 {
                status = l
                    .split_whitespace()
                    .nth(1)
                    .and_then(|c| c.parse().ok())
                    .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "bad status"))?;
                continue;
            }
            if l.is_empty() {
                break;
            }
            if let Some((k, v)) = l.split_once(':') {
                let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
                match k.as_str() {
                    "content-length" => content_length = v.parse().ok(),
                    "transfer-encoding" => chunked = v.to_ascii_lowercase().contains("chunked"),
                    "connection" => close = v.eq_ignore_ascii_case("close"),
                    _ => {}
                }
            }
        }
        let mut body = Vec::new();
        if chunked {
            loop {
                line.clear();
                c.read_line(&mut line).await?;
                let size = usize::from_str_radix(line.trim().split(';').next().unwrap_or("0"), 16)
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "bad chunk size"))?;
                if body.len() + size > MAX_BODY {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
                }
                if size == 0 {
                    // trailers until the empty line
                    loop {
                        line.clear();
                        if c.read_line(&mut line).await? == 0 || line.trim().is_empty() {
                            break;
                        }
                    }
                    break;
                }
                let start = body.len();
                body.resize(start + size, 0);
                c.read_exact(&mut body[start..]).await?;
                line.clear();
                c.read_line(&mut line).await?; // CRLF after the chunk
            }
        } else if let Some(n) = content_length {
            if n > MAX_BODY {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
            }
            body.resize(n, 0);
            c.read_exact(&mut body).await?;
        } else {
            // read to close
            c.take(MAX_BODY as u64).read_to_end(&mut body).await?;
            close = true;
        }
        if close {
            self.conn = None;
        }
        let text = String::from_utf8(body)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-utf8 body"))?;
        if !(200..300).contains(&status) {
            return Err(io::Error::other(format!(
                "http {status}: {}",
                &text[..text.len().min(200)]
            )));
        }
        Ok(text)
    }
}

/// Request body for `getTransaction` (jsonParsed, confirmed, v1-aware).
pub fn get_transaction_body(id: u64, sig: &str) -> String {
    format!(
        r#"{{"jsonrpc":"2.0","id":{id},"method":"getTransaction","params":["{sig}",{{"encoding":"jsonParsed","maxSupportedTransactionVersion":1,"commitment":"confirmed"}}]}}"#
    )
}

/// Request body for `getBlockTime`.
pub fn get_block_time_body(id: u64, slot: u64) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"getBlockTime","params":[{slot}]}}"#)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;

    async fn serve_once(resp: &'static [u8]) -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    loop {
                        let n = s.read(&mut buf).await.unwrap_or(0);
                        if n == 0 || s.write_all(resp).await.is_err() {
                            break;
                        }
                    }
                });
            }
        });
        format!("http://{addr}/")
    }

    #[test]
    fn content_length_and_chunked_and_keepalive() {
        // loopback: NO_PROXY lists 127.0.0.1 wherever an HTTPS proxy is configured
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let url = serve_once(
                b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}",
            )
            .await;
            let mut c = RpcClient::new(&url).unwrap();
            assert_eq!(c.call("{}").await.unwrap(), "{\"ok\":true}");
            // same connection again (keep-alive)
            assert_eq!(c.call("{}").await.unwrap(), "{\"ok\":true}");

            let url = serve_once(
                b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\n{\"a\"\r\n3\r\n:1}\r\n0\r\n\r\n",
            )
            .await;
            let mut c = RpcClient::new(&url).unwrap();
            assert_eq!(c.call("{}").await.unwrap(), "{\"a\":1}");

            let url = serve_once(b"HTTP/1.1 429 Too Many Requests\r\nContent-Length: 4\r\n\r\nslow").await;
            let mut c = RpcClient::new(&url).unwrap();
            let e = c.call("{}").await.unwrap_err().to_string();
            assert!(e.contains("429"), "{e}");
        });
    }

    #[test]
    fn bodies() {
        assert!(get_transaction_body(7, "abc").contains("\"jsonParsed\""));
        assert_eq!(
            get_block_time_body(1, 42),
            r#"{"jsonrpc":"2.0","id":1,"method":"getBlockTime","params":[42]}"#
        );
    }
}
