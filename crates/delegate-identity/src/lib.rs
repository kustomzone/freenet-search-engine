//! Identity delegate for the Freenet search engine.
//!
//! Provides ed25519 key generation, signing, and signature verification for
//! contributor identity management. Secret keys never leave the local node.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use freenet_stdlib::prelude::*;

pub struct IdentityDelegate;

/// Generate a new ed25519 keypair. Returns (secret_key_bytes, public_key_bytes).
pub fn generate_keypair() -> ([u8; 32], [u8; 32]) {
    let signing_key = SigningKey::generate(&mut rand::rngs::OsRng);
    let verifying_key = signing_key.verifying_key();
    (signing_key.to_bytes(), verifying_key.to_bytes())
}

/// Sign data with a secret key. Returns a 64-byte signature.
pub fn sign_data(secret_key: &[u8; 32], data: &[u8]) -> [u8; 64] {
    let signing_key = SigningKey::from_bytes(secret_key);
    let signature = signing_key.sign(data);
    signature.to_bytes()
}

/// Verify a signature against a public key and data.
pub fn verify_signature(public_key: &[u8; 32], data: &[u8], signature: &[u8; 64]) -> bool {
    let Ok(verifying_key) = VerifyingKey::from_bytes(public_key) else {
        return false;
    };
    let signature = Signature::from_bytes(signature);
    verifying_key.verify(data, &signature).is_ok()
}

#[delegate]
impl DelegateInterface for IdentityDelegate {
    fn process(
        _ctx: &mut DelegateCtx,
        _parameters: Parameters<'static>,
        _attested: Option<&'static [u8]>,
        _message: InboundDelegateMsg,
    ) -> Result<Vec<OutboundDelegateMsg>, DelegateError> {
        Ok(vec![])
    }
}
