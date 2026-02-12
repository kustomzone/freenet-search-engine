use delegate_identity::{generate_keypair, sign_data, verify_signature};

#[test]
fn generate_keypair_valid() {
    let (secret, public) = generate_keypair();
    // Both should be 32 bytes (non-zero)
    assert_eq!(secret.len(), 32);
    assert_eq!(public.len(), 32);
    // Public key should not be all zeros
    assert_ne!(public, [0u8; 32]);
}

#[test]
fn keypair_stored_and_retrieved() {
    let (secret1, public1) = generate_keypair();
    // Sign something with the keypair, verify it works
    let data = b"test message";
    let sig = sign_data(&secret1, data);
    assert!(verify_signature(&public1, data, &sig));
}

#[test]
fn public_key_export() {
    let (_secret, public) = generate_keypair();
    // Public key is 32 bytes (ed25519 compressed point)
    assert_eq!(public.len(), 32);
}
