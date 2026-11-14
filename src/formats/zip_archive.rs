use miniz_oxide::deflate::compress_to_vec;
use miniz_oxide::inflate::decompress_to_vec;
use std::collections::HashMap;

/// Pure-Rust ZIP archive reader & writer for Open Packaging Conventions (`.vsdx`, `.vssx`).
pub struct ZipArchive {
    pub files: HashMap<String, Vec<u8>>,
}

impl ZipArchive {
    pub fn new() -> Self {
        Self {
            files: HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: &str, data: Vec<u8>) {
        let normalized = name.trim_start_matches('/').replace('\\', "/");
        self.files.insert(normalized, data);
    }

    pub fn get_text(&self, name: &str) -> Option<String> {
        let normalized = name.trim_start_matches('/').replace('\\', "/");
        self.files
            .get(&normalized)
            .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
    }

    /// Parses a ZIP archive from raw bytes (supports Stored method 0 and Deflate method 8,
    /// using the Central Directory when present or falling back to Local File Headers).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 22 {
            return Err("ZIP-tiedosto on liian lyhyt".to_string());
        }

        let mut files = HashMap::new();

        // Try locating End of Central Directory (EOCD: 0x06054b50) from the end
        let mut eocd_pos = None;
        let search_start = bytes.len().saturating_sub(65557);
        for i in (search_start..=bytes.len() - 22).rev() {
            if bytes[i..i + 4] == [0x50, 0x4b, 0x05, 0x06] {
                eocd_pos = Some(i);
                break;
            }
        }

        if let Some(eocd) = eocd_pos {
            let total_entries = u16::from_le_bytes([bytes[eocd + 10], bytes[eocd + 11]]) as usize;
            let cd_offset = u32::from_le_bytes([
                bytes[eocd + 16],
                bytes[eocd + 17],
                bytes[eocd + 18],
                bytes[eocd + 19],
            ]) as usize;

            let mut pos = cd_offset;
            for _ in 0..total_entries {
                if pos + 46 > bytes.len() || bytes[pos..pos + 4] != [0x50, 0x4b, 0x01, 0x02] {
                    break;
                }
                let method = u16::from_le_bytes([bytes[pos + 10], bytes[pos + 11]]);
                let comp_size = u32::from_le_bytes([
                    bytes[pos + 20],
                    bytes[pos + 21],
                    bytes[pos + 22],
                    bytes[pos + 23],
                ]) as usize;
                let name_len = u16::from_le_bytes([bytes[pos + 28], bytes[pos + 29]]) as usize;
                let extra_len = u16::from_le_bytes([bytes[pos + 30], bytes[pos + 31]]) as usize;
                let comment_len = u16::from_le_bytes([bytes[pos + 32], bytes[pos + 33]]) as usize;
                let local_offset = u32::from_le_bytes([
                    bytes[pos + 42],
                    bytes[pos + 43],
                    bytes[pos + 44],
                    bytes[pos + 45],
                ]) as usize;

                if pos + 46 + name_len > bytes.len() {
                    break;
                }
                let raw_name = String::from_utf8_lossy(&bytes[pos + 46..pos + 46 + name_len])
                    .trim_start_matches('/')
                    .replace('\\', "/");

                if local_offset + 30 <= bytes.len()
                    && bytes[local_offset..local_offset + 4] == [0x50, 0x4b, 0x03, 0x04]
                {
                    let l_name_len = u16::from_le_bytes([
                        bytes[local_offset + 26],
                        bytes[local_offset + 27],
                    ]) as usize;
                    let l_extra_len = u16::from_le_bytes([
                        bytes[local_offset + 28],
                        bytes[local_offset + 29],
                    ]) as usize;
                    let data_start = local_offset + 30 + l_name_len + l_extra_len;
                    let data_end = data_start + comp_size;

                    if data_end <= bytes.len() && !raw_name.ends_with('/') {
                        let slice = &bytes[data_start..data_end];
                        let decompressed = match method {
                            0 => slice.to_vec(),
                            8 => decompress_to_vec(slice)
                                .map_err(|e| format!("Deflate-purkuvirhe ({}): {:?}", raw_name, e))?,
                            _ => continue,
                        };
                        files.insert(raw_name, decompressed);
                    }
                }

                pos += 46 + name_len + extra_len + comment_len;
            }
        }

        if files.is_empty() {
            return Err("ZIP-arkistosta ei löytynyt tiedostoja".to_string());
        }

        Ok(Self { files })
    }

    /// Serializes all files into a valid ZIP archive using Deflate compression.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central_dir = Vec::new();

        let mut sorted_names: Vec<&String> = self.files.keys().collect();
        sorted_names.sort();

        for name in &sorted_names {
            let raw_data = &self.files[*name];
            let crc = crc32fast::hash(raw_data);
            let compressed = compress_to_vec(raw_data, 6);
            let name_bytes = name.as_bytes();
            let local_offset = out.len() as u32;

            // Local File Header (0x04034b50)
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            out.extend_from_slice(&20u16.to_le_bytes()); // version needed: 2.0
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&8u16.to_le_bytes()); // method: 8 (Deflate)
            out.extend_from_slice(&0u16.to_le_bytes()); // mod time
            out.extend_from_slice(&0u16.to_le_bytes()); // mod date
            out.extend_from_slice(&crc.to_le_bytes());
            out.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            out.extend_from_slice(&(raw_data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name_bytes);
            out.extend_from_slice(&compressed);

            // Central Directory Header (0x02014b50)
            central_dir.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
            central_dir.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central_dir.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central_dir.extend_from_slice(&0u16.to_le_bytes()); // flags
            central_dir.extend_from_slice(&8u16.to_le_bytes()); // method: Deflate
            central_dir.extend_from_slice(&0u16.to_le_bytes()); // mod time
            central_dir.extend_from_slice(&0u16.to_le_bytes()); // mod date
            central_dir.extend_from_slice(&crc.to_le_bytes());
            central_dir.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
            central_dir.extend_from_slice(&(raw_data.len() as u32).to_le_bytes());
            central_dir.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
            central_dir.extend_from_slice(&0u16.to_le_bytes()); // extra len
            central_dir.extend_from_slice(&0u16.to_le_bytes()); // comment len
            central_dir.extend_from_slice(&0u16.to_le_bytes()); // disk number
            central_dir.extend_from_slice(&0u16.to_le_bytes()); // internal attrs
            central_dir.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            central_dir.extend_from_slice(&local_offset.to_le_bytes());
            central_dir.extend_from_slice(name_bytes);
        }

        let cd_offset = out.len() as u32;
        let cd_size = central_dir.len() as u32;
        let count = sorted_names.len() as u16;
        out.extend_from_slice(&central_dir);

        // End of Central Directory Record (0x06054b50)
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        out.extend_from_slice(&0u16.to_le_bytes()); // disk number
        out.extend_from_slice(&0u16.to_le_bytes()); // disk with CD
        out.extend_from_slice(&count.to_le_bytes()); // entries on disk
        out.extend_from_slice(&count.to_le_bytes()); // total entries
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_offset.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes()); // comment len

        out
    }
}
