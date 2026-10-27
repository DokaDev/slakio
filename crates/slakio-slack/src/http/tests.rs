//! The wire's rails: only the workspace's hosts, and TLS that trusts only a valid certificate
//! (against a TLS server on this machine with a certificate made for the test).

use super::*;
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn prepared(url: &str) -> Prepared {
    Prepared { url: url.to_string(), headers: vec![("Content-Type", "text/plain".to_string())], body: "x".to_string() }
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
}

#[test]
fn a_workspace_reaches_its_own_host_and_the_edge_cache_only() {
    rt().block_on(async {
        let endpoints = Endpoints::workspace("example.slack.com").unwrap();
        for url in [
            "https://other.slack.com/api/client.counts",
            "https://status.slack.com/x",
            "https://example.com/api/client.counts",
            "http://example.slack.com/api/client.counts",
            "https://example.slack.com.evil.test/api/x",
        ] {
            assert_eq!(exchange(&endpoints, &prepared(url)).await, Err(ONLY_SLACK), "{url}");
        }
        assert!(Endpoints::workspace("app.slack.com").is_none());
        assert!(Endpoints::workspace("example.com").is_none());
        // A local server is reached by its own address only.
        let local = Endpoints::local("127.0.0.1:9".parse().unwrap()).unwrap();
        assert_eq!(exchange(&local, &prepared("http://127.0.0.2:9/api/x")).await, Err(ONLY_LOCAL));
    });
}

/// A TLS server on this machine for `host`, answering one HTTP/1.1 request; its address and
/// the certificate.
async fn tls_server(host: &str) -> (SocketAddr, rustls::pki_types::CertificateDer<'static>) {
    let key = rcgen::generate_simple_self_signed(vec![host.to_string()]).unwrap();
    let cert = key.cert.der().clone();
    let private = rustls::pki_types::PrivateKeyDer::try_from(key.signing_key.serialize_der()).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.clone()], private)
        .unwrap();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let Ok((tcp, _)) = listener.accept().await else { return };
        let Ok(mut tls) = acceptor.accept(tcp).await else { return };
        let mut buf = vec![0u8; 4096];
        let mut got = Vec::new();
        while !got.windows(4).any(|w| w == b"\r\n\r\n") {
            let Ok(n) = tls.read(&mut buf).await else { return };
            if n == 0 {
                return;
            }
            got.extend_from_slice(&buf[..n]);
        }
        let body = br#"{"ok":true}"#;
        let head = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len());
        let _ = tls.write_all(head.as_bytes()).await;
        let _ = tls.write_all(body).await;
        let _ = tls.shutdown().await;
    });
    (addr, cert)
}

fn client_config(trusted: Option<rustls::pki_types::CertificateDer<'static>>) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(c) = trusted {
        roots.add(c).unwrap();
    }
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let mut config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(config)
}

#[test]
fn a_request_goes_over_tls_to_a_trusted_server() {
    rt().block_on(async {
        let (addr, cert) = tls_server("example.slack.com").await;
        let tcp = TcpStream::connect(addr).await.unwrap();
        let raw = over_tls(tcp, "example.slack.com", client_config(Some(cert)), "/api/x", &prepared("")).await.unwrap();
        assert_eq!((raw.status, raw.body.as_slice()), (200, br#"{"ok":true}"#.as_slice()));
    });
}

#[test]
fn a_server_without_a_trusted_certificate_gets_nothing() {
    rt().block_on(async {
        let (addr, _) = tls_server("example.slack.com").await;
        let tcp = TcpStream::connect(addr).await.unwrap();
        let e = over_tls(tcp, "example.slack.com", client_config(None), "/api/x", &prepared("")).await.unwrap_err();
        assert_eq!(e, net("the secure connection failed"));
        // A certificate for another name is not trusted for this one either.
        let (addr, cert) = tls_server("other.slack.com").await;
        let tcp = TcpStream::connect(addr).await.unwrap();
        let e =
            over_tls(tcp, "example.slack.com", client_config(Some(cert)), "/api/x", &prepared("")).await.unwrap_err();
        assert_eq!(e, net("the secure connection failed"));
    });
}
