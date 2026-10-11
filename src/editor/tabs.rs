//! Multi-tab window host.
//!
//! [`EditorWindow`] is the root view of every editor window. It owns one
//! [`Editor`] entity per tab plus the active index and renders only the active
//! editor. Each editor keeps its per-document state (blocks, history, dirty
//! flag, file path, dialogs, workspace drawer, status bar); the host owns the
//! concerns that exist once per window:
//!
//! - the platform should-close guard (an editor hosted here never installs its
//!   own),
//! - switching, opening and closing tabs,
//! - the close-window flow, which walks dirty tabs one at a time through each
//!   tab's existing unsaved-changes dialog,
//! - session drafts: when the window really closes, unsaved untitled tabs are
//!   kept in [`SessionDrafts`] instead of prompting (an explicit tab close
//!   still prompts), and are reopened on the next launch.
//!
//! Editors never reach into the host while they are being updated. They report
//! close outcomes and open requests through [`EditorTabEvent`]; the host reacts
//! once the editor update has finished. The tab strip itself is painted by the
//! active editor (see [`Editor::render_tab_strip`]) so it sits between the
//! window chrome and the document without disturbing overlay stacking.

use std::path::{Path, PathBuf};

use gpui::{prelude::FluentBuilder, *};

use super::Editor;
use crate::components::{CloseTab, NewTab, NextTab, PrevTab};
use crate::config::SessionDrafts;
use crate::i18n::{I18nManager, I18nStrings};
use crate::theme::Theme;

/// Height of the tab strip drawn under the titlebar while a window has more
/// than one tab.
pub(super) const TAB_STRIP_HEIGHT: f32 = 32.0;
const TAB_MAX_WIDTH: f32 = 220.0;

/// Notifications an editor sends to the window that hosts it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum EditorTabEvent {
    /// The user confirmed closing this tab from its unsaved-changes dialog
    /// (discarded, or saved successfully).
    CloseConfirmed,
    /// The user backed out of a pending close (cancel, or a failed save).
    CloseCancelled,
    /// Close Window was requested from inside the editor (shortcut, titlebar
    /// close button or in-window menu).
    RequestCloseWindow,
    /// Markdown files should be opened in tabs (file drop or workspace drawer).
    OpenPaths {
        paths: Vec<PathBuf>,
        open_workspace: bool,
    },
    /// Open one file (Quick Open, Search in Folder) and optionally reveal
    /// `(line, query)` in it.
    OpenPathAndReveal {
        path: PathBuf,
        reveal: Option<(usize, String)>,
    },
}

impl EventEmitter<EditorTabEvent> for Editor {}

/// Display data for one entry in the tab strip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct TabLabel {
    pub(super) title: SharedString,
    pub(super) dirty: bool,
    pub(super) active: bool,
}

struct EditorTab {
    editor: Entity<Editor>,
    _subscription: Subscription,
}

/// Window root view owning one [`Editor`] per tab.
pub struct EditorWindow {
    tabs: Vec<EditorTab>,
    active: usize,
    /// True while a close-window request is walking the dirty tabs.
    closing_window: bool,
    /// Set when writing session drafts failed, so untitled tabs fall back to
    /// the unsaved-changes prompt instead of being kept as drafts.
    drafts_unavailable: bool,
    pub(crate) close_guard_installed: bool,
    /// Re-applies the appearance-bound theme when the OS light/dark mode flips.
    _appearance_subscription: Subscription,
}

impl EditorWindow {
    pub(crate) fn new(editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let tab = Self::attach(editor, window, cx);
        let appearance_subscription = cx.observe_window_appearance(window, |_, window, cx| {
            crate::config::sync_theme_with_system_appearance(cx, window.appearance());
        });
        Self {
            tabs: vec![tab],
            active: 0,
            closing_window: false,
            drafts_unavailable: false,
            close_guard_installed: false,
            _appearance_subscription: appearance_subscription,
        }
    }

    fn attach(editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) -> EditorTab {
        let host = cx.weak_entity();
        editor.update(cx, |editor, _cx| editor.tab_host = Some(host));
        let subscription = cx.subscribe_in(&editor, window, Self::on_editor_event);
        EditorTab {
            editor,
            _subscription: subscription,
        }
    }

    pub(crate) fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub(crate) fn active_index(&self) -> usize {
        self.active
    }

    pub(crate) fn active_editor(&self) -> &Entity<Editor> {
        &self.tabs[self.active].editor
    }

    pub(crate) fn editors(&self) -> impl Iterator<Item = &Entity<Editor>> {
        self.tabs.iter().map(|tab| &tab.editor)
    }

    fn index_of(&self, editor: &Entity<Editor>) -> Option<usize> {
        self.tabs
            .iter()
            .position(|tab| tab.editor.entity_id() == editor.entity_id())
    }

    fn active_has_blocking_dialog(&self, cx: &App) -> bool {
        self.active_editor().read(cx).has_blocking_dialog()
    }

    /// Opens an empty untitled document in a new tab.
    pub(crate) fn new_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_has_blocking_dialog(cx) {
            return;
        }
        let editor = cx.new(|cx| Editor::from_markdown(cx, String::new(), None));
        self.push_tab(editor, window, cx);
    }

    fn push_tab(&mut self, editor: Entity<Editor>, window: &mut Window, cx: &mut Context<Self>) {
        let tab = Self::attach(editor, window, cx);
        self.tabs.push(tab);
        self.set_active(self.tabs.len() - 1, window, cx);
    }

    /// Shows `markdown` in this window and returns the editor displaying it.
    ///
    /// A path that is already open focuses its existing tab. A clean, empty,
    /// untitled active tab is replaced instead of left behind; anything else
    /// gets a new tab.
    pub(crate) fn open_document(
        &mut self,
        markdown: String,
        file_path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Editor> {
        if let Some(path) = file_path.as_deref()
            && let Some(index) = self.tab_index_for_path(path, cx)
        {
            self.set_active(index, window, cx);
            return self.tabs[index].editor.clone();
        }

        let editor = cx.new(|cx| Editor::from_markdown(cx, markdown, file_path));
        let replace_active = {
            let active = self.active_editor().read(cx);
            active.is_pristine_untitled(cx) && !active.has_blocking_dialog()
        };
        if replace_active {
            let tab = Self::attach(editor.clone(), window, cx);
            self.tabs[self.active] = tab;
            self.activate_current(window, cx);
        } else {
            self.push_tab(editor.clone(), window, cx);
        }
        editor
    }

    fn tab_index_for_path(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| {
            tab.editor
                .read(cx)
                .file_path
                .as_deref()
                .is_some_and(|open| same_file(open, path))
        })
    }

    /// Reads each Markdown file and opens it in a tab, reporting read failures
    /// on this window.
    pub(crate) fn open_paths(
        &mut self,
        paths: Vec<PathBuf>,
        open_workspace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for path in paths {
            match std::fs::read_to_string(&path) {
                Ok(markdown) => {
                    let editor = self.open_document(markdown, Some(path.clone()), window, cx);
                    if open_workspace {
                        editor.update(cx, |editor, cx| editor.open_workspace_drawer(cx));
                    }
                    crate::app_menu::record_recent_file_from_editor(&path, cx);
                }
                Err(err) => {
                    let strings = cx.global::<I18nManager>().strings().clone();
                    let detail = format!("failed to read '{}': {err}", path.display());
                    let buttons = [strings.info_dialog_ok.as_str()];
                    let _ = window.prompt(
                        PromptLevel::Critical,
                        &strings.open_failed_title,
                        Some(&detail),
                        &buttons,
                        cx,
                    );
                }
            }
        }
    }

    /// Switches to tab `index`. Ignored while the active tab shows a modal
    /// dialog, so a pending prompt cannot be hidden behind another tab.
    pub(crate) fn activate_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() || index == self.active || self.active_has_blocking_dialog(cx) {
            return;
        }
        self.set_active(index, window, cx);
    }

    pub(crate) fn activate_next_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len();
        if count > 1 {
            self.activate_tab((self.active + 1) % count, window, cx);
        }
    }

    pub(crate) fn activate_previous_tab(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.tabs.len();
        if count > 1 {
            self.activate_tab((self.active + count - 1) % count, window, cx);
        }
    }

    fn set_active(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index != self.active
            && let Some(outgoing) = self.tabs.get(self.active)
        {
            outgoing
                .editor
                .update(cx, |editor, cx| editor.prepare_tab_deactivation(window, cx));
        }
        self.active = index;
        self.activate_current(window, cx);
    }

    fn activate_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let dirty = self
            .active_editor()
            .update(cx, |editor, cx| editor.prepare_tab_activation(cx));
        window.set_window_edited(dirty);
        cx.notify();
    }

    /// Closes tab `index`, routing a dirty tab through its unsaved-changes
    /// dialog first. Closing the last tab closes the window.
    pub(crate) fn request_close_tab(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if index >= self.tabs.len() || self.active_has_blocking_dialog(cx) {
            return;
        }
        self.closing_window = false;
        let editor = self.tabs[index].editor.clone();
        if !editor.read(cx).document_dirty {
            self.remove_tab(index, window, cx);
            return;
        }

        self.set_active(index, window, cx);
        editor.update(cx, |editor, cx| {
            editor.on_window_should_close(window, cx);
        });
    }

    fn remove_tab(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.tabs.len() {
            return;
        }
        let removed = self.tabs.remove(index);
        removed
            .editor
            .update(cx, |editor, cx| editor.discard_session_draft(cx));
        if self.tabs.is_empty() {
            window.remove_window();
            return;
        }

        if index < self.active {
            self.active -= 1;
            cx.notify();
        } else if index == self.active {
            self.active = index.min(self.tabs.len() - 1);
            self.activate_current(window, cx);
        } else {
            cx.notify();
        }
    }

    /// Requests closing the whole window, prompting for dirty tabs first.
    pub(crate) fn request_close_window(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.on_window_should_close(window, cx) {
            window.remove_window();
        }
    }

    /// Whether untitled tabs are kept as session drafts on close instead of
    /// prompting.
    fn keeps_drafts(&self, cx: &App) -> bool {
        !self.drafts_unavailable && SessionDrafts::get(cx).is_some()
    }

    /// First tab whose unsaved changes need the user's decision before the
    /// window may close. Untitled tabs are exempt while drafts are kept.
    fn first_tab_needing_prompt(&self, cx: &App) -> Option<usize> {
        let keeps_drafts = self.keeps_drafts(cx);
        self.tabs.iter().position(|tab| {
            let editor = tab.editor.read(cx);
            editor.document_dirty && !(keeps_drafts && editor.file_path.is_none())
        })
    }

    /// True when closing this window would show an unsaved-changes dialog.
    pub(crate) fn needs_close_prompt(&self, cx: &App) -> bool {
        self.first_tab_needing_prompt(cx).is_some()
    }

    /// Returns true when the window may close: every tab is clean or an
    /// untitled draft that was just kept in the session draft store.
    /// Otherwise focuses the first tab needing a decision, shows its
    /// unsaved-changes dialog and returns false; confirming that dialog closes
    /// the tab and continues with the next one.
    pub(crate) fn on_window_should_close(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(index) = self.first_tab_needing_prompt(cx) else {
            if let Err(err) = self.keep_session_drafts(cx) {
                // Without a draft store the untitled tabs need a decision too.
                eprintln!("failed to keep unsaved drafts: {err:#}");
                self.drafts_unavailable = true;
                return self.on_window_should_close(window, cx);
            }
            self.closing_window = false;
            return true;
        };

        self.closing_window = true;
        if index != self.active {
            self.set_active(index, window, cx);
        }
        let editor = self.tabs[index].editor.clone();
        editor.update(cx, |editor, cx| editor.on_window_should_close(window, cx))
    }

    /// Writes every unsaved untitled tab to the session draft store.
    fn keep_session_drafts(&mut self, cx: &mut Context<Self>) -> anyhow::Result<()> {
        let Some(store) = SessionDrafts::get(cx).filter(|_| !self.drafts_unavailable) else {
            return Ok(());
        };
        for tab in &self.tabs {
            tab.editor
                .update(cx, |editor, cx| editor.keep_session_draft(&store, cx))?;
        }
        Ok(())
    }

    /// Reopens a restored session draft as an unsaved untitled tab.
    pub(crate) fn open_draft(
        &mut self,
        markdown: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let editor = self.open_document(markdown, None, window, cx);
        editor.update(cx, |editor, cx| editor.mark_dirty(cx));
        window.set_window_edited(true);
    }

    fn on_editor_event(
        &mut self,
        editor: &Entity<Editor>,
        event: &EditorTabEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            EditorTabEvent::CloseConfirmed => {
                if let Some(index) = self.index_of(editor) {
                    self.remove_tab(index, window, cx);
                }
                if self.tabs.is_empty() {
                    return;
                }
                if self.closing_window && self.on_window_should_close(window, cx) {
                    window.remove_window();
                }
            }
            EditorTabEvent::CloseCancelled => {
                self.closing_window = false;
            }
            EditorTabEvent::RequestCloseWindow => {
                self.request_close_window(window, cx);
            }
            EditorTabEvent::OpenPaths {
                paths,
                open_workspace,
            } => {
                self.open_paths(paths.clone(), *open_workspace, window, cx);
            }
            EditorTabEvent::OpenPathAndReveal { path, reveal } => {
                self.open_paths(vec![path.clone()], false, window, cx);
                let editor = self.active_editor().clone();
                let opened = editor.read(cx).file_path.as_deref() == Some(path.as_path());
                if let (true, Some((line, query))) = (opened, reveal.clone()) {
                    editor.update(cx, |editor, cx| editor.reveal_search_hit(line, &query, cx));
                }
            }
        }
    }

    pub(crate) fn install_close_guard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.close_guard_installed {
            self.force_install_close_guard(window, cx);
        }
    }

    pub(crate) fn force_install_close_guard(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let host = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            host.update(cx, |host, cx| host.on_window_should_close(window, cx))
                .unwrap_or(true)
        });
        self.close_guard_installed = true;
    }

    fn on_new_tab(&mut self, _: &NewTab, window: &mut Window, cx: &mut Context<Self>) {
        self.new_tab(window, cx);
    }

    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.request_close_tab(self.active, window, cx);
    }

    fn on_next_tab(&mut self, _: &NextTab, window: &mut Window, cx: &mut Context<Self>) {
        self.activate_next_tab(window, cx);
    }

    fn on_prev_tab(&mut self, _: &PrevTab, window: &mut Window, cx: &mut Context<Self>) {
        self.activate_previous_tab(window, cx);
    }
}

impl Render for EditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.install_close_guard(window, cx);
        div()
            .size_full()
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .child(self.active_editor().clone())
    }
}

/// Compares two document paths, falling back to a literal comparison when
/// either cannot be canonicalized (e.g. the file was deleted).
fn same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

impl Editor {
    pub(super) fn is_tab_hosted(&self) -> bool {
        self.tab_host.is_some()
    }

    /// True while a modal dialog owns the editor, so tab switching must wait.
    pub(super) fn has_blocking_dialog(&self) -> bool {
        self.show_unsaved_changes_dialog || self.show_drop_replace_dialog
    }

    /// A clean, never-saved document with no content. Opening a file replaces
    /// such a tab instead of keeping an empty placeholder around.
    pub(super) fn is_pristine_untitled(&self, cx: &App) -> bool {
        self.file_path.is_none()
            && !self.document_dirty
            && self.serialized_document_text(cx).trim().is_empty()
    }

    /// Completes a close the user confirmed in the unsaved-changes dialog: a
    /// hosted editor hands the decision to its window, a standalone editor
    /// closes its window directly.
    pub(super) fn finish_confirmed_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_tab_hosted() {
            cx.emit(EditorTabEvent::CloseConfirmed);
        } else {
            window.remove_window();
        }
    }

    /// Stores this document as a session draft when it is untitled with
    /// unsaved content; otherwise drops any draft it had.
    fn keep_session_draft(&mut self, store: &SessionDrafts, cx: &App) -> anyhow::Result<()> {
        if self.file_path.is_some() || !self.document_dirty {
            self.discard_session_draft(cx);
            return Ok(());
        }
        let markdown = self.serialized_document_text(cx);
        if markdown.trim().is_empty() {
            self.discard_session_draft(cx);
            return Ok(());
        }
        let id = self
            .draft_id
            .get_or_insert_with(SessionDrafts::new_draft_id);
        store.write(id, &markdown)
    }

    /// Forgets this document's session draft (saved, discarded or closed).
    pub(super) fn discard_session_draft(&mut self, cx: &App) {
        if let Some(id) = self.draft_id.take()
            && let Some(store) = SessionDrafts::get(cx)
        {
            store.remove(&id);
        }
    }

    pub(super) fn notify_close_cancelled(&mut self, cx: &mut Context<Self>) {
        if self.is_tab_hosted() {
            cx.emit(EditorTabEvent::CloseCancelled);
        }
    }

    /// Called on the outgoing tab: transient UI is dismissed and the focused
    /// block is remembered so the caret returns there when the tab is shown
    /// again.
    fn prepare_tab_deactivation(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);
        if let Some(focused) = self.document.focused_block_entity_id(window, cx) {
            self.active_entity_id = Some(focused);
        }
    }

    /// Called on the incoming tab; returns its dirty flag for the window's
    /// edited marker.
    fn prepare_tab_activation(&mut self, cx: &mut Context<Self>) -> bool {
        self.pending_window_title_refresh = true;
        if !self.has_blocking_dialog() {
            self.pending_focus = self
                .active_entity_id
                .filter(|id| self.focusable_entity_by_id(*id).is_some())
                .or_else(|| self.first_focusable_entity_id(cx));
        }
        cx.notify();
        self.document_dirty
    }

    fn tab_title(&self, strings: &I18nStrings) -> SharedString {
        self.file_path
            .as_deref()
            .and_then(Path::file_name)
            .map(|name| SharedString::from(name.to_string_lossy().into_owned()))
            .unwrap_or_else(|| SharedString::from(strings.tab_untitled.clone()))
    }

    /// Labels for the host window's tabs, or `None` when the strip is hidden
    /// (standalone editor or a single tab).
    pub(super) fn tab_labels(
        &self,
        strings: &I18nStrings,
        cx: &Context<Self>,
    ) -> Option<(WeakEntity<EditorWindow>, Vec<TabLabel>)> {
        let host_weak = self.tab_host.clone()?;
        let host = host_weak.upgrade()?;
        let host = host.read(cx);
        if host.tab_count() < 2 {
            return None;
        }

        let own_id = cx.entity_id();
        let labels = host
            .editors()
            .enumerate()
            .map(|(index, editor)| {
                let (title, dirty) = if editor.entity_id() == own_id {
                    (self.tab_title(strings), self.document_dirty)
                } else {
                    let editor = editor.read(cx);
                    (editor.tab_title(strings), editor.document_dirty)
                };
                TabLabel {
                    title,
                    dirty,
                    active: index == host.active_index(),
                }
            })
            .collect();
        Some((host_weak, labels))
    }

    /// Renders the tab strip below the titlebar and menu bar at `top`.
    pub(super) fn render_tab_strip(
        &self,
        theme: &Theme,
        strings: &I18nStrings,
        top: f32,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let (host, labels) = self.tab_labels(strings, cx)?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let transparent = hsla(0., 0., 0., 0.);
        let hover_bg = c.status_bar_button_hover;

        let tabs = labels.into_iter().enumerate().map(|(index, label)| {
            let activate_host = host.clone();
            let middle_close_host = host.clone();
            let close_host = host.clone();
            let title = if label.dirty {
                SharedString::from(format!("\u{25CF} {}", label.title))
            } else {
                label.title
            };

            let close_button = div()
                .id(("editor-tab-close", index))
                .flex_shrink_0()
                .size(px(18.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.0))
                .text_color(c.status_bar_text_dim)
                .hover(move |style| style.bg(hover_bg))
                .child("\u{00D7}")
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = close_host.update(cx, |host, cx| {
                        host.request_close_tab(index, window, cx);
                    });
                });

            div()
                .id(("editor-tab", index))
                .h(px(TAB_STRIP_HEIGHT - 6.0))
                .max_w(px(TAB_MAX_WIDTH))
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(6.0))
                .pl(px(10.0))
                .pr(px(4.0))
                .rounded(px(6.0))
                .cursor_pointer()
                .text_size(px(d.status_bar_text_size))
                .bg(if label.active {
                    c.editor_background
                } else {
                    transparent
                })
                .text_color(if label.active {
                    c.status_bar_text
                } else {
                    c.status_bar_text_dim
                })
                .when(!label.active, |tab| {
                    tab.hover(move |style| style.bg(hover_bg))
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    let _ = activate_host.update(cx, |host, cx| {
                        host.activate_tab(index, window, cx);
                    });
                })
                .on_mouse_down(MouseButton::Middle, move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = middle_close_host.update(cx, |host, cx| {
                        host.request_close_tab(index, window, cx);
                    });
                })
                .child(div().min_w(px(0.0)).truncate().child(title))
                .child(close_button)
                .into_any_element()
        });

        let new_tab_host = host.clone();
        let new_tab_button = div()
            .id("editor-tab-new")
            .flex_shrink_0()
            .size(px(TAB_STRIP_HEIGHT - 8.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.0))
            .cursor_pointer()
            .text_color(c.status_bar_text_dim)
            .hover(move |style| style.bg(hover_bg))
            .child("+")
            .on_click(move |_, window, cx| {
                let _ = new_tab_host.update(cx, |host, cx| host.new_tab(window, cx));
            });

        Some(
            div()
                .id("editor-tab-strip")
                .absolute()
                .top(px(top))
                .left_0()
                .right_0()
                .h(px(TAB_STRIP_HEIGHT))
                .flex()
                .items_center()
                .gap(px(2.0))
                .px(px(6.0))
                .overflow_x_scroll()
                .bg(c.status_bar_background)
                .border_b(px(1.0))
                .border_color(c.dialog_border)
                .children(tabs)
                .child(new_tab_button)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests;
