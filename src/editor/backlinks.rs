//! Sidebar > Links: the documents of the workspace folder that link to the
//! current one, each with the lines holding the links. Clicking a line opens
//! that document at the link.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::Editor;
use super::link_index::{LinkIndex, TagCount};
use crate::i18n::I18nStrings;
use crate::theme::Theme;

impl Editor {
    /// Picks up the folder's link index from the shared cache, which reads it
    /// in the background when needed.
    pub(super) fn sync_workspace_links(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.link_root() else {
            self.workspace.link_index = None;
            return;
        };
        let revalidate = !std::mem::replace(&mut self.workspace.links_checked, true);
        let index = super::link_index::link_index(&root, revalidate, cx);
        if index.is_some() || self.workspace.link_index_root.as_ref() != Some(&root) {
            self.workspace.link_index = index;
            self.workspace.link_index_root = Some(root);
        }
        self.sync_workspace_outgoing();
        self.sync_workspace_mentions(cx);
    }

    /// Reads the outgoing links again when the text or the index changed.
    fn sync_workspace_outgoing(&mut self) {
        let (Some(index), Some(path)) =
            (self.workspace.link_index.as_ref(), self.file_path.as_ref())
        else {
            self.workspace.outgoing = None;
            return;
        };
        if self
            .workspace
            .outgoing
            .as_ref()
            .is_some_and(|(source, known, _)| {
                Arc::ptr_eq(known, index) && *source == self.last_stable_source_text
            })
        {
            return;
        }
        let source = path.canonicalize().unwrap_or_else(|_| path.clone());
        let links = super::mentions::outgoing_links(index, &source, &self.last_stable_source_text);
        self.workspace.outgoing =
            Some((self.last_stable_source_text.clone(), index.clone(), links));
    }

    /// The folder whose links count: the opened folder, else the vault (the
    /// nearest folder above the document with `.obsidian` or `.git`, below the
    /// home folder), else the document's folder.
    pub(super) fn link_root(&self) -> Option<PathBuf> {
        if let Some(folder) = self.workspace.folder.clone() {
            return Some(folder);
        }
        let parent = self.file_path.as_deref()?.parent()?;
        Some(
            vault_root(parent, std::env::home_dir().as_deref())
                .unwrap_or(parent)
                .to_path_buf(),
        )
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
        if let Some(tag) = self.workspace.selected_tag.as_deref() {
            return self.render_tag_documents(index, tag, theme, editor);
        }
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(18.0))
            .child(self.render_backlinks_section(index, current, theme, strings, editor))
            .child(self.render_outgoing_section(index, theme, strings, editor))
            .child(self.render_mentions_section(index, theme, strings, editor))
            .child(self.render_tags_section(index, current, theme, strings, editor))
            .into_any_element()
    }

    fn render_backlinks_section(
        &self,
        index: &LinkIndex,
        current: &Path,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let backlinks = index.backlinks(current);
        let heading = section_heading(
            format!(
                "{} ({})",
                strings.workspace_backlinks_heading,
                backlinks.len()
            ),
            theme,
        );
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

/// Small muted title above a section of the Links tab.
fn section_heading(text: String, theme: &Theme) -> Div {
    div()
        .px(px(10.0))
        .pb(px(6.0))
        .text_size(px(theme.typography.text_size * 0.78))
        .font_weight(FontWeight::MEDIUM)
        .text_color(theme.colors.dialog_muted)
        .child(text)
}

impl Editor {
    /// The tags of the current document and of the whole vault as chips;
    /// clicking one lists its documents.
    fn render_tags_section(
        &self,
        index: &LinkIndex,
        current: &Path,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let own = index.tags_of(current);
        let all = index.all_tags();
        let chips = |id: &'static str, tags: Vec<TagCount>| {
            div().px(px(8.0)).flex().flex_wrap().gap(px(4.0)).children(
                tags.into_iter()
                    .take(MAX_TAG_CHIPS)
                    .enumerate()
                    .map(|(position, tag)| tag_chip(id, position, tag, theme, editor)),
            )
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(14.0))
            .when(!own.is_empty(), |section| {
                section.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(section_heading(
                            strings.workspace_tags_document.clone(),
                            theme,
                        ))
                        .child(chips("workspace-own-tag", own)),
                )
            })
            .when(!all.is_empty(), |section| {
                section.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(section_heading(
                            format!("{} ({})", strings.workspace_tags_all, all.len()),
                            theme,
                        ))
                        .child(chips("workspace-tag", all)),
                )
            })
            .into_any_element()
    }

    /// The notes the current document links to; missing ones are muted.
    fn render_outgoing_section(
        &self,
        index: &LinkIndex,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let links = self
            .workspace
            .outgoing
            .as_ref()
            .map(|(_, _, links)| links.as_slice())
            .unwrap_or_default();
        let rows = links.iter().enumerate().map(|(position, link)| {
            let (title, folder) = match &link.target {
                Some(path) => (
                    path.file_stem()
                        .map(|stem| stem.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    folder_label(index.root(), path),
                ),
                None => (
                    link.written.clone(),
                    strings.workspace_outgoing_missing.clone(),
                ),
            };
            let row = div()
                .id(ElementId::named_usize("workspace-outgoing", position))
                .w_full()
                .px(px(10.0))
                .py(px(4.0))
                .flex()
                .items_center()
                .gap(px(6.0))
                .rounded(px(6.0))
                .child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(t.text_size * 0.92))
                        .text_color(if link.target.is_some() {
                            c.text_default
                        } else {
                            c.dialog_muted
                        })
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
                });
            match link.target.clone() {
                Some(path) => {
                    let open_editor = editor.clone();
                    row.hover(|row| row.bg(c.dialog_secondary_button_hover))
                        .cursor_pointer()
                        .on_click(move |_event, window, cx| {
                            let path = path.clone();
                            let _ = open_editor.update(cx, |editor, cx| {
                                editor.open_workspace_file(path, window, cx);
                            });
                        })
                }
                None => row,
            }
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(section_heading(
                format!("{} ({})", strings.workspace_outgoing_heading, links.len()),
                theme,
            ))
            .children(rows)
            .into_any_element()
    }

    /// Other documents naming the current one in plain text, each line
    /// opening that document at the mention.
    fn render_mentions_section(
        &self,
        index: &LinkIndex,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let mentions = self
            .workspace
            .mentions
            .as_ref()
            .and_then(|state| state.mentions.as_deref());
        let heading = match mentions {
            Some(mentions) => format!(
                "{} ({})",
                strings.workspace_mentions_heading,
                mentions.len()
            ),
            None => format!("{} …", strings.workspace_mentions_heading),
        };
        let rows = mentions
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(position, mention)| {
                let reveal_editor = editor.clone();
                let path = mention.source.clone();
                let reveal = (mention.line, mention.text.clone());
                let mut title = mention
                    .source
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let folder = folder_label(index.root(), &mention.source);
                if !folder.is_empty() {
                    title = format!("{title} · {folder}");
                }
                div()
                    .id(ElementId::named_usize("workspace-mention", position))
                    .w_full()
                    .px(px(10.0))
                    .py(px(4.0))
                    .flex()
                    .flex_col()
                    .rounded(px(6.0))
                    .hover(|row| row.bg(c.dialog_secondary_button_hover))
                    .cursor_pointer()
                    .child(
                        div()
                            .text_size(px(t.text_size * 0.86))
                            .text_color(c.text_default)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(px(t.text_size * 0.8))
                            .line_height(px(t.text_size * 0.8 * 1.4))
                            .text_color(c.dialog_muted)
                            .max_h(px(t.text_size * 0.8 * 1.4 * 2.0))
                            .overflow_hidden()
                            .child(mention.context.clone()),
                    )
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
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(section_heading(heading, theme))
            .children(rows)
            .into_any_element()
    }

    /// The documents carrying `tag`, below a row leading back.
    fn render_tag_documents(
        &self,
        index: &LinkIndex,
        tag: &str,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let documents = index.documents_with_tag(tag);
        let back_editor = editor.clone();
        let back = div()
            .id("workspace-tag-back")
            .px(px(10.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .hover(|row| row.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .text_size(px(t.text_size * 0.92))
            .font_weight(FontWeight::MEDIUM)
            .text_color(c.text_default)
            .child(format!("← #{tag} ({})", documents.len()))
            .on_click(move |_event, _window, cx| {
                let _ = back_editor.update(cx, |editor, cx| {
                    editor.workspace.selected_tag = None;
                    cx.notify();
                });
            });
        let rows = documents.into_iter().enumerate().map(|(position, path)| {
            let title = path
                .file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_default();
            let folder = folder_label(index.root(), &path);
            let open_editor = editor.clone();
            div()
                .id(ElementId::named_usize("workspace-tag-document", position))
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
                        .text_size(px(t.text_size * 0.92))
                        .text_color(c.text_default)
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
                    let path = path.clone();
                    let _ = open_editor.update(cx, |editor, cx| {
                        editor.open_workspace_file(path, window, cx);
                    });
                })
        });
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .child(back)
            .children(rows)
            .into_any_element()
    }
}

/// Tag chips shown per section at most.
const MAX_TAG_CHIPS: usize = 80;

fn tag_chip(
    id: &'static str,
    position: usize,
    tag: TagCount,
    theme: &Theme,
    editor: &WeakEntity<Editor>,
) -> Stateful<Div> {
    let c = &theme.colors;
    let t = &theme.typography;
    let editor = editor.clone();
    let name = tag.name.clone();
    div()
        .id(ElementId::named_usize(id, position))
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(10.0))
        .border(px(1.0))
        .border_color(c.dialog_border)
        .hover(|chip| chip.bg(c.dialog_secondary_button_hover))
        .cursor_pointer()
        .text_size(px(t.text_size * 0.78))
        .text_color(c.text_default)
        .child(format!("#{} {}", tag.name, tag.documents))
        .on_click(move |_event, _window, cx| {
            let name = name.clone();
            let _ = editor.update(cx, |editor, cx| {
                editor.workspace.selected_tag = Some(name);
                cx.notify();
            });
        })
}

/// The nearest folder from `dir` upwards holding `.obsidian` or `.git`,
/// never `home` or a folder above it.
fn vault_root<'a>(dir: &'a Path, home: Option<&Path>) -> Option<&'a Path> {
    dir.ancestors()
        .take_while(|ancestor| {
            ancestor.parent().is_some() && home.is_none_or(|home| !home.starts_with(ancestor))
        })
        .find(|ancestor| ancestor.join(".obsidian").is_dir() || ancestor.join(".git").exists())
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

#[cfg(test)]
mod tests {
    use super::vault_root;

    #[test]
    fn vault_root_is_the_nearest_marked_folder_below_home() {
        let home = std::env::temp_dir().join(format!("velotype-home-{}", uuid::Uuid::new_v4()));
        let vault = home.join("notes");
        let deep = vault.join("people").join("team");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::create_dir_all(home.join(".git")).unwrap();
        assert_eq!(vault_root(&deep, Some(&home)), None);
        std::fs::create_dir_all(vault.join(".obsidian")).unwrap();
        assert_eq!(vault_root(&deep, Some(&home)), Some(vault.as_path()));
        let _ = std::fs::remove_dir_all(home);
    }
}
