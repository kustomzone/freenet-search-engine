use search_common::tokenization::*;

#[test]
fn tokenize_splits_words() {
    let tokens = tokenize("hello world");
    assert_eq!(tokens, vec!["hello", "world"]);
}

#[test]
fn tokenize_lowercases() {
    let tokens = tokenize("Hello WORLD");
    assert_eq!(tokens, vec!["hello", "world"]);
}

#[test]
fn tokenize_removes_punctuation() {
    let tokens = tokenize("hello, world!");
    assert_eq!(tokens, vec!["hello", "world"]);
}

#[test]
fn tokenize_removes_stop_words() {
    let tokens = tokenize("the quick brown fox");
    assert_eq!(tokens, vec!["quick", "brown", "fox"]);
}

#[test]
fn tokenize_strips_accents() {
    let tokens = tokenize("caf\u{00E9} r\u{00E9}sum\u{00E9}");
    assert_eq!(tokens, vec!["cafe", "resume"]);
}

#[test]
fn tokenize_handles_unicode() {
    // Chinese characters should be preserved (not stripped)
    let tokens = tokenize("\u{4F60}\u{597D}");
    assert!(!tokens.is_empty());
}

#[test]
fn tokenize_empty_string() {
    let tokens = tokenize("");
    assert!(tokens.is_empty());
}

#[test]
fn tokenize_only_stop_words() {
    let tokens = tokenize("the and or");
    assert!(tokens.is_empty());
}

#[test]
fn is_stop_word_true() {
    assert!(is_stop_word("the"));
}

#[test]
fn is_stop_word_false() {
    assert!(!is_stop_word("hello"));
}
