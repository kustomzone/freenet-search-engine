use search_common::hashing::*;

#[test]
fn metadata_hash_deterministic() {
    let h1 = metadata_hash("title", "desc", "snippet");
    let h2 = metadata_hash("title", "desc", "snippet");
    assert_eq!(h1, h2);
}

#[test]
fn metadata_hash_different_title() {
    let h1 = metadata_hash("title1", "desc", "snippet");
    let h2 = metadata_hash("title2", "desc", "snippet");
    assert_ne!(h1, h2);
}

#[test]
fn metadata_hash_different_description() {
    let h1 = metadata_hash("title", "desc1", "snippet");
    let h2 = metadata_hash("title", "desc2", "snippet");
    assert_ne!(h1, h2);
}

#[test]
fn metadata_hash_different_snippet() {
    let h1 = metadata_hash("title", "desc", "snippet1");
    let h2 = metadata_hash("title", "desc", "snippet2");
    assert_ne!(h1, h2);
}

#[test]
fn metadata_hash_separator_matters() {
    // "a\0b" with desc "c" vs "a" with desc "\0b\0c" should differ
    let h1 = metadata_hash("a\0b", "c", "d");
    let h2 = metadata_hash("a", "b\0c", "d");
    assert_ne!(h1, h2);
}

#[test]
fn shard_for_word_deterministic() {
    let s1 = shard_for_word("hello", 16);
    let s2 = shard_for_word("hello", 16);
    assert_eq!(s1, s2);
}

#[test]
fn shard_for_word_in_range() {
    for count in [1, 2, 4, 8, 16, 32, 255] {
        let shard = shard_for_word("test", count);
        assert!(shard < count, "shard {} >= count {}", shard, count);
    }
}

#[test]
fn shard_for_word_distribution() {
    let shard_count: u8 = 16;
    let mut shard_hits = vec![0u32; shard_count as usize];
    for i in 0..1000 {
        let word = format!("word{}", i);
        let shard = shard_for_word(&word, shard_count);
        shard_hits[shard as usize] += 1;
    }
    // Every shard should get at least some hits (not degenerate)
    for (i, &hits) in shard_hits.iter().enumerate() {
        assert!(hits > 0, "shard {} got zero hits out of 1000 words", i);
    }
}
