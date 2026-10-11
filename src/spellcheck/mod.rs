//! Spell checking against the operating system's dictionaries.
//!
//! The checker is a gpui [`Global`] holding a platform backend plus a cache of
//! results per checked text, so repainting a block does not re-run the
//! platform check. Results are byte ranges into the checked text. Only
//! Windows (ISpellChecker, Windows 8+) has a backend today; elsewhere the
//! checker reports no errors and offers no languages.

use std::collections::HashMap;
use std::ops::Range;

use gpui::{App, Global};

#[cfg(windows)]
mod windows_backend;

/// Platform spell-checking backend for one language.
pub(crate) trait SpellBackend {
    /// Byte ranges of misspelled words in `text`.
    fn misspelled(&self, text: &str) -> Vec<Range<usize>>;
    /// Replacement suggestions for one misspelled word.
    fn suggestions(&self, word: &str) -> Vec<String>;
    /// Adds `word` to the user dictionary.
    fn add_to_dictionary(&self, word: &str);
    /// Ignores `word` for the rest of the session.
    fn ignore(&self, word: &str);
}

/// Cap for cached texts before the cache is reset; blocks re-check lazily.
const CACHE_LIMIT: usize = 4096;

pub(crate) struct SpellChecker {
    language: Option<String>,
    backend: Option<Box<dyn SpellBackend>>,
    cache: HashMap<String, Vec<Range<usize>>>,
}

impl Global for SpellChecker {}

impl SpellChecker {
    /// Installs the global checker for `preferred` (a BCP-47 tag such as
    /// `de-DE`), falling back to the system locale and then `en-US`.
    pub(crate) fn init(cx: &mut App, preferred: Option<&str>) {
        let checker = Self::for_language(preferred);
        cx.set_global(checker);
    }

    fn for_language(preferred: Option<&str>) -> Self {
        let system = sys_locale::get_locale();
        let candidates = preferred
            .map(str::to_string)
            .into_iter()
            .chain(system)
            .chain(std::iter::once("en-US".to_string()));
        for language in candidates {
            if let Some(backend) = create_backend(&language) {
                return Self {
                    language: Some(language),
                    backend: Some(backend),
                    cache: HashMap::new(),
                };
            }
        }
        Self {
            language: None,
            backend: None,
            cache: HashMap::new(),
        }
    }

    /// Switches the dictionary language; unknown languages keep the current one.
    pub(crate) fn set_language(cx: &mut App, language: &str) {
        if let Some(backend) = create_backend(language) {
            cx.set_global(Self {
                language: Some(language.to_string()),
                backend: Some(backend),
                cache: HashMap::new(),
            });
        }
    }

    pub(crate) fn language(cx: &App) -> Option<String> {
        cx.try_global::<Self>()
            .and_then(|checker| checker.language.clone())
    }

    /// Languages the platform can check, as BCP-47 tags.
    pub(crate) fn available_languages() -> Vec<String> {
        available_backend_languages()
    }

    /// Misspelled byte ranges of `text`, cached per text.
    pub(crate) fn misspelled(cx: &mut App, text: &str) -> Vec<Range<usize>> {
        if text.trim().is_empty() {
            return Vec::new();
        }
        let Some(checker) = cx.try_global::<Self>() else {
            return Vec::new();
        };
        if let Some(ranges) = checker.cache.get(text) {
            return ranges.clone();
        }
        let Some(backend) = checker.backend.as_ref() else {
            return Vec::new();
        };
        let ranges = backend.misspelled(text);
        let checker = cx.global_mut::<Self>();
        if checker.cache.len() >= CACHE_LIMIT {
            checker.cache.clear();
        }
        checker.cache.insert(text.to_string(), ranges.clone());
        ranges
    }

    pub(crate) fn suggestions(cx: &App, word: &str) -> Vec<String> {
        cx.try_global::<Self>()
            .and_then(|checker| checker.backend.as_ref())
            .map(|backend| backend.suggestions(word))
            .unwrap_or_default()
    }

    /// Adds or ignores a word, then drops cached results so every block is
    /// re-checked on its next paint.
    pub(crate) fn accept_word(cx: &mut App, word: &str, permanently: bool) {
        if !cx.has_global::<Self>() {
            return;
        }
        let checker = cx.global_mut::<Self>();
        if let Some(backend) = checker.backend.as_ref() {
            if permanently {
                backend.add_to_dictionary(word);
            } else {
                backend.ignore(word);
            }
        }
        checker.cache.clear();
    }
}

#[cfg(windows)]
fn create_backend(language: &str) -> Option<Box<dyn SpellBackend>> {
    windows_backend::WindowsSpellBackend::new(language)
        .map(|backend| Box::new(backend) as Box<dyn SpellBackend>)
}

#[cfg(not(windows))]
fn create_backend(_language: &str) -> Option<Box<dyn SpellBackend>> {
    None
}

#[cfg(windows)]
fn available_backend_languages() -> Vec<String> {
    windows_backend::supported_languages()
}

#[cfg(not(windows))]
fn available_backend_languages() -> Vec<String> {
    Vec::new()
}

/// Converts a UTF-16 code-unit range reported by a platform API into a byte
/// range of `text`, or `None` when it does not land on character boundaries.
pub(crate) fn utf16_range_to_utf8(text: &str, start: usize, len: usize) -> Option<Range<usize>> {
    let end = start + len;
    let mut utf16 = 0usize;
    let mut byte_start = None;
    for (byte, ch) in text.char_indices() {
        if utf16 == start {
            byte_start = Some(byte);
        }
        if utf16 == end {
            return byte_start.map(|byte_start| byte_start..byte);
        }
        utf16 += ch.len_utf16();
    }
    if utf16 == end {
        return byte_start.map(|byte_start| byte_start..text.len());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::utf16_range_to_utf8;

    #[test]
    fn maps_utf16_ranges_to_bytes() {
        let text = "Grüße 🎉 Welt";
        // "Grüße" is 5 UTF-16 units and 7 bytes.
        assert_eq!(utf16_range_to_utf8(text, 0, 5), Some(0..7));
        // The emoji is 2 UTF-16 units; "Welt" starts after it and a space.
        let welt = text.find("Welt").unwrap();
        assert_eq!(utf16_range_to_utf8(text, 9, 4), Some(welt..welt + 4));
        // Splitting the emoji surrogate pair is rejected.
        assert_eq!(utf16_range_to_utf8(text, 7, 1), None);
    }
}
