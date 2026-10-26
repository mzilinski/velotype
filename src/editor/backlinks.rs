//! Sidebar > Links: the documents of the workspace folder that link to the
//! current one, each with the lines holding the links. Clicking a line opens
//! that document at the link.

use std::path::{Path, PathBuf};

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use crate::i18n::I18nStrings;
use crate::theme::Theme;

impl Editor {
    /// Picks up the folder's link index from the shared cache, which reads it
    /// in the background when needed.
    pub(super) fn sync_workspace_links(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.workspace_root() else {
            self.workspace.link_index = None;
            return;
        };
        let revalidate = !std::mem::replace(&mut self.workspace.links_checked, true);
        let index = super::link_index::link_index(&root, revalidate, cx);
        if index.is_some() || self.workspace.link_index_root.as_ref() != Some(&root) {
            self.workspace.link_index = index;
            self.workspace.link_index_root = Some(root);
        }
    }

    /// Opens `path` (or stays in the current document) and selects `label`
    /// on source `line`.
    pub(super) fn open_path_and_reveal(
        &mut self,
        path: PathBuf,
        reveal: Option<(usize, String)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.file_path.as_deref() == Some(path.as_path()) {
            if let Some((line, query)) = reveal {
                self.reveal_search_hit(line, &query, cx);
            }
            return;
        }
        if self.is_tab_hosted() {
            cx.emit(super::EditorTabEvent::OpenPathAndReveal { path, reveal });
        } else {
            self.request_dropped_markdown_replace(path, window, cx);
        }
    }

    pub(super) fn render_workspace_links(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let Some(current) = self.file_path.as_deref() else {
            return self.render_workspace_empty_state(
                "",
                &strings.workspace_backlinks_empty,
                theme,
            );
        };
        let Some(index) = self.workspace.link_index.as_ref() else {
            return self.render_workspace_empty_state("", &strings.workspace_links_loading, theme);
        };
        let backlinks = index.backlinks(current);
        let heading = div()
            .px(px(10.0))
            .pb(px(6.0))
            .text_size(px(t.text_size * 0.78))
            .font_weight(FontWeight::MEDIUM)
            .text_color(c.dialog_muted)
            .child(format!(
                "{} ({})",
                strings.workspace_backlinks_heading,
                backlinks.len()
            ));
        if backlinks.is_empty() {
            return div()
                .w_full()
                .flex()
                .flex_col()
                .child(heading)
                .child(
                    div()
                        .px(px(10.0))
                        .text_size(px(t.text_size * 0.85))
                        .text_color(c.dialog_muted)
                        .child(strings.workspace_backlinks_empty.clone()),
                )
                .into_any_element();
        }

        let groups = backlinks.chunk_by(|left, right| left.source == right.source);
        let rows = groups.enumerate().map(|(group_index, links)| {
            let source = links[0].source.clone();
            let title = source
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let folder = folder_label(index.root(), &source);
            let open_editor = editor.clone();
            let open_path = source.clone();
            let header = div()
                .id(ElementId::named_usize(
                    "workspace-backlink-source",
                    group_index,
                ))
                .w_full()
                .px(px(10.0))
                .py(px(4.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded(px(6.0))
                .hover(|row| row.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w_full()
                        .text_size(px(t.text_size * 0.92))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c.text_default)
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(title),
                )
                .when(!folder.is_empty(), |row| {
                    row.child(
                        div()
                            .min_w(px(0.0))
                            .text_size(px(t.text_size * 0.74))
                            .text_color(c.dialog_muted)
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(folder),
                    )
                })
                .on_click(move |_event, window, cx| {
                    let path = open_path.clone();
                    let _ = open_editor.update(cx, |editor, cx| {
                        editor.open_workspace_file(path, window, cx);
                    });
                });
            let lines = links.iter().enumerate().map(|(link_index, link)| {
                let reveal_editor = editor.clone();
                let path = source.clone();
                let reveal = (link.line, link.label.clone());
                div()
                    .id(ElementId::named_usize(
                        "workspace-backlink",
                        group_index * 10_000 + link_index,
                    ))
                    .w_full()
                    .ml(px(10.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .border_l(px(2.0))
                    .border_color(c.dialog_border)
                    .hover(|row| row.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .text_size(px(t.text_size * 0.8))
                    .line_height(px(t.text_size * 0.8 * 1.4))
                    .text_color(c.dialog_muted)
                    .max_h(px(t.text_size * 0.8 * 1.4 * 3.0))
                    .overflow_hidden()
                    .child(link.context.clone())
                    .on_click(move |_event, window, cx| {
                        let path = path.clone();
                        let reveal = Some(reveal.clone());
                        let _ = reveal_editor.update(cx, |editor, cx| {
                            editor.open_path_and_reveal(path, reveal, window, cx);
                        });
                    })
            });
            div()
                .w_full()
                .pr(px(10.0))
                .flex()
                .flex_col()
                .gap(px(2.0))
                .child(header)
                .children(lines)
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(heading)
            .children(rows)
            .into_any_element()
    }
}

/// Folder of `path` relative to the workspace root, empty at the root.
fn folder_label(root: &Path, path: &Path) -> String {
    let parent = path.parent().unwrap_or(path);
    parent
        .strip_prefix(root)
        .unwrap_or(parent)
        .to_string_lossy()
        .replace('\\', "/")
}
