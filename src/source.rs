//! Source byte decoding.

use crate::error::{fail, ErrorKind, Result, ResultExt};

/// Decode UTF-8 or BOM-marked UTF-16 source, rejecting malformed bytes instead of replacing them.
pub(crate) fn decode_source(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let le = bytes[0] == 0xff;
        let data = &bytes[2..];
        if !data.len().is_multiple_of(2) {
            fail!(Source, "invalid UTF-16 source");
        }
        let units: Vec<u16> = data
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        Ok(String::from_utf16(&units).context(ErrorKind::Source, "invalid UTF-16 source")?)
    } else {
        Ok(
            std::str::from_utf8(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes))
                .context(ErrorKind::Source, "invalid UTF-8 source")?
                .to_owned(),
        )
    }
}
