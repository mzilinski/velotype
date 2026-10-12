//! File > Auto Save and backups. With Auto Save on, a document that has a
//! file is saved a moment after it was changed. Before a save overwrites a
//! file, the version on disk is copied to the backup folder (at most one
//! copy per interval, a limited number per file); File > Show Backups opens
//! the folder of the current file.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use gpui::{Context, Window};

use super::Editor;
use crate::components::ShowBackups;

/// How long after a change Auto Save writes the file.
const AUTOSAVE_DELAY: Duration = Duration::from_secs(2);
/// A new backup is made only when the newest one is older than this.
const BACKUP_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Backups kept per file.
const BACKUPS_PER_FILE: usize = 20;

/// FNV-1a, stable across Rust versions (backup folders must keep their name).
fn stable_hash(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The backup folder of `file` below `backups_root`: its name plus a hash
/// of the full path, so equal names in different folders do not mix.
pub(crate) fn backup_dir_for(backups_root: &Path, file: &Path) -> PathBuf {
    let name = file
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "document".to_string());
    let key = file.to_string_lossy().to_lowercase();
    backups_root.join(format!("{name}-{:016x}", stable_hash(&key)))
}

/// Backups in `dir`, newest first.
fn backups_in(dir: &Path) -> Vec<(PathBuf, SystemTime)> {
    let mut backups = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| {
                    let modified = entry.metadata().ok()?.modified().ok()?;
                    entry.path().is_file().then(|| (entry.path(), modified))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    backups.sort_by(|left, right| right.1.cmp(&left.1).then(right.0.cmp(&left.0)));
    backups
}

/// Copies the current version of `file` into its backup folder unless the
/// newest backup is recent, then removes the oldest ones beyond the limit.
/// Returns the new backup, if one was made.
pub(crate) fn back_up_before_save(
    backups_root: &Path,
    file: &Path,
    timestamp: &str,
    now: SystemTime,
) -> std::io::Result<Option<PathBuf>> {
    if !file.is_file() {
        return Ok(None);
    }
    let dir = backup_dir_for(backups_root, file);
    let recent = backups_in(&dir).first().is_some_and(|(_, modified)| {
        now.duration_since(*modified)
            .is_ok_and(|age| age < BACKUP_INTERVAL)
    });
    if recent {
        return Ok(None);
    }
    std::fs::create_dir_all(&dir)?;
    let extension = file
        .extension()
        .map(|extension| format!(".{}", extension.to_string_lossy()))
        .unwrap_or_default();
    let target = dir.join(format!("{timestamp}{extension}"));
    std::fs::copy(file, &target)?;
    // The copy keeps the original's modification time; the rate limit
    // needs the time of the backup.
    std::fs::File::options()
        .write(true)
        .open(&target)?
        .set_modified(now)?;
    for (old, _) in backups_in(&dir).into_iter().skip(BACKUPS_PER_FILE) {
        let _ = std::fs::remove_file(old);
    }
    Ok(Some(target))
}

fn backups_root() -> Option<PathBuf> {
    if cfg!(test) {
        return None;
    }
    crate::config::VelotypeConfigDirs::from_system()
        .ok()
        .map(|dirs| dirs.backups_dir())
}

impl Editor {
    /// Backs up `path` before it is overwritten; failures only log.
    pub(super) fn back_up_file_before_save(&self, path: &Path) {
        let Some(root) = backups_root() else {
            return;
        };
        let timestamp = super::images::image_timestamp();
        if let Err(err) = back_up_before_save(&root, path, &timestamp, SystemTime::now()) {
            eprintln!("failed to back up '{}': {err}", path.display());
        }
    }

    /// Schedules an Auto Save for a changed document that has a file.
    pub(super) fn schedule_autosave(&mut self, cx: &mut Context<Self>) {
        let enabled = crate::config::EditorSettings::writing(cx).autosave;
        if !enabled
            || !self.document_dirty
            || self.file_path.is_none()
            || self.read_only
            || self.autosave_suspended
            || self.autosave_task.is_some()
        {
            return;
        }
        self.autosave_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(AUTOSAVE_DELAY).await;
            let _ = this.update(cx, |editor, cx| {
                editor.autosave_task = None;
                if editor.document_dirty && editor.file_path.is_some() && !editor.read_only {
                    // Saved on the next frame, which has the window.
                    editor.pending_autosave = true;
                    cx.notify();
                }
            });
        }));
    }

    /// Performs a due Auto Save. A failed one (shown once) pauses Auto Save
    /// until the next successful save, so the error does not repeat.
    pub(super) fn sync_pending_autosave(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.pending_autosave) {
            return;
        }
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if !self.document_dirty || self.read_only {
            return;
        }
        if !self.save_to_existing_path(&path, window, cx) {
            self.autosave_suspended = true;
        }
    }

    pub(super) fn on_show_backups(
        &mut self,
        _: &ShowBackups,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let dir = self
            .file_path
            .as_deref()
            .zip(backups_root())
            .map(|(file, root)| backup_dir_for(&root, file))
            .filter(|dir| dir.is_dir());
        match dir {
            Some(dir) => cx.open_with_system(&dir),
            None => {
                let buttons = [strings.info_dialog_ok.as_str()];
                let _ = window.prompt(
                    gpui::PromptLevel::Info,
                    &strings.menu_show_backups,
                    Some(&strings.backups_none),
                    &buttons,
                    cx,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("velotype-backup-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn backup_folders_depend_on_the_full_path() {
        let root = Path::new("backups");
        let a = backup_dir_for(root, Path::new("C:/one/notes.md"));
        let b = backup_dir_for(root, Path::new("C:/two/notes.md"));
        assert_ne!(a, b);
        assert!(
            a.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("notes.md-")
        );
        // Stable across runs and Rust versions.
        assert_eq!(stable_hash("abc"), 0xe71f_a219_0541_574b);
    }

    #[test]
    fn backups_are_rate_limited_and_capped() {
        let dir = temp_dir();
        let file = dir.join("doc.md");
        std::fs::write(&file, "first").unwrap();
        let root = dir.join("backups");
        let now = SystemTime::now();

        let first = back_up_before_save(&root, &file, "20261012-100000", now)
            .unwrap()
            .expect("first backup");
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "first");
        // A second save right after makes no new copy.
        assert!(
            back_up_before_save(&root, &file, "20261012-100001", now)
                .unwrap()
                .is_none()
        );
        // Much later it does.
        let later = now + BACKUP_INTERVAL + Duration::from_secs(1);
        assert!(
            back_up_before_save(&root, &file, "20261012-102000", later)
                .unwrap()
                .is_some()
        );

        // Never more than the limit.
        let backup_dir = backup_dir_for(&root, &file);
        for index in 0..BACKUPS_PER_FILE + 5 {
            std::fs::write(backup_dir.join(format!("old-{index:02}.md")), "x").unwrap();
        }
        let much_later = later + BACKUP_INTERVAL * 2;
        back_up_before_save(&root, &file, "20261012-110000", much_later).unwrap();
        assert_eq!(backups_in(&backup_dir).len(), BACKUPS_PER_FILE);

        // A file that does not exist yet has nothing to back up.
        assert!(
            back_up_before_save(&root, &dir.join("new.md"), "x", now)
                .unwrap()
                .is_none()
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod editor_tests {
    use gpui::TestAppContext;

    use crate::editor::Editor;

    #[gpui::test]
    async fn autosave_writes_the_file_after_a_change(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            crate::config::EditorSettings::set_writing_for_test(
                cx,
                crate::config::WritingPreferences {
                    autosave: true,
                    ..Default::default()
                },
            );
        });
        let dir = std::env::temp_dir().join(format!("velotype-autosave-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("doc.md");
        std::fs::write(&path, "Hello").unwrap();
        let (editor, cx) = cx.add_window_view(|_window, cx| {
            Editor::from_markdown(cx, "Hello".to_string(), Some(path.clone()))
        });
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                let block = editor.document.first_root().unwrap().clone();
                block.update(cx, |block, cx| {
                    block.move_to(5, cx);
                    gpui::EntityInputHandler::replace_text_in_range(
                        block, None, " world", window, cx,
                    );
                });
            })
        });
        cx.run_until_parked();
        assert!(editor.read_with(cx, |editor, _| editor.document_dirty));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Hello");

        cx.executor()
            .advance_clock(super::AUTOSAVE_DELAY + std::time::Duration::from_millis(100));
        cx.run_until_parked();
        // The next frame performs the save.
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "Hello world");
        assert!(!editor.read_with(cx, |editor, _| editor.document_dirty));
        let _ = std::fs::remove_dir_all(dir);
    }
}
