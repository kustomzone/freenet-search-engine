use freenet_stdlib::prelude::ContractInterface;
use search_common::hashing::shard_for_word;

#[test]
fn routing_consistent() {
    let word = "consistency";
    let s1 = shard_for_word(word, 16);
    let s2 = shard_for_word(word, 16);
    let s3 = shard_for_word(word, 16);
    assert_eq!(s1, s2);
    assert_eq!(s2, s3);
}

#[test]
fn routing_in_range() {
    let words = [
        "hello", "world", "rust", "freenet", "search", "engine", "test",
    ];
    for count in [1, 2, 4, 8, 16, 32, 255] {
        for word in &words {
            let shard = shard_for_word(word, count);
            assert!(
                shard < count,
                "shard_for_word({}, {}) = {} >= {}",
                word,
                count,
                shard,
                count
            );
        }
    }
}

#[test]
fn shard_id_matches_validation() {
    use search_common::types::*;
    use std::collections::BTreeMap;

    let shard_count = 16u8;
    let word = "validate";
    let correct_shard = shard_for_word(word, shard_count);

    // State with correct shard_id should validate
    let mut index = BTreeMap::new();
    index.insert(
        word.to_string(),
        vec![TermEntry {
            contract_key: "c1".to_string(),
            snippet: "s1".to_string(),
            tf_idf_score: 1000,
        }],
    );

    let correct_state = ShardState {
        shard_id: correct_shard,
        index: index.clone(),
    };

    let mut buf = Vec::new();
    ciborium::ser::into_writer(&correct_state, &mut buf).unwrap();

    let result = contract_fulltext_shard::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(buf),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_ok());

    // State with wrong shard_id should fail
    let wrong_shard = (correct_shard + 1) % shard_count;
    let wrong_state = ShardState {
        shard_id: wrong_shard,
        index,
    };

    let mut buf2 = Vec::new();
    ciborium::ser::into_writer(&wrong_state, &mut buf2).unwrap();

    let result = contract_fulltext_shard::Contract::validate_state(
        freenet_stdlib::prelude::Parameters::from(vec![]),
        freenet_stdlib::prelude::State::from(buf2),
        freenet_stdlib::prelude::RelatedContracts::default(),
    );
    assert!(result.is_err());
}
