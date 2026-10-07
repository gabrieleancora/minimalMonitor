use std::{net::SocketAddr, sync::Arc, time::Duration};

use axum::{Router, routing::get};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{self, pki_types::PrivatePkcs8KeyDer},
};

use crate::Result;

const DEADLINE: Duration = Duration::from_secs(5);

pub struct Fixtures {
    http: SocketAddr,
    tls: SocketAddr,
    certificate: reqwest::Certificate,
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
        let certificate = reqwest::Certificate::from_der(key.cert.der())?;
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
    pub trusted: reqwest::Client,
    untrusted: reqwest::Client,
}

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
            trusted: builder(fixtures.tls)
                .add_root_certificate(fixtures.certificate.clone())
                .build()?,
            untrusted: builder(fixtures.tls).build()?,
        })
    }

    pub async fn verify_tls(&self, fixtures: &Fixtures) -> Result<()> {
        // Fixed fixture destinations are explicit narrow loopback exceptions. No
        // user URL/DNS input exists; do not expose these clients as production probes.
        let url = format!("https://localhost:{}/", fixtures.tls.port());
        assert_eq!(self.trusted.get(&url).send().await?.status(), 200);
        let untrusted = self.untrusted.get(&url).send().await.unwrap_err();
        assert!(untrusted.is_connect() && !untrusted.is_timeout());
        let wrong_host = self
            .trusted
            .get(format!("https://{}/", fixtures.tls))
            .send()
            .await
            .unwrap_err();
        assert!(wrong_host.is_connect() && !wrong_host.is_timeout());
        Ok(())
    }
}

pub async fn http_probe(client: &reqwest::Client, address: SocketAddr) -> Result<()> {
    assert!(address.ip().is_loopback());
    let response = client.get(format!("http://{address}/")).send().await?;
    assert_eq!(response.status(), 200);
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
        let client = builder(address)
            .timeout(Duration::from_millis(100))
            .build()?;
        let error = client
            .get(format!("http://{address}/"))
            .send()
            .await
            .unwrap_err();
        assert!(error.is_timeout());
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
