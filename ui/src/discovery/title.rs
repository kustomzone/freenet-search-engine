use std::io::{Cursor, Write};

use crate::state::{AppEntry, APP_CATALOG};

/// Maximum bytes to decompress from xz archive.
const MAX_DECOMPRESS_BYTES: usize = 30 * 1024 * 1024;

/// Maximum bytes of HTML to inspect for title/description extraction.
const HTML_SNIPPET_LIMIT: usize = 10240;

/// Extract title and description from web container state bytes.
///
/// Web container format:
///   [metadata_size: u64 BE][metadata: CBOR][web_size: u64 BE][web: xz-compressed tar]
pub fn extract_title_from_state(state: &[u8]) -> (Option<String>, Option<String>) {
    let tar_data = match decompress_web_container(state) {
        Some(data) => {
            tracing::info!("xz decompressed {} bytes of tar data", data.len());
            data
        }
        None => {
            tracing::warn!("xz decompression failed or returned empty");
            return (None, None);
        }
    };

    let html = match find_file_in_tar(&tar_data, "index.html") {
        Some(content) => {
            tracing::info!("Found index.html ({} bytes)", content.len());
            content
        }
        None => {
            tracing::warn!("index.html not found in tar ({} bytes)", tar_data.len());
            return (None, None);
        }
    };

    let snippet = truncate_at_char_boundary(&html, HTML_SNIPPET_LIMIT);

    let title = extract_title_from_html(snippet);
    let description = extract_description_from_html(snippet);

    if title.is_none() {
        let preview_end = snippet.len().min(200);
        tracing::warn!(
            "No title found in index.html (first {} chars: {:?})",
            preview_end,
            &snippet[..preview_end]
        );
    }

    (title, description)
}

/// Try multiple strategies to extract a title from HTML.
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

/// Try multiple strategies to extract a description from HTML.
pub fn extract_description_from_html(html: &str) -> Option<String> {
    if let Some(d) = extract_meta_content(html, "description") {
        return Some(d);
    }
    if let Some(d) = extract_meta_property(html, "og:description") {
        return Some(d);
    }
    None
}

/// Extract the version number from web container CBOR metadata.
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

/// Scan CBOR bytes for the text key "version" and parse the following unsigned integer.
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

/// Update or create an APP_CATALOG entry.
pub fn update_catalog_entry(
    key: &str,
    title: Option<&str>,
    description: Option<&str>,
    size: Option<u64>,
    version: Option<u64>,
) {
    let now = js_sys::Date::now() as u64 / 1000;
    let mut catalog = APP_CATALOG.write();
    let entry = catalog.entry(key.to_string()).or_insert_with(|| AppEntry {
        title: None,
        description: None,
        first_seen: now,
        last_seen: now,
        size_bytes: None,
        subscribers: 0,
        version: None,
    });
    if let Some(t) = title {
        entry.title = Some(t.to_string());
    }
    if let Some(d) = description {
        entry.description = Some(d.to_string());
    }
    if let Some(s) = size {
        entry.size_bytes = Some(s);
    }
    if let Some(v) = version {
        entry.version = Some(v);
    }
    entry.last_seen = now;
}

/// Parse web container format and decompress the xz tar archive.
fn decompress_web_container(state: &[u8]) -> Option<Vec<u8>> {
    if state.len() < 16 {
        return None;
    }

    let metadata_size = u64::from_be_bytes(state[..8].try_into().ok()?) as usize;
    if metadata_size == 0 || metadata_size > 1024 {
        return None;
    }

    let web_offset = 8 + metadata_size;
    if state.len() < web_offset + 8 {
        return None;
    }

    let web_size = u64::from_be_bytes(state[web_offset..web_offset + 8].try_into().ok()?) as usize;
    let xz_start = web_offset + 8;
    let xz_end = xz_start + web_size;

    if state.len() < xz_end || web_size == 0 {
        return None;
    }

    let xz_data = &state[xz_start..xz_end];

    if xz_data.len() >= 6 {
        let is_xz = &xz_data[..6] == b"\xfd7zXZ\x00";
        tracing::info!(
            "Compressed data: {} bytes, xz_magic={}, first_bytes={:02x?}",
            xz_data.len(),
            is_xz,
            &xz_data[..xz_data.len().min(12)]
        );
    }

    let mut reader = Cursor::new(xz_data);
    let mut writer = LimitedWriter::new(MAX_DECOMPRESS_BYTES);
    match lzma_rs::xz_decompress(&mut reader, &mut writer) {
        Ok(()) => {
            if writer.buf.is_empty() {
                None
            } else {
                Some(writer.buf)
            }
        }
        Err(e) => {
            tracing::warn!(
                "xz decompression error (got {} bytes before failure): {}",
                writer.buf.len(),
                e
            );
            None
        }
    }
}

/// Find a file in tar data by suffix and return its content as a string.
/// Handles GNU long name entries (type 'L') and pax headers (type 'x').
fn find_file_in_tar(tar_data: &[u8], filename: &str) -> Option<String> {
    let mut offset = 0;
    let mut long_name: Option<String> = None;

    while offset + 512 <= tar_data.len() {
        let header = &tar_data[offset..offset + 512];

        if header.iter().all(|&b| b == 0) {
            break;
        }

        let type_flag = header[156];

        let name_end = header[..100].iter().position(|&b| b == 0).unwrap_or(100);
        let header_name = std::str::from_utf8(&header[..name_end]).unwrap_or("");

        let size_str = std::str::from_utf8(&header[124..136])
            .unwrap_or("0")
            .trim_matches(|c: char| c == '\0' || c == ' ');
        let file_size = usize::from_str_radix(size_str, 8).unwrap_or(0);

        let data_start = offset + 512;
        let data_end = match data_start.checked_add(file_size) {
            Some(end) => end,
            None => break, // overflow
        };
        let padded = match file_size.checked_add(511) {
            Some(v) => v & !511,
            None => break, // overflow
        };
        let next_offset = match data_start.checked_add(padded) {
            Some(v) => v,
            None => break, // overflow
        };

        match type_flag {
            b'L' => {
                if data_end <= tar_data.len() {
                    let name_data = &tar_data[data_start..data_end];
                    long_name = std::str::from_utf8(name_data)
                        .ok()
                        .map(|s| s.trim_end_matches('\0').to_string());
                }
                offset = next_offset;
                continue;
            }
            b'x' | b'g' => {
                offset = next_offset;
                continue;
            }
            b'5' => {
                long_name = None;
                offset = next_offset;
                continue;
            }
            _ => {}
        }

        let name = long_name.as_deref().unwrap_or(header_name);

        if name.ends_with(filename) && data_end <= tar_data.len() {
            let content = &tar_data[data_start..data_end];
            return std::str::from_utf8(content).ok().map(|s| s.to_string());
        }

        long_name = None;
        offset = next_offset;
    }

    None
}

/// Writer that caps output at a byte limit.
struct LimitedWriter {
    buf: Vec<u8>,
    limit: usize,
}

impl LimitedWriter {
    fn new(limit: usize) -> Self {
        Self {
            buf: Vec::with_capacity(limit.min(512 * 1024)),
            limit,
        }
    }
}

impl Write for LimitedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        let remaining = self.limit.saturating_sub(self.buf.len());
        if remaining == 0 {
            return Err(std::io::Error::other("decompression limit reached"));
        }
        let n = data.len().min(remaining);
        self.buf.extend_from_slice(&data[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Truncate a string at a byte limit, respecting UTF-8 char boundaries.
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

/// Extract content between <tag> and </tag> (case-insensitive, first match).
/// Uses the lowercased string for index computation and verifies char boundaries
/// before extracting from the original to preserve casing.
pub fn extract_tag(html: &str, tag: &str) -> Option<String> {
    let lower = html.to_lowercase();
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start_idx = lower.find(&open)?;
    let content_start = lower[start_idx..].find('>')? + start_idx + 1;
    let end_idx = lower[content_start..].find(&close)? + content_start;
    // If lowercasing shifted byte offsets, fall back to the lowercased version
    let content = if html.is_char_boundary(content_start) && html.is_char_boundary(end_idx) {
        html[content_start..end_idx].trim()
    } else {
        lower[content_start..end_idx].trim()
    };
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

/// Extract <meta name="NAME" content="..."> value.
pub fn extract_meta_content(html: &str, name: &str) -> Option<String> {
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

/// Extract <meta property="PROP" content="..."> value (e.g., og:title).
pub fn extract_meta_property(html: &str, property: &str) -> Option<String> {
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
