use search_common::{extraction, hashing, normalization, web_container};

/// Extracted metadata from a web container state.
pub struct ExtractedMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub snippet: String,
    pub mini_snippet: String,
    pub metadata_hash: [u8; 32],
}

/// Full extraction pipeline: state bytes -> extracted metadata.
///
/// Decompresses the web container, finds index.html, and extracts
/// title, description, snippet, mini_snippet, and metadata_hash.
pub fn extract_metadata(state: &[u8]) -> Option<ExtractedMetadata> {
    let tar_data = web_container::decompress_web_container(state)?;
    let html = web_container::find_file_in_tar(&tar_data, "index.html")?;

    let title =
        extraction::extract_title_from_html(&html).map(|t| normalization::canonical_title(&t));
    let description = extraction::extract_description_from_html(&html)
        .map(|d| normalization::canonical_description(&d));
    let snippet = normalization::canonical_snippet(&extraction::extract_snippet(&html, 2000), 2000);
    let mini_snippet =
        normalization::canonical_snippet(&extraction::extract_mini_snippet(&html, 300), 300);

    let metadata_hash = hashing::metadata_hash(
        title.as_deref().unwrap_or(""),
        description.as_deref().unwrap_or(""),
        &snippet,
    );

    Some(ExtractedMetadata {
        title,
        description,
        snippet,
        mini_snippet,
        metadata_hash,
    })
}
