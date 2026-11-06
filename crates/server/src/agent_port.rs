//! Local mode's port for agents, on the desktop (docs/local.md#agent-接入): HTTPS with a certificate
//! made for this run. Its address and CA certificate go in the address file, and the command line
//! trusts that certificate alone: on a port another program has taken since, the handshake fails
//! before anything is sent, and no reply can be faked.

use std::net::Ipv4Addr;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto;
use hyper_util::service::TowerToHyperService;
use rcgen::{BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::rustls::ServerConfig;
use tokio_rustls::rustls::crypto::ring;
use tokio_rustls::rustls::pki_types::PrivateKeyDer;

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct AgentPort {
    listener: TcpListener,
    acceptor: TlsAcceptor,
}

/// Listens on 127.0.0.1 at a port the system picks, and writes the address file in `dir`.
pub(crate) async fn listen(dir: &Path) -> anyhow::Result<AgentPort> {
    let (config, ca) = certificate()?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = serde_json::json!({ "url": format!("https://{}", listener.local_addr()?), "ca": ca });
    // Replaced in one step: whoever reads it gets the last run's or this one's, never part of either.
    let (partial, file) = (dir.join("address.partial"), dir.join("address"));
    std::fs::write(&partial, address.to_string())?;
    std::fs::rename(partial, file)?;
    Ok(AgentPort { listener, acceptor: TlsAcceptor::from(Arc::new(config)) })
}

/// The server's TLS configuration, and the PEM of the CA that signed its certificate. A CA because
/// rustls won't take a server's own certificate as the root it trusts. Its key goes once it has
/// signed, the server's stays in memory.
fn certificate() -> anyhow::Result<(ServerConfig, String)> {
    let mut ca = CertificateParams::new(Vec::<String>::new())?;
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign];
    ca.distinguished_name.push(DnType::CommonName, "NextTodo 本机模式");
    let ca = CertifiedIssuer::self_signed(ca, KeyPair::generate()?)?;
    let key = KeyPair::generate()?;
    let mut params = CertificateParams::new(vec!["127.0.0.1".to_owned()])?;
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    params.use_authority_key_identifier_extension = true;
    let cert = params.signed_by(&key, &ca)?;
    let config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_single_cert(vec![cert.der().clone()], PrivateKeyDer::Pkcs8(key.serialize_der().into()))?;
    Ok((config, ca.pem()))
}

impl AgentPort {
    /// Serves `router`, each connection's handshake on its own and within a time limit, so that a
    /// slow one holds up no other.
    pub(crate) fn serve(self, router: Router) {
        tokio::spawn(async move {
            loop {
                let tcp = match self.listener.accept().await {
                    Ok((tcp, _)) => tcp,
                    // Out of file descriptors say: wait for some to be let go of.
                    Err(e) => {
                        tracing::error!("agent port: accepting failed: {e}");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                };
                let (acceptor, router) = (self.acceptor.clone(), router.clone());
                tokio::spawn(async move {
                    let Ok(Ok(tls)) = tokio::time::timeout(HANDSHAKE_TIMEOUT, acceptor.accept(tcp)).await else { return };
                    // A connection the agent breaks off is no error of ours.
                    let _ = auto::Builder::new(TokioExecutor::new())
                        .serve_connection(TokioIo::new(tls), TowerToHyperService::new(router))
                        .await;
                });
            }
        });
    }
}
