use proptest::prelude::*;
use search_common::{extraction, hashing, normalization};

proptest! {
    #[test]
    fn normalize_is_idempotent(s in ".*") {
        let once = normalization::normalize_text(&s);
        let twice = normalization::normalize_text(&once);
        prop_assert_eq!(once, twice);
    }

    #[test]
    fn metadata_hash_is_deterministic(
        title in "[a-zA-Z0-9 ]{0,100}",
        desc in "[a-zA-Z0-9 ]{0,100}",
        snippet in "[a-zA-Z0-9 ]{0,200}"
    ) {
        let h1 = hashing::metadata_hash(&title, &desc, &snippet);
        let h2 = hashing::metadata_hash(&title, &desc, &snippet);
        prop_assert_eq!(h1, h2);
    }

    #[test]
    fn shard_always_in_range(word in "[a-z]{1,20}", count in 1u8..=255) {
        let shard = hashing::shard_for_word(&word, count);
        prop_assert!(shard < count);
    }

    #[test]
    fn canonical_title_length(s in ".{0,1000}") {
        let result = normalization::canonical_title(&s);
        prop_assert!(result.chars().count() <= 256);
    }

    #[test]
    fn tokenize_no_stop_words(s in "[a-zA-Z ]{0,200}") {
        let tokens = search_common::tokenization::tokenize(&s);
        for t in &tokens {
            prop_assert!(
                !search_common::tokenization::is_stop_word(t),
                "stop word '{}' in tokens", t
            );
        }
    }

    #[test]
    fn extract_title_deterministic(html in "<title>[a-zA-Z0-9 ]{1,50}</title>") {
        let r1 = extraction::extract_title_from_html(&html);
        let r2 = extraction::extract_title_from_html(&html);
        prop_assert_eq!(r1, r2);
    }
}
