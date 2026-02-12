use search_common::normalization::*;

#[test]
fn normalize_trims() {
    let result = normalize_text("  hello  ");
    assert_eq!(result, "hello");
}

#[test]
fn normalize_collapses_whitespace() {
    let result = normalize_text("hello   world");
    assert_eq!(result, "hello world");
}

#[test]
fn normalize_only_space_char() {
    // Tabs and newlines should become single space
    let result = normalize_text("hello\t\nworld");
    assert_eq!(result, "hello world");
}

#[test]
fn normalize_removes_bom() {
    let result = normalize_text("\u{FEFF}hello");
    assert_eq!(result, "hello");
}

#[test]
fn normalize_nfc() {
    // Decomposed e + combining acute accent -> composed e-acute
    let decomposed = "e\u{0301}"; // e + combining acute
    let result = normalize_text(decomposed);
    assert_eq!(result, "\u{00E9}"); // e-acute NFC form
}

#[test]
fn normalize_idempotent() {
    let input = "  Hello\t\n  World  \u{FEFF}  ";
    let once = normalize_text(input);
    let twice = normalize_text(&once);
    assert_eq!(once, twice);
}

#[test]
fn canonical_title_truncates() {
    let long_title = "A".repeat(300);
    let result = canonical_title(&long_title);
    assert!(result.chars().count() <= 256);
}

#[test]
fn canonical_title_normalizes() {
    let result = canonical_title("  Hello   World  ");
    assert_eq!(result, "Hello World");
}

#[test]
fn canonical_description_truncates() {
    let long_desc = "B".repeat(1100);
    let result = canonical_description(&long_desc);
    assert!(result.chars().count() <= 1024);
}

#[test]
fn canonical_snippet_truncates() {
    let long_snippet = "C".repeat(200);
    let result = canonical_snippet(&long_snippet, 100);
    assert!(result.chars().count() <= 100);
}

#[test]
fn canonical_snippet_respects_char_boundaries() {
    // Use multibyte characters (each e-acute is 2 bytes in UTF-8)
    let multibyte = "\u{00E9}".repeat(200);
    let result = canonical_snippet(&multibyte, 100);
    assert!(result.chars().count() <= 100);
    // Must be valid UTF-8 (would panic if not)
    let _ = result.as_bytes();
}
