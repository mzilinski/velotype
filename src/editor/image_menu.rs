//! Format > Image: insert an image from a file dialog, and for the image at
//! the caret: open its location, zoom, switch between Markdown and HTML
//! syntax, copy or move its file to another folder (rewriting the link) and
//! delete its file.

use std::path::{Path, PathBuf};

use gpui::*;

use super::Editor;
use super::images::{image_link_path, markdown_link_destination, unique_file_path};
use crate::components::markdown::image::{ImageResolvedSource, resolve_image_source};
use crate::components::{
    BlockKind, BlockRecord, CopyImage, CopyImageTo, DeleteImageFile, InsertImage, MoveImageTo,
    RevealImageFile, SetImageRoot, SetImageZoom, ToggleImageSyntax, UndoCaptureKind,
};
use crate::i18n::I18nManager;

/// `<img>` for an image without zoom (Format > Image > switch to HTML).
pub(crate) fn html_image_source(src: &str, alt: &str) -> String {
    let escape = |value: &str| {
        value
            .replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    format!("<img src=\"{}\" alt=\"{}\" />", escape(src), escape(alt))
}

/// Where a copied or moved image lands: `folder/<file name>`, numbered when
/// that name is taken (unless it is the file itself).
fn target_in_folder(file: &Path, folder: &Path) -> PathBuf {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "image".to_string());
    let target = folder.join(&name);
    if target == file || !target.exists() {
        target
    } else {
        unique_file_path(folder, &name)
    }
}

/// The image at the caret: its block, Markdown source, alt text and file.
struct CaretImage {
    entity_id: EntityId,
    src: String,
    alt: String,
    file: Option<PathBuf>,
}

impl Editor {
    fn caret_image(&self, window: &Window, cx: &App) -> Option<CaretImage> {
        let entity_id = self
            .document
            .focused_block_entity_id(window, cx)
            .or(self.active_entity_id)?;
        let (src, alt) = self.zoomable_image(entity_id, cx)?;
        let file = match resolve_image_source(
            &crate::components::markdown::image::apply_image_root(
                &src,
                self.image_root_dir.as_deref(),
            ),
            self.image_base_dir().as_deref(),
        ) {
            ImageResolvedSource::Local(path) => path.is_file().then_some(path),
            ImageResolvedSource::Remote(_) => None,
        };
        Some(CaretImage {
            entity_id,
            src,
            alt,
            file,
        })
    }

    /// The image at the caret, or a hint that there is none (or no file).
    fn caret_image_or_hint(
        &self,
        need_file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<CaretImage> {
        let image = self
            .caret_image(window, cx)
            .filter(|image| !need_file || image.file.is_some());
        if image.is_none() {
            let strings = cx.global::<I18nManager>().strings().clone();
            let message = if need_file {
                &strings.image_menu_no_local_image
            } else {
                &strings.image_menu_no_image
            };
            let buttons = [strings.info_dialog_ok.as_str()];
            let _ = window.prompt(
                PromptLevel::Info,
                &strings.menu_image,
                Some(message),
                &buttons,
                cx,
            );
        }
        image
    }

    /// Replaces the image block `entity_id` with `record` as one undo step.
    pub(super) fn replace_image_block(
        &mut self,
        entity_id: EntityId,
        record: BlockRecord,
        cx: &mut Context<Self>,
    ) {
        let Some(location) = self.document.find_block_location(entity_id) else {
            return;
        };
        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        let replacement = Self::new_block(cx, record);
        self.document.insert_blocks_at(
            location.parent.clone(),
            location.index + 1,
            vec![replacement.clone()],
            cx,
        );
        self.document.with_structure_mutation(cx, |document, cx| {
            let _ = document.remove_block_by_id_raw(entity_id, cx);
        });
        if self.active_entity_id == Some(entity_id) {
            self.active_entity_id = Some(replacement.entity_id());
        }
        if self.pending_focus == Some(entity_id) {
            self.pending_focus = None;
        }
        self.rebuild_image_runtimes(cx);
        self.mark_dirty(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }

    pub(super) fn on_insert_image(
        &mut self,
        _: &InsertImage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let title = cx
            .global::<I18nManager>()
            .strings()
            .menu_insert_image
            .clone();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(title.trim_end_matches('…').to_string().into()),
        });
        let window_handle = window.window_handle();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = prompt.await else {
                return;
            };
            let images = paths
                .into_iter()
                .filter(|path| super::images::is_droppable_image_path(path))
                .collect::<Vec<_>>();
            if images.is_empty() {
                return;
            }
            let _ = cx.update_window(window_handle, move |_view: AnyView, window, cx| {
                let _ = this.update(cx, |editor, cx| {
                    editor.insert_dropped_images(&images, window, cx)
                });
            });
        })
        .detach();
    }

    pub(super) fn on_reveal_image_file(
        &mut self,
        _: &RevealImageFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(CaretImage {
            file: Some(file), ..
        }) = self.caret_image_or_hint(true, window, cx)
        {
            cx.reveal_path(&file);
        }
    }

    pub(super) fn on_set_image_zoom(
        &mut self,
        action: &SetImageZoom,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        if let Some(image) = self.caret_image_or_hint(false, window, cx) {
            self.apply_image_zoom(image.entity_id, action.percent, cx);
        }
    }

    pub(super) fn on_toggle_image_syntax(
        &mut self,
        _: &ToggleImageSyntax,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let Some(image) = self.caret_image_or_hint(false, window, cx) else {
            return;
        };
        let is_html = self
            .focusable_entity_by_id(image.entity_id)
            .is_some_and(|block| block.read(cx).kind() == BlockKind::HtmlBlock);
        let record = if is_html {
            // Back to Markdown (drops a zoom, like Typora).
            BlockRecord::paragraph(format!(
                "![{}]({})",
                image.alt.replace('[', "\\[").replace(']', "\\]"),
                markdown_link_destination(&image.src)
            ))
        } else {
            BlockRecord::html(html_image_source(&image.src, &image.alt))
        };
        self.replace_image_block(image.entity_id, record, cx);
    }

    pub(super) fn on_delete_image_file(
        &mut self,
        _: &DeleteImageFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let Some(CaretImage {
            entity_id,
            file: Some(file),
            ..
        }) = self.caret_image_or_hint(true, window, cx)
        else {
            return;
        };
        let strings = cx.global::<I18nManager>().strings().clone();
        let detail = strings
            .image_delete_confirm
            .replace("{file}", &file.display().to_string());
        let buttons = [
            strings.image_delete_button.as_str(),
            strings.export_cancel.as_str(),
        ];
        let prompt = window.prompt(
            PromptLevel::Warning,
            &strings.menu_delete_image_file,
            Some(&detail),
            &buttons,
            cx,
        );
        let window_handle = window.window_handle();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            if prompt.await != Ok(0) {
                return;
            }
            let result = super::file_ops::move_to_trash(&file);
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window, cx| match result {
                    Ok(()) => {
                        let _ = this.update(cx, |editor, cx| {
                            editor.replace_image_block(
                                entity_id,
                                BlockRecord::paragraph(String::new()),
                                cx,
                            )
                        });
                    }
                    Err(err) => show_image_error(window, cx, err.to_string()),
                },
            );
        })
        .detach();
    }

    pub(super) fn on_copy_image_to(
        &mut self,
        _: &CopyImageTo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transfer_image(false, window, cx);
    }

    pub(super) fn on_move_image_to(
        &mut self,
        _: &MoveImageTo,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.transfer_image(true, window, cx);
    }

    /// Copies (or moves) the image file at the caret to a chosen folder and
    /// points the link to the new file.
    fn transfer_image(&mut self, move_file: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.read_only {
            return;
        }
        let Some(CaretImage {
            src,
            file: Some(file),
            ..
        }) = self.caret_image_or_hint(true, window, cx)
        else {
            return;
        };
        let strings = cx.global::<I18nManager>().strings().clone();
        let title = if move_file {
            strings.menu_move_image_to.clone()
        } else {
            strings.menu_copy_image_to.clone()
        };
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(title.trim_end_matches('…').to_string().into()),
        });
        let document_dir = self
            .file_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let window_handle = window.window_handle();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let folder = match prompt.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.remove(0),
                _ => return,
            };
            let target = target_in_folder(&file, &folder);
            let result = if target == file {
                Ok(())
            } else if move_file {
                super::file_info::move_file(&file, &target)
            } else {
                std::fs::copy(&file, &target)
                    .map(|_| ())
                    .map_err(anyhow::Error::from)
            };
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window, cx| match result {
                    Ok(()) => {
                        let link = image_link_path(&target, document_dir.as_deref());
                        let _ = this.update(cx, |editor, cx| {
                            editor.replace_uploaded_images(&[(src, link)], cx)
                        });
                    }
                    Err(err) => show_image_error(window, cx, err.to_string()),
                },
            );
        })
        .detach();
    }
}

/// The gpui image format for a file extension (PNG when unknown).
fn clipboard_image_format(file: &Path) -> ImageFormat {
    let extension = file
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "gif" => ImageFormat::Gif,
        "webp" => ImageFormat::Webp,
        "svg" => ImageFormat::Svg,
        "bmp" => ImageFormat::Bmp,
        "tif" | "tiff" => ImageFormat::Tiff,
        _ => ImageFormat::Png,
    }
}

/// `markdown` with `key: value` set in its YAML front matter (added when
/// there is none, replaced when the key exists).
pub(crate) fn set_front_matter_value(markdown: &str, key: &str, value: &str) -> String {
    let entry = format!("{key}: {value}");
    let mut lines = markdown.split('\n').map(str::to_string).collect::<Vec<_>>();
    let has_front_matter = lines
        .first()
        .is_some_and(|line| line.trim_start_matches('\u{feff}').trim() == "---");
    let end = has_front_matter
        .then(|| {
            lines
                .iter()
                .enumerate()
                .skip(1)
                .find(|(_, line)| matches!(line.trim(), "---" | "..."))
                .map(|(index, _)| index)
        })
        .flatten();
    let Some(end) = end else {
        return format!("---\n{entry}\n---\n\n{markdown}");
    };
    let prefix = format!("{key}:");
    match (1..end).find(|index| lines[*index].trim_start().starts_with(&prefix)) {
        Some(index) => lines[index] = entry,
        None => lines.insert(end, entry),
    }
    lines.join("\n")
}

impl Editor {
    pub(super) fn on_copy_image(
        &mut self,
        _: &CopyImage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(CaretImage {
            file: Some(file), ..
        }) = self.caret_image_or_hint(true, window, cx)
        else {
            return;
        };
        match std::fs::read(&file) {
            Ok(bytes) => cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                clipboard_image_format(&file),
                bytes,
            ))),
            Err(err) => show_image_error(window, cx, err.to_string()),
        }
    }

    /// Format > Image > Set Image Root Folder…: writes `typora-root-url`
    /// into the front matter; image sources starting with `/` are then
    /// taken from that folder.
    pub(super) fn on_set_image_root(
        &mut self,
        _: &SetImageRoot,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.read_only {
            return;
        }
        let title = cx
            .global::<I18nManager>()
            .strings()
            .menu_set_image_root
            .clone();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(title.trim_end_matches('…').to_string().into()),
        });
        let window_handle = window.window_handle();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let folder = match prompt.await {
                Ok(Ok(Some(mut paths))) if !paths.is_empty() => paths.remove(0),
                _ => return,
            };
            let _ = cx.update_window(window_handle, move |_view: AnyView, _window, cx| {
                let _ = this.update(cx, |editor, cx| editor.set_image_root(&folder, cx));
            });
        })
        .detach();
    }

    fn set_image_root(&mut self, folder: &Path, cx: &mut Context<Self>) {
        let value = match self.file_path.as_deref().and_then(Path::parent) {
            Some(dir) => super::images::relative_link_path(dir, folder),
            None => folder.to_string_lossy().replace('\\', "/"),
        };
        let source = self.current_document_source(cx);
        let updated = set_front_matter_value(&source, "typora-root-url", &value);
        let selection = self.capture_source_selection_snapshot(cx).range;
        let end = updated.len();
        self.apply_source_edit(
            super::markdown_commands::SourceEdit {
                source: updated,
                selection: selection.start.min(end)..selection.end.min(end),
            },
            cx,
        );
        self.rebuild_image_runtimes(cx);
    }
}

fn show_image_error(window: &mut Window, cx: &mut App, detail: String) {
    let strings = cx.global::<I18nManager>().strings().clone();
    let buttons = [strings.info_dialog_ok.as_str()];
    let _ = window.prompt(
        PromptLevel::Critical,
        &strings.menu_image,
        Some(&detail),
        &buttons,
        cx,
    );
}

#[cfg(test)]
mod tests {
    use super::{html_image_source, set_front_matter_value, target_in_folder};

    #[test]
    fn front_matter_values_are_added_or_replaced() {
        assert_eq!(
            set_front_matter_value("# Doc", "typora-root-url", "./site"),
            "---\ntypora-root-url: ./site\n---\n\n# Doc"
        );
        assert_eq!(
            set_front_matter_value("---\ntitle: x\n---\n# Doc", "typora-root-url", "../r"),
            "---\ntitle: x\ntypora-root-url: ../r\n---\n# Doc"
        );
        assert_eq!(
            set_front_matter_value(
                "---\ntypora-root-url: old\n---\nbody",
                "typora-root-url",
                "new"
            ),
            "---\ntypora-root-url: new\n---\nbody"
        );
    }

    #[test]
    fn html_syntax_escapes_attributes() {
        assert_eq!(
            html_image_source("a b.png", "say \"hi\""),
            "<img src=\"a b.png\" alt=\"say &quot;hi&quot;\" />"
        );
    }

    #[test]
    fn copies_get_a_free_name_in_the_target_folder() {
        let dir = std::env::temp_dir().join(format!("velotype-imgmenu-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("target")).unwrap();
        let file = dir.join("pic.png");
        std::fs::write(&file, b"png").unwrap();
        assert_eq!(
            target_in_folder(&file, &dir.join("target")),
            dir.join("target").join("pic.png")
        );
        std::fs::write(dir.join("target").join("pic.png"), b"other").unwrap();
        let free = target_in_folder(&file, &dir.join("target"));
        assert_ne!(free, dir.join("target").join("pic.png"));
        assert!(!free.exists());
        // The file's own folder: nothing to copy.
        assert_eq!(target_in_folder(&file, &dir), file);
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod editor_tests {
    use gpui::TestAppContext;

    use crate::components::{SetImageZoom, ToggleImageSyntax};
    use crate::editor::Editor;

    #[gpui::test]
    async fn switch_syntax_and_zoom_the_image_at_the_caret(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "![alt](pic.png)".to_string(), None)
        });
        let focus_first = |editor: &mut Editor| {
            let first = editor.document.first_root().unwrap().entity_id();
            editor.active_entity_id = Some(first);
        };
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                focus_first(editor);
                editor.on_toggle_image_syntax(&ToggleImageSyntax, window, cx);
                assert_eq!(
                    editor.current_document_source(cx),
                    "<img src=\"pic.png\" alt=\"alt\" />"
                );
                focus_first(editor);
                editor.on_toggle_image_syntax(&ToggleImageSyntax, window, cx);
                assert_eq!(editor.current_document_source(cx), "![alt](pic.png)");
                focus_first(editor);
                editor.on_set_image_zoom(&SetImageZoom { percent: 67 }, window, cx);
                assert_eq!(
                    editor.current_document_source(cx),
                    "<img src=\"pic.png\" alt=\"alt\" style=\"zoom:67%;\" />"
                );
                // One undo step per change.
                editor.undo_document(cx);
                assert_eq!(editor.current_document_source(cx), "![alt](pic.png)");
            })
        });
    }
}
