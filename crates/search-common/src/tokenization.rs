use unicode_normalization::UnicodeNormalization;

/// English stop words list.
pub const STOP_WORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "if", "in", "into", "is", "it",
    "no", "not", "of", "on", "or", "such", "that", "the", "their", "then", "there", "these",
    "they", "this", "to", "was", "will", "with",
];

/// Tokenize text into words: split on non-alphanumeric, lowercase, strip accents, remove stop words.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(normalize_token)
        .filter(|s| !s.is_empty())
        .filter(|s| !is_stop_word(s))
        .collect()
}

/// Normalize a single token: lowercase, strip accents.
pub fn normalize_token(token: &str) -> String {
    let lower = token.to_lowercase();
    strip_accents(&lower)
}

/// Check if a word is a stop word.
pub fn is_stop_word(word: &str) -> bool {
    STOP_WORDS.contains(&word)
}

fn strip_accents(s: &str) -> String {
    s.nfd().filter(|c| !is_combining_mark(*c)).collect()
}

fn is_combining_mark(c: char) -> bool {
    matches!(c as u32,
        0x0300..=0x036F |
        0x0483..=0x0489 |
        0x0591..=0x05BD |
        0x05BF           |
        0x05C1..=0x05C2 |
        0x05C4..=0x05C5 |
        0x05C7           |
        0x0610..=0x061A |
        0x064B..=0x065F |
        0x0670           |
        0x06D6..=0x06DC |
        0x06DF..=0x06E4 |
        0x06E7..=0x06E8 |
        0x06EA..=0x06ED |
        0x0711           |
        0x0730..=0x074A |
        0x07A6..=0x07B0 |
        0x0816..=0x0819 |
        0x081B..=0x0823 |
        0x0825..=0x0827 |
        0x0829..=0x082D |
        0xFE20..=0xFE2F
    )
}
