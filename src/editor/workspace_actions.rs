//! File-tree context menu (Open, New File, New Folder, Rename, Move to
//! Trash, Copy Path, Show in File Manager) and the name dialog it uses.
//!
//! Changes on disk refresh the drawer of every tab in the window; tabs that
//! show a renamed file follow it, tabs of a trashed file close.

use std::path::{Path, PathBuf};

use gpui::*;

use super::Editor;
use super::file_ops::{self, NameError};
use crate::components::single_line_input::{InputSubmitted, SingleLineInput, SingleLineOverflow};
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// Right-click menu over a file-tree entry.
pub(super) struct TreeMenu {
    position: Point<Pixels>,
    target: PathBuf,
    is_dir: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TreeMenuAction {
    Open,
    NewFile,
    NewFolder,
    Rename,
    Delete,
    CopyPath,
    Reveal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum NameDialogKind {
    NewFile { dir: PathBuf },
    NewFolder { dir: PathBuf },
    Rename { path: PathBuf },
}

/// Small dialog asking for a file or folder name.
pub(super) struct NameDialog {
    kind: NameDialogKind,
    input: Entity<SingleLineInput>,
    error: Option<String>,
    _subscription: Subscription,
}

/// An open document of this window: the editor, its file and dirty state.
pub(super) type OpenDocument = (Entity<Editor>, Option<PathBuf>, bool);

impl Editor {
    /// Every document open in this window (just this one without a tab
    /// host). This editor's own state is read directly since it may be
    /// mid-update.
    pub(super) fn open_documents(&self, cx: &mut Context<Self>) -> Vec<OpenDocument> {
        let own = || (cx.entity(), self.file_path.clone(), self.document_dirty);
        let Some(host) = self.tab_host.as_ref().and_then(WeakEntity::upgrade) else {
            return vec![own()];
        };
        let own_id = cx.entity_id();
        host.read(cx)
            .editors()
            .map(|editor| {
                if editor.entity_id() == own_id {
                    (editor.clone(), self.file_path.clone(), self.document_dirty)
                } else {
                    let read = editor.read(cx);
                    (editor.clone(), read.file_path.clone(), read.document_dirty)
                }
            })
            .collect()
    }

    /// Rescans the drawer of every tab in the window.
    pub(super) fn refresh_workspace_trees(&mut self, cx: &mut Context<Self>) {
        let own_id = cx.entity_id();
        for (editor, _, _) in self.open_documents(cx) {
            if editor.entity_id() == own_id {
                self.invalidate_workspace_tree(cx);
            } else {
                editor.update(cx, |editor, cx| editor.invalidate_workspace_tree(cx));
            }
        }
    }

    pub(super) fn open_tree_menu(
        &mut self,
        target: PathBuf,
        is_dir: bool,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.close_menu_bar(cx);
        self.dismiss_contextual_overlays(cx);
        self.workspace.tree_menu = Some(TreeMenu {
            position,
            target,
            is_dir,
        });
        cx.notify();
    }

    pub(super) fn close_tree_menu(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = self.workspace.tree_menu.take().is_some();
        if closed {
            cx.notify();
        }
        closed
    }

    fn run_tree_menu_action(
        &mut self,
        action: TreeMenuAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.workspace.tree_menu.take() else {
            return;
        };
        cx.notify();
        let strings = cx.global::<I18nManager>().strings().clone();
        let folder = if menu.is_dir {
            menu.target.clone()
        } else {
            menu.target
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| menu.target.clone())
        };
        match action {
            TreeMenuAction::Open => {
                if menu.is_dir {
                    self.prompt_open_folder_at(menu.target, cx);
                } else {
                    self.open_workspace_file(menu.target, window, cx);
                }
            }
            TreeMenuAction::NewFile => {
                let name = file_ops::unique_name(
                    &folder,
                    &strings.workspace_default_file_name,
                    Some("md"),
                );
                self.open_name_dialog(NameDialogKind::NewFile { dir: folder }, name, window, cx);
            }
            TreeMenuAction::NewFolder => {
                let name =
                    file_ops::unique_name(&folder, &strings.workspace_default_folder_name, None);
                self.open_name_dialog(NameDialogKind::NewFolder { dir: folder }, name, window, cx);
            }
            TreeMenuAction::Rename => {
                let name = menu
                    .target
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.open_name_dialog(
                    NameDialogKind::Rename { path: menu.target },
                    name,
                    window,
                    cx,
                );
            }
            TreeMenuAction::Delete => self.request_move_to_trash(menu.target, window, cx),
            TreeMenuAction::CopyPath => {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    menu.target.to_string_lossy().into_owned(),
                ));
            }
            TreeMenuAction::Reveal => cx.reveal_path(&menu.target),
        }
    }

    /// "Open" on a folder makes it the workspace folder of the window.
    fn prompt_open_folder_at(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        self.apply_workspace_folder(Some(folder), cx);
    }

    fn open_name_dialog(
        &mut self,
        kind: NameDialogKind,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Select the name without its extension, like file managers do.
        let stem_len = match &kind {
            NameDialogKind::NewFolder { .. } => initial.len(),
            _ => initial
                .rfind('.')
                .filter(|dot| *dot > 0)
                .unwrap_or(initial.len()),
        };
        let input = cx.new(|cx| {
            let mut input = SingleLineInput::new(initial.clone(), "", cx)
                .with_overflow(SingleLineOverflow::Clip);
            input.select_range(0..stem_len, cx);
            input
        });
        let subscription =
            cx.subscribe_in(&input, window, |this, _, _: &InputSubmitted, window, cx| {
                this.confirm_name_dialog(window, cx);
            });
        window.focus(&input.focus_handle(cx));
        self.workspace.name_dialog = Some(NameDialog {
            kind,
            input,
            error: None,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn close_name_dialog(&mut self, cx: &mut Context<Self>) -> bool {
        let closed = self.workspace.name_dialog.take().is_some();
        if closed {
            if let Some(active) = self.active_entity_id {
                self.focus_block(active);
            }
            cx.notify();
        }
        closed
    }

    fn confirm_name_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dialog) = self.workspace.name_dialog.as_ref() else {
            return;
        };
        let name = dialog.input.read(cx).value().to_string();
        let kind = dialog.kind.clone();
        match self.apply_name(&kind, &name, window, cx) {
            Ok(()) => {
                self.close_name_dialog(cx);
            }
            Err(message) => {
                if let Some(dialog) = self.workspace.name_dialog.as_mut() {
                    dialog.error = Some(message);
                    cx.notify();
                }
            }
        }
    }

    /// Carries out the dialog; the error is shown in the dialog.
    fn apply_name(
        &mut self,
        kind: &NameDialogKind,
        name: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let strings = cx.global::<I18nManager>().strings().clone();
        let name_error = |error: NameError| match error {
            NameError::Invalid => strings.workspace_name_invalid.clone(),
            NameError::Exists => strings.workspace_name_exists.clone(),
        };
        match kind {
            NameDialogKind::NewFile { dir } => {
                let path = file_ops::new_file_path(dir, name).map_err(name_error)?;
                file_ops::create_file(&path).map_err(|err| err.to_string())?;
                self.refresh_workspace_trees(cx);
                self.open_workspace_file(path, window, cx);
            }
            NameDialogKind::NewFolder { dir } => {
                let path = file_ops::new_folder_path(dir, name).map_err(name_error)?;
                file_ops::create_folder(&path).map_err(|err| err.to_string())?;
                self.workspace
                    .expanded
                    .insert(super::workspace::file_node_id(dir));
                self.refresh_workspace_trees(cx);
            }
            NameDialogKind::Rename { path } => {
                let target = file_ops::renamed_path(path, name).map_err(name_error)?;
                file_ops::rename(path, &target).map_err(|err| err.to_string())?;
                self.follow_renamed_path(path, &target, cx);
                self.refresh_workspace_trees(cx);
            }
        }
        Ok(())
    }

    /// Points open tabs (and the opened folder) at the new location after a
    /// rename of `from` to `to`.
    fn follow_renamed_path(&mut self, from: &Path, to: &Path, cx: &mut Context<Self>) {
        let own_id = cx.entity_id();
        for (editor, open_path, _) in self.open_documents(cx) {
            let Some(new_path) = open_path
                .as_deref()
                .and_then(|open| file_ops::path_after_rename(open, from, to))
            else {
                continue;
            };
            let update = |editor: &mut Editor, cx: &mut Context<Editor>| {
                editor.file_path = Some(new_path.clone());
                editor.pending_window_title_refresh = true;
                cx.notify();
            };
            if editor.entity_id() == own_id {
                update(self, cx);
            } else {
                editor.update(cx, update);
            }
        }
        if let Some(folder) = self.workspace.folder.clone()
            && let Some(new_folder) = file_ops::path_after_rename(&folder, from, to)
        {
            self.apply_workspace_folder(Some(new_folder), cx);
        }
    }

    /// Asks before moving `path` to the trash; refuses while a file below it
    /// is open with unsaved changes. Clean tabs of trashed files close.
    pub(super) fn request_move_to_trash(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = cx.global::<I18nManager>().strings().clone();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let affected = |open: &Path| open.starts_with(&path);
        let documents = self.open_documents(cx);
        if documents
            .iter()
            .any(|(_, open, dirty)| *dirty && open.as_deref().is_some_and(affected))
        {
            let detail = strings
                .workspace_delete_dirty_message
                .replace("{name}", &name);
            let buttons = [strings.info_dialog_ok.as_str()];
            let _ = window.prompt(
                PromptLevel::Warning,
                &strings.workspace_menu_delete,
                Some(&detail),
                &buttons,
                cx,
            );
            return;
        }
        let detail = strings
            .workspace_delete_confirm_message
            .replace("{name}", &name);
        let buttons = [
            strings.workspace_menu_delete.as_str(),
            strings.preferences_cancel.as_str(),
        ];
        let answer = window.prompt(
            PromptLevel::Warning,
            &strings.workspace_menu_delete,
            Some(&detail),
            &buttons,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.move_to_trash_now(&path, window, cx);
            });
        })
        .detach();
    }

    pub(super) fn move_to_trash_now(
        &mut self,
        path: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Err(err) = file_ops::move_to_trash(path) {
            let strings = cx.global::<I18nManager>().strings().clone();
            let detail = err.to_string();
            let buttons = [strings.info_dialog_ok.as_str()];
            let _ = window.prompt(
                PromptLevel::Critical,
                &strings.file_operation_failed_title,
                Some(&detail),
                &buttons,
                cx,
            );
            return;
        }
        self.refresh_workspace_trees(cx);
        if self
            .workspace
            .folder
            .as_deref()
            .is_some_and(|folder| folder.starts_with(path))
        {
            self.apply_workspace_folder(None, cx);
        }
        // Tabs of trashed files close (they are clean, see above).
        let trashed = path.to_path_buf();
        if let Some(host) = self.tab_host.as_ref().and_then(WeakEntity::upgrade) {
            window.defer(cx, move |window, cx| {
                host.update(cx, |host, cx| host.close_tabs_under(&trashed, window, cx));
            });
        } else if self
            .file_path
            .as_deref()
            .is_some_and(|open| open.starts_with(path))
        {
            self.file_path = None;
            self.pending_window_title_refresh = true;
            self.mark_dirty(cx);
        }
    }

    pub(super) fn render_tree_menu(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let menu = self.workspace.tree_menu.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();
        let mut entries = vec![
            (TreeMenuAction::Open, s.workspace_menu_open.clone()),
            (TreeMenuAction::NewFile, s.workspace_menu_new_file.clone()),
            (
                TreeMenuAction::NewFolder,
                s.workspace_menu_new_folder.clone(),
            ),
        ];
        let is_root = self.workspace.root.as_deref() == Some(menu.target.as_path());
        if !is_root {
            entries.push((TreeMenuAction::Rename, s.workspace_menu_rename.clone()));
            entries.push((TreeMenuAction::Delete, s.workspace_menu_delete.clone()));
        }
        entries.push((TreeMenuAction::CopyPath, s.workspace_menu_copy_path.clone()));
        entries.push((
            TreeMenuAction::Reveal,
            s.menu_reveal_in_file_manager.clone(),
        ));

        let items = entries
            .into_iter()
            .enumerate()
            .map(|(index, (action, label))| {
                div()
                    .id(("workspace-tree-menu-item", index))
                    .h(px(d.menu_item_height))
                    .px(px(d.menu_item_padding_x))
                    .flex()
                    .items_center()
                    .rounded(px(d.menu_item_radius))
                    .hover(|this| this.bg(c.dialog_secondary_button_hover))
                    .active(|this| this.opacity(0.92))
                    .cursor_pointer()
                    .text_size(px(d.menu_text_size))
                    .font_weight(t.dialog_body_weight.to_font_weight())
                    .text_color(c.dialog_secondary_button_text)
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.run_tree_menu_action(action, window, cx)
                    }))
            });

        Some(
            div()
                .id("workspace-tree-menu-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.close_tree_menu(cx);
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(|this, _, _, cx| {
                        this.close_tree_menu(cx);
                    }),
                )
                .child(
                    div()
                        .id("workspace-tree-menu")
                        .absolute()
                        .left(menu.position.x)
                        .top(menu.position.y)
                        .w(px(d.context_menu_panel_width))
                        .p(px(d.menu_panel_padding))
                        .flex()
                        .flex_col()
                        .gap(px(d.menu_panel_gap))
                        .occlude()
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .children(items),
                )
                .into_any_element(),
        )
    }

    pub(super) fn render_name_dialog(
        &self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let dialog = self.workspace.name_dialog.as_ref()?;
        let c = &theme.colors;
        let d = &theme.dimensions;
        let t = &theme.typography;
        let s = cx.global::<I18nManager>().strings().clone();
        let title = match &dialog.kind {
            NameDialogKind::NewFile { .. } => s.workspace_menu_new_file.clone(),
            NameDialogKind::NewFolder { .. } => s.workspace_menu_new_folder.clone(),
            NameDialogKind::Rename { .. } => s.workspace_menu_rename.clone(),
        };
        let button = |id: &'static str, label: String| {
            div()
                .id(id)
                .h(px(30.0))
                .px(px(14.0))
                .flex()
                .items_center()
                .rounded(px(d.menu_item_radius))
                .border(px(d.dialog_border_width))
                .border_color(c.dialog_border)
                .hover(|this| this.bg(c.dialog_secondary_button_hover))
                .cursor_pointer()
                .text_size(px(d.menu_text_size))
                .text_color(c.dialog_secondary_button_text)
                .child(label)
        };
        Some(
            div()
                .id("workspace-name-dialog-overlay")
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .bottom_0()
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .id("workspace-name-dialog")
                        .w(px(380.0))
                        .p(px(16.0))
                        .flex()
                        .flex_col()
                        .gap(px(10.0))
                        .bg(c.dialog_surface)
                        .border(px(d.dialog_border_width))
                        .border_color(c.dialog_border)
                        .rounded(px(d.menu_panel_radius))
                        .shadow_lg()
                        .text_size(px(t.dialog_body_size))
                        .capture_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                            if event.keystroke.key == "escape" {
                                this.close_name_dialog(cx);
                                cx.stop_propagation();
                            }
                        }))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(c.dialog_body)
                                .child(title),
                        )
                        .child(dialog.input.clone())
                        .children(dialog.error.clone().map(|error| {
                            div()
                                .text_size(px(d.menu_text_size))
                                .text_color(c.dialog_muted)
                                .child(error)
                        }))
                        .child(
                            div()
                                .flex()
                                .justify_end()
                                .gap(px(8.0))
                                .child(
                                    button("workspace-name-cancel", s.preferences_cancel.clone())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.close_name_dialog(cx);
                                        })),
                                )
                                .child(
                                    button("workspace-name-ok", s.info_dialog_ok.clone()).on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.confirm_name_dialog(window, cx);
                                        }),
                                    ),
                                ),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::{TestAppContext, VisualTestContext};

    use super::NameDialogKind;
    use crate::editor::Editor;

    fn init(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
    }

    #[gpui::test]
    async fn new_file_rename_and_trash_follow_the_open_document(cx: &mut TestAppContext) {
        init(cx);
        let root = std::env::temp_dir().join(format!("velotype-tree-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let note = root.join("note.md");
        fs::write(&note, "hello").unwrap();

        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "hello".to_string(), Some(note.clone()))
        });
        let cx: &mut VisualTestContext = cx;

        // Rename the open file: the document follows it.
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .apply_name(
                        &NameDialogKind::Rename { path: note.clone() },
                        "renamed",
                        window,
                        cx,
                    )
                    .unwrap();
            })
        });
        let renamed = root.join("renamed.md");
        assert!(renamed.is_file() && !note.exists());
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.file_path.as_deref(), Some(renamed.as_path()))
        });

        // Invalid and taken names are reported, nothing changes on disk.
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let dir = root.clone();
                assert!(
                    editor
                        .apply_name(
                            &NameDialogKind::NewFile { dir: dir.clone() },
                            "bad?",
                            window,
                            cx
                        )
                        .is_err()
                );
                assert!(
                    editor
                        .apply_name(&NameDialogKind::NewFile { dir }, "renamed", window, cx)
                        .is_err()
                );
                editor
                    .apply_name(
                        &NameDialogKind::NewFolder { dir: root.clone() },
                        "sub",
                        window,
                        cx,
                    )
                    .unwrap();
            })
        });
        assert!(root.join("sub").is_dir());

        // Trashing the open file detaches the document (no tab host here).
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.move_to_trash_now(&renamed, window, cx)
            })
        });
        assert!(!renamed.exists());
        editor.read_with(cx, |editor, _| {
            assert_eq!(editor.file_path, None);
            assert!(editor.document_dirty);
        });
        let _ = fs::remove_dir_all(root);
    }
}
