use std::io::{Cursor, Write};

const MAX_DECOMPRESS_BYTES: usize = 30 * 1024 * 1024;

/// Check if state bytes look like a valid web container.
/// Format: [metadata_size: u64 BE][metadata: CBOR][web_size: u64 BE][tar.xz data]
pub fn detect_web_container(state: &[u8]) -> bool {
    if state.len() < 16 {
        return false;
    }
    let metadata_size = u64::from_be_bytes(match state[..8].try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    });
    if metadata_size == 0 || metadata_size > 1024 {
        return false;
    }
    let web_offset = 8 + metadata_size as usize;
    if state.len() < web_offset + 8 {
        return false;
    }
    let web_size = u64::from_be_bytes(match state[web_offset..web_offset + 8].try_into() {
        Ok(arr) => arr,
        Err(_) => return false,
    });
    if web_size == 0 || web_size > 100 * 1024 * 1024 {
        return false;
    }
    let expected_total = 8 + metadata_size as usize + 8 + web_size as usize;
    expected_total == state.len()
}

/// Parse web container format and decompress the xz tar archive.
/// Returns the decompressed tar data.
pub fn decompress_web_container(state: &[u8]) -> Option<Vec<u8>> {
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
        Err(_) => None,
    }
}

/// Find a file in tar data by filename suffix and return its content as a string.
pub fn find_file_in_tar(tar_data: &[u8], filename: &str) -> Option<String> {
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
            None => break,
        };
        let padded = match file_size.checked_add(511) {
            Some(v) => v & !511,
            None => break,
        };
        let next_offset = match data_start.checked_add(padded) {
            Some(v) => v,
            None => break,
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
