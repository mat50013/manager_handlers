use rustls::client::danger::HandshakeSignatureValid;
use rustls::pki_types::{CertificateDer, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName, Error, SignatureScheme};
use std::collections::HashSet;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

/// A custom implementation of the `ClientCertVerifier` trait that validates client certificates
/// against a set of allowed common names (CNs).
///
/// This verifier wraps a default verifier implementation and adds an additional check to ensure
/// that the client certificate's CN is in a pre-approved list of names.
///
/// # Fields
/// * `default_verifier` - The underlying certificate verifier that performs standard TLS verification
/// * `allowed_names_set` - A set of allowed common names (CNs) that are permitted to connect
pub(crate) struct CustomClientCertVerifier {
    pub(crate) default_verifier: Arc<dyn ClientCertVerifier>,
    pub(crate) allowed_names_set: HashSet<String>,
}
/// Implements the `Debug` trait for `CustomClientCertVerifier`.
impl Debug for CustomClientCertVerifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CustomClientCertVerifier").finish()
    }
}
impl ClientCertVerifier for CustomClientCertVerifier {
    /// Returns the distinguished names of root certificate authorities.
    ///
    /// This method provides hints about which root CAs are trusted,
    /// used during the TLS handshake process.
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.default_verifier.root_hint_subjects()
    }
    /// Verifies a client certificate chain and ensures the certificate's Common Name is allowed.
    ///
    /// This method first delegates to the default verifier to perform standard certificate validation,
    /// then extracts the Common Name from the client certificate and checks if it's in the allowed list.
    ///
    /// # Arguments
    /// * `end_entity` - The client's end-entity certificate
    /// * `intermediates` - Any intermediate certificates in the chain
    /// * `now` - The current time used for certificate validity checks
    ///
    /// # Returns
    /// * `Result<ClientCertVerified, Error>` - Success if verification passes, or an error if it fails
    fn verify_client_cert(&self, end_entity: &CertificateDer<'_>, intermediates: &[CertificateDer<'_>], now: UnixTime) -> Result<ClientCertVerified, Error> {
        let verified = self.default_verifier.verify_client_cert(end_entity, intermediates, now)?;

        let (_, cert) = x509_parser::parse_x509_certificate(end_entity.as_ref()).map_err(|_| Error::General("Failed to parse client certificate".into()))?;
        let cn = cert.subject().iter_common_name().next().and_then(|attr| attr.as_str().ok()).unwrap_or("");

        if !self.allowed_names_set.contains(cn) {
            return Err(Error::General("Invalid client certificate subject".into()));
        }

        Ok(verified)
    }
    /// Verifies the digital signature for TLS 1.2 connections.
    ///
    /// Delegates signature verification to the default verifier implementation.
    fn verify_tls12_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.default_verifier.verify_tls12_signature(message, cert, dss)
    }
    /// Verifies the digital signature for TLS 1.3 connections.
    ///
    /// Delegates signature verification to the default verifier implementation.
    fn verify_tls13_signature(&self, message: &[u8], cert: &CertificateDer<'_>, dss: &DigitallySignedStruct) -> Result<HandshakeSignatureValid, Error> {
        self.default_verifier.verify_tls13_signature(message, cert, dss)
    }
    /// Returns the list of supported signature schemes for certificate verification.
    ///
    /// Delegates to the default verifier to provide the standard supported schemes.
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.default_verifier.supported_verify_schemes()
    }
}
