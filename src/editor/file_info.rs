//! File > Move To… and File > Properties.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::Context as _;
use gpui::*;

use super::Editor;
use crate::components::{MoveDocument, ShowProperties};
use crate::i18n::{I18nManager, I18nStrings};

/// Moves `from` to `to`, also across drives (copy, then delete).
pub(crate) fn move_file(from: &Path, to: &Path) -> anyhow::Result<()> {
    if from == to {
        return Ok(());
    }
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)
        .with_context(|| format!("failed to copy '{}' to '{}'", from.display(), to.display()))?;
    std::fs::remove_file(from)
        .with_context(|| format!("failed to remove '{}' after copying it", from.display()))
}

fn format_time(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Local>::from(time)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB ({bytes} B)", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB ({bytes} B)", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// The lines of the Properties dialog.
pub(crate) fn properties_text(
    strings: &I18nStrings,
    path: Option<&Path>,
    text: &str,
    line_ending: &str,
) -> String {
    let mut lines = Vec::new();
    match path {
        Some(path) => {
            lines.push(format!("{}: {}", strings.properties_path, path.display()));
            if let Ok(metadata) = std::fs::metadata(path) {
                lines.push(format!(
                    "{}: {}",
                    strings.properties_size,
                    format_size(metadata.len())
                ));
                if let Ok(created) = metadata.created() {
                    lines.push(format!(
                        "{}: {}",
                        strings.properties_created,
                        format_time(created)
                    ));
                }
                if let Ok(modified) = metadata.modified() {
                    lines.push(format!(
                        "{}: {}",
                        strings.properties_modified,
                        format_time(modified)
                    ));
                }
            }
        }
        None => lines.push(strings.properties_not_saved.clone()),
    }
    lines.push(String::new());
    lines.push(format!(
        "{}: {}",
        strings.properties_words,
        super::status_bar::count_words(text)
    ));
    lines.push(format!(
        "{}: {}",
        strings.properties_characters,
        text.chars().filter(|ch| *ch != '\n').count()
    ));
    let line_count = if text.is_empty() {
        0
    } else {
        text.lines().count()
    };
    lines.push(format!("{}: {line_count}", strings.properties_lines));
    lines.push(format!(
        "{}: UTF-8, {line_ending}",
        strings.properties_encoding
    ));
    lines.join("\n")
}

impl Editor {
    pub(super) fn on_move_document(
        &mut self,
        _: &MoveDocument,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(old) = self.file_path.clone() else {
            // Nothing on disk yet: moving means choosing where to save it.
            self.save_document_as(window, cx);
            return;
        };
        let directory = old
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let name = old
            .file_name()
            .map(|name| name.to_string_lossy().to_string());
        let prompt = cx.prompt_for_new_path(&directory, name.as_deref());
        let window_handle = window.window_handle();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let new = match prompt.await {
                Ok(Ok(Some(path))) => path,
                _ => return,
            };
            let result = move_file(&old, &new);
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window, cx| match result {
                    Ok(()) => {
                        let _ = this.update(cx, |editor, cx| editor.document_moved(new, cx));
                    }
                    Err(err) => {
                        let strings = cx.global::<I18nManager>().strings().clone();
                        let buttons = [strings.info_dialog_ok.as_str()];
                        let _ = window.prompt(
                            PromptLevel::Critical,
                            &strings.move_failed_title,
                            Some(&err.to_string()),
                            &buttons,
                            cx,
                        );
                    }
                },
            );
        })
        .detach();
    }

    /// The document's file now lives at `path`.
    fn document_moved(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        crate::app_menu::record_recent_file_from_editor(&path, cx);
        self.file_path = Some(path);
        self.pending_window_title_refresh = true;
        self.sync_workspace_after_document_path_change(cx);
        self.refresh_workspace_trees(cx);
        self.rebuild_image_runtimes(cx);
        cx.notify();
    }

    pub(super) fn on_show_properties(
        &mut self,
        _: &ShowProperties,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = cx.global::<I18nManager>().strings().clone();
        let text = self.serialized_document_text(cx);
        let detail = properties_text(
            &strings,
            self.file_path.as_deref(),
            &text,
            self.line_ending().label(),
        );
        let title = self
            .file_path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| strings.menu_properties.clone());
        let buttons = [strings.info_dialog_ok.as_str()];
        let _ = window.prompt(PromptLevel::Info, &title, Some(&detail), &buttons, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{move_file, properties_text};

    #[test]
    fn moves_files_and_keeps_content() {
        let dir = std::env::temp_dir().join(format!("velotype-move-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let from = dir.join("a.md");
        std::fs::write(&from, "content").unwrap();
        let to = dir.join("sub").join("b.md");
        move_file(&from, &to).unwrap();
        assert!(!from.exists());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "content");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn properties_list_counts_and_file_data() {
        let strings = crate::i18n::I18nManager::default().strings().clone();
        let dir = std::env::temp_dir().join(format!("velotype-props-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");
        std::fs::write(&path, "# Title\n\nTwo words").unwrap();
        let text = properties_text(&strings, Some(&path), "# Title\n\nTwo words", "LF");
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains("18 B"), "{text}");
        assert!(
            text.contains(&format!("{}: 3", strings.properties_words)),
            "{text}"
        );
        assert!(
            text.contains(&format!("{}: 3", strings.properties_lines)),
            "{text}"
        );
        assert!(text.contains("UTF-8, LF"), "{text}");
        let unsaved = properties_text(&strings, None, "", "CRLF");
        assert!(
            unsaved.starts_with(&strings.properties_not_saved),
            "{unsaved}"
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
