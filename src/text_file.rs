//! Reading documents from disk. Besides UTF-8, text saved as UTF-16 (with a
//! byte order mark) or in the Windows code page 1252 opens too; binary files
//! (Word documents, images, ...) are refused with a clear message instead of
//! a decoding error.

use std::path::Path;

use anyhow::{Context as _, Result, bail};

/// File extensions that open through the Pandoc import instead of as text.
pub(crate) const IMPORT_EXTENSIONS: &[&str] = &["docx", "odt", "rtf", "epub"];

/// Whether `path` is a document that Pandoc imports (Word, OpenDocument, ...).
pub(crate) fn is_import_document(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            IMPORT_EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

/// The error for a file that is not text.
#[derive(Debug)]
pub(crate) struct NotText;

impl std::fmt::Display for NotText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("not a text file")
    }
}

impl std::error::Error for NotText {}

/// Reads `path` as text (see the module documentation).
pub(crate) fn read_text_file(path: &Path) -> Result<String> {
    let bytes =
        std::fs::read(path).with_context(|| format!("failed to read '{}'", path.display()))?;
    decode_text(&bytes).with_context(|| format!("failed to read '{}'", path.display()))
}

pub(crate) fn decode_text(bytes: &[u8]) -> Result<String> {
    if let Some(rest) = bytes.strip_prefix(b"\xEF\xBB\xBF") {
        return Ok(String::from_utf8(rest.to_vec()).map_err(|_| NotText)?);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFF\xFE") {
        return decode_utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xFE\xFF") {
        return decode_utf16(rest, u16::from_be_bytes);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        if text.contains('\0') {
            bail!(NotText);
        }
        return Ok(text.to_string());
    }
    if looks_binary(bytes) {
        bail!(NotText);
    }
    Ok(bytes.iter().map(|&byte| windows_1252(byte)).collect())
}

fn decode_utf16(bytes: &[u8], read: fn([u8; 2]) -> u16) -> Result<String> {
    if !bytes.len().is_multiple_of(2) {
        bail!(NotText);
    }
    let units = bytes.as_chunks::<2>().0.iter().map(|pair| read(*pair));
    char::decode_utf16(units)
        .collect::<std::result::Result<String, _>>()
        .map_err(|_| NotText.into())
}

/// NUL bytes or many control characters: not a text file.
fn looks_binary(bytes: &[u8]) -> bool {
    let sample = &bytes[..bytes.len().min(8192)];
    let control = sample
        .iter()
        .filter(|&&byte| byte < 0x20 && !matches!(byte, b'\t' | b'\n' | b'\r' | 0x0C))
        .count();
    sample.contains(&0) || control * 20 > sample.len()
}

fn windows_1252(byte: u8) -> char {
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8D}', 'Ž',
        '\u{8F}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9D}',
        'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)],
        _ => char::from(byte),
    }
}

#[cfg(test)]
mod tests {
    use super::{NotText, decode_text, is_import_document};
    use std::path::Path;

    #[test]
    fn decodes_utf8_utf16_and_windows_1252() {
        assert_eq!(decode_text("# Grüße".as_bytes()).unwrap(), "# Grüße");
        assert_eq!(decode_text(b"\xEF\xBB\xBF# A").unwrap(), "# A");
        let utf16: Vec<u8> = [0xFF, 0xFE]
            .into_iter()
            .chain("# Ä".encode_utf16().flat_map(u16::to_le_bytes))
            .collect();
        assert_eq!(decode_text(&utf16).unwrap(), "# Ä");
        assert_eq!(
            decode_text(b"Gr\xFC\xDFe \x80 \x84x\x93").unwrap(),
            "Grüße € „x“"
        );
    }

    #[test]
    fn refuses_binary_files() {
        // A .docx is a zip archive.
        let zip = b"PK\x03\x04\x14\x00\x06\x00\x08\x00\x00\x00!\x00\xdf\xa4\xd2lZ\x01";
        let err = decode_text(zip).unwrap_err();
        assert!(err.downcast_ref::<NotText>().is_some(), "{err:#}");
        assert!(decode_text(b"abc\0def").is_err());
    }

    #[test]
    fn word_and_open_document_files_are_imported() {
        assert!(is_import_document(Path::new("C:/Docs/Dok1.DOCX")));
        assert!(is_import_document(Path::new("a.odt")));
        assert!(!is_import_document(Path::new("a.md")));
        assert!(!is_import_document(Path::new("docx")));
    }
}
