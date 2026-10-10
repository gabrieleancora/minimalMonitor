//! A one-shot HTTP/1 probe, not a pooled general-purpose/browser client.
use std::{io, net::SocketAddr, sync::Arc, time::Duration};

use http_body_util::Empty;
use hyper::{Request, StatusCode, body::Bytes, client::conn::http1};
use hyper_util::rt::TokioIo;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{
        ClientConfig, RootCertStore,
        pki_types::{CertificateDer, ServerName},
    },
};

use crate::Result;

pub const HEADER_BYTES: usize = 32 * 1024;
pub const HEADER_COUNT: usize = 100;

#[derive(Clone)]
pub struct ProbeClient {
    tls: Arc<ClientConfig>,
    deadline: Duration,
}

impl ProbeClient {
    pub fn new(extra_root: Option<CertificateDer<'static>>) -> Result<Self> {
        let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        if let Some(root) = extra_root {
            roots.add(root)?;
        }
        let tls = ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        Ok(Self {
            tls: Arc::new(tls),
            deadline: Duration::from_secs(5),
        })
    }

    #[cfg(test)]
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    pub async fn status(&self, address: SocketAddr, tls_name: Option<&str>) -> Result<StatusCode> {
        // Explicit narrow fixture exception only. Later destination-policy code
        // must supply validated addresses while preserving the original TLS name.
        if !address.ip().is_loopback() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "fixture policy").into());
        }
        tokio::time::timeout(self.deadline, async {
            let stream = TcpStream::connect(address).await?;
            if let Some(host) = tls_name {
                let name = ServerName::try_from(host.to_owned())?;
                let stream = TlsConnector::from(self.tls.clone())
                    .connect(name, stream)
                    .await?;
                request(stream, format!("{host}:{}", address.port())).await
            } else {
                request(stream, address.to_string()).await
            }
        })
        .await?
    }
}

async fn request<T>(stream: T, authority: String) -> Result<StatusCode>
where
    T: AsyncRead + AsyncWrite + Unpin,
{
    let mut builder = http1::Builder::new();
    // Both limits matter: cap accepted wire header bytes (including status line
    // and CRLF terminator) and cap the connection's adaptive read buffer strategy.
    builder
        .max_header_size(HEADER_BYTES)
        .max_buf_size(HEADER_BYTES)
        .max_headers(HEADER_COUNT);
    let (mut sender, connection) = builder.handshake(TokioIo::new(stream)).await?;
    let request = Request::builder()
        .uri("/")
        .header(hyper::header::HOST, authority)
        .header(hyper::header::CONNECTION, "close")
        .body(Empty::<Bytes>::new())?;
    tokio::pin!(connection);
    // Poll the driver in this probe future: no detached task, pool, retry, redirect,
    // proxy inheritance, cookie jar or decompressor. Canceling drops the socket.
    let response_future = sender.send_request(request);
    tokio::pin!(response_future);
    let response = tokio::select! {
        response = &mut response_future => response?,
        result = &mut connection => {
            result?;
            // A zero-body/Connection: close response can finish the driver in
            // the same poll that it delivers headers to the response future.
            response_future.await?
        }
    };
    let status = response.status();
    // Never poll/store a response body. Dropping the driver closes the connection.
    drop(response);
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    use tokio_rustls::{
        TlsAcceptor,
        rustls::{ServerConfig, pki_types::PrivatePkcs8KeyDer},
    };

    fn single_header(total: usize) -> Vec<u8> {
        let prefix = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nX-Fixture: ";
        let suffix = b"\r\n\r\n";
        let mut response = prefix.to_vec();
        response.extend(vec![b'x'; total - prefix.len() - suffix.len()]);
        response.extend(suffix);
        assert_eq!(response.len(), total);
        response
    }

    async fn reply(response: Vec<u8>, tls: bool, chunk: usize) -> Result<StatusCode> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let key = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let client = ProbeClient::new(Some(key.cert.der().clone()))?;
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![key.cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.signing_key.serialize_der()).into(),
            )?;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            if tls {
                let stream = TlsAcceptor::from(Arc::new(config))
                    .accept(stream)
                    .await
                    .unwrap();
                write_response(stream, &response, chunk).await;
            } else {
                write_response(stream, &response, chunk).await;
            }
        });
        let result = client.status(address, tls.then_some("localhost")).await;
        server.abort();
        let _ = server.await;
        result
    }

    async fn write_response<T: AsyncRead + AsyncWrite + Unpin>(
        mut stream: T,
        response: &[u8],
        chunk: usize,
    ) {
        let mut input = [0; 4096];
        let _received = stream.read(&mut input).await.unwrap();
        for part in response.chunks(chunk) {
            if stream.write_all(part).await.is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    }

    fn assert_header_limit(error: Box<dyn std::error::Error + Send + Sync>) {
        let error = error
            .downcast::<hyper::Error>()
            .expect("Hyper parser error");
        assert!(
            error.is_parse_too_large(),
            "expected header limit, got {error}"
        );
    }

    #[tokio::test]
    async fn accepts_exact_boundary_rejects_one_byte_over_and_large_single_header() -> Result<()> {
        for tls in [false, true] {
            assert_eq!(
                reply(single_header(HEADER_BYTES), tls, usize::MAX).await?,
                StatusCode::OK
            );
            for total in [HEADER_BYTES + 1, 33 * 1024, 256 * 1024] {
                assert_header_limit(
                    reply(single_header(total), tls, usize::MAX)
                        .await
                        .unwrap_err(),
                );
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn rejects_cumulative_and_fragmented_headers_and_unterminated_block() {
        let mut cumulative = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n".to_vec();
        for index in 0..64 {
            cumulative.extend(format!("X-{index}: {}\r\n", "x".repeat(600)).bytes());
        }
        cumulative.extend(b"\r\n");
        for tls in [false, true] {
            assert_header_limit(
                reply(cumulative.clone(), tls, usize::MAX)
                    .await
                    .unwrap_err(),
            );
        }
        assert_header_limit(
            reply(single_header(HEADER_BYTES + 1), false, 113)
                .await
                .unwrap_err(),
        );
        let mut partial = single_header(HEADER_BYTES + 8);
        partial.truncate(HEADER_BYTES);
        assert_header_limit(reply(partial, false, 113).await.unwrap_err());
    }

    #[tokio::test]
    async fn rejects_excess_header_count_and_does_not_follow_redirect_or_read_body() -> Result<()> {
        let mut many = b"HTTP/1.1 200 OK\r\n".to_vec();
        for index in 0..=HEADER_COUNT {
            many.extend(format!("X-{index}: x\r\n").bytes());
        }
        many.extend(b"\r\n");
        assert_header_limit(reply(many, false, usize::MAX).await.unwrap_err());
        assert_eq!(reply(b"HTTP/1.1 302 Found\r\nLocation: http://192.0.2.1/\r\nContent-Length: 999999999\r\n\r\n".to_vec(), false, usize::MAX).await?, StatusCode::FOUND);
        assert_eq!(
            reply(
                b"HTTP/1.1 200 OK\r\nContent-Length: 999999999\r\n\r\n".to_vec(),
                false,
                usize::MAX
            )
            .await?,
            StatusCode::OK
        );
        Ok(())
    }

    #[tokio::test]
    async fn tls_deadline_closes_socket_and_denied_fixture_target_never_connects() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = [0; 4096];
            // Read the ClientHello but deliberately never respond. EOF proves
            // that timing out drops the TLS future and its underlying socket.
            while stream.read(&mut bytes).await.unwrap() != 0 {}
        });
        let client = ProbeClient::new(None)?.with_deadline(Duration::from_millis(100));
        let error = client.status(address, Some("localhost")).await.unwrap_err();
        assert!(
            error
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some()
        );
        tokio::time::timeout(Duration::from_secs(1), server).await??;
        let error = client
            .status("192.0.2.1:80".parse()?, None)
            .await
            .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::PermissionDenied
        );
        Ok(())
    }
}
