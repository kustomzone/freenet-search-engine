use search_common::scoring::*;
use search_common::types::Status;

#[test]
fn tfidf_basic() {
    // term appears 5 times in a 100-word doc, 10 docs total, 2 have the term
    let score = integer_tf_idf(5, 100, 10, 2);
    // tf = 5 * 10000 / 100 = 500
    // idf = log2(10 * 10000 / 2) ~ log2(50000) ~ 15.6 -> integer approx
    // We just check it produces a reasonable positive value
    assert!(score > 0);
}

#[test]
fn tfidf_zero_docs_with_term() {
    // Edge case: no docs have the term (shouldn't happen but mustn't panic)
    // Implementations may return 0 or handle gracefully
    let score = integer_tf_idf(1, 10, 10, 0);
    // Just verify it doesn't panic - score could be 0 or some sentinel
    let _ = score;
}

#[test]
fn tfidf_single_term() {
    // One term in a one-word doc, only doc
    let score = integer_tf_idf(1, 1, 1, 1);
    assert!(score > 0);
}

#[test]
fn rank_score_confirmed_bonus() {
    let confirmed = rank_score(100, 5, 10, &Status::Confirmed);
    let pending = rank_score(100, 5, 10, &Status::Pending);
    assert!(
        confirmed > pending,
        "Confirmed should score higher than Pending"
    );
}

#[test]
fn rank_score_disputed_penalty() {
    let disputed = rank_score(100, 5, 10, &Status::Disputed);
    let pending = rank_score(100, 5, 10, &Status::Pending);
    assert!(
        disputed < pending,
        "Disputed should score lower than Pending"
    );
}

#[test]
fn combined_score_formula() {
    // Combined = relevance * 7000 / 10000 + rank * 3000 / 10000
    let score = combined_score(10000, 10000);
    // 10000 * 7000 / 10000 + 10000 * 3000 / 10000 = 7000 + 3000 = 10000
    assert_eq!(score, 10000);
}

#[test]
fn combined_score_overflow_safety() {
    // Large inputs should not cause u32 overflow
    let score = combined_score(u32::MAX / 2, u32::MAX / 2);
    // Just verify no panic
    assert!(score > 0);
}
