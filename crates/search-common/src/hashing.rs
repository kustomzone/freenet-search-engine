use sha2::{Digest, Sha256};

/// Compute metadata hash: sha256(len(title) + title + len(description) + description + len(snippet) + snippet).
/// Uses length-prefixed fields to avoid ambiguity with embedded null bytes.
pub fn metadata_hash(title: &str, description: &str, snippet: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update((title.len() as u64).to_be_bytes());
    hasher.update(title.as_bytes());
    hasher.update((description.len() as u64).to_be_bytes());
    hasher.update(description.as_bytes());
    hasher.update((snippet.len() as u64).to_be_bytes());
    hasher.update(snippet.as_bytes());
    hasher.finalize().into()
}

/// Determine which shard a word belongs to: sha256(word) % shard_count.
pub fn shard_for_word(word: &str, shard_count: u8) -> u8 {
    let mut hasher = Sha256::new();
    hasher.update(word.as_bytes());
    let hash: [u8; 32] = hasher.finalize().into();
    let val = u32::from_be_bytes([hash[0], hash[1], hash[2], hash[3]]);
    (val % shard_count as u32) as u8
}
