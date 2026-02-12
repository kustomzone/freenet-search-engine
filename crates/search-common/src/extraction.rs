use crate::web_container::{decompress_web_container, find_file_in_tar};

const HTML_SNIPPET_LIMIT: usize = 10240;

/// Extract title from HTML using priority: <title> > og:title > application-name > <h1>
pub fn extract_title_from_html(html: &str) -> Option<String> {
    if let Some(t) = extract_tag(html, "title") {
        return Some(t);
    }
    if let Some(t) = extract_meta_property(html, "og:title") {
        return Some(t);
    }
    if let Some(t) = extract_meta_content(html, "application-name") {
        return Some(t);
    }
    if let Some(t) = extract_tag(html, "h1") {
        return Some(t);
    }
    None
}

/// Extract description from HTML: <meta description> > og:description
pub fn extract_description_from_html(html: &str) -> Option<String> {
    if let Some(d) = extract_meta_content(html, "description") {
        return Some(d);
    }
    if let Some(d) = extract_meta_property(html, "og:description") {
        return Some(d);
    }
    None
}

/// Extract visible text snippet from HTML (max chars specified by limit).
/// Strips script, style tags and HTML tags, collapses whitespace.
pub fn extract_snippet(html: &str, max_chars: usize) -> String {
    let mut text = html.to_string();

    // Strip <script>...</script> (case-insensitive)
    loop {
        let lower = text.to_lowercase();
        if let Some(start) = lower.find("<script") {
            if let Some(end_rel) = lower[start..].find("</script>") {
                let end = start + end_rel + "</script>".len();
                text = format!("{}{}", &text[..start], &text[end..]);
                continue;
            }
        }
        break;
    }

    // Strip <style>...</style> (case-insensitive)
    loop {
        let lower = text.to_lowercase();
        if let Some(start) = lower.find("<style") {
            if let Some(end_rel) = lower[start..].find("</style>") {
                let end = start + end_rel + "</style>".len();
                text = format!("{}{}", &text[..start], &text[end..]);
                continue;
            }
        }
        break;
    }

    // Strip all HTML tags
    let text = strip_html_tags(&text);

    // Collapse whitespace
    let mut result = String::with_capacity(text.len());
    let mut prev_space = false;
    for c in text.chars() {
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
    let trimmed = result.trim();

    // Truncate at char boundary
    truncate_at_char_boundary(trimmed, max_chars).to_string()
}

/// Extract a mini snippet (short version for catalog browsing display).
pub fn extract_mini_snippet(html: &str, max_chars: usize) -> String {
    extract_snippet(html, max_chars)
}

/// Extract title and description from web container state bytes.
/// Combines web_container decompression + HTML extraction.
pub fn extract_title_from_state(state: &[u8]) -> (Option<String>, Option<String>) {
    let tar_data = match decompress_web_container(state) {
        Some(data) => data,
        None => return (None, None),
    };

    let html = match find_file_in_tar(&tar_data, "index.html") {
        Some(content) => content,
        None => return (None, None),
    };

    let snippet = truncate_at_char_boundary(&html, HTML_SNIPPET_LIMIT);
    let title = extract_title_from_html(snippet);
    let description = extract_description_from_html(snippet);

    (title, description)
}

/// Extract version number from web container CBOR metadata.
pub fn extract_version_from_state(state: &[u8]) -> Option<u64> {
    if state.len() < 16 {
        return None;
    }
    let metadata_size = u64::from_be_bytes(state[..8].try_into().ok()?) as usize;
    if metadata_size == 0 || metadata_size > 1024 {
        return None;
    }
    let metadata_end = 8 + metadata_size;
    if state.len() < metadata_end {
        return None;
    }
    let metadata = &state[8..metadata_end];
    extract_cbor_version(metadata)
}

fn extract_cbor_version(metadata: &[u8]) -> Option<u64> {
    let marker = b"\x67version";
    let pos = metadata.windows(marker.len()).position(|w| w == marker)?;
    let val_start = pos + marker.len();
    if val_start >= metadata.len() {
        return None;
    }
    parse_cbor_uint(&metadata[val_start..])
}

fn parse_cbor_uint(data: &[u8]) -> Option<u64> {
    let first = *data.first()?;
    match first {
        0x00..=0x17 => Some(first as u64),
        0x18 => data.get(1).map(|&b| b as u64),
        0x19 if data.len() >= 3 => Some(u16::from_be_bytes([data[1], data[2]]) as u64),
        0x1a if data.len() >= 5 => Some(u32::from_be_bytes(data[1..5].try_into().ok()?) as u64),
        0x1b if data.len() >= 9 => Some(u64::from_be_bytes(data[1..9].try_into().ok()?)),
        _ => None,
    }
}

fn extract_tag(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start_idx = lower.find(&open)?;
    let content_start = lower[start_idx..].find('>')? + start_idx + 1;
    let end_idx = lower[content_start..].find(&close)? + content_start;
    let content = if html.is_char_boundary(content_start) && html.is_char_boundary(end_idx) {
        html[content_start..end_idx].trim()
    } else {
        lower[content_start..end_idx].trim()
    };
    if content.is_empty() {
        None
    } else {
        // Strip inner HTML tags from the content
        let stripped = strip_html_tags(content);
        let trimmed = stripped.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    }
}

fn extract_meta_content(html: &str, name: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let needle = format!("name=\"{}\"", name);
    let idx = lower.find(&needle)?;
    let tag_start = lower[..idx].rfind('<')?;
    let tag_end = lower[idx..].find('>')? + idx;
    let tag = &lower[tag_start..=tag_end];
    let content_start = tag.find("content=\"")? + 9;
    let content_end = tag[content_start..].find('"')? + content_start;
    let abs_start = tag_start + content_start;
    let abs_end = tag_start + content_end;
    let value = if html.is_char_boundary(abs_start) && html.is_char_boundary(abs_end) {
        &html[abs_start..abs_end]
    } else {
        &lower[abs_start..abs_end]
    };
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn extract_meta_property(html: &str, property: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let needle = format!("property=\"{}\"", property);
    let idx = lower.find(&needle)?;
    let tag_start = lower[..idx].rfind('<')?;
    let tag_end = lower[idx..].find('>')? + idx;
    let tag = &lower[tag_start..=tag_end];
    let content_start = tag.find("content=\"")? + 9;
    let content_end = tag[content_start..].find('"')? + content_start;
    let abs_start = tag_start + content_start;
    let abs_end = tag_start + content_end;
    let value = if html.is_char_boundary(abs_start) && html.is_char_boundary(abs_end) {
        &html[abs_start..abs_end]
    } else {
        &lower[abs_start..abs_end]
    };
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }
    result
}

fn truncate_at_char_boundary(s: &str, limit: usize) -> &str {
    if s.len() <= limit {
        return s;
    }
    let mut end = limit;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}
