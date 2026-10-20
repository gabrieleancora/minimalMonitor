use super::*;
use crate::{
    destination::{Policy, Target},
    probes::{ProbePermit, Resolver, SystemResolver},
};
use http_body_util::{BodyExt, Full};
use hyper::{
    Request,
    body::{Body, Bytes},
    client::conn::http1,
};
use hyper_util::rt::TokioIo;
use std::net::SocketAddr;
use std::sync::atomic::AtomicU64;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, RootCertStore, pki_types::ServerName},
};

const HOST: &str = "api.telegram.org";
pub struct Telegram {
    token: Option<Arc<Token>>,
    tls: Arc<ClientConfig>,
    resolver: Arc<dyn Resolver>,
    observed_retry: AtomicU64,
}
impl Telegram {
    pub fn new(token: Option<Arc<Token>>) -> Self {
        let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        Self {
            token,
            tls: Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            ),
            resolver: Arc::new(SystemResolver),
            observed_retry: AtomicU64::new(0),
        }
    }
    async fn execute(&self, chat: i64, text: &str, permit: ProbePermit) -> Delivery {
        let end = tokio::time::Instant::now() + Duration::from_secs(10);
        let Some(token) = &self.token else {
            return Delivery::Configuration;
        };
        let Ok(answers) = self.resolver.resolve(HOST.to_owned(), permit).await else {
            return Delivery::Transient;
        };
        // No target grants, custom endpoints, proxies, redirects or DNS cache.
        let Ok(policy) = Policy::new(vec![]) else {
            return Delivery::Configuration;
        };
        let Ok(target) = Target::tcp(HOST, 443) else {
            return Delivery::Configuration;
        };
        if answers.len() > 8 || policy.authorize(&target, &answers).is_err() {
            return Delivery::Transient;
        }
        let addresses: Vec<_> = answers
            .into_iter()
            .take(2)
            .map(|ip| SocketAddr::new(ip, 443))
            .collect();
        let Some(stream) = connect_any(&addresses, self.tls.clone(), end).await else {
            return Delivery::Transient;
        };
        // Fallback is only before POST. Once submitted, outcome goes to the
        // durable retry state; never issue a second POST in the same attempt.
        request(stream, token, chat, text, &self.observed_retry).await
    }
}
#[cfg(test)]
async fn send_to(
    address: SocketAddr,
    tls: Arc<ClientConfig>,
    token: &Token,
    chat: i64,
    text: &str,
    observed_retry: &AtomicU64,
) -> Delivery {
    let Some(stream) = connect_any(
        &[address],
        tls,
        tokio::time::Instant::now() + Duration::from_secs(10),
    )
    .await
    else {
        return Delivery::Transient;
    };
    request(stream, token, chat, text, observed_retry).await
}
async fn connect_any(
    addresses: &[SocketAddr],
    tls: Arc<ClientConfig>,
    end: tokio::time::Instant,
) -> Option<tokio_rustls::client::TlsStream<TcpStream>> {
    let count = addresses.len().min(2);
    for (index, address) in addresses.iter().take(count).enumerate() {
        let budget =
            end.saturating_duration_since(tokio::time::Instant::now()) / (count - index) as u32;
        let connect = async {
            let stream = TcpStream::connect(address).await.ok()?;
            let name = ServerName::try_from(HOST).ok()?;
            TlsConnector::from(tls.clone())
                .connect(name, stream)
                .await
                .ok()
        };
        if let Ok(Some(stream)) = tokio::time::timeout(budget, connect).await {
            return Some(stream);
        }
    }
    None
}
impl Transport for Telegram {
    fn timeout_outcome(&self) -> Delivery {
        match self.observed_retry.load(Ordering::Relaxed) {
            0 => Delivery::Transient,
            retry_after_seconds => Delivery::RateLimited {
                retry_after_seconds,
            },
        }
    }
    fn send<'a>(&'a self, chat: i64, text: &'a str, permit: ProbePermit) -> SendFuture<'a> {
        Box::pin(async move {
            self.observed_retry.store(0, Ordering::Relaxed);
            tokio::time::timeout(Duration::from_secs(10), self.execute(chat, text, permit))
                .await
                .unwrap_or_else(|_| self.timeout_outcome())
        })
    }
}
async fn request<T: AsyncRead + AsyncWrite + Unpin>(
    stream: T,
    token: &Token,
    chat: i64,
    text: &str,
    observed_retry: &AtomicU64,
) -> Delivery {
    if text.encode_utf16().count() > 3000 || chat == 0 {
        return Delivery::Configuration;
    }
    let body =
        serde_json::json!({"chat_id":chat,"text":text,"link_preview_options":{"is_disabled":true}})
            .to_string();
    let request = Request::builder()
        .method("POST")
        .uri(format!("/bot{}/sendMessage", token.0.as_str()))
        .header(hyper::header::HOST, HOST)
        .header(hyper::header::CONTENT_TYPE, "application/json")
        .header(hyper::header::CONNECTION, "close")
        .body(Full::new(Bytes::from(body)));
    let Ok(request) = request else {
        return Delivery::Configuration;
    };
    let mut builder = http1::Builder::new();
    builder
        .max_header_size(32768)
        .max_buf_size(32768)
        .max_headers(100);
    let Ok((mut sender, connection)) = builder.handshake(TokioIo::new(stream)).await else {
        return Delivery::Transient;
    };
    tokio::pin!(connection);
    let exchange = async {
        let mut response = sender.send_request(request).await.map_err(|_| ())?;
        let status = response.status().as_u16();
        let header_retry = response
            .headers()
            .get(hyper::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        if status == 429 {
            observed_retry.store(header_retry.max(60), Ordering::Relaxed);
        }
        if response
            .body()
            .size_hint()
            .upper()
            .is_some_and(|n| n > 65536)
        {
            return Ok(classify(status, &[], header_retry));
        }
        let mut body = Vec::with_capacity(4096);
        while let Some(frame) = response.body_mut().frame().await {
            let frame = match frame {
                Ok(frame) => frame,
                Err(_) => return Ok(classify(status, &[], header_retry)),
            };
            if let Ok(bytes) = frame.into_data() {
                if body.len() + bytes.len() > 65536 {
                    return Ok(classify(status, &[], header_retry));
                }
                body.extend_from_slice(&bytes);
            }
        }
        Ok::<_, ()>(classify(status, &body, header_retry))
    };
    tokio::pin!(exchange);
    tokio::select! {
        result=&mut exchange=>result.unwrap_or_else(|_|fallback(observed_retry)),
        result=&mut connection=>{
            if result.is_err() {fallback(observed_retry)} else {exchange.await.unwrap_or_else(|_|fallback(observed_retry))}
        }
    }
}
fn fallback(retry: &AtomicU64) -> Delivery {
    match retry.load(Ordering::Relaxed) {
        0 => Delivery::Transient,
        retry_after_seconds => Delivery::RateLimited {
            retry_after_seconds,
        },
    }
}
fn classify(status: u16, body: &[u8], header_retry: u64) -> Delivery {
    let data = serde_json::from_slice::<serde_json::Value>(body).ok();
    let code = data
        .as_ref()
        .and_then(|d| d.get("error_code"))
        .and_then(|v| v.as_u64())
        .unwrap_or(u64::from(status));
    if status == 429 || code == 429 {
        let retry = data
            .as_ref()
            .and_then(|d| d.get("parameters"))
            .and_then(|d| d.get("retry_after"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        return Delivery::RateLimited {
            retry_after_seconds: retry.max(header_retry),
        };
    }
    if (500..=599).contains(&status) || (500..=599).contains(&code) {
        return Delivery::Transient;
    }
    if (400..=499).contains(&status) || (400..=499).contains(&code) {
        return Delivery::Configuration;
    }
    if status == 200
        && let Some(data) = data
        && data.get("ok").and_then(|v| v.as_bool()) == Some(true)
        && let Some(id) = data
            .get("result")
            .and_then(|r| r.get("message_id"))
            .and_then(|v| v.as_i64())
        && id > 0
    {
        return Delivery::Delivered(id);
    }
    Delivery::Transient
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    fn token() -> Token {
        Token(Zeroizing::new(format!("12345:{}", "X".repeat(32))))
    }
    async fn wire(response: Vec<u8>) -> (Delivery, serde_json::Value) {
        let (client, mut server) = tokio::io::duplex(131072);
        let responder = async move {
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            let header_end = loop {
                let n = server.read(&mut buf).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
                if let Some(index) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    break index + 4;
                }
            };
            let headers = String::from_utf8(bytes[..header_end].to_vec()).unwrap();
            assert!(headers.starts_with("POST /bot12345:"));
            assert!(headers.contains("/sendMessage HTTP/1.1"));
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("host: api.telegram.org")
            );
            let len: usize = headers
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .and_then(|v| v.parse().ok())
                })
                .unwrap();
            while bytes.len() < header_end + len {
                let n = server.read(&mut buf).await.unwrap();
                assert!(n > 0);
                bytes.extend_from_slice(&buf[..n]);
            }
            let body = serde_json::from_slice(&bytes[header_end..header_end + len]).unwrap();
            server.write_all(&response).await.unwrap();
            server.shutdown().await.unwrap();
            body
        };
        let token = token();
        let retry = AtomicU64::new(0);
        let (delivery, body) = tokio::join!(
            request(client, &token, i64::MIN, "plain <text>", &retry),
            responder
        );
        (delivery, body)
    }
    fn response(status: u16, body: &[u8]) -> Vec<u8> {
        let mut bytes = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes();
        bytes.extend_from_slice(body);
        bytes
    }
    #[tokio::test]
    async fn actual_http_request_and_bounded_telegram_response_classification() {
        let (outcome, body) =
            wire(response(200, br#"{"ok":true,"result":{"message_id":123}}"#)).await;
        assert_eq!(outcome, Delivery::Delivered(123));
        assert_eq!(body["chat_id"].as_i64(), Some(i64::MIN));
        assert_eq!(body["text"], "plain <text>");
        assert_eq!(body["link_preview_options"]["is_disabled"], true);
        assert!(body.get("parse_mode").is_none());
        for (status,body,expected) in [
            (429,br#"{"ok":false,"error_code":429,"parameters":{"retry_after":120},"description":"sensitive ignored"}"#.as_slice(),Delivery::RateLimited {retry_after_seconds:120}),
            (200,br#"{"ok":false,"error_code":429,"parameters":{"retry_after":1}}"#.as_slice(),Delivery::RateLimited {retry_after_seconds:1}),
            (401,br#"{"ok":false,"error_code":401}"#.as_slice(),Delivery::Configuration),
            (403,b"forbidden".as_slice(),Delivery::Configuration),
            (500,b"failed".as_slice(),Delivery::Transient),
            (200,b"invalid-json".as_slice(),Delivery::Transient),
            (302,b"redirect".as_slice(),Delivery::Transient),
        ] {assert_eq!(wire(response(status,body)).await.0,expected);}
        assert_eq!(
            classify(429, br#"{"parameters":{"retry_after":120}}"#, 300),
            Delivery::RateLimited {
                retry_after_seconds: 300
            }
        );
    }
    #[tokio::test]
    async fn oversized_fixed_and_streaming_responses_and_headers_fail_boundedly() {
        let oversized = vec![b' '; 65537];
        assert_eq!(wire(response(200, &oversized)).await.0, Delivery::Transient);
        let chunked = format!(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n{:x}\r\n{}\r\n0\r\n\r\n",
            oversized.len(),
            " ".repeat(oversized.len())
        );
        assert_eq!(wire(chunked.into_bytes()).await.0, Delivery::Transient);
        let body = br#"{"ok":true,"result":{"message_id":1}}"#;
        let padding = 65536 - body.len();
        let mut boundary = body.to_vec();
        boundary.extend(vec![b' '; padding]);
        assert_eq!(
            wire(response(200, &boundary)).await.0,
            Delivery::Delivered(1)
        );
        let headers = format!(
            "HTTP/1.1 200 OK\r\nX-Large: {}\r\nContent-Length: 0\r\n\r\n",
            "x".repeat(32768)
        );
        assert_eq!(wire(headers.into_bytes()).await.0, Delivery::Transient);
    }
    #[tokio::test(start_paused = true)]
    async fn stalled_response_hits_total_deadline_and_cancellation_releases_slot() {
        let bounds = WorkBounds::default();
        let permit = Arc::new(bounds.notification().unwrap());
        let (client, _server) = tokio::io::duplex(4096);
        let token = token();
        let retry = AtomicU64::new(0);
        assert!(
            tokio::time::timeout(
                Duration::from_secs(10),
                request(client, &token, -100, "test", &retry)
            )
            .await
            .is_err()
        );
        drop(permit);
        assert!(bounds.notification().is_ok());
    }
    #[tokio::test]
    async fn tls_requires_trusted_root_and_official_server_name() {
        use tokio_rustls::rustls::{
            ServerConfig,
            pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer},
        };
        for (name, trusted, expected) in [
            (HOST, true, Delivery::Delivered(7)),
            (HOST, false, Delivery::Transient),
            ("wrong.invalid", true, Delivery::Transient),
        ] {
            let certified = rcgen::generate_simple_self_signed(vec![name.into()]).unwrap();
            let cert = certified.cert.der().clone();
            let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(
                certified.signing_key.serialize_der(),
            ));
            let server = ServerConfig::builder()
                .with_no_client_auth()
                .with_single_cert(vec![cert.clone()], key)
                .unwrap();
            let mut roots = RootCertStore::empty();
            if trusted {
                roots.add(cert).unwrap();
            }
            let tls = Arc::new(
                ClientConfig::builder()
                    .with_root_certificates(roots)
                    .with_no_client_auth(),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let task = tokio::spawn(async move {
                let (socket, _) = listener.accept().await.unwrap();
                if let Ok(mut stream) = tokio_rustls::TlsAcceptor::from(Arc::new(server))
                    .accept(socket)
                    .await
                {
                    let mut buf = [0; 8192];
                    let _ = stream.read(&mut buf).await;
                    stream
                        .write_all(&response(200, br#"{"ok":true,"result":{"message_id":7}}"#))
                        .await
                        .unwrap();
                }
            });
            let outcome = if name == HOST && trusted {
                let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let refused = closed.local_addr().unwrap();
                drop(closed);
                let stream = connect_any(
                    &[refused, address],
                    tls,
                    tokio::time::Instant::now() + Duration::from_secs(10),
                )
                .await
                .unwrap();
                request(stream, &token(), -100, "test", &AtomicU64::new(0)).await
            } else {
                send_to(address, tls, &token(), -100, "test", &AtomicU64::new(0)).await
            };
            assert_eq!(outcome, expected);
            task.await.unwrap();
        }
    }
    #[tokio::test(start_paused = true)]
    async fn incomplete_429_body_preserves_bot_wide_wait_on_timeout() {
        let (client, mut server) = tokio::io::duplex(4096);
        let retry = AtomicU64::new(0);
        let token = token();
        let respond = async {
            let mut buf = [0; 4096];
            let _ = server.read(&mut buf).await.unwrap();
            server
                .write_all(b"HTTP/1.1 429 Busy\r\nRetry-After: 120\r\nContent-Length: 100\r\n\r\n")
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(20)).await;
        };
        let timeout = tokio::time::timeout(
            Duration::from_secs(10),
            request(client, &token, -100, "test", &retry),
        );
        let (result, _) = tokio::join!(timeout, respond);
        assert!(result.is_err());
        assert_eq!(
            fallback(&retry),
            Delivery::RateLimited {
                retry_after_seconds: 120
            }
        );
    }
    struct Answers(Vec<std::net::IpAddr>);
    impl Resolver for Answers {
        fn resolve(&self, host: String, _permit: ProbePermit) -> crate::probes::Resolution {
            assert_eq!(host, HOST);
            let ips = self.0.clone();
            Box::pin(async move { Ok(ips) })
        }
    }
    #[tokio::test]
    async fn official_dns_is_whole_set_validated_without_private_grants() {
        for ips in [
            vec!["127.0.0.1".parse().unwrap()],
            vec![
                "8.8.8.8".parse().unwrap(),
                "169.254.169.254".parse().unwrap(),
            ],
            vec!["8.8.8.8".parse().unwrap(); 9],
        ] {
            let mut telegram = Telegram::new(Some(Arc::new(token())));
            telegram.resolver = Arc::new(Answers(ips));
            let bounds = WorkBounds::default();
            assert_eq!(
                telegram
                    .send(-100, "test", Arc::new(bounds.notification().unwrap()))
                    .await,
                Delivery::Transient
            );
        }
    }
}
