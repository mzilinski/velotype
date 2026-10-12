//! File-system operations behind the workspace tree and the File menu:
//! creating, renaming and trashing files and folders. Kept free of UI so
//! the naming rules are unit-tested.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result, bail};

/// Why a typed name cannot be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NameError {
    Invalid,
    Exists,
}

/// Characters that are not allowed in file names on at least one platform.
const FORBIDDEN: &[char] = &['/', '\\', ':', '*', '?', '"', '<', '>', '|'];

/// Checks a single file or folder name typed by the user.
pub(super) fn validate_name(name: &str) -> Result<(), NameError> {
    let trimmed = name.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.ends_with('.')
        || trimmed.contains(FORBIDDEN)
        || trimmed.chars().any(char::is_control)
    {
        return Err(NameError::Invalid);
    }
    Ok(())
}

/// The path for a new Markdown file named `name` in `dir`: `.md` is added
/// when the name has no extension.
pub(super) fn new_file_path(dir: &Path, name: &str) -> Result<PathBuf, NameError> {
    validate_name(name)?;
    let mut path = dir.join(name.trim());
    if path.extension().is_none() {
        path.set_extension("md");
    }
    if path.exists() {
        return Err(NameError::Exists);
    }
    Ok(path)
}

/// The path for a new folder named `name` in `dir`.
pub(super) fn new_folder_path(dir: &Path, name: &str) -> Result<PathBuf, NameError> {
    validate_name(name)?;
    let path = dir.join(name.trim());
    if path.exists() {
        return Err(NameError::Exists);
    }
    Ok(path)
}

/// The target of renaming `old` to `name` in the same folder. A Markdown
/// file keeps its extension when the new name has none.
pub(super) fn renamed_path(old: &Path, name: &str) -> Result<PathBuf, NameError> {
    validate_name(name)?;
    let parent = old.parent().unwrap_or(Path::new(""));
    let mut path = parent.join(name.trim());
    if old.is_file()
        && path.extension().is_none()
        && let Some(extension) = old.extension()
    {
        path.set_extension(extension);
    }
    if path == old {
        return Ok(path);
    }
    // Only a case change of the same entry may "exist" on case-insensitive
    // file systems.
    let same_entry_case_change = path
        .to_string_lossy()
        .eq_ignore_ascii_case(&old.to_string_lossy());
    if path.exists() && !same_entry_case_change {
        return Err(NameError::Exists);
    }
    Ok(path)
}

/// A name not yet taken in `dir`: `base.ext`, then `base 2.ext`, …
pub(super) fn unique_name(dir: &Path, base: &str, extension: Option<&str>) -> String {
    let file_name = |index: usize| {
        let stem = if index == 1 {
            base.to_string()
        } else {
            format!("{base} {index}")
        };
        match extension {
            Some(extension) => format!("{stem}.{extension}"),
            None => stem,
        }
    };
    (1..)
        .map(file_name)
        .find(|name| !dir.join(name).exists())
        .unwrap_or_else(|| file_name(1))
}

pub(super) fn create_file(path: &Path) -> Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("failed to create '{}'", path.display()))?;
    Ok(())
}

pub(super) fn create_folder(path: &Path) -> Result<()> {
    std::fs::create_dir(path).with_context(|| format!("failed to create '{}'", path.display()))
}

pub(super) fn rename(old: &Path, new: &Path) -> Result<()> {
    if old == new {
        return Ok(());
    }
    std::fs::rename(old, new).with_context(|| {
        format!(
            "failed to rename '{}' to '{}'",
            old.display(),
            new.display()
        )
    })
}

/// Moves a file or folder to the system trash. Tests delete directly so they
/// never touch the real trash.
pub(super) fn move_to_trash(path: &Path) -> Result<()> {
    if !path.exists() {
        bail!("'{}' does not exist", path.display());
    }
    if cfg!(test) {
        return if path.is_dir() {
            std::fs::remove_dir_all(path).map_err(Into::into)
        } else {
            std::fs::remove_file(path).map_err(Into::into)
        };
    }
    trash::delete(path).with_context(|| format!("failed to move '{}' to the trash", path.display()))
}

/// `path` after `from` was renamed to `to`: unchanged unless it is `from`
/// itself or lies below it.
pub(super) fn path_after_rename(path: &Path, from: &Path, to: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix(from).ok()?;
    Some(if rest.as_os_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(rest)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("velotype-fileops-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn names_are_validated() {
        assert_eq!(validate_name("notes"), Ok(()));
        assert_eq!(validate_name("Notizen 2026.md"), Ok(()));
        for bad in ["", "  ", ".", "..", "a/b", "a\\b", "what?", "name.", "x:y"] {
            assert_eq!(validate_name(bad), Err(NameError::Invalid), "{bad:?}");
        }
    }

    #[test]
    fn new_paths_add_md_and_detect_collisions() {
        let dir = temp_dir();
        assert_eq!(new_file_path(&dir, "a").unwrap(), dir.join("a.md"));
        assert_eq!(new_file_path(&dir, "b.txt").unwrap(), dir.join("b.txt"));
        create_file(&dir.join("a.md")).unwrap();
        assert_eq!(new_file_path(&dir, "a"), Err(NameError::Exists));
        assert_eq!(unique_name(&dir, "a", Some("md")), "a 2.md");
        assert_eq!(unique_name(&dir, "c", Some("md")), "c.md");

        create_folder(&dir.join("sub")).unwrap();
        assert_eq!(new_folder_path(&dir, "sub"), Err(NameError::Exists));
        assert_eq!(unique_name(&dir, "sub", None), "sub 2");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn rename_keeps_markdown_extension_and_moves_paths_below() {
        let dir = temp_dir();
        let old = dir.join("old.md");
        create_file(&old).unwrap();
        let new = renamed_path(&old, "new").unwrap();
        assert_eq!(new, dir.join("new.md"));
        rename(&old, &new).unwrap();
        assert!(new.is_file() && !old.exists());
        // Case-only change of the same file is allowed.
        assert_eq!(renamed_path(&new, "NEW").unwrap(), dir.join("NEW.md"));

        let folder = dir.join("folder");
        assert_eq!(
            path_after_rename(&folder.join("x/y.md"), &folder, &dir.join("renamed")),
            Some(dir.join("renamed").join("x/y.md"))
        );
        assert_eq!(path_after_rename(&new, &folder, &dir), None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn trash_removes_files_and_folders() {
        let dir = temp_dir();
        let file = dir.join("gone.md");
        create_file(&file).unwrap();
        move_to_trash(&file).unwrap();
        assert!(!file.exists());
        assert!(move_to_trash(&file).is_err());
        let _ = fs::remove_dir_all(dir);
    }
}
