mod fixtures;

use search_common::web_container::*;

#[test]
fn detect_valid_container() {
    let container = fixtures::make_web_container("<html><body>Hello</body></html>");
    assert!(detect_web_container(&container));
}

#[test]
fn detect_too_short() {
    assert!(!detect_web_container(&[0u8; 10]));
}

#[test]
fn detect_bad_metadata_size() {
    // metadata_size = 0 means no metadata, should be invalid
    let mut data = Vec::new();
    data.extend_from_slice(&0u64.to_be_bytes()); // metadata_size = 0
    data.extend_from_slice(&0u64.to_be_bytes()); // web_size = 0
    assert!(!detect_web_container(&data));
}

#[test]
fn detect_huge_metadata() {
    // metadata_size too large (>1024 bytes seems unreasonable for metadata)
    let mut data = Vec::new();
    data.extend_from_slice(&2000u64.to_be_bytes());
    // Not enough actual data to back this size
    data.extend(std::iter::repeat_n(0u8, 100));
    assert!(!detect_web_container(&data));
}

#[test]
fn detect_size_mismatch() {
    // Create a valid container then truncate it
    let container = fixtures::make_web_container("<html>Test</html>");
    let truncated = &container[..container.len() / 2];
    assert!(!detect_web_container(truncated));
}

#[test]
fn decompress_valid() {
    let html = "<html><body>Test Content</body></html>";
    let container = fixtures::make_web_container(html);
    let tar_data = decompress_web_container(&container);
    assert!(tar_data.is_some());
}

#[test]
fn decompress_invalid() {
    assert!(decompress_web_container(b"not a valid container at all").is_none());
}

#[test]
fn find_file_in_tar_found() {
    let html = "<html><body>Found Me</body></html>";
    let container = fixtures::make_web_container(html);
    let tar_data = decompress_web_container(&container).unwrap();
    let content = find_file_in_tar(&tar_data, "index.html");
    assert_eq!(content, Some(html.to_string()));
}

#[test]
fn find_file_in_tar_missing() {
    let html = "<html><body>Test</body></html>";
    let container = fixtures::make_web_container(html);
    let tar_data = decompress_web_container(&container).unwrap();
    let content = find_file_in_tar(&tar_data, "nonexistent.html");
    assert_eq!(content, None);
}

#[test]
fn find_file_handles_long_names() {
    // Create a tar with a filename at the max length for the standard header
    let long_name = format!("{}/index.html", "a".repeat(80));
    let html = "<html><body>Long path</body></html>";
    let tar_data = fixtures::make_tar(&long_name, html.as_bytes());

    // XZ compress it
    let mut compressed = Vec::new();
    lzma_rs::xz_compress(&mut std::io::Cursor::new(&tar_data), &mut compressed).unwrap();

    // Build web container manually
    let metadata_map: std::collections::BTreeMap<String, u64> =
        [("version".to_string(), 1)].into_iter().collect();
    let mut metadata = Vec::new();
    ciborium::ser::into_writer(&metadata_map, &mut metadata).unwrap();

    let mut container = Vec::new();
    container.extend_from_slice(&(metadata.len() as u64).to_be_bytes());
    container.extend_from_slice(&metadata);
    container.extend_from_slice(&(compressed.len() as u64).to_be_bytes());
    container.extend_from_slice(&compressed);

    let decompressed = decompress_web_container(&container).unwrap();
    let content = find_file_in_tar(&decompressed, "index.html");
    assert_eq!(content, Some(html.to_string()));
}
