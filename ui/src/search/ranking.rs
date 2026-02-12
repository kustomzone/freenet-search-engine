use crate::state::SearchResult;
use search_common::tokenization;
use std::collections::HashSet;

/// Sort results by combined_score descending.
pub fn rank_results(results: &mut [SearchResult]) {
    results.sort_by(|a, b| b.combined_score.cmp(&a.combined_score));
}

/// Wrap matching words in `<mark>` tags, truncate to ~300 chars around first match.
pub fn highlight_snippet(snippet: &str, terms: &[String]) -> String {
    if snippet.is_empty() || terms.is_empty() {
        return snippet.to_string();
    }

    let term_set: HashSet<&str> = terms.iter().map(|s| s.as_str()).collect();

    let mut result = String::with_capacity(snippet.len() + terms.len() * 13);
    let mut first_match_pos: Option<usize> = None;
    let mut word_start: Option<usize> = None;

    let chars: Vec<char> = snippet.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_alphanumeric() {
            if word_start.is_none() {
                word_start = Some(i);
            }
        } else if let Some(start) = word_start {
            let word: String = chars[start..i].iter().collect();
            let normalized = tokenization::normalize_token(&word);
            if term_set.contains(normalized.as_str()) {
                if first_match_pos.is_none() {
                    first_match_pos = Some(result.len());
                }
                result.push_str("<mark>");
                result.push_str(&word);
                result.push_str("</mark>");
            } else {
                result.push_str(&word);
            }
            result.push(ch);
            word_start = None;
        } else {
            result.push(ch);
        }
    }

    // Handle trailing word
    if let Some(start) = word_start {
        let word: String = chars[start..].iter().collect();
        let normalized = tokenization::normalize_token(&word);
        if term_set.contains(normalized.as_str()) {
            if first_match_pos.is_none() {
                first_match_pos = Some(result.len());
            }
            result.push_str("<mark>");
            result.push_str(&word);
            result.push_str("</mark>");
        } else {
            result.push_str(&word);
        }
    }

    // Truncate to ~300 chars centered around first match
    if result.len() > 300 {
        truncate_around_match(&result, first_match_pos.unwrap_or(0), 300)
    } else {
        result
    }
}

fn truncate_around_match(text: &str, match_byte: usize, max_len: usize) -> String {
    let text_len = text.len();
    if text_len <= max_len {
        return text.to_string();
    }

    let half = max_len / 2;

    // Scale match position in highlighted text proportionally
    let (start, end) = if match_byte <= half {
        (0, max_len.min(text_len))
    } else if match_byte + half >= text_len {
        (text_len.saturating_sub(max_len), text_len)
    } else {
        (match_byte - half, (match_byte + half).min(text_len))
    };

    // Snap to char boundaries
    let start = snap_to_char_boundary(text, start, true);
    let end = snap_to_char_boundary(text, end, false);

    let mut result = String::new();
    if start > 0 {
        result.push_str("...");
    }
    result.push_str(&text[start..end]);
    if end < text_len {
        result.push_str("...");
    }
    result
}

fn snap_to_char_boundary(s: &str, pos: usize, forward: bool) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    if s.is_char_boundary(pos) {
        return pos;
    }
    if forward {
        let mut p = pos;
        while p < s.len() && !s.is_char_boundary(p) {
            p += 1;
        }
        p
    } else {
        let mut p = pos;
        while p > 0 && !s.is_char_boundary(p) {
            p -= 1;
        }
        p
    }
}
