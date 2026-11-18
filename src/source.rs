//! Source byte decoding.

use crate::error::{fail, ErrorKind, Result, ResultExt};

/// Decode UTF-8 or BOM-marked UTF-16 source, rejecting malformed bytes instead of replacing them.
///
/// UTF-8 input takes over `bytes`' buffer, so the common case allocates and
/// copies nothing; a UTF-8 BOM is removed in place.
pub(crate) fn decode_source(mut bytes: Vec<u8>) -> Result<String> {
    #[cfg(test)]
    crate::work::count(crate::work::Unit::Decode);
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
        if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
            bytes.drain(..3);
        }
        Ok(String::from_utf8(bytes)
            .map_err(|error| error.utf8_error())
            .context(ErrorKind::Source, "invalid UTF-8 source")?)
    }
}

#[cfg(test)]
mod tests {
    use super::decode_source;

    #[test]
    fn utf8_is_decoded_and_a_bom_is_removed() {
        assert_eq!(decode_source(b"class A {}".to_vec()).unwrap(), "class A {}");
        assert_eq!(
            decode_source(b"\xef\xbb\xbfclass A {}".to_vec()).unwrap(),
            "class A {}"
        );
        // Only the first BOM is removed; a second one is text.
        assert_eq!(
            decode_source(b"\xef\xbb\xbf\xef\xbb\xbfx".to_vec()).unwrap(),
            "\u{feff}x"
        );
        assert_eq!(decode_source(b"\xc3\xa9".to_vec()).unwrap(), "é");
        assert_eq!(decode_source(Vec::new()).unwrap(), "");
    }

    #[test]
    fn invalid_utf8_is_rejected_with_or_without_a_bom() {
        for bytes in [&b"a\xffb"[..], b"\xef\xbb\xbf\xc3", b"\xef\xbb"] {
            let error = decode_source(bytes.to_vec()).unwrap_err();
            assert!(
                error.to_string().contains("invalid UTF-8 source"),
                "{error}"
            );
        }
    }

    #[test]
    fn utf16_with_a_bom_is_decoded_in_either_byte_order() {
        assert_eq!(decode_source(b"\xff\xfeA\0\xe9\0".to_vec()).unwrap(), "Aé");
        assert_eq!(decode_source(b"\xfe\xff\0A\0\xe9".to_vec()).unwrap(), "Aé");
        let odd = decode_source(b"\xff\xfeA".to_vec()).unwrap_err();
        assert!(odd.to_string().contains("invalid UTF-16 source"), "{odd}");
        let unpaired = decode_source(b"\xff\xfe\x00\xd8".to_vec()).unwrap_err();
        assert!(
            unpaired.to_string().contains("invalid UTF-16 source"),
            "{unpaired}"
        );
    }
}
