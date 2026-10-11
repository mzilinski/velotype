//! Windows ISpellChecker backend (Windows 8+), using the dictionaries the
//! user has installed for their languages plus their personal word list.

use std::ops::Range;

use windows::Win32::Globalization::{
    CORRECTIVE_ACTION_NONE, ISpellChecker, ISpellCheckerFactory, ISpellingError,
    SpellCheckerFactory,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, IEnumString,
};
use windows::core::{HSTRING, PWSTR};

use super::{SpellBackend, utf16_range_to_utf8};

pub(super) struct WindowsSpellBackend {
    checker: ISpellChecker,
}

fn factory() -> Option<ISpellCheckerFactory> {
    // The UI thread normally has COM initialized already; a repeated or
    // mode-mismatched init is harmless here.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        CoCreateInstance(&SpellCheckerFactory, None, CLSCTX_INPROC_SERVER).ok()
    }
}

/// Drains an `IEnumString`, freeing each COM-allocated string.
fn collect_strings(strings: &IEnumString) -> Vec<String> {
    let mut values = Vec::new();
    loop {
        let mut item = [PWSTR::null()];
        let mut fetched = 0u32;
        let result = unsafe { strings.Next(&mut item, Some(&mut fetched)) };
        if result.is_err() || fetched == 0 || item[0].is_null() {
            break;
        }
        unsafe {
            if let Ok(value) = item[0].to_string() {
                values.push(value);
            }
            CoTaskMemFree(Some(item[0].0 as *const _));
        }
    }
    values
}

#[cfg(test)]
pub(super) fn supported_languages() -> Vec<String> {
    let Some(factory) = factory() else {
        return Vec::new();
    };
    unsafe { factory.SupportedLanguages() }
        .map(|languages| collect_strings(&languages))
        .unwrap_or_default()
}

impl WindowsSpellBackend {
    pub(super) fn new(language: &str) -> Option<Self> {
        let factory = factory()?;
        let tag = HSTRING::from(language);
        let supported = unsafe { factory.IsSupported(&tag) }.ok()?;
        if !supported.as_bool() {
            return None;
        }
        let checker = unsafe { factory.CreateSpellChecker(&tag) }.ok()?;
        Some(Self { checker })
    }
}

impl SpellBackend for WindowsSpellBackend {
    fn misspelled(&self, text: &str) -> Vec<Range<usize>> {
        let Ok(errors) = (unsafe { self.checker.Check(&HSTRING::from(text)) }) else {
            return Vec::new();
        };
        let mut ranges = Vec::new();
        loop {
            let mut error: Option<ISpellingError> = None;
            let result = unsafe { errors.Next(&mut error) };
            let Some(error) = error.filter(|_| result.is_ok()) else {
                break;
            };
            let (Ok(start), Ok(len), Ok(action)) =
                (unsafe { (error.StartIndex(), error.Length(), error.CorrectiveAction()) })
            else {
                continue;
            };
            if action == CORRECTIVE_ACTION_NONE {
                continue;
            }
            if let Some(range) = utf16_range_to_utf8(text, start as usize, len as usize) {
                ranges.push(range);
            }
        }
        ranges
    }

    fn suggestions(&self, word: &str) -> Vec<String> {
        unsafe { self.checker.Suggest(&HSTRING::from(word)) }
            .map(|suggestions| collect_strings(&suggestions))
            .unwrap_or_default()
    }

    fn add_to_dictionary(&self, word: &str) {
        let _ = unsafe { self.checker.Add(&HSTRING::from(word)) };
    }

    fn ignore(&self, word: &str) {
        let _ = unsafe { self.checker.Ignore(&HSTRING::from(word)) };
    }
}

#[cfg(test)]
mod tests {
    use super::{SpellBackend, WindowsSpellBackend, supported_languages};

    #[test]
    fn checks_and_suggests_with_the_system_dictionary() {
        // Use whatever language Windows actually has a dictionary for, so the
        // test cannot pass by skipping.
        let languages = supported_languages();
        let language = languages
            .first()
            .expect("Windows reports no spell-check languages");
        let backend = WindowsSpellBackend::new(language).expect("supported language loads");
        let text = "Qxzvrtl Wrtzqp";
        let ranges = backend.misspelled(text);
        assert_eq!(ranges, vec![0..7, 8..14], "{language}");
        backend.ignore("Qxzvrtl");
        assert_eq!(backend.misspelled(text), vec![8..14], "{language}");
    }
}
