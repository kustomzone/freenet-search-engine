use std::collections::BTreeMap;
use std::io::Cursor;

/// Build a tar archive containing a single file.
pub fn make_tar(filename: &str, content: &[u8]) -> Vec<u8> {
    let mut tar = Vec::new();
    // 512-byte header
    let mut header = [0u8; 512];
    // name field (0..100)
    let name_bytes = filename.as_bytes();
    header[..name_bytes.len()].copy_from_slice(name_bytes);
    // mode (100..108) = "0000644\0"
    header[100..108].copy_from_slice(b"0000644\0");
    // uid (108..116) = "0000000\0"
    header[108..116].copy_from_slice(b"0000000\0");
    // gid (116..124) = "0000000\0"
    header[116..124].copy_from_slice(b"0000000\0");
    // size (124..136) = octal string
    let size_str = format!("{:011o}\0", content.len());
    header[124..136].copy_from_slice(size_str.as_bytes());
    // mtime (136..148) = "00000000000\0"
    header[136..148].copy_from_slice(b"00000000000\0");
    // type flag (156) = '0' (regular file)
    header[156] = b'0';
    // magic (257..263) = "ustar\0"
    header[257..263].copy_from_slice(b"ustar\0");
    // version (263..265) = "00"
    header[263..265].copy_from_slice(b"00");
    // checksum (148..156) - compute
    // First fill with spaces for checksum calculation
    header[148..156].copy_from_slice(b"        ");
    let cksum: u32 = header.iter().map(|&b| b as u32).sum();
    let cksum_str = format!("{:06o}\0 ", cksum);
    header[148..156].copy_from_slice(cksum_str.as_bytes());

    tar.extend_from_slice(&header);
    tar.extend_from_slice(content);
    // Pad to 512-byte boundary
    let padding = (512 - (content.len() % 512)) % 512;
    tar.extend(std::iter::repeat_n(0u8, padding));
    // End-of-archive marker (two 512-byte blocks of zeros)
    tar.extend(std::iter::repeat_n(0u8, 1024));
    tar
}

/// Build a valid web container with the given HTML content.
/// Format: [metadata_size: u64 BE][metadata: CBOR][web_size: u64 BE][tar.xz data]
#[allow(dead_code)]
pub fn make_web_container(html: &str) -> Vec<u8> {
    make_web_container_with_metadata(html, 1)
}

/// Build a valid web container with custom version metadata.
pub fn make_web_container_with_metadata(html: &str, version: u64) -> Vec<u8> {
    let tar_data = make_tar("index.html", html.as_bytes());

    // XZ compress
    let mut compressed = Vec::new();
    lzma_rs::xz_compress(&mut Cursor::new(&tar_data), &mut compressed).unwrap();

    // CBOR metadata with version key
    let metadata_map: BTreeMap<String, u64> =
        [("version".to_string(), version)].into_iter().collect();
    let mut metadata = Vec::new();
    ciborium::ser::into_writer(&metadata_map, &mut metadata).unwrap();

    let mut result = Vec::new();
    result.extend_from_slice(&(metadata.len() as u64).to_be_bytes());
    result.extend_from_slice(&metadata);
    result.extend_from_slice(&(compressed.len() as u64).to_be_bytes());
    result.extend_from_slice(&compressed);
    result
}
