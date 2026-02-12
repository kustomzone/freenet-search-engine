use unicode_normalization::UnicodeNormalization;

/// Normalize text: trim, collapse whitespace (U+0020 only), Unicode NFC normalization, BOM removal.
pub fn normalize_text(text: &str) -> String {
    let no_bom = text.replace('\u{FEFF}', "");
    let nfc: String = no_bom.nfc().collect();
    let mut result = String::with_capacity(nfc.len());
    let mut prev_space = false;
    for c in nfc.chars() {
        if c.is_whitespace() {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    result.trim().to_string()
}

fn truncate_to_chars(s: &str, max_chars: usize) -> &str {
    if s.chars().count() <= max_chars {
        return s;
    }
    let byte_idx = s
        .char_indices()
        .nth(max_chars)
        .map(|(idx, _)| idx)
        .unwrap_or(s.len());
    &s[..byte_idx]
}

/// Canonical title: normalize_text + truncate to 256 chars at char boundary.
pub fn canonical_title(title: &str) -> String {
    let normalized = normalize_text(title);
    truncate_to_chars(&normalized, 256).to_string()
}

/// Canonical description: normalize_text + truncate to 1024 chars at char boundary.
pub fn canonical_description(description: &str) -> String {
    let normalized = normalize_text(description);
    truncate_to_chars(&normalized, 1024).to_string()
}

/// Canonical snippet: normalize_text + truncate to given max chars at char boundary.
pub fn canonical_snippet(snippet: &str, max_chars: usize) -> String {
    let normalized = normalize_text(snippet);
    truncate_to_chars(&normalized, max_chars).to_string()
}
