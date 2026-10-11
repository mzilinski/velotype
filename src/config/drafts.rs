//! Session drafts: unsaved untitled documents kept across app restarts.
//!
//! When an editor window closes, every untitled tab with unsaved content is
//! written to `<config>/drafts/<id>.md` instead of prompting, and the next
//! launch reopens those drafts as unsaved tabs. Restoring consumes the files,
//! so a second running instance does not reopen the same drafts; an open draft
//! is written again (under the same id) the next time its window closes.
//!
//! The store is only active when [`SessionDrafts::init`] installed it as a
//! global, which `main` does; tests and other embedders keep the plain
//! unsaved-changes prompt unless they install a store explicitly.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use gpui::{App, Global};

use super::VelotypeConfigDirs;

const DRAFT_EXTENSION: &str = "md";

/// Directory-backed draft store installed as a GPUI global.
#[derive(Debug, Clone)]
pub(crate) struct SessionDrafts {
    dir: PathBuf,
}

impl Global for SessionDrafts {}

impl SessionDrafts {
    /// Installs the store rooted in the platform config directory.
    pub(crate) fn init(cx: &mut App) {
        match VelotypeConfigDirs::from_system() {
            Ok(dirs) => cx.set_global(Self::new(dirs.drafts_dir())),
            Err(err) => eprintln!("session drafts disabled: {err}"),
        }
    }

    pub(crate) fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Returns the installed store, if draft keeping is enabled.
    pub(crate) fn get(cx: &App) -> Option<Self> {
        cx.try_global::<Self>().cloned()
    }

    #[cfg(test)]
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// A new draft id. Ids sort in creation order, which is the order drafts
    /// are restored in.
    pub(crate) fn new_draft_id() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or_default();
        format!("{nanos:024}-{}", uuid::Uuid::new_v4().simple())
    }

    fn draft_path(&self, id: &str) -> PathBuf {
        self.dir.join(id).with_extension(DRAFT_EXTENSION)
    }

    /// Writes (or overwrites) the draft stored under `id`.
    pub(crate) fn write(&self, id: &str, markdown: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("failed to create '{}'", self.dir.display()))?;
        let path = self.draft_path(id);
        std::fs::write(&path, markdown)
            .with_context(|| format!("failed to write '{}'", path.display()))
    }

    /// Deletes the draft stored under `id`, if any.
    pub(crate) fn remove(&self, id: &str) {
        let path = self.draft_path(id);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => eprintln!("failed to remove draft '{}': {err}", path.display()),
        }
    }

    /// Reads every stored draft in creation order and removes it from disk.
    /// Unreadable drafts are left in place and reported.
    pub(crate) fn take_all(&self) -> Vec<String> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
            Err(err) => {
                eprintln!("failed to read drafts in '{}': {err}", self.dir.display());
                return Vec::new();
            }
        };

        let mut paths: Vec<PathBuf> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_draft_file(path))
            .collect();
        paths.sort();

        let mut drafts = Vec::with_capacity(paths.len());
        for path in paths {
            match std::fs::read_to_string(&path) {
                Ok(markdown) => {
                    if let Err(err) = std::fs::remove_file(&path) {
                        eprintln!("failed to remove draft '{}': {err}", path.display());
                    }
                    if !markdown.trim().is_empty() {
                        drafts.push(markdown);
                    }
                }
                Err(err) => eprintln!("failed to read draft '{}': {err}", path.display()),
            }
        }
        drafts
    }
}

fn is_draft_file(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|extension| extension == DRAFT_EXTENSION)
}

#[cfg(test)]
mod tests {
    use super::SessionDrafts;

    fn temp_store() -> SessionDrafts {
        SessionDrafts::new(
            std::env::temp_dir().join(format!("velotype-drafts-{}", uuid::Uuid::new_v4())),
        )
    }

    #[test]
    fn missing_draft_directory_restores_nothing() {
        let store = temp_store();
        assert!(store.take_all().is_empty());
        assert!(!store.dir().exists());
    }

    #[test]
    fn drafts_restore_in_creation_order_and_are_consumed() {
        let store = temp_store();
        let first = SessionDrafts::new_draft_id();
        let second = SessionDrafts::new_draft_id();
        assert!(first < second, "draft ids sort in creation order");

        store.write(&second, "second draft").unwrap();
        store.write(&first, "first draft").unwrap();
        store.write(&first, "first draft, edited").unwrap();

        assert_eq!(
            store.take_all(),
            vec![
                "first draft, edited".to_string(),
                "second draft".to_string()
            ]
        );
        assert!(store.take_all().is_empty(), "restored drafts are consumed");

        let _ = std::fs::remove_dir_all(store.dir());
    }

    #[test]
    fn removed_and_blank_drafts_are_not_restored() {
        let store = temp_store();
        let kept = SessionDrafts::new_draft_id();
        let removed = SessionDrafts::new_draft_id();
        let blank = SessionDrafts::new_draft_id();
        store.write(&kept, "kept").unwrap();
        store.write(&removed, "removed").unwrap();
        store.write(&blank, "  \n").unwrap();
        store.remove(&removed);
        store.remove("never-written");

        assert_eq!(store.take_all(), vec!["kept".to_string()]);

        let _ = std::fs::remove_dir_all(store.dir());
    }
}
