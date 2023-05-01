use log;
use ring::digest;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::time::timeout;
use tokio_rustls::rustls::client::*;
use tokio_rustls::rustls::server::*;
use tokio_rustls::rustls::*;
use tokio_rustls::TlsStream;

const CONNECTION_TIMEOUT: Duration = Duration::from_millis(2000);

pub struct BepVerifier {
    names: Vec<DistinguishedName>,
    peer_id: Vec<u8>,
}

impl BepVerifier {
    pub fn new(peer_id: Vec<u8>) -> Self {
        assert!(peer_id.len() == 32);
        BepVerifier {
            names: vec![],
            peer_id,
        }
    }

    pub fn check_cert(&self, cert: &Certificate) -> bool {
        let hash = digest::digest(&digest::SHA256, &cert.0);
        if self.peer_id.len() != hash.as_ref().len() {
            log::error!(
                "Expected len {} certificate, got {}",
                self.peer_id.len(),
                hash.as_ref().len()
            );
            return false;
        }
        for i in 0..self.peer_id.len() {
            if self.peer_id.get(i).unwrap() != hash.as_ref().get(i).unwrap() {
                return false;
            }
        }
        true
    }
}

impl ServerCertVerifier for BepVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &Certificate,
        _intermediates: &[Certificate],
        _server_name: &ServerName,
        _scts: &mut dyn Iterator<Item = &[u8]>,
        _ocsp_response: &[u8],
        _now: SystemTime,
    ) -> Result<ServerCertVerified, Error> {
        if self.check_cert(end_entity) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(Error::General("Invalid certificate".to_string()))
        }
    }
}

impl ClientCertVerifier for BepVerifier {
    fn verify_client_cert(
        &self,
        end_entity: &Certificate,
        _intermediates: &[Certificate],
        _now: SystemTime,
    ) -> Result<ClientCertVerified, Error> {
        if self.check_cert(end_entity) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(Error::General("Invalid certificate".to_string()))
        }
    }

    fn client_auth_root_subjects(&self) -> &[DistinguishedName] {
        &self.names
    }
}

pub async fn verify_connection(
    stream: (impl AsyncWrite + AsyncRead + Unpin + std::marker::Send + 'static),
    certificate: Certificate,
    key: PrivateKey,
    peer_id: Vec<u8>,
    connector: bool,
) -> tokio::io::Result<TlsStream<impl AsyncWrite + AsyncRead>> {
    let verifier = Arc::new(BepVerifier::new(peer_id));
    if connector {
        let config = tokio_rustls::rustls::ClientConfig::builder()
            .with_safe_defaults()
            .with_custom_certificate_verifier(verifier)
            .with_single_cert(vec![certificate.clone()], key.clone())
            .unwrap();

        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));

        let clientstream = timeout(
            CONNECTION_TIMEOUT,
            tokio_rustls::TlsConnector::connect(
                &connector,
                "example.com".try_into().unwrap(),
                stream,
            ),
        )
        .await?;
        Ok(clientstream?.try_into().unwrap())
    } else {
        let config = tokio_rustls::rustls::ServerConfig::builder()
            .with_safe_defaults()
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![certificate.clone()], key.clone())
            .unwrap();

        let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
        let serverstream = timeout(
            CONNECTION_TIMEOUT,
            tokio_rustls::TlsAcceptor::accept(&acceptor, stream),
        )
        .await?;
        Ok(serverstream?.try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_correct_cert() {
        use rcgen::generate_simple_self_signed;
        use ring::digest;
        let _ = env_logger::builder().is_test(true).try_init();
        let subject_alt_names = vec!["hello.world.example".to_string(), "localhost".to_string()];

        let gencert1 = generate_simple_self_signed(subject_alt_names.clone()).unwrap();
        let cert1 = tokio_rustls::rustls::Certificate(gencert1.serialize_der().unwrap());
        let key1 = tokio_rustls::rustls::PrivateKey(gencert1.serialize_private_key_der());
        let hash1 = digest::digest(&digest::SHA256, &cert1.0).as_ref().to_vec();
        let gencert2 = generate_simple_self_signed(subject_alt_names.clone()).unwrap();
        let cert2 = tokio_rustls::rustls::Certificate(gencert2.serialize_der().unwrap());
        let key2 = tokio_rustls::rustls::PrivateKey(gencert2.serialize_private_key_der());
        let hash2 = digest::digest(&digest::SHA256, &cert2.0).as_ref().to_vec();

        let (client, server) = tokio::io::duplex(8192);
        let v1 = tokio::spawn(verify_connection(client, cert1, key1, hash2, true));
        let v2 = tokio::spawn(verify_connection(server, cert2, key2, hash1, false));
        let (r1, r2) = tokio::join!(v1, v2);
        r1.unwrap().unwrap();
        r2.unwrap().unwrap();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_incorrect_client_cert() {
        use rcgen::generate_simple_self_signed;
        use ring::digest;
        let _ = env_logger::builder().is_test(true).try_init();
        let subject_alt_names = vec!["hello.world.example".to_string(), "localhost".to_string()];

        let gencert1 = generate_simple_self_signed(subject_alt_names.clone()).unwrap();
        let cert1 = tokio_rustls::rustls::Certificate(gencert1.serialize_der().unwrap());
        let key1 = tokio_rustls::rustls::PrivateKey(gencert1.serialize_private_key_der());
        let gencert2 = generate_simple_self_signed(subject_alt_names.clone()).unwrap();
        let cert2 = tokio_rustls::rustls::Certificate(gencert2.serialize_der().unwrap());
        let key2 = tokio_rustls::rustls::PrivateKey(gencert2.serialize_private_key_der());
        let hash2 = digest::digest(&digest::SHA256, &cert2.0).as_ref().to_vec();

        let (client, server) = tokio::io::duplex(8192);
        let v1 = tokio::spawn(verify_connection(client, cert1, key1, hash2.clone(), true));
        let v2 = tokio::spawn(verify_connection(server, cert2, key2, hash2, false));
        let (_, r2) = tokio::join!(v1, v2);
        assert!(r2.unwrap().is_err());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_incorrect_server_cert() {
        use rcgen::generate_simple_self_signed;
        use ring::digest;
        let _ = env_logger::builder().is_test(true).try_init();
        let subject_alt_names = vec!["hello.world.example".to_string(), "localhost".to_string()];

        let gencert1 = generate_simple_self_signed(subject_alt_names.clone()).unwrap();
        let cert1 = tokio_rustls::rustls::Certificate(gencert1.serialize_der().unwrap());
        let key1 = tokio_rustls::rustls::PrivateKey(gencert1.serialize_private_key_der());
        let hash1 = digest::digest(&digest::SHA256, &cert1.0).as_ref().to_vec();
        let gencert2 = generate_simple_self_signed(subject_alt_names.clone()).unwrap();
        let cert2 = tokio_rustls::rustls::Certificate(gencert2.serialize_der().unwrap());
        let key2 = tokio_rustls::rustls::PrivateKey(gencert2.serialize_private_key_der());

        let (client, server) = tokio::io::duplex(8192);
        let v1 = tokio::spawn(verify_connection(client, cert1, key1, hash1.clone(), true));
        let v2 = tokio::spawn(verify_connection(server, cert2, key2, hash1, false));
        let (r1, _) = tokio::join!(v1, v2);
        assert!(r1.unwrap().is_err());
    }
}
