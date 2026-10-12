//! Edit > Line Endings: documents keep the line endings they were loaded
//! with (the editor works with `\n` internally) and can be switched between
//! Windows (CRLF) and Unix (LF). An optional final newline is added on save.

use gpui::{App, Context, Global, Window};

use super::Editor;
use crate::components::{SetLineEndingCrlf, SetLineEndingLf, ToggleFinalNewline};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum LineEnding {
    #[default]
    Lf,
    Crlf,
}

impl LineEnding {
    /// The line ending a file uses: CRLF when most of its line breaks are
    /// `\r\n`, LF otherwise (also for text without line breaks).
    pub(crate) fn detect(text: &str) -> Self {
        let breaks = text.matches('\n').count();
        let crlf = text.matches("\r\n").count();
        if crlf > 0 && crlf * 2 >= breaks {
            Self::Crlf
        } else {
            Self::Lf
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Lf => "LF",
            Self::Crlf => "CRLF",
        }
    }
}

/// `text` (with `\n` line breaks) as it is written to disk.
pub(crate) fn text_for_disk(text: &str, ending: LineEnding, final_newline: bool) -> String {
    let mut text = text.to_string();
    if final_newline && !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    match ending {
        LineEnding::Lf => text,
        LineEnding::Crlf => text.replace('\n', "\r\n"),
    }
}

impl Editor {
    pub(crate) fn line_ending(&self) -> LineEnding {
        self.line_ending
    }

    /// The document text as it is saved: with the document's line endings
    /// and, when enabled, a final newline.
    pub(super) fn document_text_for_disk(&self, cx: &App) -> String {
        let final_newline = crate::config::EditorSettings::writing(cx).final_newline;
        text_for_disk(
            &self.serialized_document_text(cx),
            self.line_ending,
            final_newline,
        )
    }

    /// Switches the document's line endings; the file changes on the next
    /// save, so the document becomes dirty.
    pub(crate) fn set_line_ending(&mut self, ending: LineEnding, cx: &mut Context<Self>) {
        if self.line_ending == ending {
            return;
        }
        self.line_ending = ending;
        if !self.is_read_only() {
            self.mark_dirty(cx);
        }
        cx.notify();
    }

    /// Tells the menus the line endings of the document in the active
    /// window, rebuilding them when that changed.
    pub(super) fn publish_menu_state(&self, cx: &mut Context<Self>) {
        let published = cx.try_global::<MenuLineEnding>().map(|state| state.0);
        if published == Some(self.line_ending) {
            return;
        }
        cx.set_global(MenuLineEnding(self.line_ending));
        refresh_menus(cx);
    }

    pub(super) fn toggle_line_ending(&mut self, cx: &mut Context<Self>) {
        let next = match self.line_ending {
            LineEnding::Lf => LineEnding::Crlf,
            LineEnding::Crlf => LineEnding::Lf,
        };
        self.set_line_ending(next, cx);
    }

    pub(super) fn on_set_line_ending_crlf(
        &mut self,
        _: &SetLineEndingCrlf,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_line_ending(LineEnding::Crlf, cx);
    }

    pub(super) fn on_set_line_ending_lf(
        &mut self,
        _: &SetLineEndingLf,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_line_ending(LineEnding::Lf, cx);
    }

    pub(super) fn on_toggle_final_newline(
        &mut self,
        _: &ToggleFinalNewline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        crate::config::EditorSettings::update_writing(cx, |writing| {
            writing.final_newline = !writing.final_newline
        });
        refresh_menus(cx);
        cx.notify();
    }
}

/// Line endings of the document in the active window, for the menu.
struct MenuLineEnding(LineEnding);

impl Global for MenuLineEnding {}

pub(crate) fn menu_line_ending_is_crlf(cx: &App) -> bool {
    cx.try_global::<MenuLineEnding>()
        .is_some_and(|state| state.0 == LineEnding::Crlf)
}

/// Rebuilds the menus once the current update is done (the builder reads
/// state that may be borrowed right now). Tests keep their menus.
fn refresh_menus(cx: &mut App) {
    if cfg!(test) {
        return;
    }
    cx.defer(crate::app_menu::install_menus);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_the_dominant_line_ending() {
        assert_eq!(LineEnding::detect("a\r\nb\r\nc"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("a\nb\nc"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("one line"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\r\nb\nc\nd\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\r\nb\r\nc\n"), LineEnding::Crlf);
    }

    #[test]
    fn writes_line_endings_and_final_newline() {
        assert_eq!(text_for_disk("a\nb", LineEnding::Crlf, false), "a\r\nb");
        assert_eq!(text_for_disk("a\nb", LineEnding::Crlf, true), "a\r\nb\r\n");
        assert_eq!(text_for_disk("a\nb\n", LineEnding::Lf, true), "a\nb\n");
        assert_eq!(text_for_disk("a\nb", LineEnding::Lf, false), "a\nb");
        assert_eq!(text_for_disk("", LineEnding::Lf, true), "");
    }

    #[gpui::test]
    async fn saving_keeps_and_switches_line_endings(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!("velotype-eol-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("doc.md");
        let source = "# Title\r\n\r\nBody text\r\n";
        std::fs::write(&path, source).unwrap();

        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, source.to_string(), Some(path.clone()))
        });
        editor.update(cx, |editor, _| {
            assert_eq!(editor.line_ending(), LineEnding::Crlf)
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.save_to_existing_path(&path, window, cx)
            })
        });
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("# Title\r\n\r\nBody text"), "{saved:?}");
        assert!(!saved.replace("\r\n", "").contains('\n'), "{saved:?}");

        // Switching marks the document dirty and writes LF on the next save.
        editor.update(cx, |editor, cx| {
            editor.toggle_line_ending(cx);
            assert_eq!(editor.line_ending(), LineEnding::Lf);
            assert!(editor.document_dirty);
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.save_to_existing_path(&path, window, cx)
            })
        });
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(!saved.contains('\r'), "{saved:?}");
        assert!(saved.contains("# Title\n\nBody text"), "{saved:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
