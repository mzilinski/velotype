//! Editor-facing export flow and file writing.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use gpui::*;

use super::Editor;
use crate::config::read_app_preferences;
use crate::export::{self as document_export, ExportFormat, PdfExportOptions};
use crate::fonts::FontSettings;
use crate::i18n::I18nManager;
use crate::theme::{Theme, ThemeManager};

/// The last export of a document (File > Export with Previous Settings).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LastExport {
    Builtin(ExportFormat, PathBuf),
    Pandoc(crate::export::pandoc::PandocFormat, PathBuf),
}

impl Editor {
    /// Suggests an export directory and filename using the document stem and format extension.
    fn export_dialog_defaults(&self, format: ExportFormat) -> (PathBuf, String) {
        self.export_dialog_defaults_for(format.extension())
    }

    /// Save-dialog folder and file name for an export with `extension`.
    pub(super) fn export_dialog_defaults_for(&self, extension: &str) -> (PathBuf, String) {
        if let Some(path) = self.file_path.as_ref() {
            let directory = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
                .unwrap_or("untitled");
            return (directory, format!("{stem}.{extension}"));
        }

        (
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            format!("untitled.{extension}"),
        )
    }

    /// Uses the nonempty document stem as the export title, or `Untitled`.
    pub(super) fn export_title(&self) -> String {
        self.file_path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().to_string())
            .filter(|stem| !stem.is_empty())
            .unwrap_or_else(|| "Untitled".to_string())
    }

    /// Renders HTML as UTF-8 bytes or PDF through Chromium with the supplied font snapshot.
    fn render_export_bytes(
        format: ExportFormat,
        markdown: &str,
        theme: &Theme,
        title: &str,
        source_base_dir: Option<&Path>,
        fonts: &FontSettings,
        pdf_options: &PdfExportOptions,
    ) -> anyhow::Result<Vec<u8>> {
        match format {
            ExportFormat::HtmlPlain => Ok(document_export::render_html_without_styles(
                markdown,
                theme,
                title,
                source_base_dir,
            )
            .into_bytes()),
            ExportFormat::Html => Ok(document_export::render_html_with_base_dir_and_fonts(
                markdown,
                theme,
                title,
                source_base_dir,
                fonts,
            )
            .into_bytes()),
            ExportFormat::Pdf => document_export::render_pdf_with_fonts(
                markdown,
                theme,
                title,
                source_base_dir,
                fonts,
                pdf_options,
            ),
        }
    }

    /// Renders first, then writes bytes with destination context on filesystem errors.
    fn write_export_bytes(
        format: ExportFormat,
        markdown: &str,
        theme: &Theme,
        title: &str,
        path: &Path,
        source_base_dir: Option<&Path>,
        fonts: &FontSettings,
        pdf_options: &PdfExportOptions,
    ) -> anyhow::Result<()> {
        let bytes = Self::render_export_bytes(
            format,
            markdown,
            theme,
            title,
            source_base_dir,
            fonts,
            pdf_options,
        )?;
        std::fs::write(path, bytes).with_context(|| format!("failed to write '{}'", path.display()))
    }

    /// Synchronously exports the current text, theme, fonts, and resource base directory.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn export_document_to_path(
        &self,
        format: ExportFormat,
        path: &Path,
        cx: &App,
    ) -> anyhow::Result<()> {
        let markdown = self.serialized_document_text(cx);
        let theme = cx.global::<ThemeManager>().unzoomed().clone();
        let title = self.export_title();
        let source_base_dir = self.file_path.as_ref().and_then(|path| path.parent());
        let fonts = FontSettings::current(cx);
        Self::write_export_bytes(
            format,
            &markdown,
            &theme,
            &title,
            path,
            source_base_dir,
            &fonts,
            &PdfExportOptions::default(),
        )
    }

    /// Captures export inputs before prompting and renders/writes on a worker thread.
    /// Cancellation is silent; worker, rendering, and write errors are shown in the window.
    pub(crate) fn export_document_via_prompt(
        &mut self,
        format: ExportFormat,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Check for a browser before the save dialog so users without one are
        // not asked for a file name only to see the export fail afterwards.
        if format == ExportFormat::Pdf && document_export::find_chromium_executable().is_none() {
            self.show_pdf_browser_missing_prompt(window, cx);
            return;
        }

        let (default_dir, suggested_name) = self.export_dialog_defaults(format);
        let prompt = cx.prompt_for_new_path(&default_dir, Some(&suggested_name));
        let window_handle = window.window_handle();

        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut path = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    let detail = err.to_string();
                    let _ = cx.update_window(
                        window_handle,
                        move |_view: AnyView, window: &mut Window, cx: &mut App| {
                            show_export_error(window, cx, &detail);
                        },
                    );
                    return;
                }
            };

            if path.extension().is_none() {
                path.set_extension(format.extension());
            }
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |editor, cx| {
                        editor.export_document_to(format, path, window, cx);
                    });
                },
            );
        })
        .detach();
    }

    /// Exports the document as `format` to `path` in the background and
    /// remembers it for File > Export with Previous Settings.
    pub(super) fn export_document_to(
        &mut self,
        format: ExportFormat,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if format == ExportFormat::Pdf && document_export::find_chromium_executable().is_none() {
            self.show_pdf_browser_missing_prompt(window, cx);
            return;
        }
        let markdown = self.serialized_document_text(cx);
        let theme = cx.global::<ThemeManager>().unzoomed().clone();
        let title = self.export_title();
        let fonts = FontSettings::current(cx);
        let pdf_options = match format {
            ExportFormat::Pdf => read_app_preferences()
                .map(|preferences| preferences.pdf_export)
                .unwrap_or_default(),
            ExportFormat::Html | ExportFormat::HtmlPlain => PdfExportOptions::default(),
        };
        let source_base_dir = self
            .file_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf);
        let window_handle = window.window_handle();
        self.last_export = Some(LastExport::Builtin(format, path.clone()));

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let output_path = path.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    Self::write_export_bytes(
                        format,
                        &markdown,
                        &theme,
                        &title,
                        &path,
                        source_base_dir.as_deref(),
                        &fonts,
                        &pdf_options,
                    )
                    .map_err(|err| err.to_string())
                })
                .await;
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| match result {
                    Ok(()) => show_export_succeeded(window, cx, output_path),
                    Err(detail) => show_export_error(window, cx, &detail),
                },
            );
        })
        .detach();
    }

    /// File > Export with Previous Settings: repeats the last export of this
    /// document to the same file without asking.
    pub(super) fn on_export_again(
        &mut self,
        _: &crate::components::ExportAgain,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match self.last_export.clone() {
            Some(LastExport::Builtin(format, path)) => {
                self.export_document_to(format, path, window, cx)
            }
            Some(LastExport::Pandoc(format, path)) => {
                self.export_with_pandoc_to(format, path, window, cx)
            }
            None => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    PromptLevel::Info,
                    &strings.menu_export_again,
                    Some(&strings.export_again_nothing),
                    &buttons,
                    cx,
                );
            }
        }
    }

    /// Explains that PDF export needs a Chromium-family browser and offers an
    /// HTML export instead.
    fn show_pdf_browser_missing_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let strings = cx.global::<I18nManager>().strings().clone();
        let buttons = [
            strings.export_as_html_instead.as_str(),
            strings.export_cancel.as_str(),
        ];
        let prompt = window.prompt(
            PromptLevel::Warning,
            &strings.export_pdf_browser_missing_title,
            Some(&strings.export_pdf_browser_missing_message),
            &buttons,
            cx,
        );
        let window_handle = window.window_handle();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            if prompt.await != Ok(0) {
                return;
            }
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = this.update(cx, |editor, cx| {
                        editor.export_document_via_prompt(ExportFormat::Html, window, cx);
                    });
                },
            );
        })
        .detach();
    }
}

/// Presents a localized critical export error with the supplied diagnostic detail.
/// Confirms a finished export and offers to open the file with the system's
/// default application.
fn show_export_succeeded(window: &mut Window, cx: &mut App, path: PathBuf) {
    let strings = cx.global::<I18nManager>().strings().clone();
    let detail = strings
        .export_succeeded_message_template
        .replace("{path}", &path.display().to_string());
    let buttons = [
        strings.export_open_file.as_str(),
        strings.export_close.as_str(),
    ];
    let prompt = window.prompt(
        PromptLevel::Info,
        &strings.export_succeeded_title,
        Some(&detail),
        &buttons,
        cx,
    );
    let window_handle = window.window_handle();
    cx.spawn(async move |cx| {
        if prompt.await != Ok(0) {
            return;
        }
        let _ = cx.update_window(window_handle, move |_view: AnyView, _window, cx| {
            cx.open_with_system(&path);
        });
    })
    .detach();
}

pub(super) fn show_export_error(window: &mut Window, cx: &mut App, detail: &str) {
    let strings = cx.global::<I18nManager>().strings().clone();
    let buttons = [strings.info_dialog_ok.as_str()];
    let _ = window.prompt(
        PromptLevel::Critical,
        &strings.export_failed_title,
        Some(detail),
        &buttons,
        cx,
    );
}

#[cfg(test)]
mod export_again_tests {
    use gpui::TestAppContext;

    use super::LastExport;
    use crate::editor::Editor;
    use crate::export::ExportFormat;

    #[gpui::test]
    async fn exports_are_remembered_for_export_again(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_window, cx| Editor::from_markdown(cx, "# Doc".to_string(), None));
        let path =
            std::env::temp_dir().join(format!("velotype-export-{}.html", uuid::Uuid::new_v4()));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                assert!(editor.last_export.is_none());
                editor.export_document_to(ExportFormat::HtmlPlain, path.clone(), window, cx);
                assert_eq!(
                    editor.last_export,
                    Some(LastExport::Builtin(ExportFormat::HtmlPlain, path.clone()))
                );
            })
        });
    }
}
