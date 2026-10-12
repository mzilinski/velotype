//! File > Print…: the document is rendered like the HTML export (images
//! embedded) into a temporary file that opens in the default browser, which
//! shows its print dialog right away.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui::*;

use super::Editor;
use super::export::show_export_error;
use crate::components::PrintDocument;
use crate::export::render_html_with_base_dir_and_fonts;
use crate::fonts::FontSettings;
use crate::theme::ThemeManager;

/// Print files older than this are removed when printing again.
const KEEP_PRINT_FILES: Duration = Duration::from_secs(60 * 60);

/// `html` with a script that opens the print dialog once the page loaded.
pub(crate) fn with_print_script(html: &str) -> String {
    const SCRIPT: &str = "<script>window.addEventListener('load', () => setTimeout(() => window.print(), 200));</script>";
    match html.rfind("</body>") {
        Some(index) => format!("{}{SCRIPT}\n{}", &html[..index], &html[index..]),
        None => format!("{html}{SCRIPT}"),
    }
}

fn print_dir() -> PathBuf {
    std::env::temp_dir().join("velotype-print")
}

/// Removes print files left over from earlier runs.
fn remove_old_print_files(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let now = SystemTime::now();
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age > KEEP_PRINT_FILES);
        if old {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

impl Editor {
    pub(super) fn on_print_document(
        &mut self,
        _: &PrintDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let markdown = self.serialized_document_text(cx);
        let theme = cx.global::<ThemeManager>().unzoomed().clone();
        let fonts = FontSettings::current(cx);
        let title = self.export_title();
        let base_dir = self.file_path.as_ref().and_then(|path| path.parent());
        let html = with_print_script(&render_html_with_base_dir_and_fonts(
            &markdown, &theme, &title, base_dir, &fonts,
        ));
        let dir = print_dir();
        remove_old_print_files(&dir);
        let id = uuid::Uuid::new_v4().simple().to_string();
        let path = dir.join(format!("{title}-{}.html", &id[..8]));
        let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, html));
        match written {
            Ok(()) => cx.open_with_system(&path),
            Err(err) => show_export_error(window, cx, &err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::with_print_script;

    #[test]
    fn print_script_goes_before_the_body_end() {
        let html = with_print_script("<html><body><p>x</p></body></html>");
        assert!(html.contains("window.print()"));
        assert!(html.ends_with("</body></html>"));
        assert!(with_print_script("<p>x</p>").starts_with("<p>x</p><script>"));
    }
}
