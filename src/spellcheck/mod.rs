//! Spell checking.
//!
//! The checker is a gpui [`Global`] holding one backend plus a cache of
//! results per checked text, so repainting a block does not re-run the check.
//! Results are byte ranges into the checked text.
//!
//! Hunspell dictionaries ([`hunspell`]) are preferred: they work on every
//! platform, cover many languages and can be combined (e.g. German and
//! English at once). When none is installed yet, Windows falls back to the
//! system ISpellChecker for the system language.

use std::collections::HashMap;
use std::ops::Range;

use gpui::{App, AsyncApp, Global};

pub(crate) mod hunspell;
#[cfg(windows)]
mod windows_backend;

/// Spell-checking backend for the active languages.
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
    /// Active BCP-47 tags, in the order the user enabled them.
    languages: Vec<String>,
    backend: Option<Box<dyn SpellBackend>>,
    cache: HashMap<String, Vec<Range<usize>>>,
}

impl Global for SpellChecker {}

/// Parses the persisted language preference: comma-separated BCP-47 tags.
pub(crate) fn parse_languages(preference: &str) -> Vec<String> {
    preference
        .split(',')
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string)
        .collect()
}

impl SpellChecker {
    /// Installs the global checker for the comma-separated `preference`; an
    /// empty preference follows the system locale.
    pub(crate) fn init(cx: &mut App, preference: &str) {
        let mut languages = parse_languages(preference);
        if languages.is_empty() {
            languages.extend(sys_locale::get_locale());
        }
        cx.set_global(Self::for_languages(languages));
    }

    fn for_languages(languages: Vec<String>) -> Self {
        let backend = create_backend(&languages);
        Self {
            languages,
            backend,
            cache: HashMap::new(),
        }
    }

    pub(crate) fn languages(cx: &App) -> Vec<String> {
        cx.try_global::<Self>()
            .map(|checker| checker.languages.clone())
            .unwrap_or_default()
    }

    /// Enables or disables `tag`. A dictionary that is not installed yet is
    /// downloaded in the background first; `done` runs on the UI thread with
    /// the new language list (persist it) or the download error.
    pub(crate) fn toggle_language(
        cx: &mut App,
        tag: String,
        done: impl FnOnce(&mut App, anyhow::Result<Vec<String>>) + 'static,
    ) {
        let mut languages = Self::languages(cx);
        if let Some(index) = languages.iter().position(|active| *active == tag) {
            languages.remove(index);
            cx.set_global(Self::for_languages(languages.clone()));
            done(cx, Ok(languages));
            return;
        }
        if hunspell::is_installed(&tag) {
            languages.push(tag);
            cx.set_global(Self::for_languages(languages.clone()));
            done(cx, Ok(languages));
            return;
        }
        let download_tag = tag.clone();
        let download = cx
            .background_executor()
            .spawn(async move { hunspell::download(&download_tag) });
        cx.spawn(async move |cx: &mut AsyncApp| {
            let result = download.await;
            let _ = cx.update(move |cx| match result {
                Ok(()) => {
                    let mut languages = Self::languages(cx);
                    if !languages.contains(&tag) {
                        languages.push(tag);
                    }
                    cx.set_global(Self::for_languages(languages.clone()));
                    done(cx, Ok(languages));
                }
                Err(err) => done(cx, Err(err)),
            });
        })
        .detach();
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

/// Hunspell for every installed active language; otherwise the platform
/// checker for the first language it supports.
fn create_backend(languages: &[String]) -> Option<Box<dyn SpellBackend>> {
    if let Some(backend) = hunspell::HunspellBackend::new(languages) {
        return Some(Box::new(backend));
    }
    platform_backend(languages)
}

#[cfg(windows)]
fn platform_backend(languages: &[String]) -> Option<Box<dyn SpellBackend>> {
    languages.iter().find_map(|language| {
        windows_backend::WindowsSpellBackend::new(language)
            .map(|backend| Box::new(backend) as Box<dyn SpellBackend>)
    })
}

#[cfg(not(windows))]
fn platform_backend(_languages: &[String]) -> Option<Box<dyn SpellBackend>> {
    None
}

/// Converts a UTF-16 code-unit range reported by a platform API into a byte
/// range of `text`, or `None` when it does not land on character boundaries.
#[cfg_attr(not(windows), allow(dead_code))]
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
    use super::{parse_languages, utf16_range_to_utf8};

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

    #[test]
    fn parses_language_preference_list() {
        assert_eq!(parse_languages(" de-DE, en-US ,,"), vec!["de-DE", "en-US"]);
        assert!(parse_languages("").is_empty());
    }
}
