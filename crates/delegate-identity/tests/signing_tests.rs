use delegate_identity::{generate_keypair, sign_data, verify_signature};

#[test]
fn sign_and_verify() {
    let (secret, public) = generate_keypair();
    let data = b"hello freenet";
    let signature = sign_data(&secret, data);
    assert!(verify_signature(&public, data, &signature));
}

#[test]
fn verify_invalid_signature() {
    let (secret, public) = generate_keypair();
    let data = b"original message";
    let mut signature = sign_data(&secret, data);
    // Tamper with the signature
    signature[0] ^= 0xFF;
    assert!(!verify_signature(&public, data, &signature));
}

#[test]
fn different_message_different_sig() {
    let (secret, _public) = generate_keypair();
    let sig1 = sign_data(&secret, b"message one");
    let sig2 = sign_data(&secret, b"message two");
    assert_ne!(sig1, sig2);
}
