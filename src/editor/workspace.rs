//! Lightweight workspace panel state, file-tree scanning, and outline parsing.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use gpui::prelude::FluentBuilder as _;
use gpui::*;

use super::{BlockKind, Editor};
use crate::i18n::I18nStrings;
use crate::theme::Theme;

/// Asset path for directory nodes.
const FOLDER_ICON: &str = "icon/workspace/folder.svg";
/// Asset path for Markdown file nodes.
const MARKDOWN_ICON: &str = "icon/workspace/markdown.svg";
/// Fraction of viewport width requested by the workspace panel.
const WORKSPACE_PANEL_TARGET_RATIO: f32 = 0.15;
/// Lower width bound for the workspace panel, in pixels.
const WORKSPACE_PANEL_MIN_WIDTH: f32 = 240.0;
/// Upper width bound for the workspace panel, in pixels.
const WORKSPACE_PANEL_MAX_WIDTH: f32 = 360.0;
/// Height of each visible tree row, in pixels.
const WORKSPACE_NODE_HEIGHT: f32 = 28.0;
/// Additional left inset per tree depth, in pixels.
const WORKSPACE_NODE_INDENT: f32 = 18.0;
/// Folder entries read for one file tree at most, so a huge folder (a home
/// folder, a whole drive) cannot keep the scan busy for minutes.
const WORKSPACE_SCAN_LIMIT: usize = 20_000;
/// A cached tree older than this is read again (in the background) when a tab
/// starts showing it or the drawer opens.
const WORKSPACE_TREE_MAX_AGE: Duration = Duration::from_secs(10);
/// Roots whose trees are kept at most.
const WORKSPACE_TREE_CACHE_SIZE: usize = 16;

/// The active workspace panel view.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum WorkspaceTab {
    /// Directory and Markdown file tree.
    #[default]
    Files,
    /// Headings parsed from the current document.
    Outline,
    Articles,
    /// Documents linking to the current one.
    Links,
}

/// Navigation target represented by a workspace tree node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum WorkspaceTreeKind {
    /// A directory that may contain further visible nodes.
    Directory(PathBuf),
    /// An openable Markdown file.
    MarkdownFile(PathBuf),
    /// A heading at a zero-based source line and ATX level.
    Heading { line: usize, level: u8 },
}

/// A file-tree or outline entry with stable identity and nested children.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkspaceTreeNode {
    /// Stable path- or source-line-based node identifier.
    id: String,
    /// Text shown for the node.
    label: String,
    /// Navigation action and underlying target.
    kind: WorkspaceTreeKind,
    /// Descendants displayed when the node is expanded.
    children: Vec<WorkspaceTreeNode>,
}

/// Selected file path or outline node identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
enum WorkspaceSelection {
    /// Selected file in the Files tab.
    File(PathBuf),
    /// Selected node ID in the Outline tab.
    Outline(String),
}

/// Cached workspace trees and interaction state for one editor window.
#[derive(Default)]
pub(super) struct WorkspaceState {
    /// Whether the drawer is visible.
    pub(super) is_open: bool,
    pub(super) active_tab: WorkspaceTab,
    /// Folder opened with Open Folder; shared by all tabs of a window. Without
    /// one the workspace is the folder of the current file.
    pub(super) folder: Option<PathBuf>,
    /// Options of Search in Folder, kept between palette openings.
    pub(super) search_options: super::workspace_search::SearchOptions,
    pub(super) root: Option<PathBuf>,
    /// The root's tree from [`WorkspaceTreeCache`]; None while it is read.
    file_tree: Option<Arc<WorkspaceTreeNode>>,
    file_error: Option<String>,
    /// The tree stopped at [`WORKSPACE_SCAN_LIMIT`] entries.
    tree_truncated: bool,
    /// The cached tree was checked for age since the root changed or the
    /// drawer opened.
    tree_checked: bool,
    /// Headings parsed from the current source.
    outline_tree: Vec<WorkspaceTreeNode>,
    /// Source text corresponding to `outline_tree`.
    outline_source: Option<String>,
    pub(super) expanded: HashSet<String>,
    selected: Option<WorkspaceSelection>,
    /// Open right-click menu of the file tree and name dialog, if any.
    pub(super) tree_menu: Option<super::workspace_actions::TreeMenu>,
    pub(super) name_dialog: Option<super::workspace_actions::NameDialog>,
    /// Articles tab: the folder's documents, newest first (None = not read).
    pub(super) articles: Option<Vec<super::articles::Article>>,
    pub(super) articles_task: Option<Task<()>>,
    /// Links tab: the index of `link_index_root` (None = not read yet).
    pub(super) link_index: Option<Arc<super::link_index::LinkIndex>>,
    pub(super) link_index_root: Option<PathBuf>,
    /// The cached index was checked for age since the drawer opened, the tab
    /// was shown or the document was saved.
    pub(super) links_checked: bool,
}

impl Editor {
    /// Toggles the workspace drawer and refreshes its models when opening.
    pub(crate) fn toggle_workspace_drawer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace.is_open {
            self.workspace.is_open = false;
        } else {
            self.close_menu_bar(cx);
            self.dismiss_contextual_overlays(cx);
            self.workspace.is_open = true;
            self.workspace.tree_checked = false;
            self.workspace.links_checked = false;
            self.sync_workspace_models(cx);
            window.activate_window();
        }
        cx.notify();
    }

    /// Handles the workspace toggle application action.
    pub(crate) fn on_toggle_workspace_action(
        &mut self,
        _: &crate::components::ToggleWorkspace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_drawer(window, cx);
    }

    /// Invalidates workspace caches after the current document path changes.
    pub(super) fn sync_workspace_after_document_path_change(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.as_deref() {
            super::link_index::mark_link_index_stale(path, cx);
        }
        self.workspace.links_checked = false;
        self.workspace.root = None;
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.outline_source = None;
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
    }

    /// Drops the cached file tree so the drawer rescans the disk.
    pub(super) fn invalidate_workspace_tree(&mut self, cx: &mut Context<Self>) {
        if let Some(root) = self.workspace_root() {
            forget_workspace_tree(&root, cx);
        }
        self.reset_workspace_tree(cx);
    }

    /// Lets this tab pick up its root's tree again, after the cache entry was
    /// dropped (see [`forget_workspace_tree`]).
    pub(super) fn reset_workspace_tree(&mut self, cx: &mut Context<Self>) {
        self.workspace.root = None;
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        self.workspace.articles = None;
        self.workspace.articles_task = None;
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
        cx.notify();
    }

    /// Refreshes both file and outline views from current editor state.
    fn sync_workspace_models(&mut self, cx: &mut Context<Self>) {
        self.sync_workspace_file_tree(cx);
        self.sync_workspace_outline();
        match self.workspace.active_tab {
            WorkspaceTab::Articles => self.sync_workspace_articles(cx),
            WorkspaceTab::Links => self.sync_workspace_links(cx),
            WorkspaceTab::Files | WorkspaceTab::Outline => {}
        }
    }

    /// Opens the sidebar on `tab` (View > File Tree / Outline / Articles).
    pub(super) fn show_workspace_tab(
        &mut self,
        tab: WorkspaceTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.workspace.is_open {
            self.toggle_workspace_drawer(window, cx);
        }
        self.set_workspace_tab(tab, cx);
    }

    pub(super) fn on_show_file_tree(
        &mut self,
        _: &crate::components::ShowFileTree,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_workspace_tab(WorkspaceTab::Files, window, cx);
    }

    /// Uses the current file's parent directory as the workspace root.
    pub(super) fn on_show_outline(
        &mut self,
        _: &crate::components::ShowOutline,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_workspace_tab(WorkspaceTab::Outline, window, cx);
    }

    /// Scans a changed root and updates the selected file path.
    pub(super) fn on_show_articles(
        &mut self,
        _: &crate::components::ShowArticles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_workspace_tab(WorkspaceTab::Articles, window, cx);
    }

    pub(super) fn on_show_links(
        &mut self,
        _: &crate::components::ShowLinks,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.show_workspace_tab(WorkspaceTab::Links, window, cx);
    }

    /// Names in the drawer's file tree below the root, or None while it is
    /// read.
    #[cfg(test)]
    pub(crate) fn workspace_tree_labels(&mut self, cx: &mut Context<Self>) -> Option<Vec<String>> {
        self.sync_workspace_models(cx);
        let tree = self.workspace.file_tree.as_ref()?;
        Some(
            tree.children
                .iter()
                .map(|node| node.label.clone())
                .collect(),
        )
    }

    /// The opened folder, or else the folder of the current file.
    pub(super) fn workspace_root(&self) -> Option<PathBuf> {
        self.workspace
            .folder
            .clone()
            .or_else(|| self.file_path.as_ref()?.parent().map(Path::to_path_buf))
    }

    /// Sets the opened folder of this editor and rescans the drawer. Opening a
    /// folder also shows the drawer on its file tree.
    pub(super) fn set_workspace_folder(&mut self, folder: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.workspace.folder == folder {
            return;
        }
        let opened = folder.is_some();
        self.workspace.folder = folder;
        self.workspace.root = None;
        self.workspace.file_tree = None;
        self.workspace.file_error = None;
        if opened {
            self.workspace.is_open = true;
            self.workspace.active_tab = WorkspaceTab::Files;
        }
        if self.workspace.is_open {
            self.sync_workspace_models(cx);
        }
        cx.notify();
    }

    pub(crate) fn on_open_folder(
        &mut self,
        _: &crate::components::OpenFolder,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.prompt_open_folder(cx);
    }

    pub(crate) fn on_close_folder(
        &mut self,
        _: &crate::components::CloseFolder,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.apply_workspace_folder(None, cx);
    }

    /// Asks for a directory and opens it as the workspace of this window.
    pub(crate) fn prompt_open_folder(&mut self, cx: &mut Context<Self>) {
        let prompt_title = cx
            .global::<crate::i18n::I18nManager>()
            .strings()
            .open_folder_prompt
            .clone();
        let prompt = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some(prompt_title.into()),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = prompt.await
                && let Some(folder) = paths.into_iter().next()
            {
                let _ = this.update(cx, |this, cx| this.apply_workspace_folder(Some(folder), cx));
            }
        })
        .detach();
    }

    /// Opens or closes `folder` for every tab of this window and remembers it
    /// for the next start.
    pub(crate) fn apply_workspace_folder(
        &mut self,
        folder: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let _ = crate::config::write_last_workspace_folder(folder.as_deref());
        if let Some(host) = self.tab_host.as_ref().and_then(WeakEntity::upgrade) {
            // The host updates this editor too; defer so it is not borrowed.
            cx.defer(move |cx| {
                host.update(cx, |host, cx| host.set_workspace_folder(folder, cx));
            });
        } else {
            self.set_workspace_folder(folder, cx);
        }
    }

    /// Shows the root's tree from the shared cache, which reads it in the
    /// background when needed. Runs on every render of the drawer, so it does
    /// no disk access itself.
    fn sync_workspace_file_tree(&mut self, cx: &mut Context<Self>) {
        let next_root = self.workspace_root();
        if self.workspace.root != next_root {
            self.workspace.root = next_root.clone();
            self.workspace.file_tree = None;
            self.workspace.file_error = None;
            self.workspace.tree_truncated = false;
            self.workspace.tree_checked = false;
            if let Some(root) = next_root.as_deref() {
                self.workspace.expanded.insert(file_node_id(root));
            }
        }
        self.workspace.selected = self
            .file_path
            .as_ref()
            .map(|path| WorkspaceSelection::File(path.clone()));

        let Some(root) = next_root else {
            self.workspace.selected = None;
            return;
        };

        // Validate the root path
        if root.as_os_str().is_empty() {
            self.workspace.file_error = Some("Invalid workspace path: empty path".to_string());
            self.workspace.selected = None;
            return;
        }

        let revalidate = !std::mem::replace(&mut self.workspace.tree_checked, true);
        match workspace_tree(&root, revalidate, cx) {
            Some(Ok(scanned)) => {
                let current = self
                    .workspace
                    .file_tree
                    .as_ref()
                    .is_some_and(|tree| Arc::ptr_eq(tree, &scanned.root));
                if !current {
                    self.workspace.file_tree = Some(scanned.root);
                    self.workspace.tree_truncated = scanned.truncated;
                    self.workspace.file_error = None;
                }
            }
            Some(Err(error)) => {
                self.workspace.file_tree = None;
                self.workspace.file_error = Some(error);
            }
            None => {}
        }
    }

    /// Rebuilds the heading outline only when serialized source changed.
    /// Rebuilds the outline when the document changed. Reuses the source the
    /// editor keeps after every edit instead of serializing the document on
    /// each render of the drawer.
    fn sync_workspace_outline(&mut self) {
        if self.workspace.outline_source.as_deref() == Some(self.last_stable_source_text.as_str()) {
            return;
        }

        let source = self.last_stable_source_text.clone();
        let outline = build_outline_tree(&source);
        prune_outline_state(&mut self.workspace, &outline);
        self.workspace.outline_tree = outline;
        self.workspace.outline_source = Some(source);
    }

    /// Selects a panel tab and refreshes its models.
    fn set_workspace_tab(&mut self, tab: WorkspaceTab, cx: &mut Context<Self>) {
        if self.workspace.active_tab != tab {
            self.workspace.active_tab = tab;
            self.workspace.links_checked = false;
            self.sync_workspace_models(cx);
            cx.notify();
        }
    }

    /// Toggles expansion of a directory or heading node by ID.
    fn toggle_workspace_node(&mut self, id: &str, cx: &mut Context<Self>) {
        if !self.workspace.expanded.remove(id) {
            self.workspace.expanded.insert(id.to_string());
        }
        cx.notify();
    }

    /// Selects an outline heading and moves editor focus to its source line.
    fn select_outline_node(&mut self, id: String, line: usize, cx: &mut Context<Self>) {
        self.workspace.selected = Some(WorkspaceSelection::Outline(id));
        self.jump_to_source_line(line, cx);
        cx.notify();
    }

    /// Focuses the block that renders `line` of the serialized document and
    /// scrolls it into view, placing the caret at the start of its content.
    pub(super) fn jump_to_source_line(&mut self, line: usize, cx: &mut Context<Self>) -> bool {
        let source = self.current_document_source(cx);
        let Some(offset) = source_line_start(&source, line) else {
            return false;
        };
        let mappings = self.build_source_target_mappings(cx);
        let Some(endpoint) = self.endpoint_for_source_offset(offset, &mappings, cx) else {
            return false;
        };
        let Some(block) = self.focusable_entity_by_id(endpoint.entity_id) else {
            return false;
        };
        self.cross_block_selection = None;
        self.focus_block_range(&block, endpoint.offset..endpoint.offset, cx);
        true
    }

    /// Selects a file and routes it through the guarded document replacement flow.
    pub(super) fn open_workspace_file(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace.selected = Some(WorkspaceSelection::File(path.clone()));
        if self.is_tab_hosted() {
            cx.emit(super::EditorTabEvent::OpenPaths {
                paths: vec![path],
                open_workspace: true,
            });
            return;
        }
        self.request_dropped_markdown_replace(path, window, cx);
    }

    /// Opens the drawer without toggling or window activation, used when a tab
    /// opened from another tab's drawer should keep showing it.
    pub(super) fn open_workspace_drawer(&mut self, cx: &mut Context<Self>) {
        if !self.workspace.is_open {
            self.workspace.is_open = true;
            self.sync_workspace_models(cx);
            cx.notify();
        }
    }

    /// Renders the active workspace tab when the drawer is open.
    pub(super) fn render_workspace_panel(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        panel_width: f32,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.workspace.is_open {
            return None;
        }

        self.sync_workspace_models(cx);
        let editor = cx.entity().downgrade();
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;

        let tab = |label: String, tab: WorkspaceTab, active: bool| {
            let tab_editor = editor.clone();
            let tab_id = match tab {
                WorkspaceTab::Files => "workspace-tab-files",
                WorkspaceTab::Outline => "workspace-tab-outline",
                WorkspaceTab::Articles => "workspace-tab-articles",
                WorkspaceTab::Links => "workspace-tab-links",
            };
            div()
                .id(tab_id)
                .h(px(30.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(6.0))
                .bg(if active {
                    c.selection
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .text_size(px(t.text_size * 0.88))
                .text_color(if active {
                    c.text_default
                } else {
                    c.dialog_muted
                })
                .child(label)
                .on_click(move |_event, _window, cx| {
                    let _ = tab_editor.update(cx, |editor, cx| {
                        editor.set_workspace_tab(tab, cx);
                    });
                })
        };

        let body = match self.workspace.active_tab {
            WorkspaceTab::Files => self.render_workspace_files_tree(theme, strings, &editor),
            WorkspaceTab::Outline => self.render_workspace_outline_tree(theme, strings, &editor),
            WorkspaceTab::Articles => self.render_workspace_articles(theme, strings, &editor),
            WorkspaceTab::Links => self.render_workspace_links(theme, strings, &editor),
        };

        Some(
            div()
                .id("workspace-panel")
                .h_full()
                .w(px(panel_width))
                .flex()
                .flex_col()
                .flex_shrink_0()
                .bg(c.dialog_surface)
                .border_r(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .child(
                    div()
                        .px(px(12.0))
                        .pt(px(12.0))
                        .pb(px(8.0))
                        .flex()
                        .gap(px(8.0))
                        .border_b(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .child(tab(
                            strings.workspace_tab_files.clone(),
                            WorkspaceTab::Files,
                            self.workspace.active_tab == WorkspaceTab::Files,
                        ))
                        .child(tab(
                            strings.workspace_tab_outline.clone(),
                            WorkspaceTab::Outline,
                            self.workspace.active_tab == WorkspaceTab::Outline,
                        ))
                        .child(tab(
                            strings.workspace_tab_articles.clone(),
                            WorkspaceTab::Articles,
                            self.workspace.active_tab == WorkspaceTab::Articles,
                        ))
                        .child(tab(
                            strings.workspace_tab_links.clone(),
                            WorkspaceTab::Links,
                            self.workspace.active_tab == WorkspaceTab::Links,
                        )),
                )
                .child(
                    div()
                        .id("workspace-panel-scroll")
                        .flex_1()
                        .min_h(px(0.0))
                        .overflow_y_scroll()
                        .px(px(8.0))
                        .py(px(10.0))
                        .child(body),
                )
                .when(self.workspace.active_tab == WorkspaceTab::Files, |panel| {
                    panel.child(self.render_workspace_folder_actions(theme, strings, &editor))
                })
                .into_any_element(),
        )
    }

    /// "Open Folder…" and, with a folder open, "Close Folder" below the tree.
    fn render_workspace_folder_actions(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let button = |id: &'static str, label: String| {
            div()
                .id(id)
                .h(px(28.0))
                .px(px(10.0))
                .flex()
                .items_center()
                .rounded(px(6.0))
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .text_size(px(t.text_size * 0.85))
                .text_color(c.dialog_muted)
                .child(label)
        };
        let open_editor = editor.clone();
        let close_editor = editor.clone();
        div()
            .px(px(8.0))
            .py(px(6.0))
            .flex()
            .flex_wrap()
            .gap(px(4.0))
            .border_t(px(d.dialog_border_width))
            .border_color(c.dialog_border)
            .child(
                button("workspace-open-folder", strings.menu_open_folder.clone()).on_click(
                    move |_, _, cx| {
                        let _ = open_editor.update(cx, |editor, cx| editor.prompt_open_folder(cx));
                    },
                ),
            )
            .when(self.workspace.folder.is_some(), |row| {
                row.child(
                    button("workspace-close-folder", strings.menu_close_folder.clone()).on_click(
                        move |_, _, cx| {
                            let _ = close_editor
                                .update(cx, |editor, cx| editor.apply_workspace_folder(None, cx));
                        },
                    ),
                )
            })
            .into_any_element()
    }

    /// Renders the file tree or its empty/error state.
    fn render_workspace_files_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.root.is_none() {
            return self.render_workspace_empty_state(
                &strings.workspace_no_file_title,
                &strings.workspace_no_file_message,
                theme,
            );
        }

        if let Some(error) = self.workspace.file_error.as_ref() {
            return self.render_workspace_empty_state(
                &strings.workspace_scan_failed_title,
                error,
                theme,
            );
        }

        let Some(root) = self.workspace.file_tree.as_deref() else {
            return self.render_workspace_empty_state("", &strings.workspace_loading_files, theme);
        };

        let truncated = self.workspace.tree_truncated.then(|| {
            div()
                .px(px(8.0))
                .py(px(6.0))
                .text_size(px(theme.typography.text_size * 0.8))
                .text_color(theme.colors.dialog_muted)
                .child(strings.workspace_tree_truncated.clone())
        });

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(std::slice::from_ref(root), 0, theme, editor))
            .children(truncated)
            .into_any_element()
    }

    /// Renders the heading outline or its empty state.
    fn render_workspace_outline_tree(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        if self.workspace.outline_tree.is_empty() {
            return self.render_workspace_empty_state("", &strings.workspace_empty_outline, theme);
        }

        div()
            .w_full()
            .flex()
            .flex_col()
            .children(self.render_workspace_nodes(&self.workspace.outline_tree, 0, theme, editor))
            .into_any_element()
    }

    /// Renders a centered workspace empty-state message.
    pub(super) fn render_workspace_empty_state(
        &self,
        title: &str,
        message: &str,
        theme: &Theme,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let title = (!title.is_empty()).then(|| {
            div()
                .text_size(px(t.text_size))
                .font_weight(FontWeight::MEDIUM)
                .text_color(c.text_default)
                .child(title.to_string())
        });

        div()
            .w_full()
            .h_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.0))
            .px(px(22.0))
            .text_align(TextAlign::Center)
            .children(title)
            .child(
                div()
                    .text_size(px(t.text_size * 0.9))
                    .line_height(px(t.text_size * t.text_line_height))
                    .text_color(c.dialog_muted)
                    .child(message.to_string()),
            )
            .into_any_element()
    }

    /// Flattens expanded nodes into visible rows at their nesting depths.
    fn render_workspace_nodes(
        &self,
        nodes: &[WorkspaceTreeNode],
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> Vec<AnyElement> {
        let mut elements = Vec::new();
        for node in nodes {
            elements.push(self.render_workspace_node(node, depth, theme, editor));
            if !node.children.is_empty() && self.workspace.expanded.contains(&node.id) {
                elements.extend(self.render_workspace_nodes(
                    &node.children,
                    depth + 1,
                    theme,
                    editor,
                ));
            }
        }
        elements
    }

    /// Renders a file, directory, or heading row with its navigation handler.
    fn render_workspace_node(
        &self,
        node: &WorkspaceTreeNode,
        depth: usize,
        theme: &Theme,
        editor: &WeakEntity<Editor>,
    ) -> AnyElement {
        let c = &theme.colors;
        let t = &theme.typography;
        let is_expanded = self.workspace.expanded.contains(&node.id);
        let has_children = !node.children.is_empty();
        let selected = match (&self.workspace.selected, &node.kind) {
            (Some(WorkspaceSelection::File(selected)), WorkspaceTreeKind::MarkdownFile(path)) => {
                selected == path
            }
            (Some(WorkspaceSelection::Outline(selected)), _) => selected == &node.id,
            _ => false,
        };
        let node_id = node.id.clone();
        let click_editor = editor.clone();
        let click_kind = node.kind.clone();
        let arrow_node_id = node.id.clone();
        let arrow_editor = editor.clone();
        let arrow = if has_children {
            if is_expanded { "v" } else { ">" }
        } else {
            ""
        };

        let icon = match &node.kind {
            WorkspaceTreeKind::Directory(_) => Some((FOLDER_ICON, Hsla::from(rgba(0xf59e0bff)))),
            WorkspaceTreeKind::MarkdownFile(_) => {
                Some((MARKDOWN_ICON, Hsla::from(rgba(0x2563ebff))))
            }
            WorkspaceTreeKind::Heading { .. } => None,
        };

        let tree_menu_target = match &node.kind {
            WorkspaceTreeKind::Directory(path) => Some((path.clone(), true)),
            WorkspaceTreeKind::MarkdownFile(path) => Some((path.clone(), false)),
            WorkspaceTreeKind::Heading { .. } => None,
        };

        let label_color = if selected {
            c.text_default
        } else {
            c.dialog_muted
        };

        let mut arrow_el = div()
            .w(px(14.0))
            .h(px(18.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(12.0))
            .text_color(c.dialog_muted)
            .child(arrow);
        if has_children {
            arrow_el = arrow_el.cursor_pointer().on_mouse_down(
                MouseButton::Left,
                move |_event, _window, cx| {
                    let _ = arrow_editor.update(cx, |editor, cx| {
                        editor.toggle_workspace_node(&arrow_node_id, cx);
                    });
                    cx.stop_propagation();
                },
            );
        }

        div()
            .id(("workspace-node", stable_node_hash(&node.id)))
            .h(px(WORKSPACE_NODE_HEIGHT))
            .w_full()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(6.0))
            .pl(px(8.0 + depth as f32 * WORKSPACE_NODE_INDENT))
            .pr(px(8.0))
            .rounded(px(6.0))
            .bg(if selected {
                c.selection
            } else {
                hsla(0.0, 0.0, 0.0, 0.0)
            })
            .hover(|this| this.bg(c.dialog_secondary_button_hover))
            .cursor_pointer()
            .child(arrow_el)
            .children(icon.map(|(path, color)| {
                svg()
                    .path(path)
                    .size(px(16.0))
                    .flex_shrink_0()
                    .text_color(color)
                    .into_any_element()
            }))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .text_size(px(t.text_size * 0.9))
                    .line_height(px(t.text_size * t.text_line_height))
                    .text_color(label_color)
                    .child(node.label.clone()),
            )
            .when_some(tree_menu_target, |row, (target, is_dir)| {
                let menu_editor = editor.clone();
                row.on_mouse_down(MouseButton::Right, move |event, _window, cx| {
                    let target = target.clone();
                    let position = event.position;
                    let _ = menu_editor.update(cx, |editor, cx| {
                        editor.open_tree_menu(target, is_dir, position, cx)
                    });
                    cx.stop_propagation();
                })
            })
            .on_click(move |_event, window, cx| {
                let node_id = node_id.clone();
                let click_kind = click_kind.clone();
                let _ = click_editor.update(cx, |editor, cx| match click_kind {
                    WorkspaceTreeKind::Directory(_) => editor.toggle_workspace_node(&node_id, cx),
                    WorkspaceTreeKind::MarkdownFile(path) => {
                        editor.open_workspace_file(path, window, cx);
                    }
                    WorkspaceTreeKind::Heading { line, .. } => {
                        editor.select_outline_node(node_id, line, cx)
                    }
                });
            })
            .into_any_element()
    }
}

/// Recognizes `.md` files for the workspace tree, ignoring extension case.
/// A file tree read from disk.
#[derive(Clone)]
pub(super) struct ScannedTree {
    root: Arc<WorkspaceTreeNode>,
    truncated: bool,
}

/// Recursively scans directories, retaining directories and `.md` files.
///
/// Directories sort before files; entries within each group sort by lowercase label.
enum CachedTree {
    /// Being read; the number tells this scan from a later one.
    Scanning(u64),
    Ready {
        tree: Result<ScannedTree, String>,
        scanned_at: Instant,
        /// A newer read is running; the old tree is shown meanwhile.
        rescan: Option<u64>,
    },
}

/// File trees by root, shared by all tabs and windows: a new tab shows the
/// tree at once, and folders are read once in the background instead of on
/// the UI thread for every tab.
#[derive(Default)]
pub(crate) struct WorkspaceTreeCache {
    entries: HashMap<PathBuf, CachedTree>,
    next_scan: u64,
}

impl Global for WorkspaceTreeCache {}

/// The tree of `root`, or None while it is first read. Starts a background
/// read when the root is not cached, or with `revalidate` when its tree is
/// older than [`WORKSPACE_TREE_MAX_AGE`] (the old tree is returned meanwhile).
fn workspace_tree(
    root: &Path,
    revalidate: bool,
    cx: &mut App,
) -> Option<Result<ScannedTree, String>> {
    let cache = cx.default_global::<WorkspaceTreeCache>();
    let (result, stale) = match cache.entries.get(root) {
        Some(CachedTree::Scanning(_)) => return None,
        Some(CachedTree::Ready {
            tree,
            scanned_at,
            rescan,
        }) => (
            Some(tree.clone()),
            revalidate && rescan.is_none() && scanned_at.elapsed() > WORKSPACE_TREE_MAX_AGE,
        ),
        None => (None, true),
    };
    if !stale {
        return result;
    }

    cache.next_scan += 1;
    let scan = cache.next_scan;
    match cache.entries.get_mut(root) {
        Some(CachedTree::Ready { rescan, .. }) => *rescan = Some(scan),
        _ => {
            if cache.entries.len() >= WORKSPACE_TREE_CACHE_SIZE {
                cache
                    .entries
                    .retain(|_, entry| matches!(entry, CachedTree::Scanning(_)));
            }
            cache
                .entries
                .insert(root.to_path_buf(), CachedTree::Scanning(scan));
        }
    }

    let root = root.to_path_buf();
    let read_root = root.clone();
    let read = cx.background_executor().spawn(async move {
        let start = Instant::now();
        let tree = scan_workspace_tree(&read_root).map_err(|err| format!("{err:#}"));
        crate::perf_log::note(format!(
            "Dateibaum im Hintergrund gelesen: {} – {} ms{}",
            read_root.display(),
            start.elapsed().as_millis(),
            if tree.as_ref().is_ok_and(|tree| tree.truncated) {
                " (abgeschnitten)"
            } else {
                ""
            }
        ));
        tree
    });
    cx.spawn(async move |cx| {
        let tree = read.await;
        let _ = cx.update(|cx| {
            let cache = cx.default_global::<WorkspaceTreeCache>();
            let current = match cache.entries.get(&root) {
                Some(CachedTree::Scanning(id)) => *id == scan,
                Some(CachedTree::Ready { rescan, .. }) => *rescan == Some(scan),
                None => false,
            };
            // A dropped entry or a newer read wins over this one.
            if current {
                cache.entries.insert(
                    root,
                    CachedTree::Ready {
                        tree,
                        scanned_at: Instant::now(),
                        rescan: None,
                    },
                );
                cx.refresh_windows();
            }
        });
    })
    .detach();
    result
}

/// Drops the cached tree and link index of `root` (after files changed), so
/// the next look reads them again.
pub(super) fn forget_workspace_tree(root: &Path, cx: &mut App) {
    super::link_index::forget_link_index(root, cx);
    if let Some(cache) = cx.try_global::<WorkspaceTreeCache>()
        && cache.entries.contains_key(root)
    {
        cx.global_mut::<WorkspaceTreeCache>().entries.remove(root);
    }
}

fn scan_workspace_tree(path: &Path) -> Result<ScannedTree> {
    let mut budget = WORKSPACE_SCAN_LIMIT;
    let root = scan_workspace_dir(path, &mut budget)?;
    Ok(ScannedTree {
        root: Arc::new(root),
        truncated: budget == 0,
    })
}

/// Reads `path` recursively, counting every folder entry against `budget`
/// and stopping when it is used up.
fn scan_workspace_dir(path: &Path, budget: &mut usize) -> Result<WorkspaceTreeNode> {
    let mut children = Vec::new();
    for entry in
        fs::read_dir(path).with_context(|| format!("failed to read '{}'", path.display()))?
    {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        let entry = entry?;
        let entry_path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if super::workspace_search::is_skipped_dir_name(&entry.file_name().to_string_lossy()) {
                continue;
            }
            // Unreadable subfolders are left out instead of failing the tree,
            // and so are folders without any Markdown file below them.
            if let Ok(child) = scan_workspace_dir(&entry_path, budget)
                && !child.children.is_empty()
            {
                children.push(child);
            }
        } else if file_type.is_file() && super::workspace_search::is_document_path(&entry_path) {
            children.push(WorkspaceTreeNode {
                id: file_node_id(&entry_path),
                label: file_label(&entry_path),
                kind: WorkspaceTreeKind::MarkdownFile(entry_path),
                children: Vec::new(),
            });
        }
    }

    children.sort_by(|left, right| {
        let left_dir = matches!(left.kind, WorkspaceTreeKind::Directory(_));
        let right_dir = matches!(right.kind, WorkspaceTreeKind::Directory(_));
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.label.to_lowercase().cmp(&right.label.to_lowercase()))
    });

    Ok(WorkspaceTreeNode {
        id: file_node_id(path),
        label: file_label(path),
        kind: WorkspaceTreeKind::Directory(path.to_path_buf()),
        children,
    })
}

/// Uses the final path component as the displayed file or directory name.
fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Builds a path-based ID shared by file and directory nodes.
pub(super) fn file_node_id(path: &Path) -> String {
    format!("file:{}", path.to_string_lossy())
}

/// Hashes a node ID for GPUI element identity.
fn stable_node_hash(id: &str) -> u64 {
    use std::hash::{Hash, Hasher};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    id.hash(&mut hasher);
    hasher.finish()
}

/// Clamps the panel's target viewport fraction to its pixel width bounds.
pub(super) fn workspace_panel_width_for_viewport(viewport_width: f32) -> f32 {
    let target = viewport_width * WORKSPACE_PANEL_TARGET_RATIO;
    target.clamp(WORKSPACE_PANEL_MIN_WIDTH, WORKSPACE_PANEL_MAX_WIDTH)
}

/// Removes expansion and selection IDs for headings absent from the new outline.
fn prune_outline_state(workspace: &mut WorkspaceState, outline: &[WorkspaceTreeNode]) {
    let mut current_ids = HashSet::new();
    collect_node_ids(outline, &mut current_ids);
    workspace
        .expanded
        .retain(|id| !is_outline_node_id(id) || current_ids.contains(id));

    if matches!(
        &workspace.selected,
        Some(WorkspaceSelection::Outline(id)) if !current_ids.contains(id)
    ) {
        workspace.selected = None;
    }
}

/// Collects every node ID recursively.
fn collect_node_ids(nodes: &[WorkspaceTreeNode], ids: &mut HashSet<String>) {
    for node in nodes {
        ids.insert(node.id.clone());
        collect_node_ids(&node.children, ids);
    }
}

/// Distinguishes heading IDs from file-tree IDs.
fn is_outline_node_id(id: &str) -> bool {
    id.starts_with("outline:")
}

/// Byte offset where `line` starts, counting lines like [`str::lines`].
fn source_line_start(source: &str, line: usize) -> Option<usize> {
    if line == 0 {
        return Some(0);
    }
    source
        .match_indices('\n')
        .nth(line - 1)
        .map(|(index, _)| index + 1)
        .filter(|offset| *offset <= source.len())
}

/// Builds a nested ATX-heading outline while ignoring fenced code lines.
fn build_outline_tree(markdown: &str) -> Vec<WorkspaceTreeNode> {
    let mut roots = Vec::new();
    let mut stack: Vec<(u8, Vec<usize>)> = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for (line_index, line) in markdown.lines().enumerate() {
        let trimmed = line.trim_start();
        if let Some((marker, len)) = fence {
            if is_closing_fence(trimmed, marker, len) {
                fence = None;
            }
            continue;
        }

        if let Some(next_fence) = opening_fence(trimmed) {
            fence = Some(next_fence);
            continue;
        }

        let Some((level, title)) = BlockKind::parse_atx_heading_line(line) else {
            continue;
        };

        while stack
            .last()
            .is_some_and(|(parent_level, _)| *parent_level >= level)
        {
            stack.pop();
        }

        let node = WorkspaceTreeNode {
            id: format!("outline:{line_index}"),
            label: title,
            kind: WorkspaceTreeKind::Heading {
                line: line_index,
                level,
            },
            children: Vec::new(),
        };

        let siblings = if let Some((_, parent_path)) = stack.last() {
            children_at_path_mut(&mut roots, parent_path)
        } else {
            &mut roots
        };
        siblings.push(node);

        let mut node_path = stack
            .last()
            .map(|(_, path)| path.clone())
            .unwrap_or_default();
        node_path.push(siblings.len() - 1);
        stack.push((level, node_path));
    }

    roots
}

/// Returns the child vector at an index path through the outline tree.
fn children_at_path_mut<'a>(
    nodes: &'a mut Vec<WorkspaceTreeNode>,
    path: &[usize],
) -> &'a mut Vec<WorkspaceTreeNode> {
    let mut current = nodes;
    for &index in path {
        current = &mut current[index].children;
    }
    current
}

/// Recognizes a backtick or tilde fence of at least three markers.
fn opening_fence(trimmed: &str) -> Option<(char, usize)> {
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    (len >= 3).then_some((marker, len))
}

/// Accepts a matching fence run at least as long as the opener, with no trailing content.
fn is_closing_fence(trimmed: &str, marker: char, len: usize) -> bool {
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    count >= len && trimmed[count..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::{
        WorkspaceSelection, WorkspaceState, WorkspaceTreeKind, build_outline_tree,
        prune_outline_state, scan_workspace_dir, scan_workspace_tree,
        workspace_panel_width_for_viewport,
    };
    use std::fs;

    use crate::components::{BlockKind, Editor};
    use gpui::{AppContext, TestAppContext};

    #[test]
    fn workspace_scan_keeps_dirs_and_md_files_only() {
        let root =
            std::env::temp_dir().join(format!("velotype-workspace-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("nested")).expect("create dirs");
        fs::write(root.join("a.md"), "a").expect("write md");
        fs::write(root.join("a.txt"), "ignored").expect("write txt");
        fs::write(root.join("nested").join("b.md"), "b").expect("write nested md");
        // Folders without Markdown files and dependency folders are left out.
        fs::create_dir_all(root.join("images")).expect("create images dir");
        fs::write(root.join("images").join("a.png"), "png").expect("write png");
        fs::create_dir_all(root.join("node_modules")).expect("create node_modules");
        fs::write(root.join("node_modules").join("readme.md"), "x").expect("write dep md");

        let mut budget = usize::MAX;
        let tree = scan_workspace_dir(&root, &mut budget).expect("scan tree");
        let labels = tree
            .children
            .iter()
            .map(|node| node.label.as_str())
            .collect::<Vec<_>>();
        assert_eq!(labels, vec!["nested", "a.md"]);
        assert!(matches!(
            tree.children[0].kind,
            WorkspaceTreeKind::Directory(_)
        ));
        assert!(matches!(
            tree.children[1].kind,
            WorkspaceTreeKind::MarkdownFile(_)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn workspace_scan_stops_at_its_budget() {
        let root =
            std::env::temp_dir().join(format!("velotype-workspace-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("create dir");
        for index in 0..5 {
            fs::write(root.join(format!("{index}.md")), "x").expect("write md");
        }

        let mut budget = 3;
        let tree = scan_workspace_dir(&root, &mut budget).expect("scan tree");
        assert_eq!(budget, 0);
        assert_eq!(tree.children.len(), 3);

        let scanned = scan_workspace_tree(&root).expect("scan tree");
        assert!(!scanned.truncated);
        assert_eq!(scanned.root.children.len(), 5);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn outline_tree_skips_headings_inside_fenced_code() {
        let outline = build_outline_tree(
            "# Root\n\n```md\n# ignored\n```\n\n## Child\n\n### Grandchild\n\n# Next",
        );

        assert_eq!(outline.len(), 2);
        assert_eq!(outline[0].label, "Root");
        assert_eq!(outline[0].children[0].label, "Child");
        assert_eq!(outline[0].children[0].children[0].label, "Grandchild");
        assert_eq!(outline[1].label, "Next");
    }

    #[test]
    fn outline_expansion_state_is_not_auto_populated_and_prunes_stale_ids() {
        let outline = build_outline_tree("# Root\n\n## Child\n\n# Next");
        let mut fresh = WorkspaceState::default();
        prune_outline_state(&mut fresh, &outline);
        assert!(fresh.expanded.is_empty());

        let mut existing = WorkspaceState::default();
        existing.expanded.insert("outline:0".to_string());
        existing.expanded.insert("outline:999".to_string());
        existing
            .expanded
            .insert("workspace-dir:C:/docs".to_string());
        existing.selected = Some(WorkspaceSelection::Outline("outline:999".to_string()));

        prune_outline_state(&mut existing, &outline);

        assert!(existing.expanded.contains("outline:0"));
        assert!(existing.expanded.contains("workspace-dir:C:/docs"));
        assert!(!existing.expanded.contains("outline:999"));
        assert_eq!(existing.selected, None);
    }

    #[test]
    fn workspace_panel_width_uses_ratio_with_bounds() {
        assert_eq!(workspace_panel_width_for_viewport(1000.0), 240.0);
        assert_eq!(workspace_panel_width_for_viewport(2000.0), 300.0);
        assert_eq!(workspace_panel_width_for_viewport(4000.0), 360.0);
    }

    #[gpui::test]
    async fn outline_node_click_focuses_and_queues_scroll_for_target_heading(
        cx: &mut TestAppContext,
    ) {
        let markdown =
            "# Alpha\n\nPara under alpha.\n\n## Beta\n\nPara under beta.\n\n# Gamma".to_string();
        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, None));

        editor.update(cx, |editor, cx| {
            let beta = editor
                .document
                .visible_blocks()
                .iter()
                .find(|visible| {
                    matches!(
                        visible.entity.read(cx).kind(),
                        BlockKind::Heading { level: 2 }
                    )
                })
                .expect("## Beta heading")
                .entity
                .clone();

            // The outline is parsed from the same serialized text, where
            // "## Beta" lands on line index 4.
            editor.select_outline_node("outline:4".into(), 4, cx);

            assert_eq!(editor.pending_focus, Some(beta.entity_id()));
            assert!(editor.pending_scroll_active_block_into_view);
        });
    }
}
