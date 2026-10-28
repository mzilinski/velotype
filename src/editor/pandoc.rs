//! File > Export > (Pandoc formats) and File > Import…: run Pandoc in the
//! background and report failures in a dialog.

use std::path::{Path, PathBuf};

use gpui::*;

use super::Editor;
use super::export::show_export_error;
use crate::components::{ExportWithPandoc, ImportWithPandoc};
use crate::export::pandoc;
use crate::i18n::I18nManager;

impl Editor {
    pub(super) fn on_export_with_pandoc(
        &mut self,
        action: &ExportWithPandoc,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(format) = pandoc::export_format(&action.format) else {
            return;
        };
        if pandoc::find_pandoc().is_none() {
            show_pandoc_missing_prompt(window, cx);
            return;
        }
        let (default_dir, suggested_name) = self.export_dialog_defaults_for(format.extension);
        let prompt = cx.prompt_for_new_path(&default_dir, Some(&suggested_name));
        let window_handle = window.window_handle();

        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let mut output = match prompt.await {
                Ok(Ok(Some(path))) => path,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(err)) => {
                    report_error(window_handle, err.to_string(), cx);
                    return;
                }
            };
            if output.extension().is_none() {
                output.set_extension(format.extension);
            }
            let _ = cx.update_window(window_handle, move |_view: AnyView, window, cx| {
                let _ = this.update(cx, |editor, cx| {
                    editor.export_with_pandoc_to(format, output, window, cx);
                });
            });
        })
        .detach();
    }

    /// Converts the document with Pandoc into `output` in the background and
    /// remembers it for File > Export with Previous Settings.
    pub(super) fn export_with_pandoc_to(
        &mut self,
        format: pandoc::PandocFormat,
        output: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(executable) = pandoc::find_pandoc() else {
            show_pandoc_missing_prompt(window, cx);
            return;
        };
        let markdown = self.serialized_document_text(cx);
        let title = self.export_title();
        let resource_dir = self
            .file_path
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf);
        let window_handle = window.window_handle();
        self.last_export = Some(super::export::LastExport::Pandoc(format, output.clone()));
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    pandoc::export(
                        &executable,
                        &markdown,
                        format,
                        &output,
                        resource_dir.as_deref(),
                        &title,
                    )
                })
                .await;
            if let Err(err) = result {
                report_error(window_handle, err.to_string(), cx);
            }
        })
        .detach();
    }

    pub(super) fn on_import_with_pandoc(
        &mut self,
        _: &ImportWithPandoc,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(executable) = pandoc::find_pandoc() else {
            show_pandoc_missing_prompt(window, cx);
            return;
        };
        let title = cx
            .global::<I18nManager>()
            .strings()
            .menu_import
            .trim_end_matches('…')
            .to_string();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some(title.into()),
        });
        let window_handle = window.window_handle();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let input: PathBuf = match prompt.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.remove(0),
                Ok(Ok(_)) | Err(_) => return,
                Ok(Err(err)) => {
                    report_error(window_handle, err.to_string(), cx);
                    return;
                }
            };
            let result = cx
                .background_executor()
                .spawn(async move {
                    let media_dir = pandoc::media_dir_for(&input);
                    pandoc::import(&executable, &input, &media_dir)
                })
                .await;
            match result {
                Ok(markdown) => {
                    cx.update(|cx| {
                        crate::app_menu::open_markdown_in_current_window(cx, markdown, None)
                    });
                }
                Err(err) => report_error(window_handle, err.to_string(), cx),
            }
        })
        .detach();
    }
}

fn report_error(window_handle: AnyWindowHandle, detail: String, cx: &mut AsyncApp) {
    let _ = cx.update_window(window_handle, move |_view: AnyView, window, cx| {
        show_export_error(window, cx, &detail);
    });
}

/// Pandoc is not installed: explain and offer the download page.
fn show_pandoc_missing_prompt(window: &mut Window, cx: &mut App) {
    let strings = cx.global::<I18nManager>().strings().clone();
    let buttons = [
        strings.pandoc_open_download_page.as_str(),
        strings.export_cancel.as_str(),
    ];
    let prompt = window.prompt(
        PromptLevel::Warning,
        &strings.pandoc_missing_title,
        Some(&strings.pandoc_missing_message),
        &buttons,
        cx,
    );
    cx.spawn(async move |cx| {
        if prompt.await == Ok(0) {
            cx.update(|cx| cx.open_url(pandoc::INSTALL_URL));
        }
    })
    .detach();
}
