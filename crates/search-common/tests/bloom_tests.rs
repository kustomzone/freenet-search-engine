use search_common::bloom::BloomFilter;

#[test]
fn bloom_insert_contains() {
    let mut bf = BloomFilter::new(1024);
    bf.insert(b"hello");
    assert!(bf.contains(b"hello"));
}

#[test]
fn bloom_not_inserted() {
    let bf = BloomFilter::new(1024);
    // Very unlikely to be a false positive on an empty filter
    assert!(!bf.contains(b"this-was-never-inserted"));
}

#[test]
fn bloom_no_false_negatives() {
    let mut bf = BloomFilter::new(8192);
    let items: Vec<Vec<u8>> = (0..100)
        .map(|i| format!("item-{}", i).into_bytes())
        .collect();
    for item in &items {
        bf.insert(item);
    }
    for item in &items {
        assert!(
            bf.contains(item),
            "False negative for {:?}",
            String::from_utf8_lossy(item)
        );
    }
}

#[test]
fn bloom_false_positive_rate() {
    let mut bf = BloomFilter::new(8192);
    // Insert 100 items
    for i in 0..100 {
        bf.insert(format!("inserted-{}", i).as_bytes());
    }
    // Test 1000 other items
    let mut false_positives = 0;
    for i in 0..1000 {
        if bf.contains(format!("not-inserted-{}", i).as_bytes()) {
            false_positives += 1;
        }
    }
    // With 8192 bits, k=7, n=100: expected FP rate is very low
    assert!(
        false_positives < 50,
        "False positive rate too high: {}/1000",
        false_positives
    );
}

#[test]
fn bloom_serialization_roundtrip() {
    let mut bf = BloomFilter::new(2048);
    bf.insert(b"alpha");
    bf.insert(b"beta");
    bf.insert(b"gamma");

    let bytes = bf.to_bytes();
    let bf2 = BloomFilter::from_bytes(&bytes).expect("deserialization failed");

    assert!(bf2.contains(b"alpha"));
    assert!(bf2.contains(b"beta"));
    assert!(bf2.contains(b"gamma"));
    assert_eq!(bf, bf2);
}

#[test]
fn bloom_empty() {
    let bf = BloomFilter::new(512);
    assert!(!bf.contains(b"anything"));
    assert!(!bf.contains(b"at all"));
}
