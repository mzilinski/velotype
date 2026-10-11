//! Hunspell dictionaries (the format used by LibreOffice and Firefox),
//! checked with the pure-Rust `spellbook` crate on every platform.
//!
//! Dictionaries are not bundled: most are GPL/LGPL/MPL licensed, so they are
//! downloaded on request from the wooorm/dictionaries collection into
//! `<config>/dictionaries/<tag>.aff|.dic`. Several languages can be active at
//! once; a word is accepted when any of them knows it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::ops::Range;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use spellbook::Dictionary;
use unicode_segmentation::UnicodeSegmentation as _;

use super::SpellBackend;

/// Dictionaries offered in the Spelling Language menu: (BCP-47 tag, folder in
/// wooorm/dictionaries, native display name).
pub(crate) const KNOWN_DICTIONARIES: &[(&str, &str, &str)] = &[
    ("de-DE", "de", "Deutsch (Deutschland)"),
    ("de-AT", "de-AT", "Deutsch (Österreich)"),
    ("de-CH", "de-CH", "Deutsch (Schweiz)"),
    ("en-US", "en", "English (US)"),
    ("en-GB", "en-GB", "English (UK)"),
    ("fr-FR", "fr", "Français"),
    ("es-ES", "es", "Español"),
    ("it-IT", "it", "Italiano"),
    ("nl-NL", "nl", "Nederlands"),
    ("pt-PT", "pt-PT", "Português (Portugal)"),
    ("pt-BR", "pt", "Português (Brasil)"),
    ("pl-PL", "pl", "Polski"),
    ("cs-CZ", "cs", "Čeština"),
    ("da-DK", "da", "Dansk"),
    ("sv-SE", "sv", "Svenska"),
    ("nb-NO", "nb", "Norsk bokmål"),
    ("hu-HU", "hu", "Magyar"),
    ("tr-TR", "tr", "Türkçe"),
    ("ru-RU", "ru", "Русский"),
    ("uk-UA", "uk", "Українська"),
];

const DICTIONARY_BASE_URL: &str =
    "https://raw.githubusercontent.com/wooorm/dictionaries/main/dictionaries";
const PERSONAL_DICTIONARY_FILE: &str = "personal.txt";

pub(crate) fn dictionary_dir() -> Option<PathBuf> {
    crate::config::VelotypeConfigDirs::from_system()
        .ok()
        .map(|dirs| dirs.dictionaries_dir())
}

fn dictionary_paths(dir: &Path, tag: &str) -> (PathBuf, PathBuf) {
    (
        dir.join(format!("{tag}.aff")),
        dir.join(format!("{tag}.dic")),
    )
}

pub(crate) fn is_installed(tag: &str) -> bool {
    dictionary_dir().is_some_and(|dir| {
        let (aff, dic) = dictionary_paths(&dir, tag);
        aff.is_file() && dic.is_file()
    })
}

/// Downloads one known dictionary. Blocking; run it off the UI thread.
pub(crate) fn download(tag: &str) -> anyhow::Result<()> {
    let Some((_, folder, _)) = KNOWN_DICTIONARIES
        .iter()
        .find(|(known, _, _)| *known == tag)
    else {
        bail!("no downloadable dictionary for {tag}");
    };
    let dir = dictionary_dir().context("no configuration directory")?;
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let client = reqwest::blocking::Client::builder()
        .user_agent(concat!("Velotype/", env!("CARGO_PKG_VERSION")))
        .build()?;
    let (aff_path, dic_path) = dictionary_paths(&dir, tag);
    for (file, path) in [("index.aff", &aff_path), ("index.dic", &dic_path)] {
        let url = format!("{DICTIONARY_BASE_URL}/{folder}/{file}");
        let response = client
            .get(&url)
            .send()
            .and_then(|response| response.error_for_status())
            .with_context(|| format!("failed to download {url}"))?;
        let bytes = response.bytes()?;
        // Write to a temporary name first so a broken download never leaves a
        // half-written dictionary that looks installed.
        let partial = path.with_extension("part");
        std::fs::write(&partial, &bytes)
            .with_context(|| format!("failed to write {}", partial.display()))?;
        std::fs::rename(&partial, path)?;
    }
    Ok(())
}

fn read_personal_words(dir: &Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(PERSONAL_DICTIONARY_FILE))
        .map(|text| {
            text.lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) struct HunspellBackend {
    dictionaries: RefCell<Vec<Dictionary>>,
    ignored: RefCell<HashSet<String>>,
    personal_path: Option<PathBuf>,
}

impl HunspellBackend {
    /// Loads every installed dictionary among `tags`; `None` when none is.
    pub(super) fn new(tags: &[String]) -> Option<Self> {
        let dir = dictionary_dir()?;
        let mut dictionaries = tags
            .iter()
            .filter_map(|tag| {
                let (aff, dic) = dictionary_paths(&dir, tag);
                let aff = std::fs::read_to_string(aff).ok()?;
                let dic = std::fs::read_to_string(dic).ok()?;
                Dictionary::new(&aff, &dic)
                    .map_err(|err| eprintln!("failed to load {tag} dictionary: {err}"))
                    .ok()
            })
            .collect::<Vec<_>>();
        if dictionaries.is_empty() {
            return None;
        }
        for word in read_personal_words(&dir) {
            for dictionary in &mut dictionaries {
                let _ = dictionary.add(&word);
            }
        }
        Some(Self {
            dictionaries: RefCell::new(dictionaries),
            ignored: RefCell::new(HashSet::new()),
            personal_path: Some(dir.join(PERSONAL_DICTIONARY_FILE)),
        })
    }

    #[cfg(test)]
    pub(super) fn from_dictionaries(dictionaries: Vec<Dictionary>) -> Self {
        Self {
            dictionaries: RefCell::new(dictionaries),
            ignored: RefCell::new(HashSet::new()),
            personal_path: None,
        }
    }

    fn accepts(&self, word: &str) -> bool {
        self.ignored.borrow().contains(word)
            || self
                .dictionaries
                .borrow()
                .iter()
                .any(|dictionary| dictionary.check(word))
    }
}

/// Words that are never flagged: numbers, mixed digits, and single letters.
fn is_checkable_word(word: &str) -> bool {
    word.chars().count() > 1
        && word.chars().any(char::is_alphabetic)
        && !word.chars().any(|ch| ch.is_ascii_digit())
}

impl SpellBackend for HunspellBackend {
    fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        text.unicode_word_indices()
            .filter(|(_, word)| is_checkable_word(word) && !self.accepts(word))
            .map(|(start, word)| start..start + word.len())
            .collect()
    }

    fn suggestions(&self, word: &str) -> Vec<String> {
        let mut suggestions = Vec::new();
        for dictionary in self.dictionaries.borrow().iter() {
            let mut found = Vec::new();
            dictionary.suggest(word, &mut found);
            for suggestion in found {
                if !suggestions.contains(&suggestion) {
                    suggestions.push(suggestion);
                }
            }
        }
        suggestions
    }

    fn add_to_dictionary(&self, word: &str) {
        for dictionary in self.dictionaries.borrow_mut().iter_mut() {
            let _ = dictionary.add(word);
        }
        if let Some(path) = self.personal_path.as_ref() {
            use std::io::Write as _;
            let appended = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| writeln!(file, "{word}"));
            if let Err(err) = appended {
                eprintln!("failed to save {word} to the personal dictionary: {err}");
            }
        }
    }

    fn ignore(&self, word: &str) {
        self.ignored.borrow_mut().insert(word.to_string());
    }
}

#[cfg(test)]
mod tests {
    use spellbook::Dictionary;

    use super::{HunspellBackend, SpellBackend as _};

    // A tiny dictionary in Hunspell format: three words plus a plural suffix.
    const AFF: &str = "SET UTF-8\nSFX S Y 1\nSFX S 0 n .\n";
    const DIC: &str = "3\nHaus\nKatze/S\nStraße\n";
    const EN_DIC: &str = "2\nhouse\ncat\n";

    fn backend() -> HunspellBackend {
        HunspellBackend::from_dictionaries(vec![
            Dictionary::new(AFF, DIC).unwrap(),
            Dictionary::new("SET UTF-8\n", EN_DIC).unwrap(),
        ])
    }

    #[test]
    fn flags_unknown_words_across_all_active_dictionaries() {
        let backend = backend();
        let text = "Haus Katzen cat Strasse 2026 Hxus";
        let words = backend
            .misspelled(text)
            .into_iter()
            .map(|range| &text[range])
            .collect::<Vec<_>>();
        assert_eq!(words, vec!["Strasse", "Hxus"]);
    }

    #[test]
    fn ignore_and_add_accept_words() {
        let backend = backend();
        backend.ignore("Hxus");
        backend.add_to_dictionary("Velotype");
        assert!(backend.misspelled("Hxus Velotype").is_empty());
    }

    #[test]
    fn suggests_from_the_dictionaries() {
        let backend = backend();
        assert!(backend.suggestions("Hxus").iter().any(|s| s == "Haus"));
    }
}

/// Real dictionaries from the network; run with `cargo test -- --ignored`.
#[cfg(test)]
mod network_tests {
    use super::{HunspellBackend, SpellBackend as _, download};

    #[test]
    #[ignore = "downloads dictionaries"]
    fn german_and_english_together() {
        download("de-DE").expect("download de-DE");
        download("en-US").expect("download en-US");
        let started = std::time::Instant::now();
        let backend = HunspellBackend::new(&["de-DE".into(), "en-US".into()]).expect("load");
        eprintln!("loaded in {:?}", started.elapsed());

        let text = "Die Rechtschreibprüfung funktioniert mit Umlauten, Straßenbahnhaltestellen \
                    und dem Workflow im Meeting. Fehlerr und Hauss sind falsch.";
        let started = std::time::Instant::now();
        let words = backend
            .misspelled(text)
            .into_iter()
            .map(|range| text[range].to_string())
            .collect::<Vec<_>>();
        eprintln!("checked in {:?}: {words:?}", started.elapsed());
        assert_eq!(words, vec!["Fehlerr", "Hauss"]);
        let suggestions = backend.suggestions("Hauss");
        eprintln!("suggestions for Hauss: {suggestions:?}");
        assert!(suggestions.iter().any(|s| s == "Haus"));
    }
}
