use anyhow::{Result, anyhow};
use std::path::Path;

/// Family name and file extension extracted from a TrueType/OpenType font.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontMetadata {
    /// Best-effort family name pulled from the `name` table; falls back to
    /// the file stem when no usable record is found.
    pub family_name: String,
    /// Original file extension lowercased and dot-prefixed (e.g. `.ttf`).
    pub extension: String,
}

/// One decoded record from the TrueType `name` table.
struct NameRecord {
    platform_id: u16,
    language_id: u16,
    name_id: u16,
    value: String,
}

/// Read big-endian u16 at `offset` or return None if out of bounds.
fn read_u16_be(buffer: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    if end > buffer.len() {
        return None;
    }
    Some(u16::from_be_bytes([buffer[offset], buffer[offset + 1]]))
}

/// Read big-endian u32 at `offset` or return None if out of bounds.
fn read_u32_be(buffer: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    if end > buffer.len() {
        return None;
    }
    Some(u32::from_be_bytes([
        buffer[offset],
        buffer[offset + 1],
        buffer[offset + 2],
        buffer[offset + 3],
    ]))
}

/// Decode a UTF-16BE byte slice to a String, dropping NULs and trimming.
/// Used for Unicode and Microsoft platform name records.
fn decode_utf16_be(buffer: &[u8]) -> String {
    if buffer.len() < 2 {
        return String::new();
    }
    let mut units = Vec::with_capacity(buffer.len() / 2);
    let mut index = 0;
    while index + 1 < buffer.len() {
        units.push(u16::from_be_bytes([buffer[index], buffer[index + 1]]));
        index += 2;
    }
    String::from_utf16_lossy(&units)
        .replace('\0', "")
        .trim()
        .to_string()
}

/// Decode a single-byte (latin-1) name record, dropping NULs and trimming.
fn decode_ascii(buffer: &[u8]) -> String {
    buffer
        .iter()
        .map(|byte| *byte as char)
        .collect::<String>()
        .replace('\0', "")
        .trim()
        .to_string()
}

/// Choose the best available record for `name_id`, preferring Microsoft
/// English (3, 0x0409), then Unicode (platform 0), then any platform 3,
/// then anything else. Mirrors the TS preference list.
fn pick_best_name(records: &[NameRecord], name_id: u16) -> Option<String> {
    let candidates: Vec<&NameRecord> = records
        .iter()
        .filter(|r| r.name_id == name_id && !r.value.is_empty())
        .collect();
    if candidates.is_empty() {
        return None;
    }

    let preferred = candidates
        .iter()
        .find(|r| r.platform_id == 3 && r.language_id == 0x0409)
        .or_else(|| candidates.iter().find(|r| r.platform_id == 0))
        .or_else(|| candidates.iter().find(|r| r.platform_id == 3))
        .copied()
        .unwrap_or(candidates[0]);

    if preferred.value.is_empty() {
        None
    } else {
        Some(preferred.value.clone())
    }
}

/// Return the lowercase, dot-prefixed extension of `path` (defaulting to `.ttf`).
fn lowercase_extension(path: &Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| format!(".{}", ext.to_lowercase()))
        .unwrap_or_else(|| ".ttf".to_string())
}

/// Return the file stem (filename without extension), or "Unknown" if missing.
fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

/// Parse a font file's `name` table and return its family name plus the
/// original file extension. Best-effort: when the file is too small or the
/// `name` table is malformed, fall back to using the file stem so callers
/// can still embed the font without crashing.
pub async fn extract_font_metadata(font_path: &Path) -> Result<FontMetadata> {
    let buffer = tokio::fs::read(font_path)
        .await
        .map_err(|e| anyhow!("failed to read font file {}: {}", font_path.display(), e))?;

    if buffer.len() < 12 {
        return Err(anyhow!("Invalid font file: {}", font_path.display()));
    }

    let extension = lowercase_extension(font_path);
    let stem = file_stem(font_path);

    let num_tables = match read_u16_be(&buffer, 4) {
        Some(value) => value as usize,
        None => {
            return Ok(FontMetadata {
                family_name: stem,
                extension,
            });
        }
    };

    let mut name_table_offset: i64 = -1;
    let mut name_table_length: u32 = 0;

    for index in 0..num_tables {
        let record_offset = 12 + index * 16;
        if record_offset + 16 > buffer.len() {
            break;
        }
        let tag = &buffer[record_offset..record_offset + 4];
        if tag != b"name" {
            continue;
        }
        name_table_offset = read_u32_be(&buffer, record_offset + 8).unwrap_or(0) as i64;
        name_table_length = read_u32_be(&buffer, record_offset + 12).unwrap_or(0);
        break;
    }

    if name_table_offset < 0
        || (name_table_offset as u64).saturating_add(name_table_length as u64) > buffer.len() as u64
    {
        return Ok(FontMetadata {
            family_name: stem,
            extension,
        });
    }

    let nto = name_table_offset as usize;
    let format = match read_u16_be(&buffer, nto) {
        Some(value) => value,
        None => {
            return Ok(FontMetadata {
                family_name: stem,
                extension,
            });
        }
    };
    if format != 0 && format != 1 {
        return Ok(FontMetadata {
            family_name: stem,
            extension,
        });
    }

    let count = read_u16_be(&buffer, nto + 2).unwrap_or(0) as usize;
    let string_offset = read_u16_be(&buffer, nto + 4).unwrap_or(0) as usize;
    let storage_base = nto + string_offset;
    let mut records: Vec<NameRecord> = Vec::with_capacity(count);

    for index in 0..count {
        let record_offset = nto + 6 + index * 12;
        if record_offset + 12 > buffer.len() {
            break;
        }

        let platform_id = read_u16_be(&buffer, record_offset).unwrap_or(0);
        let language_id = read_u16_be(&buffer, record_offset + 4).unwrap_or(0);
        let name_id = read_u16_be(&buffer, record_offset + 6).unwrap_or(0);
        let length = read_u16_be(&buffer, record_offset + 8).unwrap_or(0) as usize;
        let offset = read_u16_be(&buffer, record_offset + 10).unwrap_or(0) as usize;
        let start = storage_base + offset;
        let end = start + length;

        if length == 0 || end > buffer.len() {
            continue;
        }

        let raw_value = &buffer[start..end];
        let value = if platform_id == 0 || platform_id == 3 {
            decode_utf16_be(raw_value)
        } else {
            decode_ascii(raw_value)
        };
        if value.is_empty() {
            continue;
        }

        records.push(NameRecord {
            platform_id,
            language_id,
            name_id,
            value,
        });
    }

    let family_name = pick_best_name(&records, 1)
        .or_else(|| pick_best_name(&records, 4))
        .unwrap_or(stem);

    Ok(FontMetadata {
        family_name,
        extension,
    })
}
