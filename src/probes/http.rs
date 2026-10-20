use crate::monitoring::ProbeError;
use http_body_util::Empty;
use hyper::{Request, body::Bytes, client::conn::http1};
use hyper_util::rt::TokioIo;
use std::{net::SocketAddr, sync::Arc};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::{
    TlsConnector,
    rustls::{ClientConfig, pki_types::ServerName},
};
use url::Url;

pub(super) async fn status(
    address: SocketAddr,
    url: &Url,
    tls: Arc<ClientConfig>,
) -> Result<u16, ProbeError> {
    let stream = TcpStream::connect(address)
        .await
        .map_err(|e| super::io_error(&e))?;
    if url.scheme() == "https" {
        let host = url
            .host_str()
            .ok_or(ProbeError::Policy)?
            .trim_matches(['[', ']']);
        let name = ServerName::try_from(host.to_owned()).map_err(|_| ProbeError::Policy)?;
        let stream = TlsConnector::from(tls)
            .connect(name, stream)
            .await
            .map_err(|_| ProbeError::Tls)?;
        request(stream, url).await
    } else {
        request(stream, url).await
    }
}
async fn request<T: AsyncRead + AsyncWrite + Unpin>(
    stream: T,
    url: &Url,
) -> Result<u16, ProbeError> {
    let mut builder = http1::Builder::new();
    builder
        .max_header_size(32768)
        .max_buf_size(32768)
        .max_headers(100);
    let (mut sender, connection) = builder
        .handshake(TokioIo::new(stream))
        .await
        .map_err(|_| ProbeError::Headers)?;
    let authority = &url[url::Position::BeforeHost..url::Position::AfterPort];
    let path = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let request = Request::builder()
        .method("GET")
        .uri(path)
        .header(hyper::header::HOST, authority)
        .header(hyper::header::CONNECTION, "close")
        .body(Empty::<Bytes>::new())
        .map_err(|_| ProbeError::Policy)?;
    tokio::pin!(connection);
    let response = sender.send_request(request);
    tokio::pin!(response);
    // Driver lives in the probe: cancellation closes every socket and TLS future.
    let response = tokio::select! {
        response=&mut response => response.map_err(|_| ProbeError::Headers)?,
        finished=&mut connection => { finished.map_err(|_| ProbeError::Headers)?; response.await.map_err(|_| ProbeError::Headers)? }
    };
    // Ignore Location, encoding, cookies and body. No pool or automatic retries.
    let code = response.status().as_u16();
    drop(response);
    Ok(code)
}
