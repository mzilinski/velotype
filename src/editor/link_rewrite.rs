//! Keeping links intact when a note or folder of the vault is renamed or
//! moved: links that named the old location are rewritten to the new one,
//! after asking. Documents open with unsaved changes are left alone.

use std::path::{Component, Path, PathBuf};

use gpui::*;

use super::Editor;
use super::link_index::{LinkTargets, extract_links};
use super::tabs::same_file;
use crate::i18n::I18nManager;

/// A document whose links change, with its new text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct LinkRewrite {
    pub(super) path: PathBuf,
    pub(super) text: String,
    pub(super) links: usize,
}

/// `path` after `from` was renamed to `to`; None when the move did not touch it.
fn moved(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    super::file_ops::path_after_rename(path, from, to)
}

/// The rewrites needed in `documents` (path after the move and text) after
/// `from` was moved to `to` below `root`. `files` lists the vault's documents
/// after the move.
pub(super) fn plan_link_rewrites(
    root: &Path,
    from: &Path,
    to: &Path,
    files: &[PathBuf],
    documents: impl IntoIterator<Item = (PathBuf, String)>,
) -> Vec<LinkRewrite> {
    let before = LinkTargets::new(
        root,
        files
            .iter()
            .map(|path| moved(path, to, from).unwrap_or_else(|| path.clone())),
    );
    let after = LinkTargets::new(root, files.iter().cloned());
    let mut rewrites = Vec::new();
    for (path, text) in documents {
        let old_path = moved(&path, to, from).unwrap_or_else(|| path.clone());
        let mut edits = Vec::new();
        for link in extract_links(&text) {
            let Some(range) = link.target_range.clone() else {
                continue;
            };
            let written = &text[range.clone()];
            let resolved = if link.wiki {
                before.resolve_wiki(&old_path, written)
            } else {
                before.resolve_markdown(&old_path, written)
            };
            let Some(target) = resolved else {
                continue;
            };
            let target_moved = moved(&target, from, to);
            if target_moved.is_none() && old_path == path {
                continue;
            }
            let new_target = target_moved.unwrap_or(target);
            let still = if link.wiki {
                after.resolve_wiki(&path, written)
            } else {
                after.resolve_markdown(&path, written)
            };
            // A bare name shared by several notes resolves only by chance.
            let ambiguous = link.wiki
                && !written.contains('/')
                && after.link_name(&new_target).contains('/')
                && new_target.parent() != path.parent();
            if still.as_deref() == Some(new_target.as_path()) && !ambiguous {
                continue;
            }
            let anchor = written
                .find('#')
                .map(|index| &written[index..])
                .unwrap_or("");
            let replacement = if link.wiki {
                format!("{}{anchor}", after.link_name(&new_target))
            } else {
                let file = if written.starts_with('/') {
                    format!(
                        "/{}",
                        slash_path(new_target.strip_prefix(after.root()).unwrap_or(&new_target))
                    )
                } else {
                    relative_path(path.parent().unwrap_or(root), &new_target)
                };
                let in_angles = text[..range.start].ends_with('<');
                let file = if in_angles {
                    file
                } else {
                    file.replace('%', "%25").replace(' ', "%20")
                };
                format!("{file}{anchor}")
            };
            edits.push((range, replacement));
        }
        if edits.is_empty() {
            continue;
        }
        let links = edits.len();
        let mut text = text;
        for (range, replacement) in edits.into_iter().rev() {
            text.replace_range(range, &replacement);
        }
        rewrites.push(LinkRewrite { path, text, links });
    }
    rewrites
}

fn slash_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

/// `target` relative to the folder `base`, with `/` separators.
fn relative_path(base: &Path, target: &Path) -> String {
    let base = base.components().collect::<Vec<_>>();
    let target_components = target.components().collect::<Vec<_>>();
    let common = base
        .iter()
        .zip(&target_components)
        .take_while(|(left, right)| left == right)
        .count();
    let mut parts = Vec::new();
    for component in &base[common..] {
        if matches!(component, Component::Normal(_)) {
            parts.push("..".to_string());
        }
    }
    for component in &target_components[common..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

/// Reads the vault's documents and plans the rewrites (in the background).
fn plan_for_vault(root: &Path, from: &Path, to: &Path) -> Vec<LinkRewrite> {
    let files = super::workspace_search::collect_markdown_files(root);
    let documents = files
        .iter()
        .filter_map(|path| {
            let bytes = std::fs::read(path).ok()?;
            Some((path.clone(), String::from_utf8(bytes).ok()?))
        })
        .collect::<Vec<_>>();
    plan_link_rewrites(root, from, to, &files, documents)
}

/// `path` with its folder resolved like the vault's paths, also when the
/// file itself is gone (the old name after a move).
fn canonical(path: &Path) -> PathBuf {
    if let Ok(path) = path.canonicalize() {
        return path;
    }
    match (
        path.parent().and_then(|parent| parent.canonicalize().ok()),
        path.file_name(),
    ) {
        (Some(parent), Some(name)) => parent.join(name),
        _ => path.to_path_buf(),
    }
}

impl Editor {
    /// After `from` was renamed or moved to `to`, finds the links in the vault
    /// that named it and offers to rewrite them.
    pub(super) fn offer_link_rewrites(
        &mut self,
        from: &Path,
        to: &Path,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(root) = self.link_root() else {
            return;
        };
        let root = canonical(&root);
        let (from, to) = (canonical(from), canonical(to));
        if !from.starts_with(&root) || !to.starts_with(&root) {
            return;
        }
        let name = from
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let plan = cx
            .background_executor()
            .spawn(async move { plan_for_vault(&root, &from, &to) });
        cx.spawn_in(window, async move |this, cx| {
            let rewrites = plan.await;
            if rewrites.is_empty() {
                return;
            }
            let Ok(answer) = this.update_in(cx, |_, window, cx| {
                let strings = cx.global::<I18nManager>().strings().clone();
                let links = rewrites.iter().map(|rewrite| rewrite.links).sum::<usize>();
                let detail = strings
                    .link_rewrite_message
                    .replace("{links}", &links.to_string())
                    .replace("{files}", &rewrites.len().to_string())
                    .replace("{name}", &name);
                let buttons = [
                    strings.link_rewrite_button.as_str(),
                    strings.preferences_cancel.as_str(),
                ];
                window.prompt(
                    PromptLevel::Info,
                    &strings.link_rewrite_title,
                    Some(&detail),
                    &buttons,
                    cx,
                )
            }) else {
                return;
            };
            if answer.await != Ok(0) {
                return;
            }
            let _ = this.update_in(cx, |this, window, cx| {
                this.apply_link_rewrites(rewrites, window, cx);
            });
        })
        .detach();
    }

    /// Writes the rewritten documents. Those open with unsaved changes are
    /// skipped and reported; clean open tabs are reloaded from disk.
    fn apply_link_rewrites(
        &mut self,
        rewrites: Vec<LinkRewrite>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let open = self.open_documents(cx);
        let mut skipped = Vec::new();
        let mut failed = Vec::new();
        for rewrite in rewrites {
            let open_tab = open.iter().find(|(_, open_path, _)| {
                open_path
                    .as_deref()
                    .is_some_and(|open| same_file(open, &rewrite.path))
            });
            if open_tab.is_some_and(|(_, _, dirty)| *dirty) {
                skipped.push(rewrite.path);
                continue;
            }
            if let Err(err) = std::fs::write(&rewrite.path, &rewrite.text) {
                failed.push(format!("{}: {err}", rewrite.path.display()));
                continue;
            }
            if let Some((editor, open_path, _)) = open_tab {
                let path = open_path.clone().unwrap_or_default();
                if editor.entity_id() == cx.entity_id() {
                    let _ = self.replace_document_from_path(&path, cx);
                } else {
                    editor.update(cx, |editor, cx| {
                        let _ = editor.replace_document_from_path(&path, cx);
                    });
                }
            }
        }
        if let Some(root) = self.link_root() {
            super::link_index::forget_link_index(&root, cx);
        }
        self.workspace.links_checked = false;
        if skipped.is_empty() && failed.is_empty() {
            cx.notify();
            return;
        }
        let strings = cx.global::<I18nManager>().strings().clone();
        let mut detail = String::new();
        for (heading, paths) in [
            (
                &strings.workspace_replace_skipped,
                skipped
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>(),
            ),
            (&strings.workspace_replace_failed, failed),
        ] {
            if paths.is_empty() {
                continue;
            }
            if !detail.is_empty() {
                detail.push_str("\n\n");
            }
            detail.push_str(heading);
            for path in paths {
                detail.push_str("\n• ");
                detail.push_str(&path);
            }
        }
        let buttons = [strings.info_dialog_ok.as_str()];
        let _ = window.prompt(
            PromptLevel::Warning,
            &strings.link_rewrite_title,
            Some(&detail),
            &buttons,
            cx,
        );
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{plan_link_rewrites, relative_path};

    fn plan(
        from: &str,
        to: &str,
        files: &[&str],
        documents: &[(&str, &str)],
    ) -> Vec<(String, String)> {
        let root = Path::new("/v");
        let files = files
            .iter()
            .map(|name| root.join(name))
            .collect::<Vec<PathBuf>>();
        plan_link_rewrites(
            root,
            &root.join(from),
            &root.join(to),
            &files,
            documents
                .iter()
                .map(|(name, text)| (root.join(name), text.to_string())),
        )
        .into_iter()
        .map(|rewrite| {
            (
                rewrite
                    .path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                rewrite.text,
            )
        })
        .collect()
    }

    #[test]
    fn renaming_a_note_rewrites_wiki_and_markdown_links() {
        let rewrites = plan(
            "people/Anna.md",
            "people/Anna Schmidt.md",
            &["people/Anna Schmidt.md", "Home.md", "projects/Plan.md"],
            &[
                (
                    "Home.md",
                    "[[Anna]], [[anna#Kontakt|sie]] and [A](people/Anna.md#x) and [[Other]]",
                ),
                (
                    "projects/Plan.md",
                    "[A](<../people/Anna.md>) [[people/Anna]]",
                ),
            ],
        );
        assert_eq!(
            rewrites,
            [
                (
                    "Home.md".to_string(),
                    "[[Anna Schmidt]], [[Anna Schmidt#Kontakt|sie]] and [A](people/Anna%20Schmidt.md#x) and [[Other]]"
                        .to_string()
                ),
                (
                    "projects/Plan.md".to_string(),
                    "[A](<../people/Anna Schmidt.md>) [[Anna Schmidt]]".to_string()
                ),
            ]
        );
    }

    #[test]
    fn moving_keeps_name_links_and_fixes_relative_ones() {
        // Name links still resolve after the move and stay as they are.
        let rewrites = plan(
            "Anna.md",
            "people/Anna.md",
            &["people/Anna.md", "Home.md"],
            &[
                ("Home.md", "[[Anna]] [x](Anna.md)"),
                ("people/Anna.md", "[h](Home.md)"),
            ],
        );
        assert_eq!(
            rewrites,
            [
                (
                    "Home.md".to_string(),
                    "[[Anna]] [x](people/Anna.md)".to_string()
                ),
                ("people/Anna.md".to_string(), "[h](../Home.md)".to_string()),
            ]
        );
    }

    #[test]
    fn a_name_that_becomes_ambiguous_gets_its_path() {
        let rewrites = plan(
            "a/Plan.md",
            "b/Plan.md",
            &["b/Plan.md", "c/Plan.md", "a/Notes.md"],
            &[("a/Notes.md", "[[Plan]]")],
        );
        assert_eq!(
            rewrites,
            [("a/Notes.md".to_string(), "[[b/Plan]]".to_string())]
        );
    }

    #[test]
    fn renaming_a_folder_follows_every_note_inside() {
        let rewrites = plan(
            "people",
            "team",
            &["team/Anna.md", "team/Ben.md", "Home.md"],
            &[(
                "Home.md",
                "[a](people/Anna.md) [b](/people/Ben.md) [[Anna]]",
            )],
        );
        assert_eq!(
            rewrites,
            [(
                "Home.md".to_string(),
                "[a](team/Anna.md) [b](/team/Ben.md) [[Anna]]".to_string()
            )]
        );
    }

    #[test]
    fn titles_and_untouched_links_stay() {
        let rewrites = plan(
            "a.md",
            "b.md",
            &["b.md", "Home.md", "c.md"],
            &[(
                "Home.md",
                "[x](a.md \"Title\") [y](c.md) [[c]] [z](https://a.md)",
            )],
        );
        assert_eq!(
            rewrites,
            [(
                "Home.md".to_string(),
                "[x](b.md \"Title\") [y](c.md) [[c]] [z](https://a.md)".to_string()
            )]
        );
    }

    #[test]
    fn plans_on_disk_after_a_real_rename() {
        let dir = std::env::temp_dir().join(format!("velotype-rewrite-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("people")).unwrap();
        std::fs::write(dir.join("people").join("Anna.md"), "# Anna").unwrap();
        std::fs::write(dir.join("Home.md"), "[[Anna]] and [A](people/Anna.md)").unwrap();
        let from = super::canonical(&dir.join("people").join("Anna.md"));
        std::fs::rename(&from, dir.join("people").join("Anne.md")).unwrap();
        let to = super::canonical(&dir.join("people").join("Anne.md"));
        let root = super::canonical(&dir);
        let rewrites = super::plan_for_vault(&root, &from, &to);
        assert_eq!(rewrites.len(), 1);
        assert_eq!(rewrites[0].text, "[[Anne]] and [A](people/Anne.md)");
        assert_eq!(rewrites[0].links, 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative_path(Path::new("/v/a"), Path::new("/v/b/c.md")),
            "../b/c.md"
        );
        assert_eq!(relative_path(Path::new("/v"), Path::new("/v/c.md")), "c.md");
        assert_eq!(
            relative_path(Path::new("/v/a/b"), Path::new("/v/a/c.md")),
            "../c.md"
        );
    }
}
