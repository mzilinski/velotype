//! File menu commands on the current document: Reload from Disk, Save All,
//! Show in File Manager, Reveal in Sidebar and Move File to Trash.

use gpui::*;

use super::Editor;
use super::workspace::WorkspaceTab;
use crate::i18n::I18nManager;

impl Editor {
    /// Re-reads the file from disk, asking first when there are unsaved
    /// changes.
    pub(crate) fn reload_from_disk(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if !self.document_dirty {
            self.reload_from_disk_now(window, cx);
            return;
        }
        let strings = cx.global::<I18nManager>().strings().clone();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let detail = strings.reload_confirm_message.replace("{name}", &name);
        let buttons = [
            strings.reload_confirm_button.as_str(),
            strings.preferences_cancel.as_str(),
        ];
        let answer = window.prompt(
            PromptLevel::Warning,
            &strings.menu_reload_from_disk,
            Some(&detail),
            &buttons,
            cx,
        );
        cx.spawn_in(window, async move |this, cx| {
            if answer.await == Ok(0) {
                let _ =
                    this.update_in(cx, |this, window, cx| this.reload_from_disk_now(window, cx));
            }
        })
        .detach();
    }

    fn reload_from_disk_now(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        match self.replace_document_from_path(&path, cx) {
            Ok(()) => window.set_window_edited(false),
            Err(err) => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let detail = err.to_string();
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

    /// Saves every tab of the window that has a file and unsaved changes.
    /// Untitled documents are left for Save As.
    pub(crate) fn save_all(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let own_id = cx.entity_id();
        for (editor, path, dirty) in self.open_documents(cx) {
            if !dirty || path.is_none() {
                continue;
            }
            if editor.entity_id() == own_id {
                self.save_document(window, cx);
            } else {
                editor.update(cx, |editor, cx| editor.save_document(window, cx));
            }
        }
    }

    pub(crate) fn reveal_in_file_manager(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.as_deref() {
            cx.reveal_path(path);
        }
    }

    /// Shows the drawer's file tree with the current file selected and its
    /// folders expanded.
    pub(crate) fn reveal_in_sidebar(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.file_path.clone() else {
            return;
        };
        self.workspace.is_open = true;
        self.workspace.active_tab = WorkspaceTab::Files;
        let root = self.workspace_root();
        for ancestor in path.ancestors().skip(1) {
            self.workspace
                .expanded
                .insert(super::workspace::file_node_id(ancestor));
            if root.as_deref() == Some(ancestor) {
                break;
            }
        }
        self.invalidate_workspace_tree(cx);
    }

    pub(crate) fn delete_current_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.file_path.clone() {
            self.request_move_to_trash(path, window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui::TestAppContext;

    use crate::editor::Editor;

    #[gpui::test]
    async fn reload_and_reveal_in_sidebar(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let root = std::env::temp_dir().join(format!("velotype-filemenu-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("sub")).unwrap();
        let path = root.join("sub").join("doc.md");
        fs::write(&path, "first").unwrap();

        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "first".to_string(), Some(path.clone()))
        });
        fs::write(&path, "changed on disk").unwrap();
        cx.update(|window, cx| editor.update(cx, |editor, cx| editor.reload_from_disk(window, cx)));
        editor.update(cx, |editor, cx| {
            assert_eq!(editor.current_document_source(cx), "changed on disk");
            assert!(!editor.document_dirty);

            editor.workspace.folder = Some(root.clone());
            editor.reveal_in_sidebar(cx);
            assert!(editor.workspace.is_open);
            assert!(
                editor
                    .workspace
                    .expanded
                    .contains(&crate::editor::workspace::file_node_id(&root.join("sub")))
            );
        });
        let _ = fs::remove_dir_all(root);
    }
}
