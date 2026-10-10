use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{Router, routing::get};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        self,
        pki_types::{CertificateDer, PrivatePkcs8KeyDer},
    },
};

use crate::Result;
use crate::probe_http::ProbeClient;

const DEADLINE: Duration = Duration::from_secs(5);

pub struct Fixtures {
    http: SocketAddr,
    tls: SocketAddr,
    certificate: CertificateDer<'static>,
    tasks: Vec<JoinHandle<()>>,
}

impl Fixtures {
    pub async fn start() -> Result<Self> {
        let http = TcpListener::bind("127.0.0.1:0").await?;
        let http_addr = http.local_addr()?;
        let http_task = tokio::spawn(async move {
            axum::serve(
                http,
                Router::new().route("/", get(|| async { "wp00 fixture" })),
            )
            .await
            .expect("local fixture server");
        });
        let key = rcgen::generate_simple_self_signed(vec!["localhost".into()])?;
        let certificate = key.cert.der().clone();
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                vec![key.cert.der().clone()],
                PrivatePkcs8KeyDer::from(key.signing_key.serialize_der()).into(),
            )?;
        let tls = TcpListener::bind("127.0.0.1:0").await?;
        let tls_addr = tls.local_addr()?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let tls_task = tokio::spawn(async move {
            // Fixture accepts sequentially: no unbounded connection task spawning.
            for _ in 0..64 {
                let Ok((stream, _)) = tls.accept().await else {
                    break;
                };
                let _ = tokio::time::timeout(DEADLINE, async {
                    let mut stream = acceptor.accept(stream).await?;
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request).await?;
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        )
                        .await?;
                    stream.shutdown().await
                })
                .await;
            }
        });
        Ok(Self {
            http: http_addr,
            tls: tls_addr,
            certificate,
            tasks: vec![http_task, tls_task],
        })
    }

    pub fn addresses(&self) -> (SocketAddr, SocketAddr) {
        (self.http, self.http)
    }

    pub async fn close(self) {
        for task in self.tasks {
            task.abort();
            let _ = task.await;
        }
    }
}

pub struct Clients {
    pub trusted: ProbeClient,
    untrusted: ProbeClient,
}

#[cfg(test)]
fn builder(address: SocketAddr) -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(DEADLINE)
        .connect_timeout(DEADLINE)
        .pool_max_idle_per_host(0)
        .http1_only()
        .resolve("localhost", address)
}

impl Clients {
    pub fn new(fixtures: &Fixtures) -> Result<Self> {
        Ok(Self {
            trusted: ProbeClient::new(Some(fixtures.certificate.clone()))?,
            untrusted: ProbeClient::new(None)?,
        })
    }

    pub async fn verify_tls(&self, fixtures: &Fixtures) -> Result<()> {
        // Fixed fixture destinations are explicit narrow loopback exceptions. No
        // user URL/DNS input exists; do not expose these clients as production probes.
        assert_eq!(
            self.trusted.status(fixtures.tls, Some("localhost")).await?,
            200
        );
        let untrusted = self
            .untrusted
            .status(fixtures.tls, Some("localhost"))
            .await
            .unwrap_err();
        assert!(untrusted.downcast_ref::<std::io::Error>().is_some());
        let wrong_host = self
            .trusted
            .status(fixtures.tls, Some("127.0.0.1"))
            .await
            .unwrap_err();
        assert!(wrong_host.downcast_ref::<std::io::Error>().is_some());
        Ok(())
    }
}

pub async fn http_probe(client: &ProbeClient, address: SocketAddr) -> Result<()> {
    assert!(address.ip().is_loopback());
    assert_eq!(client.status(address, None).await?, 200);
    // Only response status/headers are observed; body is dropped unread.
    Ok(())
}

pub async fn tcp_probe(address: SocketAddr) -> Result<()> {
    assert!(address.ip().is_loopback());
    drop(tokio::time::timeout(DEADLINE, TcpStream::connect(address)).await??);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_tls_validates_trust_and_hostname() -> Result<()> {
        let fixtures = Fixtures::start().await?;
        let clients = Clients::new(&fixtures)?;
        clients.verify_tls(&fixtures).await?;
        http_probe(&clients.trusted, fixtures.http).await?;
        tcp_probe(fixtures.http).await?;
        fixtures.close().await;
        Ok(())
    }

    #[tokio::test]
    async fn refused_tcp_is_failure() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        drop(listener);
        assert!(tcp_probe(address).await.is_err());
        Ok(())
    }

    #[tokio::test]
    async fn response_deadline_includes_wait_for_headers() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let _stream = stream;
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        let client = ProbeClient::new(None)?.with_deadline(Duration::from_millis(100));
        let error = client.status(address, None).await.unwrap_err();
        assert!(
            error
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some()
        );
        server.abort();
        let _ = server.await;
        Ok(())
    }

    // Documents a candidate-stack limitation, not a passing security gate.
    #[tokio::test]
    async fn characterize_reqwest_http1_header_limit_gap() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut input = [0; 4096];
            let _received = stream.read(&mut input).await.unwrap();
            let value = "x".repeat(33 * 1024);
            let response = format!(
                "HTTP/1.1 200 OK\r\nX-Fixture: {value}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            );
            stream.write_all(response.as_bytes()).await.unwrap();
        });
        let response = builder(address)
            .build()?
            .get(format!("http://{address}/"))
            .send()
            .await?;
        assert_eq!(response.headers()["x-fixture"].as_bytes().len(), 33 * 1024);
        server.await?;
        Ok(())
    }
}
