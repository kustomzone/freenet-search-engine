use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const K: usize = 7;

/// A bloom filter with k=7 hash functions for StateSummary.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BloomFilter {
    bits: Vec<u8>,
    num_bits: usize,
}

impl BloomFilter {
    /// Create a new bloom filter with the given number of bits.
    pub fn new(num_bits: usize) -> Self {
        let num_bytes = num_bits.div_ceil(8);
        Self {
            bits: vec![0u8; num_bytes],
            num_bits,
        }
    }

    /// Insert an item into the bloom filter.
    pub fn insert(&mut self, item: &[u8]) {
        for i in 0..K {
            let pos = self.hash_position(item, i as u8);
            let byte_idx = pos / 8;
            let bit_idx = pos % 8;
            if byte_idx < self.bits.len() {
                self.bits[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    /// Check if an item might be in the bloom filter.
    pub fn contains(&self, item: &[u8]) -> bool {
        for i in 0..K {
            let pos = self.hash_position(item, i as u8);
            let byte_idx = pos / 8;
            let bit_idx = pos % 8;
            if byte_idx >= self.bits.len() || (self.bits[byte_idx] & (1 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(self, &mut buf).expect("CBOR serialization failed");
        buf
    }

    /// Deserialize from bytes.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        ciborium::de::from_reader(bytes).ok()
    }

    fn hash_position(&self, item: &[u8], prefix: u8) -> usize {
        let mut hasher = Sha256::new();
        hasher.update([prefix]);
        hasher.update(item);
        let hash: [u8; 32] = hasher.finalize().into();
        let val = u64::from_be_bytes([
            hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7],
        ]);
        (val % self.num_bits as u64) as usize
    }
}
